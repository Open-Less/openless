#!/bin/bash
# Finding 輔助指令碼 - 自動收集專案資訊用於 EPIC 規劃

set -e

TAURI_DIR="openless-all/app/src-tauri"
OUTPUT_DIR=".github/finding-reports"

mkdir -p "$OUTPUT_DIR"

echo "🔍 開始 Finding 分析..."
echo ""

# ============================================
# 1. 測試覆蓋率分析
# ============================================
echo "📊 分析測試覆蓋率..."

REPORT_FILE="$OUTPUT_DIR/test-coverage-$(date +%Y%m%d).md"

cat > "$REPORT_FILE" << 'EOF'
# 測試覆蓋率 Finding 報告

## 生成時間
EOF

echo "$(date '+%Y-%m-%d %H:%M:%S')" >> "$REPORT_FILE"

cat >> "$REPORT_FILE" << 'EOF'

## 1. 現有測試檔案統計

### Rust 測試模組
EOF

echo '```' >> "$REPORT_FILE"
find "$TAURI_DIR/src" -name "*.rs" -exec grep -l "#\[cfg(test)\]" {} \; | \
  sed "s|$TAURI_DIR/src/||" >> "$REPORT_FILE"
echo '```' >> "$REPORT_FILE"

cat >> "$REPORT_FILE" << 'EOF'

### 測試數量統計
EOF

echo '```' >> "$REPORT_FILE"
echo "包含測試的檔案數: $(find "$TAURI_DIR/src" -name "*.rs" -exec grep -l "#\[cfg(test)\]" {} \; | wc -l)" >> "$REPORT_FILE"
echo "測試模組數: $(grep -r "#\[cfg(test)\]" "$TAURI_DIR/src" | wc -l)" >> "$REPORT_FILE"
echo "測試函式數: $(grep -r "#\[test\]" "$TAURI_DIR/src" | wc -l)" >> "$REPORT_FILE"
echo '```' >> "$REPORT_FILE"

cat >> "$REPORT_FILE" << 'EOF'

## 2. 核心模組程式碼量

EOF

echo '```' >> "$REPORT_FILE"
find "$TAURI_DIR/src" -name "*.rs" -exec wc -l {} + | sort -rn | head -20 >> "$REPORT_FILE"
echo '```' >> "$REPORT_FILE"

cat >> "$REPORT_FILE" << 'EOF'

## 3. 需要補測試的優先順序模組

### 高優先順序（核心功能）
- [ ] recorder.rs - 音訊採集、watchdog
- [ ] coordinator.rs - 狀態機、會話管理
- [ ] asr/volcengine.rs - WebSocket ASR
- [ ] asr/frame.rs - 二進位制幀編解碼

### 中優先順序（工具模組）
- [ ] persistence.rs - 資料持久化
- [ ] types.rs - 型別定義、狀態轉換
- [ ] insertion.rs - 文字插入
- [ ] polish.rs - 文字潤色

### 低優先順序（平臺特定）
- [ ] hotkey.rs - 熱鍵監聽
- [ ] permissions.rs - 許可權檢查
- [ ] windows_ime_*.rs - Windows IME

## 4. 測試工具調研

### 推薦工具
- **mockall**: Mock 框架，用於 mock 外部依賴
- **proptest**: 屬性測試，生成隨機測試資料
- **criterion**: 效能基準測試
- **cargo-llvm-cov**: 程式碼覆蓋率工具

### 安裝命令
```bash
cargo install cargo-llvm-cov
```

## 5. 下一步行動

1. 為 recorder.rs 編寫單元測試（T1.1-T1.6）
2. 為 asr/frame.rs 擴充套件測試（T1.7-T1.10）
3. 建立測試編寫規範文件
4. 配置 CI 自動化測試

EOF

echo "✅ 測試覆蓋率報告已生成: $REPORT_FILE"
echo ""

# ============================================
# 2. ASR 模組分析
# ============================================
echo "🎤 分析 ASR 模組..."

