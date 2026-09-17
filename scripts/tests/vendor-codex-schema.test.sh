#!/usr/bin/env bash
#
# Tests for scripts/vendor-codex-schema.sh — both modes, against a stub.
#
# Usage: bash scripts/tests/vendor-codex-schema.test.sh
#
# The re-vendor path must be exercised on every host, including the ones with
# no Codex CLI at all or one that is not the pinned version — running the real
# generator here would rewrite the vendored schema against whatever Codex
# happens to be installed. So both of the script's escape hatches are used:
# DELTA_CODEX_BIN points at a stub generator that copies a tiny fixture tree
# (unsorted keys, a v1 directory and an unlisted top-level file that must both
# be dropped), and DELTA_VENDOR_SCHEMA_DIR points at a throwaway directory
# seeded with a stale file and a README the re-vendor must leave alone. Nothing
# under backend/ is touched, and the whole file needs only bash, coreutils and
# jq.
#
# The script exits non-zero on the first failed assertion: a test that limps to
# the end reporting "3 failures" is a test nobody reads.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENDOR_SCRIPT="$(cd "$SCRIPT_DIR/.." && pwd)/vendor-codex-schema.sh"

[ -x "$VENDOR_SCRIPT" ] || {
  printf 'FAIL - vendor script is not executable: %s\n' "$VENDOR_SCRIPT" >&2
  exit 1
}
command -v jq >/dev/null 2>&1 || {
  printf 'FAIL - jq not found on PATH; it defines the normal form under test\n' >&2
  exit 1
}

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-vendor-test.XXXXXX")"
trap 'rm -rf "$WORK_DIR"' EXIT

pass() { printf 'ok   - %s\n' "$1"; }

fail() {
  printf 'FAIL - %s\n' "$1" >&2
  [ $# -gt 1 ] && printf '%s\n' "$2" >&2
  exit 1
}

assert_eq() {
  if [ "$2" = "$3" ]; then
    pass "$1"
  else
    fail "$1" "  expected: $(printf '%q' "$2")
  actual:   $(printf '%q' "$3")"
  fi
}

assert_contains() {
  case "$2" in
    *"$3"*) pass "$1" ;;
    *) fail "$1" "  expected to find $(printf '%q' "$3") in:
$2" ;;
  esac
}

assert_lacks() {
  case "$2" in
    *"$3"*) fail "$1" "  expected NOT to find $(printf '%q' "$3") in:
$2" ;;
    *) pass "$1" ;;
  esac
}

assert_file_missing() {
  if [ -e "$2" ]; then
    fail "$1" "  unexpectedly exists: $2"
  else
    pass "$1"
  fi
}

# --- The fixture the stub generator emits. --------------------------------------

FIXTURE="$WORK_DIR/fixture"
mkdir -p "$FIXTURE/v1" "$FIXTURE/v2"

# Every top-level output the vendored selection lists. Keys are deliberately
# out of order so the assertions below prove the sorting really happened.
for name in \
  codex_app_server_protocol.schemas.json \
  codex_app_server_protocol.v2.schemas.json \
  ServerRequest.json \
  CommandExecutionRequestApprovalParams.json \
  CommandExecutionRequestApprovalResponse.json \
  FileChangeRequestApprovalParams.json \
  FileChangeRequestApprovalResponse.json \
  PermissionsRequestApprovalParams.json \
  PermissionsRequestApprovalResponse.json; do
  printf '{"title": "%s", "definitions": {"zeta": 1, "alpha": 2}}' "${name%.json}" \
    >"$FIXTURE/$name"
done

# Outputs that must NOT be vendored: the legacy v1 stub and the loose top-level
# per-type files other than the server-request/approval ones.
printf '{"title": "InitializeParams"}' >"$FIXTURE/v1/InitializeParams.json"
printf '{"title": "InitializeResponse"}' >"$FIXTURE/v1/InitializeResponse.json"
printf '{"title": "ClientRequest"}' >"$FIXTURE/ClientRequest.json"
printf '{"title": "ExecCommandApprovalParams"}' >"$FIXTURE/ExecCommandApprovalParams.json"

