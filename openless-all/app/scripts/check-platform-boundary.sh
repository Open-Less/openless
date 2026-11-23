#!/usr/bin/env bash
# 平台边界检查（D1）。
#
# 目标：宿主 src-tauri 不得再直接调用平台框架 API；所有平台实现都在
# crates/openless-platform 里，宿主只通过它的能力门面调用。
#
# 允许的例外（白名单）必须逐条写在 src-tauri/PLATFORM_BOUNDARY.md 里，
# 每条都要说明为什么不能下沉。
set -euo pipefail
cd "$(dirname "$0")/.."

HOST=src-tauri/src
ALLOWLIST=src-tauri/PLATFORM_BOUNDARY.md
STATUS=0

# 直接平台 API 的"指纹"：一旦出现，说明有平台实现漏在宿主里。
PATTERNS=(
  'windows::Win32'
  'windows_core::'
  'objc2::'
  'jni::'
  'core_foundation::'
)

echo "=== 平台 API 直调（宿主） ==="
for pattern in "${PATTERNS[@]}"; do
  # 排除注释行与平台层 crate 的调用。
  matches=$(grep -rn --include='*.rs' -F "$pattern" "$HOST" 2>/dev/null \
    | grep -vE '^\s*[^:]+:[0-9]+:\s*//' \
    | grep -v 'crate::android::jni::' \
    | grep -v 'openless_platform::' || true)
  if [ -z "$matches" ]; then
    continue
  fi
  while IFS= read -r line; do
    file=${line%%:*}
    rel=${file#"$HOST"/}
    if grep -qF "$rel" "$ALLOWLIST" 2>/dev/null; then
      continue
    fi
    echo "  [未在白名单] $line"
    STATUS=1
  done <<< "$matches"
done

echo "=== 宿主对平台 crate 的按目标依赖 ==="
# 平台能力必须通过 openless-platform 的 feature 表达，不能在宿主里各自写 cfg 实现。
grep -n 'openless-platform' src-tauri/Cargo.toml | sed 's/^/  /'

echo "=== 结果 ==="
if [ "$STATUS" -eq 0 ]; then
  echo "PASS: 宿主没有白名单之外的平台 API 直调"
else
  echo "FAIL: 存在未列入 PLATFORM_BOUNDARY.md 的宿主平台 API 直调"
fi
exit "$STATUS"
