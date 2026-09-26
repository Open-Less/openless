#!/bin/bash
# Finding 辅助脚本 - 自动收集项目信息用于 EPIC 规划

set -e

TAURI_DIR="openless-all/app/src-tauri"
OUTPUT_DIR=".github/finding-reports"

mkdir -p "$OUTPUT_DIR"

echo "🔍 开始 Finding 分析..."
echo ""

# ============================================
# 1. 测试覆盖率分析
# ============================================
echo "📊 分析测试覆盖率..."

REPORT_FILE="$OUTPUT_DIR/test-coverage-$(date +%Y%m%d).md"

cat > "$REPORT_FILE" << 'EOF'
# 测试覆盖率 Finding 报告

## 生成时间
EOF

echo "$(date '+%Y-%m-%d %H:%M:%S')" >> "$REPORT_FILE"

cat >> "$REPORT_FILE" << 'EOF'

## 1. 现有测试文件统计

### Rust 测试模块
EOF

echo '```' >> "$REPORT_FILE"
find "$TAURI_DIR/src" -name "*.rs" -exec grep -l "#\[cfg(test)\]" {} \; | \
  sed "s|$TAURI_DIR/src/||" >> "$REPORT_FILE"
echo '```' >> "$REPORT_FILE"

cat >> "$REPORT_FILE" << 'EOF'

### 测试数量统计
EOF

echo '```' >> "$REPORT_FILE"
echo "包含测试的文件数: $(find "$TAURI_DIR/src" -name "*.rs" -exec grep -l "#\[cfg(test)\]" {} \; | wc -l)" >> "$REPORT_FILE"
echo "测试模块数: $(grep -r "#\[cfg(test)\]" "$TAURI_DIR/src" | wc -l)" >> "$REPORT_FILE"
echo "测试函数数: $(grep -r "#\[test\]" "$TAURI_DIR/src" | wc -l)" >> "$REPORT_FILE"
echo '```' >> "$REPORT_FILE"

cat >> "$REPORT_FILE" << 'EOF'

## 2. 核心模块代码量

EOF

echo '```' >> "$REPORT_FILE"
find "$TAURI_DIR/src" -name "*.rs" -exec wc -l {} + | sort -rn | head -20 >> "$REPORT_FILE"
echo '```' >> "$REPORT_FILE"

cat >> "$REPORT_FILE" << 'EOF'

## 3. 需要补测试的优先级模块

### 高优先级（核心功能）
- [ ] recorder.rs - 音频采集、watchdog
- [ ] coordinator.rs - 状态机、会话管理
- [ ] asr/volcengine.rs - WebSocket ASR
- [ ] asr/frame.rs - 二进制帧编解码

### 中优先级（工具模块）
- [ ] persistence.rs - 数据持久化
- [ ] types.rs - 类型定义、状态转换
- [ ] insertion.rs - 文本插入
- [ ] polish.rs - 文本润色

### 低优先级（平台特定）
- [ ] hotkey.rs - 热键监听
- [ ] permissions.rs - 权限检查
- [ ] windows_ime_*.rs - Windows IME

## 4. 测试工具调研

### 推荐工具
- **mockall**: Mock 框架，用于 mock 外部依赖
- **proptest**: 属性测试，生成随机测试数据
- **criterion**: 性能基准测试
- **cargo-llvm-cov**: 代码覆盖率工具

### 安装命令
```bash
cargo install cargo-llvm-cov
```

## 5. 下一步行动

1. 为 recorder.rs 编写单元测试（T1.1-T1.6）
2. 为 asr/frame.rs 扩展测试（T1.7-T1.10）
3. 建立测试编写规范文档
4. 配置 CI 自动化测试

EOF

echo "✅ 测试覆盖率报告已生成: $REPORT_FILE"
echo ""

# ============================================
# 2. ASR 模块分析
# ============================================
echo "🎤 分析 ASR 模块..."