ASR_REPORT="$OUTPUT_DIR/asr-analysis-$(date +%Y%m%d).md"

cat > "$ASR_REPORT" << 'EOF'
# ASR 模組 Finding 報告

## 生成時間
EOF

echo "$(date '+%Y-%m-%d %H:%M:%S')" >> "$ASR_REPORT"

cat >> "$ASR_REPORT" << 'EOF'

## 1. ASR 模組結構

EOF

echo '```' >> "$ASR_REPORT"
ls -lh "$TAURI_DIR/src/asr/" >> "$ASR_REPORT"
echo '```' >> "$ASR_REPORT"

cat >> "$ASR_REPORT" << 'EOF'

## 2. ASR 模組程式碼量

EOF

echo '```' >> "$ASR_REPORT"
wc -l "$TAURI_DIR/src/asr"/*.rs >> "$ASR_REPORT"
echo '```' >> "$ASR_REPORT"

cat >> "$ASR_REPORT" << 'EOF'

## 3. ASR Provider 介面分析

### 當前介面
- `AudioConsumer` trait: 接收 PCM 資料
- `RawTranscript` struct: ASR 輸出結果

### 問題
- 缺少統一的 ASRProvider trait
- Volcengine 和 Whisper 實現重複程式碼
- 擴充套件新 provider 需要大量手工整合

### 改進建議
定義統一的 `ASRProvider` trait，包含：
- `open_session()`: 開啟會話
- `get_audio_consumer()`: 獲取音訊消費者
- `close_session()`: 關閉會話並獲取結果
- `cancel_session()`: 取消會話

## 4. 混淆詞糾錯層設計

### 插入位置
`coordinator.rs:616-617` - ASR 結果進入 polish 之前

### 資料結構
```rust
struct CorrectionRule {
    pattern: String,        // 錯誤模式（支援正則）
    replacement: String,    // 正確詞彙
    context: Option<Vec<String>>,  // 上下文關鍵詞
    enabled: bool,
}
```

### 內建混淆詞表（初版）
- issue / iOS
- PR / 批閱
- CI / 西愛
- commit / 靠米特
- merge / 摸雞
- release / 瑞麗絲

## 5. 本地 ASR 技術選型

### 候選方案

| 專案 | 形態 | 平臺 | 加速 | License | 備註 |
|---|---|---|---|---|---|
| whisper.cpp | C/C++ | 全平臺 | Metal/CoreML/CUDA | MIT | 主流候選 |
| whisper-rs | Rust binding | 全平臺 | 同上 | MIT/Apache-2.0 | Rust 整合更順 |
| sherpa-onnx | C++ + ONNX | 全平臺 | CoreML/CUDA | Apache-2.0 | 多模型支援 |

### 推薦方案
**whisper-rs** - Rust 原生整合，跨平臺支援好

### 整合方式
1. Rust crate 直接繫結（推薦）
2. 子程序 + HTTP（備選）

## 6. 下一步行動

### Phase 1: 混淆詞糾錯（Week 1）
1. 收集 50+ 真實錯詞樣本
2. 實現 `asr/correction.rs` 模組
3. 整合到 coordinator
4. 編寫測試

### Phase 2: 本地 ASR（Week 2-4）
1. 完成技術選型文件 `docs/local-asr-plan.md`
2. 測試 whisper-rs 效能
3. 實現模型下載管理
4. 實現本地推理
5. 跨平臺測試

EOF

echo "✅ ASR 模組報告已生成: $ASR_REPORT"
echo ""

# ============================================
# 3. 依賴關係分析
# ============================================
echo "🔗 分析模組依賴關係..."

DEP_REPORT="$OUTPUT_DIR/dependencies-$(date +%Y%m%d).md"

cat > "$DEP_REPORT" << 'EOF'
# 模組依賴關係 Finding 報告

## 生成時間
EOF

echo "$(date '+%Y-%m-%d %H:%M:%S')" >> "$DEP_REPORT"

cat >> "$DEP_REPORT" << 'EOF'

## 1. Cargo 依賴

EOF

echo '```toml' >> "$DEP_REPORT"
grep -A 50 "\[dependencies\]" "$TAURI_DIR/Cargo.toml" | head -60 >> "$DEP_REPORT"
echo '```' >> "$DEP_REPORT"

cat >> "$DEP_REPORT" << 'EOF'

## 2. 模組間依賴（透過 use 語句分析）

### coordinator.rs 依賴
EOF

echo '```' >> "$DEP_REPORT"
grep "^use crate::" "$TAURI_DIR/src/coordinator.rs" | sort | uniq >> "$DEP_REPORT"
echo '```' >> "$DEP_REPORT"

cat >> "$DEP_REPORT" << 'EOF'

### recorder.rs 依賴
EOF

echo '```' >> "$DEP_REPORT"
grep "^use crate::" "$TAURI_DIR/src/recorder.rs" | sort | uniq >> "$DEP_REPORT"
echo '```' >> "$DEP_REPORT"

cat >> "$DEP_REPORT" << 'EOF'

## 3. Mock 策略建議

### 需要 Mock 的外部依賴
- **Volcengine ASR WebSocket**: 使用 mock WebSocket server
- **OpenAI Polish API**: 使用 mock HTTP server
- **Keychain**: 使用 trait abstraction + mock 實現
- **Clipboard**: 使用 trait abstraction + mock 實現
- **Audio Device**: 使用 mock audio stream

### 推薦工具
- `mockall`: 自動生成 mock
- `wiremock`: HTTP mock server
- `tokio-test`: 非同步測試工具

EOF

echo "✅ 依賴關係報告已生成: $DEP_REPORT"
echo ""

# ============================================
# 4. 生成總結
# ============================================
SUMMARY="$OUTPUT_DIR/finding-summary-$(date +%Y%m%d).md"

cat > "$SUMMARY" << EOF
# Finding 總結報告

**生成時間**: $(date '+%Y-%m-%d %H:%M:%S')

## 📊 關鍵指標

- **包含測試的檔案數**: $(find "$TAURI_DIR/src" -name "*.rs" -exec grep -l "#\[cfg(test)\]" {} \; | wc -l)
- **測試函式數**: $(grep -r "#\[test\]" "$TAURI_DIR/src" | wc -l)
- **核心模組數**: $(find "$TAURI_DIR/src" -maxdepth 1 -name "*.rs" | wc -l)
- **ASR 模組程式碼量**: $(wc -l "$TAURI_DIR/src/asr"/*.rs | tail -1 | awk '{print $1}') 行

## 📋 生成的報告

1. **測試覆蓋率報告**: $REPORT_FILE
2. **ASR 模組分析**: $ASR_REPORT
3. **依賴關係分析**: $DEP_REPORT

## 🎯 下一步行動

### 立即開始（Week 1）
1. 閱讀生成的 3 份報告
2. 更新 EPIC-001 和 EPIC-002 的 Finding 任務狀態
3. 開始實現混淆詞糾錯層（快速產出）

### 短期計劃（Week 2-3）
1. 為 recorder.rs 補測試
2. 為 asr/frame.rs 補測試
3. 編寫測試規範文件

### 中期計劃（Week 4-6）
1. 完成本地 ASR 技術選型
2. 實現本地 ASR 支援
3. 建立 CI 自動化測試

## 📝 備註

所有報告已儲存到 \`.github/finding-reports/\` 目錄。
EOF

echo "✅ 總結報告已生成: $SUMMARY"
echo ""
echo "🎉 Finding 分析完成！"
echo ""
echo "📂 報告位置: $OUTPUT_DIR/"
echo "   - $(basename $REPORT_FILE)"
echo "   - $(basename $ASR_REPORT)"
echo "   - $(basename $DEP_REPORT)"
echo "   - $(basename $SUMMARY)"
echo ""
echo "💡 下一步: 閱讀報告並更新 EPIC 文件"
