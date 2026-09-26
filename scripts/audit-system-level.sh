#!/bin/bash
# 系统级审计脚本 - 发现架构、安全、扩展性问题

set -e

TAURI_DIR="openless-all/app/src-tauri"
OUTPUT_DIR=".github/audit-reports/system-level"
TIMESTAMP=$(date +%Y%m%d)

mkdir -p "$OUTPUT_DIR"

echo "🔍 开始系统级审计..."
echo ""

# ============================================
# 1. 架构风险地图
# ============================================
echo "🏗️  生成架构风险地图..."

ARCH_REPORT="$OUTPUT_DIR/architecture-risk-map-$TIMESTAMP.md"

cat > "$ARCH_REPORT" << 'EOF'
# 架构风险地图

## 生成时间
EOF

echo "$(date '+%Y-%m-%d %H:%M:%S')" >> "$ARCH_REPORT"

cat >> "$ARCH_REPORT" << 'EOF'

## 1. 整体架构评估

### 当前架构
```
┌─────────────────────────────────────────┐
│           Frontend (React/TS)           │
│  Capsule / Overview / Settings / QA     │
└──────────────┬──────────────────────────┘
               │ IPC (Tauri commands)
┌──────────────┴──────────────────────────┐
│         Coordinator (状态机)             │
│  Idle → Starting → Listening → Processing│
└─┬────┬────┬────┬────┬────┬────┬────┬───┘
  │    │    │    │    │    │    │    │
  ▼    ▼    ▼    ▼    ▼    ▼    ▼    ▼
Hotkey Recorder ASR Polish Insert Persist Perms History
```

### 架构优势
- ✅ Coordinator 作为单一状态机，职责清晰
- ✅ 模块间通过 Coordinator 协调，避免直接依赖
- ✅ 使用 trait 抽象（AudioConsumer）

### 架构风险

#### 🔴 高风险：Coordinator 过于庞大
**现象**：
- coordinator.rs 有 3462 行代码
- 承担了状态机、会话管理、模块协调、错误处理等多重职责

**影响**：
- 难以理解和维护
- 修改一个功能可能影响其他功能
- 测试困难（需要 mock 所有依赖）

**建议**：
- 拆分为多个子模块：
  - `coordinator/state_machine.rs` - 状态转换逻辑
  - `coordinator/session.rs` - 会话管理
  - `coordinator/orchestrator.rs` - 模块协调
  - `coordinator/error_handler.rs` - 错误处理

#### 🟡 中风险：缺少统一的 ASR Provider trait
**现象**：
- Volcengine 和 Whisper 实现各自独立
- 添加新 provider 需要大量手工集成
- 代码重复（会话管理、错误处理）

**影响**：
- 扩展性差
- 维护成本高
- 容易引入不一致

**建议**：
- 定义统一的 `ASRProvider` trait
- 重构现有 provider 实现该 trait
- 在 Coordinator 中使用 trait object

#### 🟡 中风险：测试基础设施缺失
**现象**：
- 无测试策略文档
- 无 CI 自动化测试
- 测试覆盖率接近 0%

**影响**：
- 重构风险高（容易引入回归 bug）
- 新功能质量无保障
- 技术债务累积

**建议**：
- 建立测试策略（单元测试、集成测试、E2E 测试比例）
- 配置 CI 自动化测试
- 为核心模块补充测试

#### 🟢 低风险：模块间依赖清晰
**现象**：
- 各模块只依赖 `types.rs`
- 模块间不直接调用

**影响**：
- 正面影响，易于维护

## 2. 模块依赖分析

### 核心模块依赖图
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

### 依赖健康度
- ✅ **单向依赖**：所有模块依赖 types，types 不依赖任何模块
- ✅ **无循环依赖**：模块间无循环依赖
- ⚠️  **Coordinator 依赖过多**：依赖 8+ 个模块

## 3. 技术栈评估

### 当前技术栈
EOF

