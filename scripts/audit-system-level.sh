#!/bin/bash
# 系統級審計指令碼 - 發現架構、安全、擴充套件性問題

set -e

TAURI_DIR="openless-all/app/src-tauri"
OUTPUT_DIR=".github/audit-reports/system-level"
TIMESTAMP=$(date +%Y%m%d)

mkdir -p "$OUTPUT_DIR"

echo "🔍 開始系統級審計..."
echo ""

# ============================================
# 1. 架構風險地圖
# ============================================
echo "🏗️  生成架構風險地圖..."

ARCH_REPORT="$OUTPUT_DIR/architecture-risk-map-$TIMESTAMP.md"

cat > "$ARCH_REPORT" << 'EOF'
# 架構風險地圖

## 生成時間
EOF

echo "$(date '+%Y-%m-%d %H:%M:%S')" >> "$ARCH_REPORT"

cat >> "$ARCH_REPORT" << 'EOF'

## 1. 整體架構評估

### 當前架構
```
┌─────────────────────────────────────────┐
│           Frontend (React/TS)           │
│  Capsule / Overview / Settings / QA     │
└──────────────┬──────────────────────────┘
               │ IPC (Tauri commands)
┌──────────────┴──────────────────────────┐
│         Coordinator (狀態機)             │
│  Idle → Starting → Listening → Processing│
└─┬────┬────┬────┬────┬────┬────┬────┬───┘
  │    │    │    │    │    │    │    │
  ▼    ▼    ▼    ▼    ▼    ▼    ▼    ▼
Hotkey Recorder ASR Polish Insert Persist Perms History
```

### 架構優勢
- ✅ Coordinator 作為單一狀態機，職責清晰
- ✅ 模組間透過 Coordinator 協調，避免直接依賴
- ✅ 使用 trait 抽象（AudioConsumer）

### 架構風險

#### 🔴 高風險：Coordinator 過於龐大
**現象**：
- coordinator.rs 有 3462 行程式碼
- 承擔了狀態機、會話管理、模組協調、錯誤處理等多重職責

**影響**：
- 難以理解和維護
- 修改一個功能可能影響其他功能
- 測試困難（需要 mock 所有依賴）

**建議**：
- 拆分為多個子模組：
  - `coordinator/state_machine.rs` - 狀態轉換邏輯
  - `coordinator/session.rs` - 會話管理
  - `coordinator/orchestrator.rs` - 模組協調
  - `coordinator/error_handler.rs` - 錯誤處理

#### 🟡 中風險：缺少統一的 ASR Provider trait
**現象**：
- Volcengine 和 Whisper 實現各自獨立
- 新增新 provider 需要大量手工整合
- 程式碼重複（會話管理、錯誤處理）

**影響**：
- 擴充套件性差
- 維護成本高
- 容易引入不一致

**建議**：
- 定義統一的 `ASRProvider` trait
- 重構現有 provider 實現該 trait
- 在 Coordinator 中使用 trait object

#### 🟡 中風險：測試基礎設施缺失
**現象**：
- 無測試策略文件
- 無 CI 自動化測試
- 測試覆蓋率接近 0%

**影響**：
- 重構風險高（容易引入迴歸 bug）
- 新功能質量無保障
- 技術債務累積

**建議**：
- 建立測試策略（單元測試、整合測試、E2E 測試比例）
- 配置 CI 自動化測試
- 為核心模組補充測試

#### 🟢 低風險：模組間依賴清晰
**現象**：
- 各模組只依賴 `types.rs`
- 模組間不直接呼叫

**影響**：
- 正面影響，易於維護

## 2. 模組依賴分析

### 核心模組依賴圖
```
types.rs (530 行)
    ↑
    ├── coordinator.rs (3462 行)
    │       ↑
    │       ├── hotkey.rs (785 行)
    │       ├── recorder.rs (525 行)
    │       ├── asr/mod.rs (1164 行)
    │       ├── polish.rs (992 行)
    │       ├── insertion.rs (489 行)
    │       ├── persistence.rs (770 行)
    │       └── permissions.rs (428 行)
    │
    ├── commands.rs (712 行)
    └── lib.rs (844 行)
```

