#!/usr/bin/env bash
#
# Report GitHub Actions cache usage for this repository and, with --prune,
# delete the scopes that can never be restored again.
#
#   scripts/ci-cache-usage.sh [--prune] [--limit-gb N] [--repo OWNER/NAME]
#
# Why this exists: GitHub keeps 10 GB of caches per repository and evicts the
# least recently used entries. The platform dependency trees are large (the
# Windows Tauri tree alone is ~1.6 GB), so a handful of stale scopes is enough
# to evict the caches the CI jobs depend on - which turns every run into a cold
# build. This tool keeps the budget visible and drops the dead weight.
#
# Env: GH_TOKEN or a logged-in `gh` is required.
# Exit: 1 when usage is above the budget (so a scheduled run is visibly red).
set -euo pipefail

repo=${REPO:-Open-Less/openless}
limit_gb=${LIMIT_GB:-8}
prune=0

while [ $# -gt 0 ]; do
  case "$1" in
    --prune) prune=1 ;;
    --limit-gb)
      limit_gb=${2:?--limit-gb needs a number}
      shift
      ;;
    --repo)
      repo=${2:?--repo needs OWNER/NAME}
      shift
      ;;
    *)
      echo "usage: $0 [--prune] [--limit-gb N] [--repo OWNER/NAME]" >&2
      exit 2
      ;;
  esac
  shift
done

command -v gh > /dev/null 2>&1 || {
  echo "gh is required" >&2
  exit 1
}

caches=$(mktemp)
trap 'rm -f "$caches"' EXIT

gh api --paginate "repos/$repo/actions/caches?per_page=100" \
  --jq '.actions_caches[] | [.ref, .key, .size_in_bytes] | @tsv' > "$caches"

count=$(wc -l < "$caches" | tr -d ' ')
total_gb=$(awk -F'\t' '{ sum += $3 } END { printf "%.2f", sum / 1e9 }' "$caches")

echo "$repo: $count cache(s), ${total_gb} GB used, budget ${limit_gb} GB"
echo "largest scopes:"
awk -F'\t' '{ size[$1] += $3 } END { for (ref in size) printf "  %7.2f GB  %s\n", size[ref] / 1e9, ref }' "$caches" \
  | sort -rn \
  | head -10

if [ "$prune" = 1 ]; then
  pruned=0
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
    # A closed or merged pull request can never restore this scope again.
    echo "  prune  $ref (pull request $state)"
    gh api -X DELETE "repos/$repo/actions/caches?ref=$ref" > /dev/null
    pruned=$((pruned + 1))
  done
  echo "pruned $pruned scope(s)"
fi

if awk -v used="$total_gb" -v limit="$limit_gb" 'BEGIN { exit (used + 0 > limit + 0) ? 0 : 1 }'; then
  echo "::error::cache usage ${total_gb} GB exceeds the ${limit_gb} GB budget"
  exit 1
fi

echo "cache usage is inside the ${limit_gb} GB budget"
