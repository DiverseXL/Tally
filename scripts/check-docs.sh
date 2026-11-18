#!/usr/bin/env bash
# Fails if a documented repo path is missing, a doc is not ASCII, or a test
# name quoted in docs/TESTING.md is not in `cargo test --workspace -- --list`.
#
# Usage: bash scripts/check-docs.sh   (from the repository root)

set -u

fail=0
problem() {
  echo "FAIL: $*"
  fail=1
}

docs="README.md"
for file in docs/*.md; do
  docs="$docs $file"
done

# 1. ASCII only, no emoji.
for file in $docs; do
  if LC_ALL=C grep -nP '[^\x00-\x7F]' "$file"; then
    problem "$file contains non-ASCII characters (lines above)"
  fi
done

# 2. Every backticked repo path must exist. Generated, secret and out-of-repo
# paths are skipped on purpose.
skip_prefixes=".env*|reports/*|runs/*|target/*|node_modules/*"
offenders=""
for file in $docs; do
  tokens="$(grep -o '`[^`]*`' "$file" | tr -d '`')"
  while IFS= read -r token; do
    case "$token" in
      *" "*) continue ;;
      *"<"*|*">"*|*"*"*|*"$"*) continue ;;
      *"://"*) continue ;;
      /*) continue ;;
      $skip_prefixes) continue ;;
      */*) ;;
      *) continue ;;
    esac
    token="${token%[.,;:)]}"
    if [ ! -e "$token" ]; then
      offenders="$offenders$file: $token
"
    fi
  done <<< "$tokens"
done
if [ -n "$offenders" ]; then
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    problem "missing path named in docs: $line"
  done <<< "$offenders"
fi

# 3. Test names quoted in docs/TESTING.md must exist in --list.
list="$(cargo test --workspace -- --list 2>/dev/null | grep -E ': test$' | sed 's/: test$//')"
if [ -z "$list" ]; then
  problem "cargo test --workspace -- --list produced no tests"
else
  names="$(awk '
    /^```/ { fence = !fence; if (!fence) inblock = 0; next }
    fence && /^# test names/ { inblock = 1; next }
    fence && inblock {
      line = $0
      sub(/^[ \t]+/, "", line)
      if (line == "") next
      split(line, parts, " - ")
      print parts[1]
    }
  ' docs/TESTING.md)"
  inline="$(grep -o '`[a-z0-9_]*::tests::[a-z0-9_]*`' docs/TESTING.md | tr -d '`')"

  checked=0
  for name in $names $inline; do
    checked=$((checked + 1))
    if ! printf '%s\n' "$list" | grep -qxF "$name"; then
      problem "docs/TESTING.md quotes a test that cargo does not list: $name"
    fi
  done
  echo "checked $checked test names against $(printf '%s\n' "$list" | wc -l) listed tests"
fi

if [ "$fail" -ne 0 ]; then
  echo "docs check FAILED"
  exit 1
fi

echo "docs check passed"
