/*
 * SPDX-FileCopyrightText: 2025 OpenLess Contributors
 *
 * SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * 选区新鲜度探针：插件自己看一次合成器上的 PRIMARY 选区。
 *
 * ## 为什么必须有它
 *
 * fcitx5 的 clipboard addon 把 PRIMARY 文本缓存在自己的 `primary_` 里，而
 * `IClipboard` 的公共接口（`/usr/include/Fcitx5/Module/fcitx-module/clipboard/
 * clipboard_public.h`）只有 `primary()`/`setPrimary()`，**没有任何"选区变了"或
 * "这份缓存还有效吗"的通知**。更糟的是它的更新逻辑：
 * `waylandclipboard.cpp` 的 `DataOffer::receiveRealData()` 在 offer 既没有
 * `text/plain;charset=utf-8` 也没有 `text/plain` 时**直接 return**，回调不触发，
 * 于是 `setPrimary()` 不会被调用 —— 缓存里留着**上一次**的文本。
 *
 * 后果：用户先选中一段文字，再选中一张图片（或任何没有 text mime 的东西），
 * 此时 `IClipboard::primary()` 仍然返回那段旧文字，插件会把旧选区当成当前选区。
 *
 * ## 探针做了什么
 *
 * 用 `ext_data_control_device_v1`（KWin 6 提供；协议规定绑定 device 后立即补发
 * 一次 `primary_selection`，所以一次 roundtrip 就能拿到当前状态）读取**此刻**的
 * PRIMARY offer：
 *
 * - 拿到 mime 列表 → 能区分「没有 text mime（图片/文件/密码）」与「真的没有选区」，
 *   这正是 ② 需要的"失效"判据；
 * - 顺手把文本自己读出来（`receive` + 管道）→ 不依赖 fcitx5 缓存"过一会儿才跟上"
 *   的时序，拿到的就是当前选区。
 *
 * 探针不可用时（X11 会话、合成器没有 ext-data-control、合成器不支持 primary
 * selection、读数据失败）返回 `Unsupported`/`ReadFailed`，调用方退回旧行为
 * （fcitx5 缓存 + 应用自己报的 surrounding text），不会比现在更差。
 */
#ifndef OPENLESS_PRIMARY_SELECTION_H
#define OPENLESS_PRIMARY_SELECTION_H

#include <string>

namespace openless_selection {

/// 读一次 PRIMARY 的结果。
enum class PrimarySelectionStatus {
    /// 探针机制不可用：没有 Wayland 环境、合成器没有 ext-data-control、没有 seat，
    /// 或合成器不支持 primary selection。此时无法判断，调用方走旧行为。
    Unsupported,
    /// 合成器明确说当前没有 PRIMARY 选区（offer 为 NULL）。
    NoSelection,
    /// 有 PRIMARY 选区，但没有可读的 text mime（图片、文件、密码管理器…）。
    /// 这是"当前选区不是文本"的确凿证据。
    NoText,
    /// 有 text mime，但数据没能在超时内读完（来源应用卡住等）。
    ReadFailed,
    /// 读到了文本，`text` 即当前 PRIMARY 内容（可能为空字符串）。
    Text,
};

struct PrimarySelectionSnapshot {
    PrimarySelectionStatus status = PrimarySelectionStatus::Unsupported;
    std::string text;
    /// 诊断用：mime 列表或失败原因。只进日志，不参与判断。
    std::string detail;
};

/// 一次选区文本捕获的最终来源判定。
struct SelectionSource {
    /// 交给 Core 的选区文本；空表示"没有可用的文本选区"。
    std::string text;
    /// 命中的规则名，仅用于日志/诊断。
    const char *rule = "none";
};

/// 纯函数：给定一次探针结果和应用自己报的选中文本，决定这次捕获用哪个文本。
///
/// 规则（③ 的核心：不再无条件优先应用的 surrounding text）：
/// 1. 探针读到非空文本 → 用探针的文本。surrounding 只在日志里作为"两者是否一致"
///    的对照，不参与取值：它是客户端自己报的**本地缓存**，而探针是此刻的合成器真值。
/// 2. 探针明确说"有选区但没有 text mime" → 判失效，返回空。**既不回退 fcitx5 缓存，
///    也不回退 surrounding**：那正是"旧选区被当成当前选区"的来源。
/// 3. 其余情况（探针不可用/无选区/读失败，或探针读到空文本）→ 用应用报的非空选中
///    文本（仍是当前真值，很多应用不导出 PRIMARY，例如 XIM、部分工具包）。
/// 4. 都没有 → 空（没有选区）。
SelectionSource chooseSelectionSource(const PrimarySelectionSnapshot &primary,
                                      const std::string &surroundingSelectedText);

/// 读一次合成器上的当前 PRIMARY 选区。总耗时受
/// `kPrimarySelectionReadTimeoutMs` 约束（读取数据的等待上限）。
PrimarySelectionSnapshot readPrimarySelection();

/// 等待来源应用把选区数据写进管道的时间上限。参考 fcitx5 clipboard 模块用的 1s：
/// 这里更短，因为本函数跑在 DBus 方法里（会短暂占用 fcitx5 主循环），
/// 而超时的后果只是"这次拿不到选区"，不会损坏任何状态。
inline constexpr int kPrimarySelectionReadTimeoutMs = 150;

} // namespace openless_selection

#endif // OPENLESS_PRIMARY_SELECTION_H
