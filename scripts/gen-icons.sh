#!/usr/bin/env bash
#
# gen-icons.sh — derive every app icon and the favicon from assets/icon/delta.svg.
#
# The SVG is the only source; everything this script writes is committed. It
# needs only tauri-cli (`cargo tauri icon` rasterises SVG itself, and
# `make desktop-build` already requires it), the shell and python3 (to put the
# .icns entries in a stable order). Where a variant is
# needed, the source is wrapped in a derived SVG in a temp directory and that
# is rasterised instead:
#
#   backend/crates/apps/delta-desktop/icons/      the installed app (tauri.conf.json)
#     32x32.png 128x128.png 128x128@2x.png icon.png icon.ico   the source as is
#     icon.icns                                   the source inset on a
#                                                 transparent canvas (see INSET_PERCENT)
#   backend/crates/apps/delta-desktop/icons-dev/  the dev build (scripts/dev.sh
#                                                 selects it through TAURI_CONFIG):
#                                                 the same set, from the source
#                                                 with the "D" badge
#   frontend/packages/apps/web/public/
#     favicon.svg                                 a copy of the source
#     favicon-32.png                              its 32 px raster
#
# Running it again rewrites the same files with the same bytes.
#
# Usage: scripts/gen-icons.sh     (the `make icons` entry point)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SOURCE="$REPO_ROOT/assets/icon/delta.svg"
DESKTOP_DIR="$REPO_ROOT/backend/crates/apps/delta-desktop"
INSTALLED_DIR="$DESKTOP_DIR/icons"
DEV_DIR="$DESKTOP_DIR/icons-dev"
WEB_PUBLIC_DIR="$REPO_ROOT/frontend/packages/apps/web/public"

# --- Geometry -----------------------------------------------------------------

# The source's canvas and the corner radius of its background (the `rx` of its
# <rect>). The script checks the source still says so, because the badge is
# placed from it.
SIZE=512
SOURCE_RX=114

# macOS draws a Dock icon as a rounded square at about 80 % of the canvas with
# transparent margins; a full-bleed icon looks larger than its neighbours. The
# `.icns` is therefore the source scaled to INSET_PERCENT and centred.
INSET_PERCENT=80

# The dev badge: a filled disc centred on the centre of the bottom-right corner
# arc, so it sits in the corner whatever the triangles do, ringed in the
# background colour to separate it from the shape underneath, with a bold "D"
# drawn as a path (no text element, so no font). Sizes are in source units; at
# 32 px one unit is 1/16 px, so the disc is about 13 px across and the "D"'s
# stroke about 2 px.
BADGE_R=108
BADGE_RING=16
BADGE_FILL="#E5532D"
BADGE_RING_COLOR="#F4F6F8"
GLYPH_COLOR="#FFFFFF"
GLYPH_H=136       # the "D"'s height
GLYPH_STROKE=34   # its stroke width (the bowl's inner radius is GLYPH_H/2 - this)
GLYPH_STEM=50     # the straight part of the "D" before the bowl starts

# --- Helpers ------------------------------------------------------------------

