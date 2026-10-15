use std::sync::{Arc, Mutex};

use delta_usecase::ReleaseAsset;
use release_signature::testing::{
    bundle_version, test_key, BUNDLE, BUNDLE_NAME, OTHER_FILE, OTHER_KEY, OTHER_VERSION, SIGNED,
};

use super::*;
use crate::https_client::tests::{is_refused_redirect, redirect_to};
use crate::test_server::serve_routes;

const NAME: &str = BUNDLE_NAME;
const SIGNATURE: &str = "delta-desktop_0.6.0_amd64.deb.minisig";
const BODY: &[u8] = BUNDLE;

fn sha256_hex(bytes: &[u8]) -> String {
    to_hex(&Sha256::digest(bytes))
}

fn digest_of(bytes: &[u8]) -> Option<String> {
    Some(format!("sha256:{}", sha256_hex(bytes)))
}

fn ok_response(body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn not_found() -> Vec<u8> {
    b"HTTP/1.1 404 Not Found\r\ncontent-length: 9\r\nconnection: close\r\n\r\nNot Found".to_vec()
}

/// A loopback release serving `asset` at `/download/<NAME>` and
/// `signature` at `/download/<NAME>.minisig`, with the request lines it
/// received.
struct Release {
    base: String,
    received: Arc<Mutex<Vec<String>>>,
    server: tokio::task::JoinHandle<()>,
}

impl Release {
    async fn serving(asset: Vec<u8>, signature: Vec<u8>) -> Self {
        let (base, received, server) = serve_routes(vec![
            (format!("/download/{NAME}"), asset),
            (format!("/download/{SIGNATURE}"), signature),
        ])
        .await;
        Self {
            base,
            received,
            server,
        }
    }

    /// The release of [`BODY`] signed as the release workflow signs it,
    /// with the signature `signature`.
    async fn signed_with(signature: &str) -> Self {
        Self::serving(ok_response(BODY), ok_response(signature.as_bytes())).await
    }

    /// The paths requested, in order.
    fn requested(&self) -> Vec<String> {
        self.received
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.split(' ').nth(1).unwrap().to_owned())
            .collect()
    }

    fn signed(&self, digest: Option<String>) -> SignedAsset {
        SignedAsset {
            asset: ReleaseAsset {
                name: NAME.into(),
                download_url: format!("{}/download/{NAME}", self.base),
                digest,
            },
            signature_url: format!("{}/download/{SIGNATURE}", self.base),
            version: bundle_version(),
        }
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// The names of the files in `dir`, sorted; none when it does not exist.
fn files_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// A downloader checking signatures with the test key, over the tests'
/// plain HTTP.
fn downloader() -> GithubAssetDownloader {
    GithubAssetDownloader::with(test_key(), Schemes::PlainFirstHop).unwrap()
}

async fn download_with(
    downloader: &GithubAssetDownloader,
    signed: &SignedAsset,
    dir: &Path,
) -> (Result<PathBuf, AssetDownloadError>, Vec<DownloadProgress>) {
    let reports = Mutex::new(Vec::new());
    let result = downloader
        .download(signed, dir, &|progress| {
            reports.lock().unwrap().push(progress)
        })
        .await;
    (result, reports.into_inner().unwrap())
}

async fn download(
    signed: &SignedAsset,
    dir: &Path,
) -> (Result<PathBuf, AssetDownloadError>, Vec<DownloadProgress>) {
    download_with(&downloader(), signed, dir).await
}

#[tokio::test]
async fn a_signed_asset_matching_its_digest_leaves_exactly_it_and_its_signature() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    let release = Release::signed_with(SIGNED).await;
    let digest = format!("sha256:{}", sha256_hex(BODY).to_ascii_uppercase());

    let (result, reports) = download(&release.signed(Some(digest)), &dir).await;

    let path = result.unwrap();
    assert_eq!(path, dir.join(NAME));
    assert_eq!(std::fs::read(&path).unwrap(), BODY);
    assert_eq!(
        std::fs::read_to_string(dir.join(SIGNATURE)).unwrap(),
        SIGNED
    );
    assert_eq!(files_in(&dir), [NAME, SIGNATURE]);
    let mode =
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(&dir).unwrap().permissions());
    assert_eq!(
        mode & 0o077,
        0,
        "the download directory is owner-only: {mode:o}"
    );
    let total = Some(BODY.len() as u64);
    assert_eq!(
        reports.first(),
        Some(&DownloadProgress { received: 0, total })
    );
    assert_eq!(
        reports.last(),
        Some(&DownloadProgress {
            received: BODY.len() as u64,
            total
        })
    );
    // The signature first, so a refused one costs no download.
    assert_eq!(
        release.requested(),
        [
            format!("/download/{SIGNATURE}"),
            format!("/download/{NAME}")
        ]
    );
    for request in release.received.lock().unwrap().iter() {
        assert!(
            request.to_ascii_lowercase().contains("user-agent: delta/"),
            "{request}"
        );
    }
}