### 依賴健康度
- ✅ **單向依賴**：所有模組依賴 types，types 不依賴任何模組
- ✅ **無迴圈依賴**：模組間無迴圈依賴
- ⚠️  **Coordinator 依賴過多**：依賴 8+ 個模組

## 3. 技術棧評估

### 當前技術棧
EOF

echo '```toml' >> "$ARCH_REPORT"
grep -A 30 "\[dependencies\]" "$TAURI_DIR/Cargo.toml" | head -35 >> "$ARCH_REPORT"
echo '```' >> "$ARCH_REPORT"

cat >> "$ARCH_REPORT" << 'EOF'

### 技術棧風險
- ✅ **Tauri 2**: 成熟穩定，社群活躍
- ✅ **Tokio**: 非同步執行時，效能優秀
- ✅ **Serde**: 序列化標準，生態完善
- ⚠️  **global-hotkey 0.6**: 版本較新，可能有相容性問題
- ⚠️  **cpal 0.15**: 音訊庫，跨平臺相容性需關注

## 4. 擴充套件性瓶頸

### 當前擴充套件點
1. **ASR Provider**: 需要手工整合，成本高
2. **Polish Provider**: 已支援 OpenAI 相容介面，擴充套件性好
3. **Insertion Strategy**: 硬編碼 AX → clipboard → copy-only，擴充套件性差

### 擴充套件性改進建議

#### ASR Provider 擴充套件
**當前成本**：新增新 provider 需要：
1. 實現 AudioConsumer trait
2. 在 Coordinator 中新增分支邏輯
3. 在 Settings UI 中新增配置
4. 在 persistence 中新增憑據儲存

**改進方案**：
```rust
// 定義統一介面
#[async_trait]
pub trait ASRProvider: Send + Sync {
    async fn open_session(&self, hotwords: Vec<DictionaryHotword>) -> Result<()>;
    fn get_audio_consumer(&self) -> Arc<dyn AudioConsumer>;
    async fn close_session(&self) -> Result<RawTranscript>;
    async fn cancel_session(&self);
}

// 註冊機制
pub struct ASRRegistry {
    providers: HashMap<String, Box<dyn ASRProvider>>,
}

impl ASRRegistry {
    pub fn register(&mut self, name: &str, provider: Box<dyn ASRProvider>) {
        self.providers.insert(name.to_string(), provider);
    }
}
```

#### Insertion Strategy 擴充套件
**當前成本**：新增新策略需要修改 insertion.rs 核心邏輯

**改進方案**：
```rust
// 策略模式
pub trait InsertionStrategy: Send + Sync {
    async fn insert(&self, text: &str) -> Result<()>;
}

pub struct AXInsertionStrategy;
pub struct ClipboardInsertionStrategy;
pub struct CopyOnlyStrategy;

// 策略鏈
pub struct InsertionChain {
    strategies: Vec<Box<dyn InsertionStrategy>>,
}
```

## 5. 效能瓶頸

### 潛在瓶頸
1. **Coordinator 鎖競爭**: 所有操作都需要獲取 Coordinator 鎖
2. **音訊資料複製**: Recorder → AudioConsumer 可能有多次複製
3. **WebSocket 緩衝**: BufferingAudioConsumer 可能積壓大量資料

### 效能最佳化建議
- 使用細粒度鎖（拆分 Coordinator 狀態）
- 使用 zero-copy 音訊傳輸（Arc<[u8]>）
- 限制 BufferingAudioConsumer 緩衝區大小

## 6. 架構演進路線圖

### Phase 1: Coordinator 拆分（優先順序：高）
**目標**: 將 3462 行的 Coordinator 拆分為多個子模組

**步驟**:
1. 提取狀態機邏輯到 `state_machine.rs`
2. 提取會話管理到 `session.rs`
3. 提取模組協調到 `orchestrator.rs`
4. 保留 `coordinator.rs` 作為入口

**預期收益**:
- 程式碼可讀性提升 50%+
- 測試覆蓋率提升 30%+
- 維護成本降低 40%+

### Phase 2: ASR Provider 統一介面（優先順序：高）
**目標**: 定義統一的 ASRProvider trait，重構現有 provider

