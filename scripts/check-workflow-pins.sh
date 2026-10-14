#!/usr/bin/env bash
set -euo pipefail

# Fail when a workflow references an action by anything but a full commit SHA.
#
# Usage:
#   check-workflow-pins.sh [<workflows dir>]   (default: .github/workflows,
#                                               relative to the repo root)
#
# A tag or branch (`@v4`, `@stable`) can be moved to other code after review;
# a 40-character commit SHA cannot. Every `uses:` in the directory's *.yml and
# *.yaml files must therefore read
#
#   uses: owner/repo[/path]@<40 hex characters> # <version>
#
# with the version the SHA was taken from in the trailing comment, which
# Dependabot keeps current when it bumps the SHA. Local reusable workflows and
# actions (`./...`) are part of this repository and are exempt. Each offender
# is printed as `<file>:<line>: <reference>` before the script exits 1.

if [ $# -gt 1 ]; then
    echo "usage: check-workflow-pins.sh [<workflows dir>]" >&2
    exit 2
fi

if [ $# -eq 1 ]; then
    workflows_dir="$1"
else
    workflows_dir="$(git rev-parse --show-toplevel)/.github/workflows"
fi

if [ ! -d "$workflows_dir" ]; then
    echo "check-workflow-pins: no such directory: ${workflows_dir}" >&2
    exit 2
fi

status=0
checked=0

for file in "$workflows_dir"/*.yml "$workflows_dir"/*.yaml; do
    [ -f "$file" ] || continue
    line_number=0
    while IFS= read -r line || [ -n "$line" ]; do
        line_number=$((line_number + 1))
        # `uses:` as a mapping key, optionally opening a sequence item.
        case "$line" in
            *uses:*) ;;
            *) continue ;;
        esac
        printf '%s\n' "$line" | grep -Eq '^[[:space:]]*(-[[:space:]]+)?uses:' || continue

        # The value: drop the key, then a trailing comment, then quotes.
        value="${line#*uses:}"
        comment=""
        case "$value" in
            *" #"*)
                comment="${value#* #}"
                value="${value%% #*}"
                ;;
        esac
        value="$(printf '%s' "$value" | tr -d "[:space:]\"'")"
        comment="$(printf '%s' "$comment" | sed 's/^[[:space:]]*//; s/[[:space:]]*$//')"

        case "$value" in
            ./*) continue ;;
        esac
        checked=$((checked + 1))

        if ! printf '%s\n' "$value" | grep -Eq '^[^@[:space:]]+@[0-9a-f]{40}$'; then
            echo "${file}:${line_number}: ${value} is not pinned to a full commit SHA"
            status=1
        elif [ -z "$comment" ]; then
            echo "${file}:${line_number}: ${value} carries no '# <version>' comment"
            status=1
        fi
    done <"$file"
done

if [ "$status" -ne 0 ]; then
    echo "Pin each action to the commit SHA of a release, with the release in a trailing comment:"
    echo "  uses: owner/repo@<40-hex commit SHA> # vX.Y.Z"
    echo "(gh api repos/<owner>/<repo>/commits/<tag> --jq .sha prints a tag's commit SHA.)"
    exit 1
fi

echo "check-workflow-pins: ${checked} action reference(s) in ${workflows_dir} are pinned"