#[tokio::test]
async fn a_mismatching_digest_leaves_no_file() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    let release = Release::signed_with(SIGNED).await;
    let expected = sha256_hex(b"another file");

    let (result, _) = download(&release.signed(Some(format!("sha256:{expected}"))), &dir).await;

    let err = result.unwrap_err();
    assert!(
        matches!(
            &err,
            AssetDownloadError::DigestMismatch { expected: e, actual }
                if *e == expected && *actual == sha256_hex(BODY)
        ),
        "{err:?}"
    );
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

/// The digest GitHub states is of whatever was uploaded: a file replaced
/// after the release workflow signed it matches its digest, and fails its
/// signature.
#[tokio::test]
async fn a_file_matching_its_digest_but_not_its_signature_leaves_no_file() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    let replaced = b"pretend this is a Delta release bundlf\n";
    let release = Release::serving(ok_response(replaced), ok_response(SIGNED.as_bytes())).await;

    let (result, _) = download(&release.signed(digest_of(replaced)), &dir).await;

    let err = result.unwrap_err();
    assert!(
        matches!(
            err,
            AssetDownloadError::SignatureRejected(SignatureError::Invalid)
        ),
        "{err:?}"
    );
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[tokio::test]
async fn a_signature_refused_on_its_own_leaves_no_file_and_the_asset_unrequested() {
    for (signature, refused) in [
        (
            OTHER_KEY,
            (|err: &SignatureError| matches!(err, SignatureError::OtherKey))
                as fn(&SignatureError) -> bool,
        ),
        (OTHER_FILE, |err| {
            matches!(err, SignatureError::OtherFile { .. })
        }),
        (OTHER_VERSION, |err| {
            matches!(err, SignatureError::OtherVersion { .. })
        }),
        ("not a signature", |err| {
            matches!(err, SignatureError::Malformed { .. })
        }),
    ] {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("updates");
        let release = Release::signed_with(signature).await;

        let (result, reports) = download(&release.signed(digest_of(BODY)), &dir).await;

        let err = result.unwrap_err();
        assert!(
            matches!(&err, AssetDownloadError::SignatureRejected(inner) if refused(inner)),
            "{signature}: {err:?}"
        );
        assert_eq!(release.requested(), [format!("/download/{SIGNATURE}")]);
        assert!(reports.is_empty());
        assert_eq!(files_in(&dir), Vec::<String>::new());
    }
}

