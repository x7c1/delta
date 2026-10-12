# Install on macOS

The macOS bundle runs on Apple silicon (M1 and later). What the app needs on
the host and where it keeps its data are in the [install guide](README.md).

## Download and install

1. Install `tmux` if you do not have it: `brew install tmux`.
2. Open the [latest Release](https://github.com/x7c1/delta/releases/latest)
   and download `Delta_<version>_aarch64.dmg`.
3. Open the `.dmg` and drag `Delta.app` into `Applications`.

## First launch: getting past Gatekeeper

Open `Delta.app`. The first time, macOS refuses and reports that the app "is
damaged and can't be opened". Nothing is wrong with the download: the project
has no Apple developer account, so the app is neither signed by an identified
developer nor notarized by Apple, and Gatekeeper, the macOS check for
downloaded apps, describes that as damage.

Clear the quarantine flag macOS put on the download; that is what triggers the
check. You only need this once per downloaded copy:

```bash
xattr -d com.apple.quarantine /Applications/Delta.app
```

If you put the app somewhere other than `Applications`, pass that path
instead (for example `~/Applications/Delta.app`). Then open `Delta.app`
normally.

If macOS instead says that Apple "could not verify" the app, it offers a way
through System Settings: dismiss the dialog (**Done**), go to **System
Settings → Privacy & Security**, click **Open Anyway** next to the message
that Delta was blocked, then open the app again and confirm with your
password. On macOS 14 and earlier, right-click `Delta.app` in `Applications`
and choose **Open** instead. The `xattr` command above works in this case
too.

## Updating

The app updates itself from the footer, where a newer release is shown next to
the version:

1. **Update** downloads the new `.dmg` into the data directory's `updates/`
   (`~/Library/Application Support/io.github.x7c1.delta/updates/`) and checks
   its sha256 against the one the release states. The footer then says
   **Update ready** and offers **Install**.
2. **Install** replaces `Delta.app` with the one in the new `.dmg`. No
   password is asked for: the app replaces itself as you. While it runs the
   footer says **Installing…**.
3. **Restart** appears once it is installed. The app closes and opens again
   as the new version. As when you quit the app, Claude Code sessions keep
   running in tmux across the restart and Codex sessions end (see
   [Quitting, relaunching and upgrading](README.md#quitting-relaunching-and-upgrading)).

The app checks the downloaded file and the `Delta.app` inside it again before
using them, and renames the old `Delta.app` aside rather than overwriting it;
the next launch removes it. The
[security guide](../security.md#macos) says what is checked and why.
A `.dmg` the app downloaded itself carries no quarantine flag, so the new
version opens without the Gatekeeper steps above.

### Where in-app updates work

The app can replace itself only when:

- it was **moved to `/Applications`** (or another folder) before it was
  started. A `Delta.app` opened straight from its `.dmg`, or from the
  Downloads folder without being moved, runs from a read-only copy macOS
  makes of it (App Translocation); replacing that copy would change nothing.
- **you can write to the folder `Delta.app` is in.** An administrator account
  can write to `/Applications`; a standard account usually cannot, and the
  app never asks for an administrator's password to do it.

The app checks both when it starts. Where either fails, it offers no
**Install**: once the update is downloaded, the footer says why (for a
read-only copy, to move Delta to `/Applications`) and shows the
[manual way](#updating-by-hand), which puts the new Delta in `Applications`.

### Updating by hand

When the app cannot install the update itself, or the install failed, the
footer shows the path of the downloaded `.dmg`, with a copy button and a link
to the release page; a failed install also offers a retry. Open that `.dmg`
(for example with **Go → Go to Folder…** in Finder and the copied path), quit
Delta (Finder does not replace an app that is open), drag `Delta.app` into
`Applications` over the old one, then start Delta again.

If the downloaded file fails the app's check (it does not match the release's
digest, or the `.dmg` does not hold Delta at that version), the app removes it
and the footer says **Verification failed · Download again**, with a link to
the release page; no path is offered for that file. Download it again from the
footer, or get the `.dmg` from the release page.

You can also download the new `.dmg` from the Release yourself and drag the
new `Delta.app` over the old one; clear the quarantine flag again as for the
first launch. The [install guide](README.md#updating) says what happens to
your data.
