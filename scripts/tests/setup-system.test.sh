#!/usr/bin/env bash
#
# Tests for scripts/setup-system.sh — the Debian/Ubuntu installer for the
# system packages `make check` needs.
#
# Usage: bash scripts/tests/setup-system.test.sh
#
# Never installs anything: each case copies the script into a throwaway tree
# with its own frontend/pnpm-lock.yaml and runs it with PATH holding only stub
# `apt-get`, `dpkg-query`, `sudo`, `id` and `npx` (which record their
# arguments) plus links to the few real tools the script uses. The stub
# dpkg-query reports the packages the stub apt-get has "installed", so a second
# run sees the first run's result. Needs only bash and coreutils.
#
# The script exits non-zero on the first failed assertion.

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SETUP="$REPO_ROOT/scripts/setup-system.sh"

[ -x "$SETUP" ] || {
  printf 'FAIL - setup-system script is not executable: %s\n' "$SETUP" >&2
  exit 1
}

WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/delta-setup-system-test.XXXXXX")"
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

# The real tools the script (and the stubs) run, linked into each case's
# otherwise empty PATH so the host's apt-get, sudo and npx stay out of reach.
REAL_TOOLS="bash env dirname sed sort grep tr cat"

# Builds a case directory named $1: a copy of the script under scripts/, a
# lockfile (from stdin) under frontend/, and a bin/ of stubs. Further
# arguments name stubs to leave out. Prints the case directory.
make_case() {
  local dir="$WORK_DIR/$1" tool omit
  shift
  mkdir -p "$dir/scripts" "$dir/frontend" "$dir/bin"
  cp "$SETUP" "$dir/scripts/setup-system.sh"
  cat >"$dir/frontend/pnpm-lock.yaml"
  : >"$dir/installed"
  : >"$dir/calls"
  for tool in $REAL_TOOLS; do
    ln -s "$(command -v "$tool")" "$dir/bin/$tool"
  done

  cat >"$dir/bin/apt-get" <<EOF
#!$BASH
printf 'apt-get %s\n' "\$*" >>"$dir/calls"
if [ "\$1" = install ]; then
  for arg in "\$@"; do
    case "\$arg" in install|-*) ;; *) printf '%s\n' "\$arg" >>"$dir/installed" ;; esac
  done
fi
EOF
  cat >"$dir/bin/dpkg-query" <<EOF
#!$BASH
for pkg in "\$@"; do :; done
grep -qx -- "\$pkg" "$dir/installed" || exit 1
printf 'install ok installed'
EOF
  cat >"$dir/bin/sudo" <<EOF
#!$BASH
printf 'sudo %s\n' "\$*" >>"$dir/calls"
exec "\$@"
EOF
  cat >"$dir/bin/id" <<EOF
#!$BASH
printf '%s\n' "\${STUB_UID:-0}"
EOF
  cat >"$dir/bin/npx" <<EOF
#!$BASH
printf 'npx %s browsers=%s\n' "\$*" "\${PLAYWRIGHT_BROWSERS_PATH-unset}" >>"$dir/calls"
EOF
  chmod +x "$dir/bin/apt-get" "$dir/bin/dpkg-query" "$dir/bin/sudo" "$dir/bin/id" "$dir/bin/npx"
  for omit in "$@"; do
    rm "$dir/bin/$omit"
  done
  printf '%s\n' "$dir"
}

OUT=""
STATUS=0
# Runs the case's copy of the script from / (not its own directory) with only
# the case's bin/ on PATH. Extra arguments are VAR=value environment entries.
run_setup() {
  local dir="$1"
  shift
  STATUS=0
  OUT="$(cd / && env -i "PATH=$dir/bin" "$@" "$BASH" "$dir/scripts/setup-system.sh" 2>&1)" || STATUS=$?
}

lockfile() {
  cat <<'YAML'
lockfileVersion: '9.0'

importers:

  packages/apps/web:
    devDependencies:
      '@playwright/test':
        specifier: ^1.50.0
        version: 1.50.2

packages:

  '@playwright/test@1.50.2':
    resolution: {integrity: sha512-x}

snapshots:

  '@playwright/test@1.50.2':
    dependencies:
      playwright: 1.50.2
YAML
}

# --- Without apt-get, it fails with a clear message and runs nothing. ---------

dir="$(lockfile | make_case no-apt apt-get)"
run_setup "$dir"
assert_eq "no apt-get: exit status" 1 "$STATUS"
assert_contains "no apt-get: names the missing tool" "$OUT" "apt-get not found"
assert_eq "no apt-get: nothing runs" "" "$(cat "$dir/calls")"

