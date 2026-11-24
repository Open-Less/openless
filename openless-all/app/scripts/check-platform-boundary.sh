#!/usr/bin/env bash
# 平台边界检查（D1）。
#
# 目标：宿主 src-tauri 不直接调用平台框架 API；平台实现都在 crates/openless-platform。
#
# 放行规则（从宽到严）：
#   1. 整文件放行：ALLOWED_FILES 里列出的文件（见 src-tauri/PLATFORM_BOUNDARY.md，需理由）；
#   2. 逐行放行：该行带 `// boundary-allow: <理由>` 注释；
#   3. 平台层自身的调用（`openless_platform::…`）与宿主 JNI 门面（`crate::android::jni::…`）
#      不算越界 —— 它们就是平台能力门面。
set -euo pipefail
cd "$(dirname "$0")/.."

HOST=src-tauri/src
ALLOW_MARK='boundary-allow'

# 整文件放行（必须在 PLATFORM_BOUNDARY.md 里有对应理由）
ALLOWED_FILES=(
  'android/native_bridge.rs'
  'macos_card_panel.rs'
)

# 平台框架指纹
PATTERNS=(
  'windows::Win32'
  'windows_core::'
  'objc2::'
  'objc2_foundation::'
  'objc2_app_kit::'
  'tauri_nspanel::'
  'jni::'
  'core_foundation::'
  'core_graphics::'
  'dispatch2::'
  'security_framework::'
  'winreg::'
  'keyring::'
  'arboard::'
  'enigo::'
  'ndk_context::'
)

is_allowed_file() {
  local rel="$1"
  for allowed in "${ALLOWED_FILES[@]}"; do
    [ "$rel" = "$allowed" ] && return 0
  done
  return 1
}

STATUS=0
violations=0
line_allowances=0

for pattern in "${PATTERNS[@]}"; do
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    file=${line%%:*}
    rel=${file#"$HOST"/}
    if is_allowed_file "$rel"; then
      continue
    fi
    if [[ "$line" == *"$ALLOW_MARK"* ]]; then
      line_allowances=$((line_allowances + 1))
      continue
    fi
    echo "  [未放行] $line"
    violations=$((violations + 1))
    STATUS=1
  done < <(
    grep -rn --include='*.rs' -F "$pattern" "$HOST" 2>/dev/null \
      | grep -vE ':[0-9]+:[[:space:]]*(//|///|//!)' \
      | grep -v 'crate::android::jni::' \
      | grep -v 'openless_platform::' || true
  )
done

echo "=== 汇总 ==="
echo "  越界：$violations"
echo "  逐行放行：$line_allowances"
echo "  整文件放行：${#ALLOWED_FILES[@]}（$(IFS=', '; echo "${ALLOWED_FILES[*]}")）"
if [ "$STATUS" -eq 0 ]; then
  echo "PASS: 宿主没有放行规则之外的平台 API 直调"
else
  echo "FAIL: 存在未放行的平台 API 直调（加 // boundary-allow: 理由 或改到平台层）"
fi
exit "$STATUS"