/// The downloader the app uses checks with the release key, which did not
/// make the test signature.
#[tokio::test]
async fn the_apps_downloader_checks_with_the_release_key() {
    let data = tempfile::tempdir().unwrap();
    let release = Release::signed_with(SIGNED).await;

    let (result, _) = download_with(
        &GithubAssetDownloader::with(ReleaseKey::production(), Schemes::PlainFirstHop).unwrap(),
        &release.signed(digest_of(BODY)),
        data.path(),
    )
    .await;

    let err = result.unwrap_err();
    assert!(
        matches!(
            err,
            AssetDownloadError::SignatureRejected(SignatureError::OtherKey)
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_missing_signature_is_reported_with_its_status_and_the_asset_unrequested() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    let release = Release::serving(ok_response(BODY), not_found()).await;

    let (result, _) = download(&release.signed(digest_of(BODY)), &dir).await;

    let err = result.unwrap_err();
    assert!(
        matches!(err, AssetDownloadError::SignatureStatus(404)),
        "{err:?}"
    );
    assert_eq!(release.requested(), [format!("/download/{SIGNATURE}")]);
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[tokio::test]
async fn a_signature_larger_than_any_signature_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    let large = format!("{SIGNED}{}", " ".repeat(MAX_SIGNATURE_BYTES));
    let release = Release::signed_with(&large).await;

    let (result, _) = download(&release.signed(digest_of(BODY)), &dir).await;

    let err = result.unwrap_err();
    assert!(
        matches!(
            err,
            AssetDownloadError::SignatureRejected(SignatureError::TooLarge { .. })
        ),
        "{err:?}"
    );
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[tokio::test]
async fn an_asset_without_a_sha256_digest_is_never_requested() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    let release = Release::signed_with(SIGNED).await;
    for digest in [
        None,
        Some(String::new()),
        Some(sha256_hex(BODY)),
        Some(format!("sha512:{}", sha256_hex(BODY))),
        Some("sha256:abc".to_owned()),
        Some(format!("sha256:{}", "z".repeat(64))),
    ] {
        let (result, reports) = download(&release.signed(digest.clone()), &dir).await;
        let err = result.unwrap_err();
        assert!(
            matches!(&err, AssetDownloadError::NoDigest(name) if name == NAME),
            "{digest:?}: {err:?}"
        );
        assert!(reports.is_empty());
        assert_eq!(files_in(&dir), Vec::<String>::new());
    }
    assert!(release.requested().is_empty());
}

#[tokio::test]
async fn a_non_2xx_answer_is_reported_with_its_status_and_leaves_no_file() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    let release = Release::serving(not_found(), ok_response(SIGNED.as_bytes())).await;

    let (result, _) = download(&release.signed(digest_of(BODY)), &dir).await;

    let err = result.unwrap_err();
    assert!(matches!(err, AssetDownloadError::Status(404)), "{err:?}");
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[tokio::test]
async fn a_transfer_cut_short_leaves_no_file() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    // Promise more than is sent, then hang up.
    let mut response = format!(
        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        BODY.len() * 4
    )
    .into_bytes();
    response.extend_from_slice(BODY);
    let release = Release::serving(response, ok_response(SIGNED.as_bytes())).await;

    let (result, _) = download(&release.signed(digest_of(BODY)), &dir).await;

    let err = result.unwrap_err();
    assert!(
        matches!(
            err,
            AssetDownloadError::Request(_) | AssetDownloadError::Truncated { .. }
        ),
        "{err:?}"
    );
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[tokio::test]
async fn a_completed_download_removes_other_versions() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("delta-desktop_0.5.0_amd64.deb"), b"old").unwrap();
    std::fs::write(dir.join("delta-desktop_0.5.0_amd64.deb.minisig"), b"old").unwrap();
    std::fs::write(dir.join("delta-desktop_0.5.1_amd64.deb.part"), b"partial").unwrap();
    let release = Release::signed_with(SIGNED).await;

    let (result, _) = download(&release.signed(digest_of(BODY)), &dir).await;

    result.unwrap();
    assert_eq!(files_in(&dir), [NAME, SIGNATURE]);
}