cat >"$FIXTURE/v2/ThreadStartParams.json" <<'EOF'
{
  "title": "ThreadStartParams",
  "properties": { "cwd": { "type": "string" }, "approvalPolicy": { "type": "string" } },
  "$schema": "https://json-schema.org/draft/2020-12/schema"
}
EOF
printf '{"title": "TurnStartParams"}' >"$FIXTURE/v2/TurnStartParams.json"

# What `jq -S -j .` makes of the fixture above: sorted keys, two-space indent,
# no trailing newline. Spelled out rather than computed with jq, so the test
# pins the normal form instead of restating the implementation.
EXPECTED_THREAD_START="$WORK_DIR/expected-ThreadStartParams.json"
printf '%s' '{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "properties": {
    "approvalPolicy": {
      "type": "string"
    },
    "cwd": {
      "type": "string"
    }
  },
  "title": "ThreadStartParams"
}' >"$EXPECTED_THREAD_START"

# --- The stub generator. --------------------------------------------------------

BIN_DIR="$WORK_DIR/bin"
mkdir -p "$BIN_DIR"
printf 'codex-cli 9.9.9-stub\n' >"$WORK_DIR/stub-version"
: >"$WORK_DIR/omit"

# Copies the fixture tree to --out, minus whatever $WORK_DIR/omit lists — that
# file is how a test makes the generator "stop emitting" an output.
cat >"$BIN_DIR/codex" <<EOF
#!/usr/bin/env bash
set -euo pipefail
if [ "\${1:-}" = "--version" ]; then
  cat "$WORK_DIR/stub-version"
  exit 0
fi
out=""
while [ \$# -gt 0 ]; do
  case "\$1" in
    --out) out="\${2:-}"; shift 2 ;;
    *) shift ;;
  esac
done
[ -n "\$out" ] || { echo "stub generator: no --out given" >&2; exit 2; }
mkdir -p "\$out"
cp -R "$FIXTURE/." "\$out/"
while IFS= read -r relative; do
  [ -n "\$relative" ] && rm -rf "\$out/\$relative"
done <"$WORK_DIR/omit"
exit 0
EOF
chmod +x "$BIN_DIR/codex"

# --- Harness. -------------------------------------------------------------------

VENDOR_DIR="$WORK_DIR/vendor"
OUT=""
STATUS=0

# Seeds the throwaway target directory with what a previous re-vendor left:
# a README (must survive), a file the generator still emits (must be replaced),
# and two files it no longer emits (must be removed).
reset_vendor_dir() {
  rm -rf "$VENDOR_DIR"
  mkdir -p "$VENDOR_DIR/v2"
  printf '# Vendored schema README — must survive a re-vendor.\n' >"$VENDOR_DIR/README.md"
  printf '{"stale": true}' >"$VENDOR_DIR/ServerRequest.json"
  printf '{"title": "GoneInThisVersionParams"}' >"$VENDOR_DIR/v2/GoneInThisVersionParams.json"
  printf '{"title": "ClientRequest"}' >"$VENDOR_DIR/ClientRequest.json"
  : >"$WORK_DIR/omit"
}

# Runs the script with the stub generator and the throwaway target. Extra
# arguments are forwarded to the script.
run_vendor() {
  STATUS=0
  OUT="$(
    env "DELTA_CODEX_BIN=$BIN_DIR/codex" \
      "DELTA_VENDOR_SCHEMA_DIR=$VENDOR_DIR" \
      "$VENDOR_SCRIPT" "$@" 2>&1
  )" || STATUS=$?
}