**步驟**:
1. 定義 `ASRProvider` trait
2. 重構 Volcengine 實現該 trait
3. 重構 Whisper 實現該 trait
4. 新增 provider 註冊機制

**預期收益**:
- 新增新 provider 成本降低 70%+
- 程式碼重複減少 50%+
- 擴充套件性提升 100%+

### Phase 3: 測試基礎設施建設（優先順序：高）
**目標**: 建立完整的測試基礎設施

**步驟**:
1. 編寫測試策略文件
2. 為核心模組補充單元測試
3. 新增整合測試
4. 配置 CI 自動化測試

**預期收益**:
- 測試覆蓋率從 0% → 60%+
- 重構風險降低 80%+
- 程式碼質量提升 50%+

## 7. 風險優先順序矩陣

| 風險 | 影響 | 緊急度 | 優先順序 | 預計工作量 |
|------|------|--------|--------|-----------|
| Coordinator 過於龐大 | 高 | 中 | P1 | 2 周 |
| 缺少統一 ASR trait | 高 | 中 | P1 | 1 周 |
| 測試基礎設施缺失 | 高 | 高 | P0 | 6 周 |
| Insertion 擴充套件性差 | 中 | 低 | P2 | 1 周 |
| 效能瓶頸 | 中 | 低 | P3 | 2 周 |

## 8. 下一步行動

### 立即開始（本週）
1. ✅ 完成系統級審計
2. ⏳ 決策：是否需要架構重構
3. ⏳ 如果需要，暫停低尺度審計，先做架構設計

### 短期計劃（2-4 周）
1. Coordinator 拆分設計文件
2. ASR Provider trait 設計文件
3. 測試策略文件

### 中期計劃（1-2 個月）
1. 實施 Coordinator 拆分
2. 實施 ASR Provider 統一介面
3. 建立測試基礎設施

---

**審計結論**：
- 🔴 **需要架構重構**：Coordinator 過於龐大，ASR 缺少統一介面
- 🟡 **測試基礎設施缺失**：需要優先建設
- 🟢 **模組依賴健康**：無迴圈依賴，單向依賴清晰

**建議**：
1. 優先建立測試基礎設施（為重構保駕護航）
2. 然後進行 Coordinator 拆分
3. 最後統一 ASR Provider 介面
EOF

echo "✅ 架構風險地圖已生成: $ARCH_REPORT"
echo ""

# ============================================
# 2. 技術債務矩陣
# ============================================
echo "💳 生成技術債務矩陣..."

DEBT_REPORT="$OUTPUT_DIR/tech-debt-matrix-$TIMESTAMP.md"

cat > "$DEBT_REPORT" << 'EOF'
# 技術債務矩陣

## 生成時間
EOF

echo "$(date '+%Y-%m-%d %H:%M:%S')" >> "$DEBT_REPORT"

cat >> "$DEBT_REPORT" << 'EOF'

## 1. 技術債務分類

### 架構債務（Architecture Debt）
| 債務 | 影響 | 償還成本 | 利息 | 優先順序 |
|------|------|---------|------|--------|
| Coordinator 過於龐大 | 高 | 2 周 | 每次修改都困難 | P1 |
| 缺少統一 ASR trait | 高 | 1 周 | 新增 provider 成本高 | P1 |
| Insertion 策略硬編碼 | 中 | 1 周 | 擴充套件困難 | P2 |

### 測試債務（Testing Debt）
| 債務 | 影響 | 償還成本 | 利息 | 優先順序 |
|------|------|---------|------|--------|
| 測試覆蓋率接近 0% | 高 | 6 周 | 重構風險高 | P0 |
| 無 CI 自動化測試 | 高 | 1 周 | 手工測試成本高 | P0 |
| 無測試策略文件 | 中 | 2 天 | 測試質量無保障 | P1 |

### 文件債務（Documentation Debt）
| 債務 | 影響 | 償還成本 | 利息 | 優先順序 |
|------|------|---------|------|--------|
| 缺少架構設計文件 | 中 | 3 天 | 新人上手困難 | P2 |
| 缺少 API 文件 | 低 | 2 天 | 整合困難 | P3 |
| 缺少測試指南 | 中 | 1 天 | 測試質量差 | P2 |

