#!/usr/bin/env bash
#
# Report GitHub Actions cache usage for this repository and, with --prune,
# delete the entries that can never be restored again or that are superseded
# by a newer sibling.
#
#   scripts/ci-cache-usage.sh [--prune] [--keep-family N] [--dry-run] [--limit-gb N] [--repo OWNER/NAME]
#
# Why this exists: GitHub keeps 10 GB of caches per repository and evicts the
# least recently used entries. The platform dependency trees are large (the
# Windows Tauri tree alone is ~1.6 GB), so a handful of stale scopes is enough
# to evict the caches the CI jobs depend on - which turns every run into a cold
# build. This tool keeps the budget visible and drops the dead weight.
#
# What --prune deletes:
#   1. cache scopes of closed or merged pull requests (never restorable), and
#   2. with --keep-family N, all but the N newest entries of the same
#      (branch, key family) pair. A key family is the key with its trailing
#      content hashes removed, e.g.
#        v0-rust-cross-platform-Windows_NT-x64-3619e479-a77d3f83
#        -> v0-rust-cross-platform-Windows_NT-x64
#      Old siblings stay useful as `restore-keys` fallbacks, so the default
#      keeps them; the scheduled job passes 2.
# It never touches tag scopes: a tag run can only restore the default branch's
# caches, and release caches are written per tag on purpose.
#
# Env: GH_TOKEN or a logged-in `gh` is required.
# Exit: 1 when usage is above the budget (so a scheduled run is visibly red).
set -euo pipefail

repo=${REPO:-Open-Less/openless}
limit_gb=${LIMIT_GB:-8}
prune=0
keep_family=0
dry_run=0

while [ $# -gt 0 ]; do
  case "$1" in
    --prune) prune=1 ;;
    --dry-run) dry_run=1 ;;
    --keep-family)
      keep_family=${2:?--keep-family needs a number}
      shift
      ;;
    --limit-gb)
      limit_gb=${2:?--limit-gb needs a number}
      shift
      ;;
    --repo)
      repo=${2:?--repo needs OWNER/NAME}
      shift
      ;;
    *)
      echo "usage: $0 [--prune] [--keep-family N] [--dry-run] [--limit-gb N] [--repo OWNER/NAME]" >&2
      exit 2
      ;;
  esac
  shift
done

case "$keep_family" in
  '' | *[!0-9]*)
    echo "--keep-family needs a non-negative integer" >&2
    exit 2
    ;;
esac

command -v gh > /dev/null 2>&1 || {
  echo "gh is required" >&2
  exit 1
}

# Percent-encode a value for a query string. Keys contain only [A-Za-z0-9._-],
# but refs contain slashes, so encode unconditionally.
urlencode() {
  if command -v python3 > /dev/null 2>&1; then
    python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1], safe=""))' "$1"
  else
    jq -rn --arg v "$1" '$v|@uri'
  fi
}

caches=$(mktemp)
trap 'rm -f "$caches"' EXIT

gh api --paginate "repos/$repo/actions/caches?per_page=100" \
  --jq '.actions_caches[] | [.ref, .key, .size_in_bytes, .created_at] | @tsv' > "$caches"

count=$(wc -l < "$caches" | tr -d ' ')
total_gb=$(awk -F'\t' '{ sum += $3 } END { printf "%.2f", sum / 1e9 }' "$caches")

echo "$repo: $count cache(s), ${total_gb} GB used, budget ${limit_gb} GB"
echo "largest scopes:"
awk -F'\t' '{ size[$1] += $3 } END { for (ref in size) printf "  %7.2f GB  %s\n", size[ref] / 1e9, ref }' "$caches" \
  | sort -rn \
  | head -10

delete_cache() {
  # GitHub changed this endpoint: DELETE requires `key`, and `ref` only
  # narrows it. Without both, the call answers HTTP 422.
  local key=$1 ref=$2
  if [ "$dry_run" = 1 ]; then
    echo "  would prune $ref $key"
    return 0
  fi
  gh api -X DELETE \
    "repos/$repo/actions/caches?key=$(urlencode "$key")&ref=$(urlencode "$ref")" > /dev/null
}

if [ "$prune" = 1 ]; then
  pruned=0
  # 1. Closed or merged pull requests can never restore their scope again.
  for ref in $(awk -F'\t' '$1 ~ /^refs\/pull\/[0-9]+\/(merge|head)$/ { print $1 }' "$caches" | sort -u); do
    number=${ref#refs/pull/}
    number=${number%%/*}
    state=$(gh api "repos/$repo/pulls/$number" --jq .state 2> /dev/null || echo unknown)
    if [ "$state" = "open" ]; then
      echo "  keep   $ref (pull request is open)"
      continue
    fi
    if [ "$state" = "unknown" ]; then
      echo "  keep   $ref (pull request state is unreadable)"
      continue
    fi
    for key in $(awk -F'\t' -v want="$ref" '$1 == want { print $2 }' "$caches"); do
      echo "  prune  $ref (pull request $state)"
      delete_cache "$key" "$ref"
      pruned=$((pruned + 1))
    done
  done

  # 2. Superseded siblings inside one branch scope.
  if [ "$keep_family" -gt 0 ]; then
    families=$(mktemp)
    awk -F'\t' '
      function family(key) {
        while (key ~ /-[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]+$/) {
          sub(/-[0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f][0-9a-f]+$/, "", key)
        }
        return key
      }
      $1 ~ /^refs\/heads\// { print $1 "\t" family($2) "\t" $4 "\t" $2 }
    ' "$caches" | sort -t "$(printf '\t')" -k1,1 -k2,2 -k3,3 > "$families"
    # Two passes: first count the siblings, then emit the oldest ones beyond the
    # keep window. `restore-keys` only needs the previous generation, so the two
    # newest entries of a family are enough.
    while IFS=$(printf '\t') read -r ref key; do
      echo "  prune  $ref ${key} (superseded, keeping ${keep_family})"
      delete_cache "$key" "$ref"
      pruned=$((pruned + 1))
    done < <(awk -F'\t' -v keep="$keep_family" '
      FNR == NR { total[$1 "\t" $2]++; next }
      {
        group = $1 "\t" $2
        seen[group]++
        if (seen[group] <= total[group] - keep) print $1 "\t" $4
      }
    ' "$families" "$families")
    rm -f "$families"
  fi
  echo "pruned $pruned cache(s)"
fi

if awk -v used="$total_gb" -v limit="$limit_gb" 'BEGIN { exit (used + 0 > limit + 0) ? 0 : 1 }'; then
  echo "::error::cache usage ${total_gb} GB exceeds the ${limit_gb} GB budget"
  exit 1
fi

echo "cache usage is inside the ${limit_gb} GB budget"
