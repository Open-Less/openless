#!/usr/bin/env bash

set -euo pipefail

PLIST_PATH="${1:-}"
if [ -z "$PLIST_PATH" ]; then
  echo "usage: $0 <Info.plist>" >&2
  exit 2
fi

DESCRIPTION=$(/usr/bin/plutil \
  -extract NSSpeechRecognitionUsageDescription raw \
  -expect string \
  -o - \
  -- "$PLIST_PATH")

if ! node -e 'process.exit(process.argv[1].trim().length === 0 ? 1 : 0)' -- "$DESCRIPTION"; then
  echo "NSSpeechRecognitionUsageDescription must be a non-empty string in $PLIST_PATH" >&2
  exit 1
fi