log() { printf '[icons] %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

[ $# -eq 0 ] || die "usage: scripts/gen-icons.sh (takes no arguments)"
[ -f "$SOURCE" ] || die "source icon not found: $SOURCE"
grep -q "rx=\"$SOURCE_RX\"" "$SOURCE" \
  || die "$SOURCE no longer has rx=\"$SOURCE_RX\"; update SOURCE_RX (the badge is placed from it)"
cargo tauri --version >/dev/null 2>&1 \
  || die "cargo tauri not found (one-time: cargo install tauri-cli --version '^2' --locked)"
command -v python3 >/dev/null 2>&1 || die "python3 not found (it puts the .icns entries in a stable order)"

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-gen-icons.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

# The source without any XML declaration, so it can be nested in a derived SVG
# as an inner <svg> element.
source_body() {
  sed '/^<?xml/d' "$1"
}

# Rasterise an SVG into a directory with `cargo tauri icon`. Its per-file
# progress goes to a log that is shown only when it fails.
rasterise() {
  local svg="$1" out="$2"
  shift 2
  mkdir -p "$out"
  if ! cargo tauri icon "$svg" -o "$out" "$@" >"$out.log" 2>&1; then
    cat "$out.log" >&2
    die "cargo tauri icon failed on $svg"
  fi
}

# Rewrite an .icns with its entries sorted by type. tauri-cli writes them in
# hash-map order, which differs between runs; sorting keeps a rerun from
# changing the committed file's bytes. The format is a header ("icns", total
# length) followed by entries of (4-byte type, 4-byte big-endian length
# including these 8 bytes, data).
sort_icns() {
  python3 - "$1" <<'PY'
import struct, sys
path = sys.argv[1]
data = open(path, "rb").read()
if data[:4] != b"icns":
    sys.exit(f"{path}: not an icns file")
entries, i = [], 8
while i < len(data):
    length = struct.unpack(">I", data[i + 4 : i + 8])[0]
    entries.append(data[i : i + length])
    i += length
body = b"".join(sorted(entries, key=lambda entry: entry[:4]))
open(path, "wb").write(b"icns" + struct.pack(">I", 8 + len(body)) + body)
PY
}

# --- Derived SVGs ---------------------------------------------------------------

# $1 scaled to INSET_PERCENT and centred on a transparent canvas of the same size.
write_inset_svg() {
  local in="$1" out="$2"
  local scale offset
  scale="$(awk "BEGIN { print $INSET_PERCENT / 100 }")"
  offset="$(awk "BEGIN { print $SIZE * (100 - $INSET_PERCENT) / 200 }")"
  cat >"$out" <<EOF
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 $SIZE $SIZE" width="$SIZE" height="$SIZE">
  <g transform="translate($offset $offset) scale($scale)">
$(source_body "$in")
  </g>
</svg>
EOF
}

# $1 with the dev badge over its bottom-right corner.
write_badged_svg() {
  local in="$1" out="$2"
  local cx=$((SIZE - SOURCE_RX)) cy=$((SIZE - SOURCE_RX))
  # The "D": an outer outline and an inner counter, filled evenodd. The bowl is
  # a half circle of radius GLYPH_H/2 after GLYPH_STEM of straight edge; the
  # glyph is centred on the disc.
  local r=$((GLYPH_H / 2)) ri=$((GLYPH_H / 2 - GLYPH_STROKE))
  local w=$((GLYPH_STEM + r))
  local x0=$((cx - w / 2)) y0=$((cy - r))
  local xb=$((x0 + GLYPH_STEM)) y1=$((cy + r))
  local xi=$((x0 + GLYPH_STROKE)) yi0=$((cy - ri)) yi1=$((cy + ri))
  cat >"$out" <<EOF
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 $SIZE $SIZE" width="$SIZE" height="$SIZE">
$(source_body "$in")
  <circle cx="$cx" cy="$cy" r="$BADGE_R" fill="$BADGE_FILL" stroke="$BADGE_RING_COLOR" stroke-width="$BADGE_RING"/>
  <path fill="$GLYPH_COLOR" fill-rule="evenodd" d="M$x0 $y0 H$xb A$r $r 0 0 1 $xb $y1 H$x0 Z M$xi $yi0 H$xb A$ri $ri 0 0 1 $xb $yi1 H$xi Z"/>
</svg>
EOF
}

# --- Icon sets ------------------------------------------------------------------

# The files a set keeps from `cargo tauri icon`'s output (the rest — Windows
# Store logos, android/, ios/, 64x64.png — is not used by the bundle).
SET_PNGS=(32x32.png 128x128.png 128x128@2x.png icon.png)

# Write one desktop icon set into $3: the PNGs and the .ico from the SVG $1,
# the .icns from the inset variant of it. $2 names the set in the work dir.
write_set() {
  local svg="$1" name="$2" dest="$3"
  local full="$WORK_DIR/$name-full" inset_svg="$WORK_DIR/$name-inset.svg"
  local inset="$WORK_DIR/$name-inset"
  rasterise "$svg" "$full"
  write_inset_svg "$svg" "$inset_svg"
  rasterise "$inset_svg" "$inset"
  mkdir -p "$dest"
  local file
  for file in "${SET_PNGS[@]}" icon.ico; do
    cp "$full/$file" "$dest/$file"
    log "wrote ${dest#"$REPO_ROOT"/}/$file"
  done
  sort_icns "$inset/icon.icns"
  cp "$inset/icon.icns" "$dest/icon.icns"
  log "wrote ${dest#"$REPO_ROOT"/}/icon.icns (inset to $INSET_PERCENT %)"
}

write_set "$SOURCE" installed "$INSTALLED_DIR"

write_badged_svg "$SOURCE" "$WORK_DIR/dev.svg"
write_set "$WORK_DIR/dev.svg" dev "$DEV_DIR"

# --- Favicon ------------------------------------------------------------------

mkdir -p "$WEB_PUBLIC_DIR"
cp "$SOURCE" "$WEB_PUBLIC_DIR/favicon.svg"
log "wrote ${WEB_PUBLIC_DIR#"$REPO_ROOT"/}/favicon.svg"
rasterise "$SOURCE" "$WORK_DIR/favicon" -p 32
cp "$WORK_DIR/favicon/32x32.png" "$WEB_PUBLIC_DIR/favicon-32.png"
log "wrote ${WEB_PUBLIC_DIR#"$REPO_ROOT"/}/favicon-32.png"