/// Create `dir` holding [`NAME`] with `body` and [`SIGNATURE`] with
/// `signature`, when given, and each of `others` with its name as its
/// contents.
fn updates_with(dir: &Path, body: Option<&[u8]>, signature: Option<&str>, others: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    if let Some(body) = body {
        std::fs::write(dir.join(NAME), body).unwrap();
    }
    if let Some(signature) = signature {
        std::fs::write(dir.join(SIGNATURE), signature).unwrap();
    }
    for name in others {
        std::fs::write(dir.join(name), name).unwrap();
    }
}

async fn reverify(dir: &Path) -> Result<Option<PathBuf>, AssetDownloadError> {
    let signed = SignedAsset {
        asset: ReleaseAsset {
            name: NAME.into(),
            download_url: format!("http://127.0.0.1:9/{NAME}"),
            digest: digest_of(BODY),
        },
        signature_url: format!("http://127.0.0.1:9/{SIGNATURE}"),
        version: bundle_version(),
    };
    downloader().reverify(&signed, dir).await
}

#[tokio::test]
async fn a_matching_file_left_behind_is_verified_again_and_kept_alone() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    updates_with(
        &dir,
        Some(BODY),
        Some(SIGNED),
        &[
            "delta-desktop_0.5.1_amd64.deb",
            "delta-desktop_0.6.0_amd64.deb.part",
        ],
    );

    assert_eq!(reverify(&dir).await.unwrap(), Some(dir.join(NAME)));
    assert_eq!(files_in(&dir), [NAME, SIGNATURE]);
    assert_eq!(std::fs::read(dir.join(NAME)).unwrap(), BODY);
}

#[tokio::test]
async fn a_corrupt_file_left_behind_is_removed() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    updates_with(
        &dir,
        Some(b"pretend this is a Delta release bundlf\n"),
        Some(SIGNED),
        &[],
    );

    let err = reverify(&dir).await.unwrap_err();
    assert!(
        matches!(
            &err,
            AssetDownloadError::DigestMismatch { expected, .. } if *expected == sha256_hex(BODY)
        ),
        "{err:?}"
    );
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[tokio::test]
async fn a_file_left_behind_without_its_signature_is_removed() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    updates_with(&dir, Some(BODY), None, &[]);

    let err = reverify(&dir).await.unwrap_err();
    assert!(
        matches!(
            &err,
            AssetDownloadError::SignatureRejected(SignatureError::Missing { bundle })
                if bundle == NAME
        ),
        "{err:?}"
    );
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[tokio::test]
async fn a_file_left_behind_with_a_signature_that_fails_is_removed() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    updates_with(&dir, Some(BODY), Some(OTHER_KEY), &[]);

    let err = reverify(&dir).await.unwrap_err();
    assert!(
        matches!(
            err,
            AssetDownloadError::SignatureRejected(SignatureError::OtherKey)
        ),
        "{err:?}"
    );
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[tokio::test]
async fn a_file_of_an_older_release_is_removed_and_nothing_is_found() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    // A verified download of v0.5.1, left behind when v0.6.0 came out.
    updates_with(
        &dir,
        None,
        None,
        &[
            "delta-desktop_0.5.1_amd64.deb",
            "delta-desktop_0.5.1_amd64.deb.minisig",
        ],
    );

    assert_eq!(reverify(&dir).await.unwrap(), None);
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[tokio::test]
async fn no_file_left_behind_finds_nothing() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    assert_eq!(reverify(&dir).await.unwrap(), None);
    updates_with(&dir, None, None, &[]);
    assert_eq!(reverify(&dir).await.unwrap(), None);
    // A signature alone is no download, and is cleared.
    updates_with(&dir, None, Some(SIGNED), &[]);
    assert_eq!(reverify(&dir).await.unwrap(), None);
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

