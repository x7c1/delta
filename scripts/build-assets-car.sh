#!/usr/bin/env bash
#
# build-assets-car.sh — compile the installed app's Icon Composer icon
# (backend/crates/apps/delta-desktop/icons/icon.icon, written by
# `make icons`) into the Assets.car the macOS bundle carries.
#
# macOS 26 draws the tile of an app whose icon is only an .icns with a glass
# finish of its own, which blurs the triangles' edges; an app with an
# Assets.car built from an Icon Composer icon is drawn as the icon describes.
# The Tauri CLI can compile icon.icon itself, but its actool call fails when
# the CLI runs under tauri-action on CI (an NSPlaceholderArray exception with
# Xcode 26, "Bad file descriptor" with Xcode 27; tauri-apps/tauri#15315 and
# #16386), while the same call from a plain shell succeeds. So the bundle is
# given this prebuilt Assets.car instead (tauri.macos.conf.json lists it in
# bundle.icon), and the Tauri CLI only copies it.
#
# Nothing fails once those issues are fixed, so check them. When the Tauri
# CLI's actool call succeeds under tauri-action on CI, drop the workaround:
# - delete this script, its call (and comment) in the Makefile's
#   desktop-build target, and its path filter and "Build the app's
#   Assets.car" step in .github/workflows/bundle.yml;
# - delete backend/crates/apps/delta-desktop/tauri.macos.conf.json and list
#   icons/icon.icon in bundle.icon of the tauri.conf.json beside it, so that
#   the Tauri CLI compiles it itself;
# - update the icon paragraph of docs/guides/development/README.md and the
#   comment above write_icon_composer in scripts/gen-icons.sh, which both
#   name this script.
# Keep the workflow's "Check the app's Icon Composer icon" step, which then
# checks the CLI's Assets.car.
#
# actool hands the work to a long-lived ibtoold daemon, which a broken earlier
# call can leave unusable; the daemon is stopped first, and actool reads from
# /dev/null rather than whatever stdin the caller has.
#
# Needs macOS with Xcode 26 or later. Writes backend/target/icon-composer/Assets.car.
#
# Usage: scripts/build-assets-car.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
readonly ICON="$REPO_ROOT/backend/crates/apps/delta-desktop/icons/icon.icon"
readonly OUT_DIR="$REPO_ROOT/backend/target/icon-composer"
# The name tauri-bundler reads back from the Assets.car for CFBundleIconName.
readonly APP_ICON_NAME=Icon

die() { printf 'build-assets-car: %s\n' "$*" >&2; exit 1; }

[ $# -eq 0 ] || die "usage: scripts/build-assets-car.sh (takes no arguments)"
[ "$(uname -s)" = Darwin ] || die "needs macOS (actool ships with Xcode)"
[ -d "$ICON" ] || die "$ICON not found; run make icons"
command -v xcrun >/dev/null 2>&1 || die "xcrun not found; install Xcode 26 or later"

version="$(xcrun actool --version --output-format human-readable-text </dev/null \
  | sed -n 's/^short-bundle-version: *//p')"
[ -n "$version" ] || die "could not read actool's version"
[ "${version%%.*}" -ge 26 ] || die "actool $version is older than 26; install Xcode 26 or later"

work="$(mktemp -d "${TMPDIR:-/tmp}/delta-assets-car.XXXXXX")"
trap 'rm -rf "$work"' EXIT
cp -R "$ICON" "$work/Icon.icon"
mkdir -p "$work/out"

pkill -x ibtoold 2>/dev/null || true
if ! xcrun actool "$work/Icon.icon" --compile "$work/out" \
    --output-format human-readable-text --notices --warnings \
    --output-partial-info-plist "$work/out/assetcatalog_generated_info.plist" \
    --app-icon "$APP_ICON_NAME" --include-all-app-icons \
    --enable-on-demand-resources NO --development-region en \
    --target-device mac --minimum-deployment-target 26.0 --platform macosx \
    </dev/null >"$work/actool.log" 2>&1; then
  cat "$work/actool.log" >&2
  die "actool failed"
fi
# actool reports some failures in its output and still exits 0.
if grep -q 'com.apple.actool.errors' "$work/actool.log" || [ ! -f "$work/out/Assets.car" ]; then
  cat "$work/actool.log" >&2
  die "actool did not write Assets.car"
fi

mkdir -p "$OUT_DIR"
cp "$work/out/Assets.car" "$OUT_DIR/Assets.car"
printf 'build-assets-car: wrote %s (actool %s)\n' "${OUT_DIR#"$REPO_ROOT"/}/Assets.car" "$version"
