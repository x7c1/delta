#!/usr/bin/env bash
#
# gen-check.sh — fail when the @delta/wire-gen bindings on disk are not what
# the Rust wire contract generates now.
#
# The bindings are generated into a scratch directory and compared with
# frontend/packages/gateway/wire-gen/src/generated/ via `diff -r`. The working
# tree is never written and git is never consulted: the question is "do the
# files on disk match the contract?", which holds before a commit as well as
# after one. On CI's clean checkout the files on disk are HEAD's, so the same
# comparison also catches bindings that were regenerated but never committed.
#
# Usage: scripts/gen-check.sh     (the `make gen-check` entry point)
#
# Test seams (used by scripts/tests/gen-check.test.sh; unset in normal use):
#   DELTA_WIRE_GEN_DIR  the bindings directory to compare against
#   DELTA_EXPORT_TS     an executable run as `$DELTA_EXPORT_TS <out-dir>` in place
#                       of `cargo run -p delta-wire --bin export-ts -- <out-dir>`

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
BINDINGS_DIR="${DELTA_WIRE_GEN_DIR:-$REPO_ROOT/frontend/packages/gateway/wire-gen/src/generated}"

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

[ $# -eq 0 ] || die "usage: scripts/gen-check.sh (takes no arguments)"

[ -d "$BINDINGS_DIR" ] || die "bindings directory not found: $BINDINGS_DIR"

SCRATCH_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-gen-check.XXXXXX")"
trap 'rm -rf "$SCRATCH_DIR"' EXIT
FRESH_DIR="$SCRATCH_DIR/generated"

generate() {
  if [ -n "${DELTA_EXPORT_TS:-}" ]; then
    "$DELTA_EXPORT_TS" "$FRESH_DIR"
  else
    (cd "$REPO_ROOT/backend" && cargo run --quiet -p delta-wire --bin export-ts -- "$FRESH_DIR")
  fi
}
generate || die "generating the wire bindings failed"
[ -d "$FRESH_DIR" ] || die "the generator wrote nothing to $FRESH_DIR"

# `diff` exits 1 on a difference and >1 on trouble (an unreadable file); only
# the former is "stale", the latter must not masquerade as it.
DIFF_OUT="$SCRATCH_DIR/diff"
status=0
diff -ru "$BINDINGS_DIR" "$FRESH_DIR" >"$DIFF_OUT" || status=$?
cat "$DIFF_OUT"
case "$status" in
  0) echo "generated wire bindings are fresh" ;;
  1)
    echo "error: generated wire bindings are stale — run 'make gen' and commit the result" >&2
    # `make gen` only writes; it never removes a binding the contract dropped.
    if grep -q "^Only in $BINDINGS_DIR" "$DIFF_OUT"; then
      echo "       (a file 'Only in' the bindings directory is no longer generated: delete it)" >&2
    fi
    exit 1
    ;;
  *) die "diff failed (exit $status) comparing $BINDINGS_DIR with freshly generated bindings" ;;
esac
