#!/usr/bin/env bash
#
# vendor-codex-schema.sh — re-vendor the `codex app-server` JSON Schema, and
# check the vendored copy is in the normal form this repo pins.
#
# The vendored schema under
# backend/crates/gateway/codex-agent/vendor/app-server-schema/ is the
# ground-truth reference Delta's Codex wire types are reconciled against (see
# that directory's README.md). This script owns both halves of keeping it
# honest:
#
#   re-vendor (default)  run the generator into a temp directory, keep exactly
#                        the outputs the README's "Files" section lists,
#                        normalise each of them, and replace the vendored copy
#                        — dropping vendored files the generator no longer
#                        emits. README.md is never touched.
#   --check              verify every vendored .json file is byte-identical to
#                        `jq -S -j .` of itself. Reads the committed files
#                        only: no generator, no codex binary, no network.
#
# The normal form is `jq -S -j .` — sorted keys, two-space indent, no trailing
# newline. The generator's own output order is NOT stable (the same Codex
# version can emit the same definitions in a different order from one run to
# the next), so vendoring its bytes verbatim buries the real changes of a
# re-vendor in reordering noise. Sorting the keys makes a re-vendor diff show
# what actually changed. The drift canary
# (`vendored_schema_matches_the_real_generator`) compares structure rather than
# bytes, so the normalisation is invisible to it.
#
# Usage:
#   scripts/vendor-codex-schema.sh            # re-vendor from the installed codex
#   scripts/vendor-codex-schema.sh --check    # verify the vendored normal form
#   scripts/vendor-codex-schema.sh --help
#
# Environment:
#   DELTA_CODEX_BIN            codex binary to generate with (default: `codex`)
#   DELTA_VENDOR_SCHEMA_DIR    target directory (default: the vendored one).
#                              Testing-only override — it is what lets the
#                              stub-generator test exercise the re-vendor path
#                              without writing to the real vendored files.
#
# Prerequisites: `jq` for both modes; the pinned Codex CLI for the re-vendor.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CODEX_AGENT_DIR="$REPO_ROOT/backend/crates/gateway/codex-agent"
VENDOR_DIR="${DELTA_VENDOR_SCHEMA_DIR:-$CODEX_AGENT_DIR/vendor/app-server-schema}"
# A trailing slash on the override would defeat the `${file#"$VENDOR_DIR"/}`
# prefix strip below, leaving every path unmatched against the selection and so
# swept away as stale — i.e. it would empty the target directory.
VENDOR_DIR="${VENDOR_DIR%/}"
PIN_SOURCE="$CODEX_AGENT_DIR/src/schema.rs"
CODEX_BIN="${DELTA_CODEX_BIN:-codex}"

log() { printf '\033[1;36m[vendor-codex-schema]\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m[vendor-codex-schema]\033[0m %s\n' "$*" >&2; exit 1; }

# The generator's top-level outputs that are vendored, i.e. the "Files" section
# of the vendored README minus `v2/*.json` (which is taken wholesale). The rest
# of the generator's output — the v1 stub and every other loose top-level
# per-type file — is deliberately dropped: v1 describes only the legacy
# `initialize` handshake, and the other per-type files are superseded by the
# combined documents.
TOP_LEVEL_FILES="
codex_app_server_protocol.schemas.json
codex_app_server_protocol.v2.schemas.json
ServerRequest.json
CommandExecutionRequestApprovalParams.json
CommandExecutionRequestApprovalResponse.json
FileChangeRequestApprovalParams.json
FileChangeRequestApprovalResponse.json
PermissionsRequestApprovalParams.json
PermissionsRequestApprovalResponse.json
"

usage() {
  # Print the leading comment block (everything up to the first blank,
  # non-comment line), stripping the leading "# " — same idiom as dev.sh, so
  # the header above is the only copy of this script's interface docs.
  awk 'NR>1 { if ($0 !~ /^#/) exit; sub(/^# ?/, ""); print }' "${BASH_SOURCE[0]}"
}

require_jq() {
  command -v jq >/dev/null 2>&1 ||
    die "jq not found on PATH — it is what defines the vendored normal form (\`jq -S -j .\`)"
}

# Paths are reported relative to the repo root where possible: absolute temp
# paths in a failure list are noise.
relative_to_repo() { printf '%s\n' "${1#"$REPO_ROOT"/}"; }

# Lists every vendored .json file, one per line, in a stable order.
vendored_json_files() {
  find "$VENDOR_DIR" -type f -name '*.json' | LC_ALL=C sort
}

# --- --check: the vendored files are their own `jq -S -j .` ---------------------