ASR_REPORT="$OUTPUT_DIR/asr-analysis-$(date +%Y%m%d).md"

cat > "$ASR_REPORT" << 'EOF'
# ASR 模块 Finding 报告

## 生成时间
EOF

echo "$(date '+%Y-%m-%d %H:%M:%S')" >> "$ASR_REPORT"

cat >> "$ASR_REPORT" << 'EOF'

## 1. ASR 模块结构

EOF

echo '```' >> "$ASR_REPORT"
ls -lh "$TAURI_DIR/src/asr/" >> "$ASR_REPORT"
echo '```' >> "$ASR_REPORT"

cat >> "$ASR_REPORT" << 'EOF'

## 2. ASR 模块代码量

EOF

echo '```' >> "$ASR_REPORT"
wc -l "$TAURI_DIR/src/asr"/*.rs >> "$ASR_REPORT"
echo '```' >> "$ASR_REPORT"

cat >> "$ASR_REPORT" << 'EOF'

## 3. ASR Provider 接口分析

### 当前接口
- `AudioConsumer` trait: 接收 PCM 数据
- `RawTranscript` struct: ASR 输出结果

### 问题
- 缺少统一的 ASRProvider trait
- Volcengine 和 Whisper 实现重复代码
- 扩展新 provider 需要大量手工集成

### 改进建议
定义统一的 `ASRProvider` trait，包含：
- `open_session()`: 打开会话
- `get_audio_consumer()`: 获取音频消费者
- `close_session()`: 关闭会话并获取结果
- `cancel_session()`: 取消会话

## 4. 混淆词纠错层设计

### 插入位置
`coordinator.rs:616-617` - ASR 结果进入 polish 之前

### 数据结构
```rust
struct CorrectionRule {
    pattern: String,        // 错误模式（支持正则）
    replacement: String,    // 正确词汇
    context: Option<Vec<String>>,  // 上下文关键词
    enabled: bool,
}
```

### 内置混淆词表（初版）
- issue / iOS
- PR / 批阅
- CI / 西爱
- commit / 靠米特
- merge / 摸鸡
- release / 瑞丽丝

## 5. 本地 ASR 技术选型

### 候选方案

| 项目 | 形态 | 平台 | 加速 | License | 备注 |
|---|---|---|---|---|---|
| whisper.cpp | C/C++ | 全平台 | Metal/CoreML/CUDA | MIT | 主流候选 |
| whisper-rs | Rust binding | 全平台 | 同上 | MIT/Apache-2.0 | Rust 集成更顺 |
| sherpa-onnx | C++ + ONNX | 全平台 | CoreML/CUDA | Apache-2.0 | 多模型支持 |

### 推荐方案
**whisper-rs** - Rust 原生集成，跨平台支持好

### 集成方式
1. Rust crate 直接绑定（推荐）
2. 子进程 + HTTP（备选）

## 6. 下一步行动

### Phase 1: 混淆词纠错（Week 1）
1. 收集 50+ 真实错词样本
2. 实现 `asr/correction.rs` 模块
3. 集成到 coordinator
4. 编写测试

### Phase 2: 本地 ASR（Week 2-4）
1. 完成技术选型文档 `docs/local-asr-plan.md`
2. 测试 whisper-rs 性能
3. 实现模型下载管理
4. 实现本地推理
5. 跨平台测试

EOF

echo "✅ ASR 模块报告已生成: $ASR_REPORT"
echo ""

# ============================================
# 3. 依赖关系分析
# ============================================
echo "🔗 分析模块依赖关系..."

DEP_REPORT="$OUTPUT_DIR/dependencies-$(date +%Y%m%d).md"

cat > "$DEP_REPORT" << 'EOF'
# 模块依赖关系 Finding 报告

## 生成时间
EOF

echo "$(date '+%Y-%m-%d %H:%M:%S')" >> "$DEP_REPORT"

cat >> "$DEP_REPORT" << 'EOF'

## 1. Cargo 依赖

EOF

echo '```toml' >> "$DEP_REPORT"
grep -A 50 "\[dependencies\]" "$TAURI_DIR/Cargo.toml" | head -60 >> "$DEP_REPORT"
echo '```' >> "$DEP_REPORT"

cat >> "$DEP_REPORT" << 'EOF'

## 2. 模块间依赖（通过 use 语句分析）

### coordinator.rs 依赖
EOF

echo '```' >> "$DEP_REPORT"
grep "^use crate::" "$TAURI_DIR/src/coordinator.rs" | sort | uniq >> "$DEP_REPORT"
echo '```' >> "$DEP_REPORT"

cat >> "$DEP_REPORT" << 'EOF'

### recorder.rs 依赖
EOF

echo '```' >> "$DEP_REPORT"
grep "^use crate::" "$TAURI_DIR/src/recorder.rs" | sort | uniq >> "$DEP_REPORT"
echo '```' >> "$DEP_REPORT"

cat >> "$DEP_REPORT" << 'EOF'

## 3. Mock 策略建议

### 需要 Mock 的外部依赖
- **Volcengine ASR WebSocket**: 使用 mock WebSocket server
- **OpenAI Polish API**: 使用 mock HTTP server
- **Keychain**: 使用 trait abstraction + mock 实现
- **Clipboard**: 使用 trait abstraction + mock 实现
- **Audio Device**: 使用 mock audio stream

### 推荐工具
- `mockall`: 自动生成 mock
- `wiremock`: HTTP mock server
- `tokio-test`: 异步测试工具

EOF

echo "✅ 依赖关系报告已生成: $DEP_REPORT"
echo ""

# ============================================
# 4. 生成总结
# ============================================
SUMMARY="$OUTPUT_DIR/finding-summary-$(date +%Y%m%d).md"

cat > "$SUMMARY" << EOF
# Finding 总结报告

**生成时间**: $(date '+%Y-%m-%d %H:%M:%S')

## 📊 关键指标

- **包含测试的文件数**: $(find "$TAURI_DIR/src" -name "*.rs" -exec grep -l "#\[cfg(test)\]" {} \; | wc -l)
- **测试函数数**: $(grep -r "#\[test\]" "$TAURI_DIR/src" | wc -l)
- **核心模块数**: $(find "$TAURI_DIR/src" -maxdepth 1 -name "*.rs" | wc -l)
- **ASR 模块代码量**: $(wc -l "$TAURI_DIR/src/asr"/*.rs | tail -1 | awk '{print $1}') 行

## 📋 生成的报告

1. **测试覆盖率报告**: $REPORT_FILE
2. **ASR 模块分析**: $ASR_REPORT
3. **依赖关系分析**: $DEP_REPORT

## 🎯 下一步行动

### 立即开始（Week 1）
1. 阅读生成的 3 份报告
2. 更新 EPIC-001 和 EPIC-002 的 Finding 任务状态
3. 开始实现混淆词纠错层（快速产出）

### 短期计划（Week 2-3）
1. 为 recorder.rs 补测试
2. 为 asr/frame.rs 补测试
3. 编写测试规范文档

### 中期计划（Week 4-6）
1. 完成本地 ASR 技术选型
2. 实现本地 ASR 支持
3. 建立 CI 自动化测试

## 📝 备注

所有报告已保存到 \`.github/finding-reports/\` 目录。
EOF

echo "✅ 总结报告已生成: $SUMMARY"
echo ""
echo "🎉 Finding 分析完成！"
echo ""
echo "📂 报告位置: $OUTPUT_DIR/"
echo "   - $(basename $REPORT_FILE)"
echo "   - $(basename $ASR_REPORT)"
echo "   - $(basename $DEP_REPORT)"
echo "   - $(basename $SUMMARY)"
echo ""
echo "💡 下一步: 阅读报告并更新 EPIC 文档"