# What a successful re-vendor leaves behind: the selection, plus the README the
# script never touches.
EXPECTED_TREE="CommandExecutionRequestApprovalParams.json CommandExecutionRequestApprovalResponse.json FileChangeRequestApprovalParams.json FileChangeRequestApprovalResponse.json PermissionsRequestApprovalParams.json PermissionsRequestApprovalResponse.json README.md ServerRequest.json codex_app_server_protocol.schemas.json codex_app_server_protocol.v2.schemas.json v2/ThreadStartParams.json v2/TurnStartParams.json"

# Every file in the target directory, relative and space-separated.
vendored_tree() {
  (cd "$VENDOR_DIR" && find . -type f | sed -e 's|^\./||' | LC_ALL=C sort | tr '\n' ' ' | sed -e 's/ $//')
}

# --- Re-vendoring from the stub generator. --------------------------------------

reset_vendor_dir
run_vendor
assert_eq "a re-vendor from the stub exits 0" 0 "$STATUS"
assert_contains "the run prints the generator's version" "$OUT" "generator: codex-cli 9.9.9-stub"
assert_eq "exactly the listed outputs are vendored, and nothing else" \
  "$EXPECTED_TREE" "$(vendored_tree)"
assert_file_missing "the v1 stub is not vendored" "$VENDOR_DIR/v1"
assert_file_missing "an unlisted top-level generator output is not vendored" \
  "$VENDOR_DIR/ExecCommandApprovalParams.json"
assert_file_missing "a stale vendored file the generator no longer emits is removed" \
  "$VENDOR_DIR/v2/GoneInThisVersionParams.json"
assert_file_missing "a vendored file that is no longer in the selection is removed" \
  "$VENDOR_DIR/ClientRequest.json"
assert_contains "the removal is reported" "$OUT" "removed stale v2/GoneInThisVersionParams.json"
assert_eq "the README is left alone" \
  "# Vendored schema README — must survive a re-vendor." \
  "$(cat "$VENDOR_DIR/README.md")"

if cmp -s "$EXPECTED_THREAD_START" "$VENDOR_DIR/v2/ThreadStartParams.json"; then
  pass "the vendored file is the key-sorted, newline-free normal form of the generator's output"
else
  fail "the vendored file is the key-sorted, newline-free normal form of the generator's output" \
    "$(diff "$EXPECTED_THREAD_START" "$VENDOR_DIR/v2/ThreadStartParams.json" || true)"
fi

assert_eq "a file the generator still emits is replaced, not kept" \
  '{
  "definitions": {
    "alpha": 2,
    "zeta": 1
  },
  "title": "ServerRequest"
}' "$(cat "$VENDOR_DIR/ServerRequest.json")"

# The stub is not the pinned Codex, so the run must say so rather than let a
# version-skewed re-vendor look finished.
assert_contains "a generator that is not the pinned version asks for a pin bump" "$OUT" \
  "Bump VENDORED_CODEX_VERSION"

# --- --check on what the re-vendor produced. ------------------------------------

run_vendor --check
assert_eq "--check passes on freshly re-vendored files" 0 "$STATUS"
assert_contains "--check reports how many files it checked" "$OUT" \
  "11 vendored file(s) are in the normal form"

# --- --check catches a file that is not in the normal form. ---------------------

printf '{"title": "TurnStartParams", "b": 1, "a": 2}' >"$VENDOR_DIR/v2/TurnStartParams.json"
run_vendor --check
assert_eq "--check fails on an unsorted file" 1 "$STATUS"
assert_contains "--check names the offending file" "$OUT" \
  "v2/TurnStartParams.json — not in the normal form"
assert_contains "--check points at the re-vendor target" "$OUT" "make vendor-codex-schema"

printf 'not json at all' >"$VENDOR_DIR/v2/TurnStartParams.json"
run_vendor --check
assert_eq "--check fails on a file that is not JSON" 1 "$STATUS"
assert_contains "--check says why the file is rejected" "$OUT" "not valid JSON"