check_mode() {
  require_jq
  [ -d "$VENDOR_DIR" ] || die "vendored schema directory not found: $VENDOR_DIR"

  files="$(vendored_json_files)"
  # An empty directory would otherwise pass vacuously, which is the one
  # "green" this check must never report.
  [ -n "$files" ] || die "no .json files under $(relative_to_repo "$VENDOR_DIR") — nothing to check"

  work="$(mktemp -d "${TMPDIR:-/tmp}/delta-vendor-check.XXXXXX")"
  trap 'rm -rf "$work"' EXIT

  checked=0
  offenders=""
  while IFS= read -r file; do
    checked=$((checked + 1))
    if ! jq -S -j . "$file" >"$work/normalised" 2>"$work/jq-error"; then
      offenders="$offenders  $(relative_to_repo "$file") — not valid JSON: $(tr '\n' ' ' <"$work/jq-error")
"
    elif ! cmp -s "$work/normalised" "$file"; then
      offenders="$offenders  $(relative_to_repo "$file") — not in the normal form
"
    fi
  done <<EOF
$files
EOF

  if [ -n "$offenders" ]; then
    printf '%s\n' "$offenders" >&2
    die "the vendored schema is not key-sorted; re-vendor with \`make vendor-codex-schema\` (or normalise in place with \`jq -S -j . <file>\`)"
  fi

  log "$checked vendored file(s) are in the normal form (jq -S -j .)"
}

# --- default: regenerate, select, normalise, replace ---------------------------

# Prints the pinned Codex version recorded in schema.rs, or nothing if it
# cannot be read (the pin is only used for an advisory reminder).
pinned_version() {
  [ -f "$PIN_SOURCE" ] || return 0
  sed -n 's/^pub const VENDORED_CODEX_VERSION: &str = "\(.*\)";$/\1/p' "$PIN_SOURCE" | head -n 1
}

vendor_mode() {
  require_jq
  command -v "$CODEX_BIN" >/dev/null 2>&1 ||
    die "codex binary not found: $CODEX_BIN (install the pinned Codex CLI, or point DELTA_CODEX_BIN at it)"

  version="$("$CODEX_BIN" --version 2>/dev/null | head -n 1)" ||
    die "\`$CODEX_BIN --version\` failed"
  [ -n "$version" ] || die "\`$CODEX_BIN --version\` printed nothing"
  log "generator: $version"

  work="$(mktemp -d "${TMPDIR:-/tmp}/delta-vendor-codex.XXXXXX")"
  trap 'rm -rf "$work"' EXIT
  generated="$work/generated"
  mkdir -p "$generated"

  # Offline and auth-free: the generator is a static dump of the compiled-in
  # schema.
  log "Generating the app-server schema ..."
  "$CODEX_BIN" app-server generate-json-schema --out "$generated" >"$work/gen.log" 2>&1 ||
    { cat "$work/gen.log" >&2; die "\`$CODEX_BIN app-server generate-json-schema\` failed"; }

  # The selection, as paths relative to the vendored directory. A missing
  # top-level file is drift worth stopping on, not something to silently drop:
  # every one of them is a document Delta reconciles against.
  selected="$work/selected"
  : >"$selected"
  for name in $TOP_LEVEL_FILES; do
    [ -f "$generated/$name" ] ||
      die "the generator did not emit $name — the vendored selection no longer matches this Codex; reconcile the README's \"Files\" section and this script before re-vendoring"
    printf '%s\n' "$name" >>"$selected"
  done

  v2_files="$(find "$generated/v2" -maxdepth 1 -type f -name '*.json' 2>/dev/null | LC_ALL=C sort || true)"
  [ -n "$v2_files" ] || die "the generator emitted no v2/*.json — refusing to wipe the vendored v2 directory"
  while IFS= read -r file; do
    printf 'v2/%s\n' "${file##*/}" >>"$selected"
  done <<EOF
$v2_files
EOF

  mkdir -p "$VENDOR_DIR/v2"
  written=0
  while IFS= read -r relative; do
    destination="$VENDOR_DIR/$relative"
    jq -S -j . "$generated/$relative" >"$work/normalised" ||
      die "the generator's $relative is not valid JSON"
    # Written through a temp file so an interrupted run cannot leave a
    # half-written file behind in the vendored tree.
    mv "$work/normalised" "$destination"
    written=$((written + 1))
  done <"$selected"

  # Anything vendored that the generator no longer emits is stale. Only .json
  # files are considered, so README.md survives.
  removed=0
  existing="$(vendored_json_files)"
  if [ -n "$existing" ]; then
    while IFS= read -r file; do
      relative="${file#"$VENDOR_DIR"/}"
      if ! grep -qxF "$relative" "$selected"; then
        rm -f "$file"
        removed=$((removed + 1))
        log "removed stale $relative"
      fi
    done <<EOF
$existing
EOF
  fi

  log "vendored $written file(s) into $(relative_to_repo "$VENDOR_DIR"), removed $removed stale file(s)"

  pin="$(pinned_version)"
  if [ -z "$pin" ]; then
    log "NOTE: could not read VENDORED_CODEX_VERSION from $(relative_to_repo "$PIN_SOURCE"); check the version pin by hand"
  else
    case "$version" in
      *"$pin"*)
        log "the generator matches the pinned version ($pin); no pin update needed"
        ;;
      *)
        log "NOTE: the generator is not the pinned version ($pin). Bump VENDORED_CODEX_VERSION in $(relative_to_repo "$PIN_SOURCE") and the version-pin table in $(relative_to_repo "$VENDOR_DIR")/README.md in this same change."
        ;;
    esac
  fi
}

# --- Entry point ---------------------------------------------------------------

case "${1:-}" in
  "")
    vendor_mode
    ;;
  --check)
    [ $# -eq 1 ] || die "--check takes no further arguments"
    check_mode
    ;;
  -h | --help)
    usage
    ;;
  *)
    die "unknown argument: $1 (try --help)"
    ;;
esac