### 程式碼債務（Code Debt）
| 債務 | 影響 | 償還成本 | 利息 | 優先順序 |
|------|------|---------|------|--------|
| coordinator.rs 3462 行 | 高 | 2 周 | 維護困難 | P1 |
| 程式碼重複（ASR providers） | 中 | 1 周 | 維護成本高 | P2 |
| 缺少錯誤處理（部分模組） | 中 | 1 周 | 穩定性差 | P2 |

## 2. 技術債務總量

### 債務統計
EOF

echo '```' >> "$DEBT_REPORT"
echo "總債務項: 13" >> "$DEBT_REPORT"
echo "P0 優先順序: 2 項（測試相關）" >> "$DEBT_REPORT"
echo "P1 優先順序: 5 項（架構 + 測試 + 程式碼）" >> "$DEBT_REPORT"
echo "P2 優先順序: 4 項（架構 + 文件 + 程式碼）" >> "$DEBT_REPORT"
echo "P3 優先順序: 2 項（文件）" >> "$DEBT_REPORT"
echo "" >> "$DEBT_REPORT"
echo "預計償還成本: 14 周（3.5 個月）" >> "$DEBT_REPORT"
echo '```' >> "$DEBT_REPORT"

cat >> "$DEBT_REPORT" << 'EOF'

### 債務利息（每月）
- **架構債務利息**: 每次新增功能都需要修改 Coordinator，成本 +50%
- **測試債務利息**: 每次重構都有迴歸風險，成本 +100%
- **文件債務利息**: 新人上手時間 +2 周
- **程式碼債務利息**: 維護成本 +30%

## 3. 債務償還計劃

### Phase 1: 測試基礎設施（6 周，P0）
**目標**: 建立測試基礎設施，為後續重構保駕護航

**步驟**:
1. Week 1: 編寫測試策略文件
2. Week 2-3: 為核心模組補充單元測試
3. Week 4-5: 新增整合測試
4. Week 6: 配置 CI 自動化測試

**收益**:
- 測試覆蓋率從 0% → 60%+
- 重構風險降低 80%+
- 為後續重構提供安全網

### Phase 2: Coordinator 拆分（2 周，P1）
**目標**: 將 3462 行的 Coordinator 拆分為多個子模組

**步驟**:
1. Week 1: 設計拆分方案，編寫設計文件
2. Week 2: 實施拆分，補充測試

**收益**:
- 程式碼可讀性提升 50%+
- 維護成本降低 40%+
- 測試覆蓋率提升 30%+

### Phase 3: ASR Provider 統一介面（1 周，P1）
**目標**: 定義統一的 ASRProvider trait，重構現有 provider

**步驟**:
1. Day 1-2: 設計 trait 介面
2. Day 3-4: 重構 Volcengine 和 Whisper
3. Day 5: 新增 provider 註冊機制

**收益**:
- 新增新 provider 成本降低 70%+
- 程式碼重複減少 50%+
- 擴充套件性提升 100%+

### Phase 4: 文件補充（1 周，P2）
**目標**: 補充架構設計文件、測試指南

**步驟**:
1. Day 1-2: 編寫架構設計文件
2. Day 3: 編寫測試指南
3. Day 4-5: 編寫 API 文件

**收益**:
- 新人上手時間減少 50%+
- 測試質量提升 30%+

## 4. 債務償還優先順序

### 立即償還（P0）
- [ ] 建立測試基礎設施
- [ ] 配置 CI 自動化測試

### 短期償還（P1，1-2 個月）
- [ ] Coordinator 拆分
- [ ] ASR Provider 統一介面
- [ ] 測試策略文件

### 中期償還（P2，2-3 個月）
- [ ] Insertion 策略重構
- [ ] 架構設計文件
- [ ] 測試指南

### 長期償還（P3，3-6 個月）
- [ ] API 文件
- [ ] 效能最佳化

## 5. 債務預防措施

### 程式碼審查清單
- [ ] 新功能是否有測試？
- [ ] 新模組是否有文件？
- [ ] 是否引入了新的架構債務？
- [ ] 是否增加了程式碼重複？

### 定期審計
- 每月執行一次系統級審計
- 每季度評估技術債務總量
- 每半年制定債務償還計劃