# A green on an empty directory would be the one false pass that matters.
rm -rf "$VENDOR_DIR"
mkdir -p "$VENDOR_DIR"
run_vendor --check
assert_eq "--check fails when there is nothing to check" 1 "$STATUS"
assert_contains "--check says the directory is empty" "$OUT" "nothing to check"

# --- The generator dropping a listed output is drift, not a silent removal. -----

reset_vendor_dir
printf 'ServerRequest.json\n' >"$WORK_DIR/omit"
run_vendor
assert_eq "a generator missing a listed output fails the re-vendor" 1 "$STATUS"
assert_contains "the missing output is named" "$OUT" \
  "the generator did not emit ServerRequest.json"

reset_vendor_dir
printf 'v2\n' >"$WORK_DIR/omit"
run_vendor
assert_eq "a generator emitting no v2 files fails the re-vendor" 1 "$STATUS"
assert_contains "the empty v2 output is reported" "$OUT" "emitted no v2/*.json"
assert_eq "the vendored v2 directory is left intact" \
  '{"title": "GoneInThisVersionParams"}' \
  "$(cat "$VENDOR_DIR/v2/GoneInThisVersionParams.json")"

# --- A target directory given with a trailing slash. ----------------------------

# The path is used both to write the files and to recognise them again when the
# stale ones are swept; a trailing slash that reached the second use would take
# the freshly written files with it.
reset_vendor_dir
STATUS=0
OUT="$(
  env "DELTA_CODEX_BIN=$BIN_DIR/codex" \
    "DELTA_VENDOR_SCHEMA_DIR=$VENDOR_DIR/" \
    "$VENDOR_SCRIPT" 2>&1
)" || STATUS=$?
assert_eq "a target directory with a trailing slash re-vendors fine" 0 "$STATUS"
assert_eq "and keeps what it just wrote" "$EXPECTED_TREE" "$(vendored_tree)"

# --- --help prints the header block, and only that. -----------------------------

# The script's leading comment is the single copy of its interface docs, and
# `--help` prints it by re-reading the file up to the first non-comment line.
# That coupling is invisible from the header itself: inserting a blank line
# into it would cut `--help` off there, and moving the docs below the
# `set -euo pipefail` would leave it empty — while every other mode kept
# working. So pin both ends of the block and the absence of anything after it.
run_vendor --help
assert_eq "--help exits 0" 0 "$STATUS"
assert_contains "--help prints the usage lines" "$OUT" \
  "scripts/vendor-codex-schema.sh --check    # verify the vendored normal form"
assert_eq "--help prints the header through its last line" \
  'Prerequisites: `jq` for both modes; the pinned Codex CLI for the re-vendor.' \
  "$(printf '%s\n' "$OUT" | tail -n 1)"
# The shebang is the line the printer has to skip; it survives the "# " strip
# as a bare `!/usr/bin/env ...`, so match on the interpreter path rather than
# on `#!`.
assert_lacks "--help does not print the shebang line" "$OUT" '/usr/bin/env'

HELP_OUT="$OUT"
run_vendor -h
assert_eq "-h prints the same as --help" "$HELP_OUT" "$OUT"

# --- Missing prerequisites and bad arguments fail loudly. -----------------------

reset_vendor_dir
STATUS=0
OUT="$(
  env "DELTA_CODEX_BIN=$WORK_DIR/no-such-codex" \
    "DELTA_VENDOR_SCHEMA_DIR=$VENDOR_DIR" \
    "$VENDOR_SCRIPT" 2>&1
)" || STATUS=$?
assert_eq "a missing codex binary fails the re-vendor" 1 "$STATUS"
assert_contains "the missing binary is named with its override" "$OUT" "DELTA_CODEX_BIN"

run_vendor --nope
assert_eq "an unknown argument fails" 1 "$STATUS"
assert_contains "the unknown argument is named" "$OUT" "unknown argument: --nope"

printf '\nAll assertions passed.\n'
exit 0