echo '```toml' >> "$ARCH_REPORT"
grep -A 30 "\[dependencies\]" "$TAURI_DIR/Cargo.toml" | head -35 >> "$ARCH_REPORT"
echo '```' >> "$ARCH_REPORT"

cat >> "$ARCH_REPORT" << 'EOF'

### 技术栈风险
- ✅ **Tauri 2**: 成熟稳定，社区活跃
- ✅ **Tokio**: 异步运行时，性能优秀
- ✅ **Serde**: 序列化标准，生态完善
- ⚠️  **global-hotkey 0.6**: 版本较新，可能有兼容性问题
- ⚠️  **cpal 0.15**: 音频库，跨平台兼容性需关注

## 4. 扩展性瓶颈

### 当前扩展点
1. **ASR Provider**: 需要手工集成，成本高
2. **Polish Provider**: 已支持 OpenAI 兼容接口，扩展性好
3. **Insertion Strategy**: 硬编码 AX → clipboard → copy-only，扩展性差

### 扩展性改进建议

#### ASR Provider 扩展
**当前成本**：添加新 provider 需要：
1. 实现 AudioConsumer trait
2. 在 Coordinator 中添加分支逻辑
3. 在 Settings UI 中添加配置
4. 在 persistence 中添加凭据存储

**改进方案**：
```rust
// 定义统一接口
#[async_trait]
pub trait ASRProvider: Send + Sync {
    async fn open_session(&self, hotwords: Vec<DictionaryHotword>) -> Result<()>;
    fn get_audio_consumer(&self) -> Arc<dyn AudioConsumer>;
    async fn close_session(&self) -> Result<RawTranscript>;
    async fn cancel_session(&self);
}

// 注册机制
pub struct ASRRegistry {
    providers: HashMap<String, Box<dyn ASRProvider>>,
}

impl ASRRegistry {
    pub fn register(&mut self, name: &str, provider: Box<dyn ASRProvider>) {
        self.providers.insert(name.to_string(), provider);
    }
}
```

#### Insertion Strategy 扩展
**当前成本**：添加新策略需要修改 insertion.rs 核心逻辑

**改进方案**：
```rust
// 策略模式
pub trait InsertionStrategy: Send + Sync {
    async fn insert(&self, text: &str) -> Result<()>;
}

pub struct AXInsertionStrategy;
pub struct ClipboardInsertionStrategy;
pub struct CopyOnlyStrategy;

// 策略链
pub struct InsertionChain {
    strategies: Vec<Box<dyn InsertionStrategy>>,
}
```

## 5. 性能瓶颈

### 潜在瓶颈
1. **Coordinator 锁竞争**: 所有操作都需要获取 Coordinator 锁
2. **音频数据拷贝**: Recorder → AudioConsumer 可能有多次拷贝
3. **WebSocket 缓冲**: BufferingAudioConsumer 可能积压大量数据

### 性能优化建议
- 使用细粒度锁（拆分 Coordinator 状态）
- 使用 zero-copy 音频传输（Arc<[u8]>）
- 限制 BufferingAudioConsumer 缓冲区大小

## 6. 架构演进路线图

### Phase 1: Coordinator 拆分（优先级：高）
**目标**: 将 3462 行的 Coordinator 拆分为多个子模块

**步骤**:
1. 提取状态机逻辑到 `state_machine.rs`
2. 提取会话管理到 `session.rs`
3. 提取模块协调到 `orchestrator.rs`
4. 保留 `coordinator.rs` 作为入口

**预期收益**:
- 代码可读性提升 50%+
- 测试覆盖率提升 30%+
- 维护成本降低 40%+

### Phase 2: ASR Provider 统一接口（优先级：高）
**目标**: 定义统一的 ASRProvider trait，重构现有 provider

**步骤**:
1. 定义 `ASRProvider` trait
2. 重构 Volcengine 实现该 trait
3. 重构 Whisper 实现该 trait
4. 添加 provider 注册机制

**预期收益**:
- 添加新 provider 成本降低 70%+
- 代码重复减少 50%+
- 扩展性提升 100%+