#[tokio::test]
async fn a_symlink_left_behind_is_not_followed_and_is_removed() {
    let data = tempfile::tempdir().unwrap();
    // Even to a file that would match: what the link names can change.
    let elsewhere = data.path().join("elsewhere");
    std::fs::write(&elsewhere, BODY).unwrap();
    let signature_elsewhere = data.path().join("elsewhere.minisig");
    std::fs::write(&signature_elsewhere, SIGNED).unwrap();
    for (linked, target) in [(NAME, &elsewhere), (SIGNATURE, &signature_elsewhere)] {
        let dir = data.path().join("updates");
        updates_with(&dir, Some(BODY), Some(SIGNED), &[]);
        std::fs::remove_file(dir.join(linked)).unwrap();
        std::os::unix::fs::symlink(target, dir.join(linked)).unwrap();

        let err = reverify(&dir).await.unwrap_err();
        assert!(
            matches!(&err, AssetDownloadError::NotRegularFile { path } if *path == dir.join(linked)),
            "{linked}: {err:?}"
        );
        assert_eq!(files_in(&dir), Vec::<String>::new());
    }
    assert_eq!(std::fs::read(&elsewhere).unwrap(), BODY);
    assert_eq!(
        std::fs::read_to_string(&signature_elsewhere).unwrap(),
        SIGNED
    );
}

#[tokio::test]
async fn a_directory_left_behind_under_the_assets_name_is_refused() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    std::fs::create_dir_all(dir.join(NAME)).unwrap();
    std::fs::write(dir.join(SIGNATURE), SIGNED).unwrap();

    let err = reverify(&dir).await.unwrap_err();
    assert!(
        matches!(&err, AssetDownloadError::NotRegularFile { .. }),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_name_that_is_not_a_plain_file_name_is_refused() {
    let data = tempfile::tempdir().unwrap();
    for name in ["../escape.deb", "a/b.deb", "..", ""] {
        let signed = SignedAsset {
            asset: ReleaseAsset {
                name: name.into(),
                download_url: "http://127.0.0.1:9/a.deb".into(),
                digest: digest_of(BODY),
            },
            signature_url: "http://127.0.0.1:9/a.deb.minisig".into(),
            version: bundle_version(),
        };
        let (result, _) = download(&signed, data.path()).await;
        assert!(
            matches!(result, Err(AssetDownloadError::BadName(ref n)) if n == name),
            "{name}: {result:?}"
        );
    }
}

/// The downloader the app uses asks for nothing over plain HTTP: the
/// signature, the first request, is refused before any request is sent.
#[tokio::test]
async fn the_apps_downloader_refuses_a_plain_http_url() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    let release = Release::signed_with(SIGNED).await;

    let (result, _) = download_with(
        &GithubAssetDownloader::new().unwrap(),
        &release.signed(digest_of(BODY)),
        &dir,
    )
    .await;

    let err = result.unwrap_err();
    assert!(
        matches!(err, AssetDownloadError::SignatureRequest(_)),
        "{err:?}"
    );
    assert!(release.requested().is_empty());
    assert_eq!(files_in(&dir), Vec::<String>::new());
}

/// Neither the signature nor the asset follows a redirect to plain HTTP.
#[tokio::test]
async fn a_redirect_to_plain_http_is_refused_for_the_signature_and_the_asset() {
    let data = tempfile::tempdir().unwrap();
    let dir = data.path().join("updates");
    let elsewhere = "http://127.0.0.1:9/elsewhere";

    let release = Release::serving(ok_response(BODY), redirect_to(elsewhere)).await;
    let (result, _) = download(&release.signed(digest_of(BODY)), &dir).await;
    let err = result.unwrap_err();
    assert!(
        matches!(&err, AssetDownloadError::SignatureRequest(cause) if is_refused_redirect(cause.as_ref())),
        "{err:?}"
    );

    let release = Release::serving(redirect_to(elsewhere), ok_response(SIGNED.as_bytes())).await;
    let (result, _) = download(&release.signed(digest_of(BODY)), &dir).await;
    let err = result.unwrap_err();
    assert!(
        matches!(&err, AssetDownloadError::Request(cause) if is_refused_redirect(cause.as_ref())),
        "{err:?}"
    );
    assert_eq!(files_in(&dir), Vec::<String>::new());
}
