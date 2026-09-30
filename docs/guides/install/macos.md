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

Drag the new `Delta.app` from the new `.dmg` over the old one, then clear the
quarantine flag again. The
[install guide](README.md#updating) says what happens to your data.