# --- As root on a fresh system: apt-get without sudo, then the browsers. ------

dir="$(lockfile | make_case root)"
run_setup "$dir"
assert_eq "root: exit status" 0 "$STATUS"
calls="$(cat "$dir/calls")"
assert_lacks "root: no sudo" "$calls" "sudo"
assert_contains "root: updates the package lists" "$calls" "apt-get update"
assert_contains "root: installs the tools" "$calls" "apt-get install -y tmux lsof jq make"
assert_contains "root: installs the Tauri libraries" "$calls" "libwebkit2gtk-4.1-dev libgtk-3-dev"
assert_contains "root: installs the lockfile's Playwright browsers" "$calls" \
  "npx -y playwright@1.50.2 install --with-deps chromium chromium-headless-shell browsers=unset"

# --- A second run installs no apt packages and still succeeds. -----------------

: >"$dir/calls"
run_setup "$dir"
assert_eq "rerun: exit status" 0 "$STATUS"
calls="$(cat "$dir/calls")"
assert_lacks "rerun: no apt-get" "$calls" "apt-get"
assert_contains "rerun: reports the packages as installed" "$OUT" "apt packages already installed"

# --- Only the missing packages are installed. ----------------------------------

dir="$(lockfile | make_case partial)"
printf '%s\n' tmux jq >"$dir/installed"
run_setup "$dir"
calls="$(cat "$dir/calls")"
assert_contains "partial: installs what is missing" "$calls" "apt-get install -y lsof make"
assert_lacks "partial: skips tmux" "$calls" " tmux"

# --- As a regular user, apt-get goes through sudo and npx does not. -----------

dir="$(lockfile | make_case user)"
run_setup "$dir" STUB_UID=1000
assert_eq "user: exit status" 0 "$STATUS"
calls="$(cat "$dir/calls")"
assert_contains "user: apt-get update via sudo" "$calls" "sudo env DEBIAN_FRONTEND=noninteractive apt-get update"
assert_contains "user: apt-get install via sudo" "$calls" "sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y"
assert_lacks "user: npx not via sudo" "$calls" "sudo npx"

# --- As a regular user without sudo, it fails before installing. --------------

dir="$(lockfile | make_case no-sudo sudo)"
run_setup "$dir" STUB_UID=1000
assert_eq "no sudo: exit status" 1 "$STATUS"
assert_contains "no sudo: says why" "$OUT" "sudo not found"
assert_eq "no sudo: nothing runs" "" "$(cat "$dir/calls")"

# --- PLAYWRIGHT_BROWSERS_PATH reaches Playwright. ------------------------------

dir="$(lockfile | make_case browsers-path)"
run_setup "$dir" PLAYWRIGHT_BROWSERS_PATH=/opt/pw
assert_contains "browsers path: passed to playwright" "$(cat "$dir/calls")" "browsers=/opt/pw"
assert_contains "browsers path: reported" "$OUT" "into /opt/pw"

# --- Without npx, it says so after the apt packages. ---------------------------

dir="$(lockfile | make_case no-npx npx)"
run_setup "$dir"
assert_eq "no npx: exit status" 1 "$STATUS"
assert_contains "no npx: says why" "$OUT" "npx not found"

# --- A lockfile pinning two Playwright versions fails before installing. ------

dir="$({ lockfile; printf "\n  '@playwright/test@1.51.0':\n    resolution: {integrity: sha512-y}\n"; } | make_case two-versions)"
run_setup "$dir"
assert_eq "two versions: exit status" 1 "$STATUS"
assert_contains "two versions: says why" "$OUT" "expected one @playwright/test version"
assert_eq "two versions: nothing runs" "" "$(cat "$dir/calls")"

# --- A lockfile without Playwright fails before installing. -------------------

dir="$(printf "lockfileVersion: '9.0'\n" | make_case no-playwright)"
run_setup "$dir"
assert_eq "no playwright: exit status" 1 "$STATUS"
assert_contains "no playwright: says why" "$OUT" "found 0"

# --- The repository's own lockfile yields one version. -------------------------

dir="$(make_case real-lockfile <"$REPO_ROOT/frontend/pnpm-lock.yaml")"
run_setup "$dir"
assert_eq "real lockfile: exit status" 0 "$STATUS"
if grep -Eq '^npx -y playwright@[0-9]+\.[0-9]+\.[0-9]+ install ' "$dir/calls"; then
  pass "real lockfile: installs a pinned Playwright version"
else
  fail "real lockfile: installs a pinned Playwright version" "$(cat "$dir/calls")"
fi

printf 'all setup-system tests passed\n'
