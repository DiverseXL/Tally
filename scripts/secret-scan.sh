#!/usr/bin/env bash
# Exact-string secret scan.
#
# Builds a pattern file from the connection string values in every .env.* wallet
# file in the workspace root, then counts exact-string matches in the working
# tree, the index, every commit, and the four evidence directories. It prints
# counts only: no pattern and no matched line ever reaches the terminal.
#
# The wallet files are git-ignored and are read here, never written.

set -u

cd "$(dirname "$0")/.." || exit 1
# shellcheck disable=SC2312
pwd

work=$(mktemp -d) || exit 1
trap 'rm -rf "$work"' EXIT

patterns="$work/patterns"
: >"$patterns"

files=0
for file in .env.*; do
    [ -f "$file" ] || continue
    files=$((files + 1))
    awk -F= '/^[A-Z0-9_]+=/ { sub(/^[^=]*=/, ""); sub(/\r$/, ""); if (length($0) > 0) print $0 }' "$file" >>"$patterns"
done

sort -u "$patterns" -o "$patterns"
pattern_count=$(wc -l <"$patterns" | tr -d ' ')

echo "patterns: ${pattern_count} values from ${files} .env.* files (values not printed)"

if [ "$pattern_count" -eq 0 ]; then
    echo "no patterns to scan"
    exit 0
fi

report() {
    location=$1
    shift
    lines=$("$@" 2>/dev/null)
    if [ -z "$lines" ]; then
        echo "$location: files=0 hits=0"
        return
    fi
    hits=$(printf '%s\n' "$lines" | awk -F: '{ sum += $NF } END { print sum + 0 }')
    file_count=$(printf '%s\n' "$lines" | wc -l | tr -d ' ')
    echo "$location: files=${file_count} hits=${hits}"
}

report "tree" git grep -c -F -f "$patterns" --
report "index" git grep --cached -c -F -f "$patterns" --

history_lines="$work/history"
: >"$history_lines"
for commit in $(git rev-list --all); do
    git grep -c -F -f "$patterns" "$commit" 2>/dev/null >>"$history_lines"
done
sed -i 's/^[^:]*://' "$history_lines"
echo "history: files=$(wc -l <"$history_lines" | tr -d ' ') hits=$(awk '{ sum += $1 } END { print sum + 0 }' "$history_lines")"

for dir in reports runs examples web; do
    if [ ! -d "$dir" ]; then
        echo "$dir: files=0 hits=0 (missing)"
        continue
    fi
    lines=$(grep -rIc -F -f "$patterns" --exclude-dir=node_modules --exclude-dir=target "$dir" 2>/dev/null | grep -v ':0$')
    if [ -z "$lines" ]; then
        echo "$dir: files=0 hits=0"
    else
        hits=$(printf '%s\n' "$lines" | awk -F: '{ sum += $NF } END { print sum + 0 }')
        file_count=$(printf '%s\n' "$lines" | wc -l | tr -d ' ')
        echo "$dir: files=${file_count} hits=${hits}"
    fi
done
