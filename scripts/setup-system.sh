#!/usr/bin/env bash
set -euo pipefail

# Install the system packages `make check` needs on Debian/Ubuntu.
#
# Usage: scripts/setup-system.sh   (from any working directory)
#
# Installs, with apt-get:
#   - the tools the dev scripts and the suites drive (tmux, lsof, jq, make)
#   - the system libraries Tauri v2 links against, which the desktop shell's
#     build (`check-desktop-build`, `check-backend-doc`, `make desktop*`) needs
# and then, with `npx playwright install --with-deps`, the Playwright Chromium
# and its headless shell (plus the libraries they load) at the version
# frontend/pnpm-lock.yaml pins for @playwright/test, which `check-e2e` and
# `check-e2e-fake` launch. npx fetches that exact Playwright, so this works
# before `pnpm install` has run; it needs Node.js (with npx) on PATH. The
# browsers land in Playwright's default cache, or in PLAYWRIGHT_BROWSERS_PATH
# when it is set.
#
# Runs apt-get through sudo unless already root. Safe to run again: apt
# packages already installed are skipped, and Playwright skips a browser it
# already has.

# The package list lives here only; the development guide points to this
# script rather than repeating it.
TOOL_PACKAGES="tmux lsof jq make"
TAURI_PACKAGES="libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf build-essential file"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOCKFILE="$SCRIPT_DIR/../frontend/pnpm-lock.yaml"

die() {
    printf 'setup-system: %s\n' "$1" >&2
    exit 1
}

if ! command -v apt-get >/dev/null 2>&1; then
    die "apt-get not found; this script supports Debian/Ubuntu only (see docs/guides/development/README.md for other platforms)"
fi

if [ "$(id -u)" = 0 ]; then
    as_root() { "$@"; }
else
    command -v sudo >/dev/null 2>&1 || die "not running as root and sudo not found; rerun as root"
    as_root() { sudo "$@"; }
fi

# Prints the @playwright/test version the lockfile pins. The `packages:` and
# `snapshots:` sections each key it as `'@playwright/test@<version>':`.
playwright_version() {
    [ -f "$LOCKFILE" ] || die "lockfile not found: $LOCKFILE"
    local versions count
    versions="$(sed -n "s/^  '@playwright\/test@\([^'(]*\)':\$/\1/p" "$LOCKFILE" | sort -u)"
    count="$(printf '%s' "$versions" | grep -c . || true)"
    [ "$count" = 1 ] || die "expected one @playwright/test version in $LOCKFILE, found ${count}: $(printf '%s' "$versions" | tr '\n' ' ')"
    printf '%s\n' "$versions"
}

# Resolve the version before installing anything, so a malformed lockfile
# fails fast.
PLAYWRIGHT_VERSION="$(playwright_version)"

missing=""
# shellcheck disable=SC2086 # the package lists are space-separated on purpose
for pkg in $TOOL_PACKAGES $TAURI_PACKAGES; do
    status="$(dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null || true)"
    [ "$status" = "install ok installed" ] || missing="$missing $pkg"
done

if [ -n "$missing" ]; then
    printf 'setup-system: installing apt packages:%s\n' "$missing"
    as_root env DEBIAN_FRONTEND=noninteractive apt-get update
    # shellcheck disable=SC2086 # one argument per package
    as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y $missing
else
    printf 'setup-system: apt packages already installed\n'
fi

command -v npx >/dev/null 2>&1 || die "npx not found; install Node.js (it ships npx), then rerun"

printf 'setup-system: installing Playwright %s browsers into %s\n' \
    "$PLAYWRIGHT_VERSION" "${PLAYWRIGHT_BROWSERS_PATH:-the default ~/.cache/ms-playwright}"
# Playwright runs its own apt-get for the browsers' libraries, through sudo
# when not root.
npx -y "playwright@$PLAYWRIGHT_VERSION" install --with-deps chromium chromium-headless-shell

printf 'setup-system: done\n'
