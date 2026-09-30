# Install on macOS

The macOS bundle runs on Apple silicon (M1 and later); Intel Macs are not
supported. What the app needs on the host, where it keeps its data and how
updates behave are in the [install guide](README.md).

## Download and install

1. Install `tmux` if you do not have it: `brew install tmux`.
2. Open the [latest Release](https://github.com/x7c1/delta/releases/latest)
   and download `Delta_<version>_aarch64.dmg`.
3. Open the `.dmg` and drag `Delta.app` into `Applications`.

## First launch: getting past Gatekeeper

Open `Delta.app`. The first time, macOS refuses: it reports that the app "is
damaged and can't be opened" or that Apple "could not verify" it.

This happens because the project has no Apple developer account, so the app
is neither signed by an identified developer nor notarized by Apple, and
Gatekeeper, the macOS check for downloaded apps, blocks it. Either of the
following gets past the block; you only need it once per downloaded copy.

**Allow it in System Settings.** After the first refusal, dismiss the dialog
(**Done**), go to **System Settings → Privacy & Security**, find the message
that Delta was blocked near the bottom and click **Open Anyway**, then open
the app again and confirm **Open Anyway** with your password. This is the
path on macOS 15 (Sequoia) and later, where right-click (or Control-click) →
**Open** no longer gets past the block; on macOS 14 and earlier, right-click
`Delta.app` in `Applications`, choose **Open** and confirm **Open** in the
dialog. When macOS says the app is damaged, no Open Anyway button appears; use
the next workaround.

**Remove the quarantine flag.** macOS marks downloaded files with a quarantine
attribute, which is what triggers the check. Clearing it from the installed
app lets it open normally. This is the reliable fix when macOS says the app is
damaged:

```bash
xattr -d com.apple.quarantine /Applications/Delta.app
```

If you put the app somewhere other than `Applications`, pass that path
instead (for example `~/Applications/Delta.app`).

## Updating

Drag the new `Delta.app` from the new `.dmg` over the old one, then clear the
quarantine flag again (or allow it in System Settings again). The
[install guide](README.md#updating) says what happens to your data.