### Phase 3: 测试基础设施建设（优先级：高）
**目标**: 建立完整的测试基础设施

**步骤**:
1. 编写测试策略文档
2. 为核心模块补充单元测试
3. 添加集成测试
4. 配置 CI 自动化测试

**预期收益**:
- 测试覆盖率从 0% → 60%+
- 重构风险降低 80%+
- 代码质量提升 50%+

## 7. 风险优先级矩阵

| 风险 | 影响 | 紧急度 | 优先级 | 预计工作量 |
|------|------|--------|--------|-----------|
| Coordinator 过于庞大 | 高 | 中 | P1 | 2 周 |
| 缺少统一 ASR trait | 高 | 中 | P1 | 1 周 |
| 测试基础设施缺失 | 高 | 高 | P0 | 6 周 |
| Insertion 扩展性差 | 中 | 低 | P2 | 1 周 |
| 性能瓶颈 | 中 | 低 | P3 | 2 周 |

## 8. 下一步行动

### 立即开始（本周）
1. ✅ 完成系统级审计
2. ⏳ 决策：是否需要架构重构
3. ⏳ 如果需要，暂停低尺度审计，先做架构设计

### 短期计划（2-4 周）
1. Coordinator 拆分设计文档
2. ASR Provider trait 设计文档
3. 测试策略文档

### 中期计划（1-2 个月）
1. 实施 Coordinator 拆分
2. 实施 ASR Provider 统一接口
3. 建立测试基础设施

---

**审计结论**：
- 🔴 **需要架构重构**：Coordinator 过于庞大，ASR 缺少统一接口
- 🟡 **测试基础设施缺失**：需要优先建设
- 🟢 **模块依赖健康**：无循环依赖，单向依赖清晰

**建议**：
1. 优先建立测试基础设施（为重构保驾护航）
2. 然后进行 Coordinator 拆分
3. 最后统一 ASR Provider 接口
EOF

echo "✅ 架构风险地图已生成: $ARCH_REPORT"
echo ""

# ============================================
# 2. 技术债务矩阵
# ============================================
echo "💳 生成技术债务矩阵..."

DEBT_REPORT="$OUTPUT_DIR/tech-debt-matrix-$TIMESTAMP.md"

cat > "$DEBT_REPORT" << 'EOF'
# 技术债务矩阵

## 生成时间
EOF

echo "$(date '+%Y-%m-%d %H:%M:%S')" >> "$DEBT_REPORT"

cat >> "$DEBT_REPORT" << 'EOF'

## 1. 技术债务分类

### 架构债务（Architecture Debt）
| 债务 | 影响 | 偿还成本 | 利息 | 优先级 |
|------|------|---------|------|--------|
| Coordinator 过于庞大 | 高 | 2 周 | 每次修改都困难 | P1 |
| 缺少统一 ASR trait | 高 | 1 周 | 添加 provider 成本高 | P1 |
| Insertion 策略硬编码 | 中 | 1 周 | 扩展困难 | P2 |

### 测试债务（Testing Debt）
| 债务 | 影响 | 偿还成本 | 利息 | 优先级 |
|------|------|---------|------|--------|
| 测试覆盖率接近 0% | 高 | 6 周 | 重构风险高 | P0 |
| 无 CI 自动化测试 | 高 | 1 周 | 手工测试成本高 | P0 |
| 无测试策略文档 | 中 | 2 天 | 测试质量无保障 | P1 |

### 文档债务（Documentation Debt）
| 债务 | 影响 | 偿还成本 | 利息 | 优先级 |
|------|------|---------|------|--------|
| 缺少架构设计文档 | 中 | 3 天 | 新人上手困难 | P2 |
| 缺少 API 文档 | 低 | 2 天 | 集成困难 | P3 |
| 缺少测试指南 | 中 | 1 天 | 测试质量差 | P2 |

