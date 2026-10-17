use std::path::{Path, PathBuf};

use anyhow::bail;

/// The longest socket path that fits `sockaddr_un.sun_path`, with the NUL that
/// ends it: the field is 104 bytes on macOS, the smallest among the Unixes
/// Delta runs on (108 on Linux).
pub const MAX_SOCKET_PATH_LEN: usize = 103;

/// The single-instance socket in `dir` for the app `identifier`:
/// `<identifier with . and - as _>_si.sock`, the name the plugin used, so the
/// dev build (`io.github.x7c1.delta.dev`) and the installed app keep separate
/// locks.
///
/// A path longer than [`MAX_SOCKET_PATH_LEN`] cannot be bound, and is not
/// shortened into another name: it is an error, and the app then launches
/// without the lock. macOS's per-user temporary directory
/// (`/var/folders/xx/<id>/T/`) leaves room for about fifty bytes of name.
pub fn socket_path(dir: &Path, identifier: &str) -> anyhow::Result<PathBuf> {
    let name = format!("{}_si.sock", identifier.replace(['.', '-'], "_"));
    let path = dir.join(name);
    let len = path.as_os_str().len();
    if len > MAX_SOCKET_PATH_LEN {
        bail!(
            "the socket path {} is {len} bytes, over the {MAX_SOCKET_PATH_LEN} a Unix socket's \
             path may hold",
            path.display()
        );
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_is_named_after_the_identifier_in_the_given_directory() {
        let dir = Path::new("/var/folders/ab/cdef/T");
        assert_eq!(
            socket_path(dir, "io.github.x7c1.delta").unwrap(),
            dir.join("io_github_x7c1_delta_si.sock")
        );
        assert_eq!(
            socket_path(dir, "io.github.x7c1.delta.dev").unwrap(),
            dir.join("io_github_x7c1_delta_dev_si.sock")
        );
        assert_eq!(
            socket_path(dir, "my-app.id").unwrap(),
            dir.join("my_app_id_si.sock")
        );
    }

    #[test]
    fn a_path_that_fits_sun_path_exactly_is_kept() {
        let name_len = "x_si.sock".len();
        let dir = format!("/{}", "d".repeat(MAX_SOCKET_PATH_LEN - name_len - 2));
        let path = socket_path(Path::new(&dir), "x").unwrap();
        assert_eq!(path.as_os_str().len(), MAX_SOCKET_PATH_LEN);
    }

    #[test]
    fn a_path_over_the_sun_path_limit_is_an_error() {
        let name_len = "x_si.sock".len();
        let dir = format!("/{}", "d".repeat(MAX_SOCKET_PATH_LEN - name_len - 1));
        let err = socket_path(Path::new(&dir), "x").unwrap_err();
        assert!(
            err.to_string()
                .contains(&format!("is {} bytes", MAX_SOCKET_PATH_LEN + 1)),
            "{err}"
        );
    }
}
