#!/bin/bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CACHE="$ROOT/.vendor/local-speech"
MODEL="$ROOT/Resources/SenseVoice"
FRAMEWORKS="$ROOT/Packages/LocalSpeech/Frameworks"
mkdir -p "$CACHE" "$MODEL" "$FRAMEWORKS"
fetch() {
  local url="$1" sha="$2" output="$3"
  if [ -f "$output" ] && echo "$sha  $output" | shasum -a 256 -c - >/dev/null 2>&1; then return; fi
  # A new curl process recomputes the resume offset. curl --retry can
  # truncate a partially downloaded file back to its initial offset.
  local attempt
  for attempt in 1 2 3 4 5 6; do
    if curl --http1.1 -fL --continue-at - --connect-timeout 20 "$url" -o "$output.download"; then break; fi
    [ "$attempt" != 6 ] || return 1
    sleep 2
  done
  echo "$sha  $output.download" | shasum -a 256 -c -
  mv "$output.download" "$output"
}
BASE="${HF_ENDPOINT:-https://huggingface.co}/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07"
fetch "$BASE/model.int8.onnx?download=true" c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51 "$CACHE/model.int8.onnx"
fetch "$BASE/tokens.txt?download=true" f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc "$CACHE/tokens.txt"
cp "$CACHE/model.int8.onnx" "$MODEL/model.int8.onnx"
cp "$CACHE/tokens.txt" "$MODEL/tokens.txt"
# Pinned official upstream iOS-only artifacts. No Python/MLX subprocess or
# inference library is put in the keyboard extension.
fetch 'https://github.com/k2-fsa/sherpa-onnx/releases/download/xcframework/sherpa-onnx-v1.13.8-ios-static.xcframework.zip' 6b8e769cb153343270fdccbe92e3b3db0d1c421d67fa0989ab01fdf5b2fcf2de "$CACHE/sherpa.zip"
fetch 'https://github.com/csukuangfj/onnxruntime-libs/releases/download/v1.28.2/onnxruntime-ios-static-xcframework-1.28.2.xcframework.zip' 2c2299acbb461d26d4bac4bc85985d40e7c7177ed6072703ae0846d88b0b4599 "$CACHE/onnxruntime.zip"
for name in sherpa onnxruntime; do
  unzip -oq "$CACHE/$name.zip" -d "$FRAMEWORKS"
done
echo 'Bundled SenseVoice Small and official iOS CPU runtime verified.'