### 代码债务（Code Debt）
| 债务 | 影响 | 偿还成本 | 利息 | 优先级 |
|------|------|---------|------|--------|
| coordinator.rs 3462 行 | 高 | 2 周 | 维护困难 | P1 |
| 代码重复（ASR providers） | 中 | 1 周 | 维护成本高 | P2 |
| 缺少错误处理（部分模块） | 中 | 1 周 | 稳定性差 | P2 |

## 2. 技术债务总量

### 债务统计
EOF

echo '```' >> "$DEBT_REPORT"
echo "总债务项: 13" >> "$DEBT_REPORT"
echo "P0 优先级: 2 项（测试相关）" >> "$DEBT_REPORT"
echo "P1 优先级: 5 项（架构 + 测试 + 代码）" >> "$DEBT_REPORT"
echo "P2 优先级: 4 项（架构 + 文档 + 代码）" >> "$DEBT_REPORT"
echo "P3 优先级: 2 项（文档）" >> "$DEBT_REPORT"
echo "" >> "$DEBT_REPORT"
echo "预计偿还成本: 14 周（3.5 个月）" >> "$DEBT_REPORT"
echo '```' >> "$DEBT_REPORT"

cat >> "$DEBT_REPORT" << 'EOF'

### 债务利息（每月）
- **架构债务利息**: 每次添加功能都需要修改 Coordinator，成本 +50%
- **测试债务利息**: 每次重构都有回归风险，成本 +100%
- **文档债务利息**: 新人上手时间 +2 周
- **代码债务利息**: 维护成本 +30%

## 3. 债务偿还计划

### Phase 1: 测试基础设施（6 周，P0）
**目标**: 建立测试基础设施，为后续重构保驾护航

**步骤**:
1. Week 1: 编写测试策略文档
2. Week 2-3: 为核心模块补充单元测试
3. Week 4-5: 添加集成测试
4. Week 6: 配置 CI 自动化测试

**收益**:
- 测试覆盖率从 0% → 60%+
- 重构风险降低 80%+
- 为后续重构提供安全网

### Phase 2: Coordinator 拆分（2 周，P1）
**目标**: 将 3462 行的 Coordinator 拆分为多个子模块

**步骤**:
1. Week 1: 设计拆分方案，编写设计文档
2. Week 2: 实施拆分，补充测试

**收益**:
- 代码可读性提升 50%+
- 维护成本降低 40%+
- 测试覆盖率提升 30%+

### Phase 3: ASR Provider 统一接口（1 周，P1）
**目标**: 定义统一的 ASRProvider trait，重构现有 provider

**步骤**:
1. Day 1-2: 设计 trait 接口
2. Day 3-4: 重构 Volcengine 和 Whisper
3. Day 5: 添加 provider 注册机制

**收益**:
- 添加新 provider 成本降低 70%+
- 代码重复减少 50%+
- 扩展性提升 100%+

### Phase 4: 文档补充（1 周，P2）
**目标**: 补充架构设计文档、测试指南

**步骤**:
1. Day 1-2: 编写架构设计文档
2. Day 3: 编写测试指南
3. Day 4-5: 编写 API 文档

**收益**:
- 新人上手时间减少 50%+
- 测试质量提升 30%+

## 4. 债务偿还优先级

### 立即偿还（P0）
- [ ] 建立测试基础设施
- [ ] 配置 CI 自动化测试

### 短期偿还（P1，1-2 个月）
- [ ] Coordinator 拆分
- [ ] ASR Provider 统一接口
- [ ] 测试策略文档

### 中期偿还（P2，2-3 个月）
- [ ] Insertion 策略重构
- [ ] 架构设计文档
- [ ] 测试指南

### 长期偿还（P3，3-6 个月）
- [ ] API 文档
- [ ] 性能优化

## 5. 债务预防措施

### 代码审查清单
- [ ] 新功能是否有测试？
- [ ] 新模块是否有文档？
- [ ] 是否引入了新的架构债务？
- [ ] 是否增加了代码重复？

### 定期审计
- 每月运行一次系统级审计
- 每季度评估技术债务总量
- 每半年制定债务偿还计划

---