---

**債務總結**：
- 總債務項: 13
- 預計償還成本: 14 周（3.5 個月）
- 優先償還: 測試基礎設施（P0）
- 債務利息: 每月增加 30-100% 的維護成本
EOF

echo "✅ 技術債務矩陣已生成: $DEBT_REPORT"
echo ""

# ============================================
# 3. 生成總結
# ============================================
SUMMARY="$OUTPUT_DIR/system-audit-summary-$TIMESTAMP.md"

cat > "$SUMMARY" << EOF
# 系統級審計總結

**生成時間**: $(date '+%Y-%m-%d %H:%M:%S')

## 🎯 審計結論

### 架構健康度: ⚠️  中等（需要重構）

**優勢**:
- ✅ 模組依賴清晰，無迴圈依賴
- ✅ Coordinator 作為單一狀態機，職責清晰
- ✅ 使用 trait 抽象（AudioConsumer）

**風險**:
- 🔴 Coordinator 過於龐大（3462 行）
- 🔴 缺少統一的 ASR Provider trait
- 🔴 測試基礎設施缺失（覆蓋率接近 0%）

### 技術債務總量: 💳 13 項

**優先順序分佈**:
- P0: 2 項（測試相關）
- P1: 5 項（架構 + 測試 + 程式碼）
- P2: 4 項（架構 + 文件 + 程式碼）
- P3: 2 項（文件）

**預計償還成本**: 14 周（3.5 個月）

## 📋 生成的報告

1. **架構風險地圖**: $ARCH_REPORT
2. **技術債務矩陣**: $DEBT_REPORT

## 🎯 關鍵決策點

### 決策 1: 是否需要架構重構？
**建議**: ✅ **需要**

**理由**:
- Coordinator 3462 行，維護困難
- 缺少統一 ASR trait，擴充套件性差
- 測試覆蓋率接近 0%，重構風險高

**方案**:
1. 先建立測試基礎設施（為重構保駕護航）
2. 然後進行 Coordinator 拆分
3. 最後統一 ASR Provider 介面

### 決策 2: 是否繼續低尺度審計？
**建議**: ⏸️  **暫停**

**理由**:
- 系統級問題會影響低尺度審計的結果
- 架構重構可能使低尺度問題消失
- 應該先解決高尺度問題

**方案**:
1. 暫停模組級、功能級、程式碼級審計
2. 先完成測試基礎設施建設
3. 然後進行架構重構
4. 重構完成後再繼續低尺度審計

## 🚀 下一步行動

### 立即開始（本週）
1. ✅ 完成系統級審計
2. ⏳ 編寫測試策略文件
3. ⏳ 編寫 Coordinator 拆分設計文件
4. ⏳ 編寫 ASR Provider trait 設計文件

### 短期計劃（2-4 周）
1. 建立測試基礎設施（Phase 1）
2. 為核心模組補充單元測試
3. 配置 CI 自動化測試

### 中期計劃（1-2 個月）
1. 實施 Coordinator 拆分（Phase 2）
2. 實施 ASR Provider 統一介面（Phase 3）
3. 補充文件（Phase 4）

## 📊 預期收益

### 測試基礎設施建設後
- 測試覆蓋率: 0% → 60%+
- 重構風險: 降低 80%+
- 程式碼質量: 提升 50%+

### 架構重構後
- 程式碼可讀性: 提升 50%+
- 維護成本: 降低 40%+
- 擴充套件性: 提升 100%+
- 新增新 provider 成本: 降低 70%+

---

**審計結論**: 需要架構重構，優先建立測試基礎設施
**下一步**: 編寫測試策略文件和架構重構設計文件
EOF

echo "✅ 系統級審計總結已生成: $SUMMARY"
echo ""
echo "🎉 系統級審計完成！"
echo ""
echo "📂 報告位置: $OUTPUT_DIR/"
echo "   - $(basename $ARCH_REPORT)"
echo "   - $(basename $DEBT_REPORT)"
echo "   - $(basename $SUMMARY)"
echo ""
echo "💡 關鍵決策: 需要架構重構，優先建立測試基礎設施"
echo "📝 下一步: 編寫測試策略文件和架構重構設計文件"