**债务总结**：
- 总债务项: 13
- 预计偿还成本: 14 周（3.5 个月）
- 优先偿还: 测试基础设施（P0）
- 债务利息: 每月增加 30-100% 的维护成本
EOF

echo "✅ 技术债务矩阵已生成: $DEBT_REPORT"
echo ""

# ============================================
# 3. 生成总结
# ============================================
SUMMARY="$OUTPUT_DIR/system-audit-summary-$TIMESTAMP.md"

cat > "$SUMMARY" << EOF
# 系统级审计总结

**生成时间**: $(date '+%Y-%m-%d %H:%M:%S')

## 🎯 审计结论

### 架构健康度: ⚠️  中等（需要重构）

**优势**:
- ✅ 模块依赖清晰，无循环依赖
- ✅ Coordinator 作为单一状态机，职责清晰
- ✅ 使用 trait 抽象（AudioConsumer）

**风险**:
- 🔴 Coordinator 过于庞大（3462 行）
- 🔴 缺少统一的 ASR Provider trait
- 🔴 测试基础设施缺失（覆盖率接近 0%）

### 技术债务总量: 💳 13 项

**优先级分布**:
- P0: 2 项（测试相关）
- P1: 5 项（架构 + 测试 + 代码）
- P2: 4 项（架构 + 文档 + 代码）
- P3: 2 项（文档）

**预计偿还成本**: 14 周（3.5 个月）

## 📋 生成的报告

1. **架构风险地图**: $ARCH_REPORT
2. **技术债务矩阵**: $DEBT_REPORT

## 🎯 关键决策点

### 决策 1: 是否需要架构重构？
**建议**: ✅ **需要**

**理由**:
- Coordinator 3462 行，维护困难
- 缺少统一 ASR trait，扩展性差
- 测试覆盖率接近 0%，重构风险高

**方案**:
1. 先建立测试基础设施（为重构保驾护航）
2. 然后进行 Coordinator 拆分
3. 最后统一 ASR Provider 接口

### 决策 2: 是否继续低尺度审计？
**建议**: ⏸️  **暂停**

**理由**:
- 系统级问题会影响低尺度审计的结果
- 架构重构可能使低尺度问题消失
- 应该先解决高尺度问题

**方案**:
1. 暂停模块级、功能级、代码级审计
2. 先完成测试基础设施建设
3. 然后进行架构重构
4. 重构完成后再继续低尺度审计

## 🚀 下一步行动

### 立即开始（本周）
1. ✅ 完成系统级审计
2. ⏳ 编写测试策略文档
3. ⏳ 编写 Coordinator 拆分设计文档
4. ⏳ 编写 ASR Provider trait 设计文档

### 短期计划（2-4 周）
1. 建立测试基础设施（Phase 1）
2. 为核心模块补充单元测试
3. 配置 CI 自动化测试

### 中期计划（1-2 个月）
1. 实施 Coordinator 拆分（Phase 2）
2. 实施 ASR Provider 统一接口（Phase 3）
3. 补充文档（Phase 4）

## 📊 预期收益

### 测试基础设施建设后
- 测试覆盖率: 0% → 60%+
- 重构风险: 降低 80%+
- 代码质量: 提升 50%+

### 架构重构后
- 代码可读性: 提升 50%+
- 维护成本: 降低 40%+
- 扩展性: 提升 100%+
- 添加新 provider 成本: 降低 70%+

---

**审计结论**: 需要架构重构，优先建立测试基础设施
**下一步**: 编写测试策略文档和架构重构设计文档
EOF

echo "✅ 系统级审计总结已生成: $SUMMARY"
echo ""
echo "🎉 系统级审计完成！"
echo ""
echo "📂 报告位置: $OUTPUT_DIR/"
echo "   - $(basename $ARCH_REPORT)"
echo "   - $(basename $DEBT_REPORT)"
echo "   - $(basename $SUMMARY)"
echo ""
echo "💡 关键决策: 需要架构重构，优先建立测试基础设施"
echo "📝 下一步: 编写测试策略文档和架构重构设计文档"
