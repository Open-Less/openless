//! macOS Accessibility 读取实现。
//!
//! 手写 FFI，与 `lib.rs::macos_capsule_ax` / `selection.rs::macos_ax` 同源（仓库没有
//! 引入 accessibility crate 的先例，这里保持一致）。新增的只有：`AXValue` 全文、
//! `kAXValueCFRangeType` 的 CFRange 解包、大文档走 `AXStringForRange` +
//! `AXNumberOfCharacters`，以及那两份旧代码都缺的 **messaging timeout**。
//!
//! ## 坐标系
//!
//! AX 的所有文本下标都是 **UTF-16 code unit**，而窗口算法按 char 走。中文在 UTF-16
//! 里 1 个单元、emoji 2 个，两套坐标必须显式换算 —— 见
//! [`utf16_offset_to_char_offset`](super::utf16_offset_to_char_offset)。
//!
//! ## 本文件只在 `spawn_blocking` 里跑
//!
//! 每个 AX 调用都可能阻塞到 `AX_MESSAGING_TIMEOUT_SECS`，绝不能出现在 tokio worker 上。
//! 调度由 [`super::probe_around_cursor`] 负责。

use std::ffi::{c_void, CStr};
use std::os::raw::c_char;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use core_foundation::base::TCFType;
use core_foundation::runloop::{
    kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopRunResult, CFRunLoopSource, CFRunLoopSourceRef,
};

use super::diff::{
    edit_is_within_typed_text, is_vocab_worthy, minimal_edit, multiple_edits, PositionedEdit,
};
use super::{
    evaluate_gate, plan_window, utf16_offset_to_char_offset, window_around_cursor, EditObservation,
    GateInputs, ReadOutcome, SelectionObservation, AX_MESSAGING_TIMEOUT_SECS,
    EDIT_WATCH_MAX_LIFETIME,
};

/// 超过这个 UTF-16 长度就不整篇 `AXValue` 读回来，改走 `AXStringForRange` 只取光标附近。
///
/// 在一篇十万字的文档上 `AXValue` 会把整篇跨进程拷过来，光是 marshalling 就够撞上
/// 超时；而我们最终只要几百字。阈值取得比任何合理预算都大得多，正常文档仍走简单路径。
const FULL_TEXT_MAX_UTF16: usize = 20_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocumentLength {
    Unknown,
    WithinLimit(usize),
    OverLimit(usize),
}

fn classify_document_length(total: Option<usize>, limit: usize) -> DocumentLength {
    match total {
        None => DocumentLength::Unknown,
        Some(total) if total <= limit => DocumentLength::WithinLimit(total),
        Some(total) => DocumentLength::OverLimit(total),
    }
}

/// 一条光标通知要跟最后一次文本变化隔多久，才算「用户真的把光标移开了」。
///
/// 两种通知是**成对**发出来的：打一个字，`AXValueChanged` 和 `AXSelectedTextChanged`
/// 相隔几毫秒先后到达。不设这道门槛，第二条就会被当成「光标移开」——于是每敲一个键都
/// 判定一次，而中间态全被拒，等用户真正打完时已经没有待判定的改动了。真机上就是这样
/// 一次都没学到的。
///
/// 300ms：远大于配对通知的间隔（毫秒级），远小于「停手再去点别处」的间隔。
const CARET_MOVE_QUIET: Duration = Duration::from_millis(300);

/// 「这一处改完了」的**兜底**判据：多久没动静就判一次。
///
/// 主判据是语义的 —— 光标离开这一处（见 `value_changed_shim`）。时间只用来兜住那些
/// 不发光标事件的 app。
///
/// 为什么必须有「改完了」这个概念：把「扣德克斯」改成 `Codex` 的击键序列是删掉四个字
/// → C → o → d → e → x。每一步都是一次通知，而中间态「扣德克斯 → C」「→ Co」
/// 「→ Cod」全都是形式合法的**跨文种**改动。判早了，一次改词就会产生一串半截候选。
///
/// 5 秒而不是 1 秒出头：它已经不是主判据了，放宽只会更不容易抓到中间态。用户改到一半
/// 停下来想事情，也不该被切断。
///
/// 定稿后使用多段 Myers 差分，因此「改完词继续写」中的纯插入和补标点不会再把两个
/// 不连续区域卷成一条长候选。计时从最后一次文本变化重新开始，慢速输入专名不会因首个
/// 字符后的硬截止而产生半截候选。
const EDIT_SETTLE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettlementBoundary {
    CaretMoved,
    QuietTimeout,
    ObserverEnding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum EditSettlementState {
    #[default]
    Idle,
    Editing {
        last_change: Instant,
    },
}

impl EditSettlementState {
    fn note_text_change(&mut self, now: Instant) {
        *self = Self::Editing { last_change: now };
    }

    fn should_settle(self, boundary: SettlementBoundary, now: Instant) -> bool {
        let Self::Editing { last_change } = self else {
            return false;
        };
        match boundary {
            SettlementBoundary::CaretMoved => {
                now.saturating_duration_since(last_change) >= CARET_MOVE_QUIET
            }
            SettlementBoundary::QuietTimeout => {
                now.saturating_duration_since(last_change) >= EDIT_SETTLE_TIMEOUT
            }
            SettlementBoundary::ObserverEnding => true,
        }
    }
}

/// 等「我们自己的落字生效」最多等多久，超过就以当前文档状态为基线。
///
/// 目标 app 对插入的文本做过加工时（智能引号、自动补全、字形转换），我们永远等不到
/// 那段文字原样出现。等不到就一直不锚定，等于功能静默失效 —— 宁可基线略有偏差。
const BASELINE_ANCHOR_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextAnchor {
    start: usize,
    end: usize,
}

fn locate_typed_span(text: &str, typed_text: &str, caret: Option<usize>) -> Option<TextAnchor> {
    if typed_text.is_empty() {
        return None;
    }
    let mut matches: Vec<TextAnchor> = text
        .match_indices(typed_text)
        .map(|(byte_start, value)| {
            let start = text[..byte_start].chars().count();
            TextAnchor {
                start,
                end: start + value.chars().count(),
            }
        })
        .collect();
    match (matches.len(), caret) {
        (0, _) => None,
        (1, _) => matches.pop(),
        (_, Some(caret)) => matches
            .into_iter()
            .min_by_key(|span| span.end.abs_diff(caret)),
        // 同一句在文档里出现多次而拿不到光标时，猜位置会重新退化成内容围栏。
        _ => None,
    }
}

fn positioned_edit_is_within_anchor(edit: &PositionedEdit, anchor: TextAnchor) -> bool {
    edit.source_start >= anchor.start
        && edit.source_end <= anchor.end
        && edit.source_start < edit.source_end
}

fn remap_anchor(anchor: TextAnchor, edits: &[PositionedEdit]) -> TextAnchor {
    fn map_offset(offset: usize, edits: &[PositionedEdit], prefer_end: bool) -> usize {
        let mut delta: isize = 0;
        for edit in edits {
            let source_len = edit.source_end - edit.source_start;
            let target_len = edit.target_end - edit.target_start;
            if edit.source_end <= offset {
                delta += target_len as isize - source_len as isize;
                continue;
            }
            if edit.source_start < offset && offset < edit.source_end {
                return if prefer_end {
                    edit.target_end
                } else {
                    edit.target_start
                };
            }
            break;
        }
        offset.saturating_add_signed(delta)
    }

    TextAnchor {
        start: map_offset(anchor.start, edits, false),
        end: map_offset(anchor.end, edits, true),
    }
}

#[repr(C)]
struct OpaqueAxRef(c_void);
type AxUiElementRef = *mut OpaqueAxRef;
type CFStringRef = *const c_void;
type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFTypeId = usize;
type AxError = i32;
type AxValueRef = *const c_void;

/// CoreFoundation 的 `CFRange`（`CFIndex` = `isize`）。
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CFRange {
    location: isize,
    length: isize,
}

const AX_ERROR_SUCCESS: AxError = 0;
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_AX_VALUE_CF_RANGE_TYPE: i32 = 4;
/// `kCFNumberCFIndexType` —— 按 `CFIndex`（isize）取值，与 AX 的下标宽度一致。
const K_CF_NUMBER_CF_INDEX_TYPE: i32 = 14;

/// AXObserver 的不透明句柄。
#[repr(C)]
struct OpaqueAxObserver(c_void);
type AxObserverRef = *mut OpaqueAxObserver;

type AxObserverCallback = unsafe extern "C" fn(
    observer: AxObserverRef,
    element: AxUiElementRef,
    notification: CFStringRef,
    refcon: *mut c_void,
);

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AxUiElementRef;
    fn AXUIElementGetPid(element: AxUiElementRef, pid: *mut i32) -> AxError;
    fn AXObserverCreate(
        application: i32,
        callback: AxObserverCallback,
        observer: *mut AxObserverRef,
    ) -> AxError;
    fn AXObserverAddNotification(
        observer: AxObserverRef,
        element: AxUiElementRef,
        notification: CFStringRef,
        refcon: *mut c_void,
    ) -> AxError;
    fn AXObserverRemoveNotification(
        observer: AxObserverRef,
        element: AxUiElementRef,
        notification: CFStringRef,
    ) -> AxError;
    fn AXObserverGetRunLoopSource(observer: AxObserverRef) -> CFRunLoopSourceRef;
    fn AXUIElementSetMessagingTimeout(element: AxUiElementRef, timeout: f32) -> AxError;
    fn AXUIElementCopyAttributeValue(
        element: AxUiElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AxError;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: AxUiElementRef,
        parameterized_attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> AxError;
    fn AXValueGetValue(value: AxValueRef, value_type: i32, out: *mut c_void) -> u8;
    fn AXValueCreate(value_type: i32, value_ptr: *const c_void) -> AxValueRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: CFTypeRef);
    fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
    fn CFGetTypeID(cf: CFTypeRef) -> CFTypeId;
    fn CFStringGetTypeID() -> CFTypeId;
    fn CFNumberGetTypeID() -> CFTypeId;
    fn CFStringCreateWithCString(
        allocator: CFAllocatorRef,
        cstr: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetCStringPtr(s: CFStringRef, encoding: u32) -> *const c_char;
    // 返回 `u8` 而不是 `bool`：CoreFoundation 的 `Boolean` 是 `unsigned char`，不是
    // C 的 `_Bool`。Rust 的 `bool` 要求位模式**恰好**是 0 或 1，其余一律 UB —— 拿它
    // 接一个 `unsigned char` 是在赌 CF 永远只返回 0/1。同文件的 `AXValueGetValue`
    // 早就是 `u8` 了，这两个当初照抄 `selection.rs` 抄进来的（那边至今还是 `bool`，
    // 属于本模块开头声明过「不得复制」的那类既有缺陷）。
    fn CFStringGetCString(
        s: CFStringRef,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> u8;
    fn CFStringGetLength(s: CFStringRef) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFNumberGetValue(number: CFTypeRef, number_type: i32, value_ptr: *mut c_void) -> u8;
}

/// 拿到焦点元素的结果。`Ready` 里的 ref **调用方负责 `CFRelease`**。
enum GatedElement {
    Ready(AxUiElementRef),
    Blocked(super::BlockReason),
    Unavailable(&'static str),
}

/// **拿到焦点元素的唯一入口 —— 想读宿主 app 的任何东西都必须从这里拿。**
///
/// 把「取元素」和「过闸门」焊死在一起，是因为它们分开过一次就出过事：闸门原本只装在
/// 读取路径上，手改观察器自己另开了一条取元素的路，于是在终端里听写时上下文读取被正确
/// 拦住、观察器却照样把终端全文读走。**闸门漏一条路径 = 没有闸门。**
///
/// 顺序有讲究，两段判定不能合并：
///
/// 1. 先用**前台 app**粗判一道（Secure Input、bundle 黑名单）—— 命中就一条 AX 消息都
///    不发，这是为了省事，不是最终判据；
/// 2. 拿到焦点元素后，用**元素自己的 pid** 换真正的 bundle，连同 `role` / `subrole`
///    再判一次 —— 这一道才算数。
///
/// 第二道为什么必须重新取 bundle：前台 app 是在取元素**之前**采样的，而每个 AX 调用
/// 都可能阻塞到 [`AX_MESSAGING_TIMEOUT_SECS`]。用户在这中间切了 app，第一道就会拿旧
/// app 的身份，放行一个属于新 app 的元素 —— 终端、密码管理器正是靠 bundle 黑名单拦的。
/// 拿元素自己的 pid 去问「你是谁」，这个时间窗就不存在了；顺带也修好了「焦点元素归属
/// 与前台 app 本来就可能不一致」这件事。
///
/// `AXUIElementSetMessagingTimeout` 也在这里统一设。不设就继承 AX 默认的 ~6 秒，对着
/// 一个卡死的 app 就是 6 秒冻结 —— 这是本模块最重要的一行。
unsafe fn focused_element_passing_the_gate(mut gate: GateInputs) -> GatedElement {
    if let Some(reason) = evaluate_gate(&gate) {
        return GatedElement::Blocked(reason);
    }

    let system = AXUIElementCreateSystemWide();
    if system.is_null() {
        return GatedElement::Unavailable("system-wide AX element unavailable");
    }
    // 系统级 element 上的设置会成为本进程的默认值。
    AXUIElementSetMessagingTimeout(system, AX_MESSAGING_TIMEOUT_SECS);

    let focused = copy_element_attr(system, b"AXFocusedUIElement\0");
    CFRelease(system as CFTypeRef);

    let Some(focused) = focused else {
        return GatedElement::Unavailable("no focused UI element (AX permission or no focus)");
    };
    // 显式再设一次：进程默认值只对「之后创建」的 ref 生效，对已有 ref 补一刀更稳。
    AXUIElementSetMessagingTimeout(focused, AX_MESSAGING_TIMEOUT_SECS);

    // 拿元素自己的身份重判，别再信第一道用的那个前台 app。
    //
    // **确认不了归属就不读 —— 这里必须失败关闭。** 取不到 pid 或查不到 bundle 时，
    // 如果沿用第一道那个采样值，闸门就退回按「谁在最前面」判定，等于这个修复没做；
    // 而把 `bundle_id` 清成 `None` 同样不行 —— `evaluate_gate` 对缺失的元数据是放行的
    //（见 `missing_metadata_does_not_block_by_itself`），那是另一种 fail-open。
    //
    // 代价是没有 bundle id 的进程读不到上下文。那类进程本来就很少，而「宁可不读」正是
    // 这个功能对隐私的基本承诺。
    let mut pid: i32 = 0;
    let owner = (AXUIElementGetPid(focused, &mut pid) == AX_ERROR_SUCCESS && pid > 0)
        .then(|| crate::selection::bundle_id_for_pid(pid))
        .flatten();
    let Some(owner) = owner else {
        CFRelease(focused as CFTypeRef);
        return GatedElement::Unavailable("could not confirm which app owns the focused element");
    };
    gate.bundle_id = Some(owner);
    // Secure Input 是全局状态，顺手也刷新一次 —— 同样可能在这几次 AX 调用期间才打开。
    gate.secure_input = crate::unicode_keystroke::is_secure_input_enabled();
    gate.role = copy_string_attr(focused, b"AXRole\0");
    gate.subrole = copy_string_attr(focused, b"AXSubrole\0");
    if let Some(reason) = evaluate_gate(&gate) {
        CFRelease(focused as CFTypeRef);
        return GatedElement::Blocked(reason);
    }

    GatedElement::Ready(focused)
}

/// 同步读取光标周围的文档。**只允许在 `spawn_blocking` 上下文里调用。**
///
/// `gate` 带着调用方已经填好的 `secure_input` / `bundle_id`；
/// [`focused_element_passing_the_gate`] 会补上 `role` / `subrole` 并做最终判定。
pub(super) fn read_around_cursor_blocking(budget_chars: usize, gate: GateInputs) -> ReadOutcome {
    unsafe {
        let focused = match focused_element_passing_the_gate(gate) {
            GatedElement::Ready(el) => el,
            GatedElement::Blocked(reason) => return ReadOutcome::Blocked(reason),
            GatedElement::Unavailable(why) => return ReadOutcome::Unavailable(why),
        };
        let outcome = read_document(focused, budget_chars);
        CFRelease(focused as CFTypeRef);
        outcome
    }
}

/// Review 确认前读取完整控件快照。必须与观察器相同：先过安全闸门、先问长度，绝不对
/// 超限或长度未知的控件发无界 `AXValue` 读取。
pub(super) fn read_full_document_blocking(gate: GateInputs) -> Option<String> {
    unsafe {
        let focused = match focused_element_passing_the_gate(gate) {
            GatedElement::Ready(element) => element,
            GatedElement::Blocked(_) | GatedElement::Unavailable(_) => return None,
        };
        let within_limit = matches!(
            classify_document_length(
                copy_index_attr(focused, b"AXNumberOfCharacters\0"),
                FULL_TEXT_MAX_UTF16,
            ),
            DocumentLength::WithinLimit(_)
        );
        let text = within_limit
            .then(|| copy_string_attr(focused, b"AXValue\0"))
            .flatten();
        CFRelease(focused as CFTypeRef);
        text
    }
}

unsafe fn read_document(focused: AxUiElementRef, budget_chars: usize) -> ReadOutcome {
    let Some(cursor_utf16) = copy_caret_offset(focused) else {
        return ReadOutcome::Unavailable("AXSelectedTextRange unavailable (not a text element?)");
    };
    let total_utf16 = match classify_document_length(
        copy_index_attr(focused, b"AXNumberOfCharacters\0"),
        FULL_TEXT_MAX_UTF16,
    ) {
        DocumentLength::Unknown => {
            return ReadOutcome::Unavailable(
                "AXNumberOfCharacters unavailable; refusing an unbounded AXValue read",
            );
        }
        DocumentLength::WithinLimit(total) => {
            // 小文档（绝大多数情况）：整篇读回来，按 char 精确截窗。AXValue 不可读时
            // 仍可用已知总长度走下面的有界 AXStringForRange 回落。
            if let Some(text) = copy_string_attr(focused, b"AXValue\0") {
                let cursor = utf16_offset_to_char_offset(&text, cursor_utf16);
                return ReadOutcome::Window(window_around_cursor(&text, cursor, budget_chars));
            }
            total
        }
        DocumentLength::OverLimit(total) => total,
    };

    // 回落：文档太大，或者该控件压根不给 AXValue（Electron 类常见）。改成只跟它要
    // 光标附近的一段。UTF-16 预算给两倍 —— 宁可多要一点回来自己裁，也不要因为
    // char/UTF-16 换算差把上文截秃。
    let span = plan_window(total_utf16, cursor_utf16, budget_chars.saturating_mul(2));
    if span.len == 0 {
        return ReadOutcome::Window(super::DocumentWindow {
            text: String::new(),
            cursor: 0,
        });
    }
    let Some(text) = copy_string_for_range(focused, span.start, span.len) else {
        return ReadOutcome::Unavailable("AXStringForRange unavailable");
    };
    let cursor = utf16_offset_to_char_offset(&text, span.cursor_in_span);
    ReadOutcome::Window(window_around_cursor(&text, cursor, budget_chars))
}

/// 读 `AXSelectedTextRange` 的起点 —— 没有选区时它就是光标位置（length == 0）。
unsafe fn copy_caret_offset(focused: AxUiElementRef) -> Option<usize> {
    let range = copy_selected_range(focused)?;
    caret_offset_from_location(range.location)
}

/// 把 `AXSelectedTextRange` 的 location 翻成光标偏移。**负数是「没有光标」，不是 0。**
///
/// 部分 app（尤其 Electron 那一类）在没有插入点或元素不是文本控件时返回
/// `kCFNotFound`（-1）。原本这里 `.max(0)`，等于把「不知道光标在哪」当成「光标在开头」
/// —— 于是我们读回文档**开头**那几百个字，再当作「光标附近」发给 LLM。错得静默：
/// 日志里看到的是 `before=0 after=N`，像是「上文为空」，实际是读错了地方。
///
/// 返回 `None` 让 `read_document` 走 `Unavailable` 分支：这次不发上下文，探针里也能
/// 看到原因。宁可没有上下文，不要错的上下文。
fn caret_offset_from_location(location: isize) -> Option<usize> {
    (location >= 0).then_some(location as usize)
}

unsafe fn copy_selected_range(focused: AxUiElementRef) -> Option<CFRange> {
    let value = copy_attr(focused, b"AXSelectedTextRange\0")?;
    let mut range = CFRange::default();
    let ok = AXValueGetValue(
        value as AxValueRef,
        K_AX_VALUE_CF_RANGE_TYPE,
        &mut range as *mut _ as *mut c_void,
    );
    CFRelease(value);
    (ok != 0).then_some(range)
}

/// `AXStringForRange(range)` —— 只把光标附近那段跨进程拷回来。
unsafe fn copy_string_for_range(
    focused: AxUiElementRef,
    start: usize,
    len: usize,
) -> Option<String> {
    let attr = cfstring_from_static(b"AXStringForRange\0")?;
    let range = CFRange {
        location: start as isize,
        length: len as isize,
    };
    let range_value = AXValueCreate(
        K_AX_VALUE_CF_RANGE_TYPE,
        &range as *const _ as *const c_void,
    );
    if range_value.is_null() {
        CFRelease(attr);
        return None;
    }

    let mut out: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyParameterizedAttributeValue(focused, attr, range_value, &mut out);
    CFRelease(attr);
    CFRelease(range_value);
    if err != AX_ERROR_SUCCESS || out.is_null() {
        return None;
    }

    let text = if CFGetTypeID(out) == CFStringGetTypeID() {
        cfstring_to_rust(out)
    } else {
        None
    };
    CFRelease(out);
    text
}

/// 读一个属性并保证它真的是 CFString。
///
/// 类型检查不是多余的：`AXValue` 在滑块上是数字、在复选框上是布尔。不检查就会把
/// 一个 CFNumber 当字符串解，轻则乱码重则读越界。
unsafe fn copy_string_attr(element: AxUiElementRef, attribute: &[u8]) -> Option<String> {
    let value = copy_attr(element, attribute)?;
    let text = if CFGetTypeID(value) == CFStringGetTypeID() {
        cfstring_to_rust(value)
    } else {
        None
    };
    CFRelease(value);
    text
}

/// 读一个 CFNumber 属性并按 `CFIndex` 取值。
unsafe fn copy_index_attr(element: AxUiElementRef, attribute: &[u8]) -> Option<usize> {
    let value = copy_attr(element, attribute)?;
    if CFGetTypeID(value) != CFNumberGetTypeID() {
        CFRelease(value);
        return None;
    }
    let mut out: isize = 0;
    let ok = CFNumberGetValue(
        value,
        K_CF_NUMBER_CF_INDEX_TYPE,
        &mut out as *mut _ as *mut c_void,
    );
    CFRelease(value);
    if ok != 0 && out >= 0 {
        Some(out as usize)
    } else {
        None
    }
}

/// 读一个属性，值本身就是另一个 AXUIElement（如 `AXFocusedUIElement`）。
unsafe fn copy_element_attr(element: AxUiElementRef, attribute: &[u8]) -> Option<AxUiElementRef> {
    copy_attr(element, attribute).map(|value| value as AxUiElementRef)
}

/// 读任意属性的原始 CFTypeRef。**调用方负责 `CFRelease`。**
unsafe fn copy_attr(element: AxUiElementRef, attribute: &[u8]) -> Option<CFTypeRef> {
    let attr = cfstring_from_static(attribute)?;
    let mut value: CFTypeRef = std::ptr::null();
    let err = AXUIElementCopyAttributeValue(element, attr, &mut value);
    CFRelease(attr);
    if err != AX_ERROR_SUCCESS || value.is_null() {
        None
    } else {
        Some(value)
    }
}

unsafe fn cfstring_from_static(bytes_with_nul: &[u8]) -> Option<CFStringRef> {
    let cstr = CStr::from_bytes_with_nul(bytes_with_nul).ok()?;
    let s = CFStringCreateWithCString(std::ptr::null(), cstr.as_ptr(), K_CF_STRING_ENCODING_UTF8);
    if s.is_null() {
        None
    } else {
        Some(s)
    }
}

unsafe fn cfstring_to_rust(s: CFStringRef) -> Option<String> {
    let direct = CFStringGetCStringPtr(s, K_CF_STRING_ENCODING_UTF8);
    if !direct.is_null() {
        return CStr::from_ptr(direct).to_str().ok().map(str::to_string);
    }
    let length = CFStringGetLength(s);
    if length <= 0 {
        return Some(String::new());
    }
    let max_bytes = CFStringGetMaximumSizeForEncoding(length, K_CF_STRING_ENCODING_UTF8) + 1;
    let mut buf: Vec<u8> = vec![0; max_bytes as usize];
    let ok = CFStringGetCString(
        s,
        buf.as_mut_ptr() as *mut c_char,
        max_bytes,
        K_CF_STRING_ENCODING_UTF8,
    );
    if ok == 0 {
        return None;
    }
    CStr::from_ptr(buf.as_ptr() as *const c_char)
        .to_str()
        .ok()
        .map(str::to_string)
}

// ═══════════════════════════════════════════════════════════════════════════
// 手改监听（AXObserver）
// ═══════════════════════════════════════════════════════════════════════════
//
// 形状照抄 `device_watch.rs`（CoreAudio 设备监听）：专用线程 → 注册回调（user_data
// 双重间接封装闭包胖指针）→ `CFRunLoop::run_in_mode(1s)` 轮转 + 退出 flag → 退出前
// 反注册 → 失败只 warn。那边注释解释了为什么不用 `CFRunLoopRun()` + 跨线程
// `CFRunLoopStop`：跨线程停 runloop 有竞态且会漏线程。这里一模一样。
//
// **必须保证解除**。观察器泄漏意味着我们一直持有别的 app 的 AX 引用、一直被它的每次
// 击键唤醒 —— 既是资源泄漏也是隐私问题。所以有三重保险：调用方 disarm、60 秒硬超时、
// 前台 app 一换就自杀。

/// 跨线程传递 AX 引用的载体。
///
/// `AXUIElementRef` 是 CFType，跨线程使用本身没问题（CF 引用计数是原子的），但裸指针
/// 不是 `Send`。照 `unicode_keystroke::PreviousInputSource` 的既有做法：存成 `usize`
/// + 手动 `Send`，交接前 `CFRetain`、用完 `CFRelease`。
///
/// 在调用线程上抓元素、而不是让工作线程自己去读 `AXFocusedUIElement`，是因为武装发生
/// 在落字刚结束那一刻，此时焦点一定还在目标控件上；让新线程晚几毫秒再读，用户可能
/// 已经点到别处了。
struct SendableElement(usize);
unsafe impl Send for SendableElement {}

impl SendableElement {
    /// # Safety
    /// `element` 必须是有效的 `AXUIElementRef`。本函数自己 retain，调用方的那一份
    /// 所有权不受影响（仍需自行 release）。
    unsafe fn retained(element: AxUiElementRef) -> Self {
        CFRetain(element as CFTypeRef);
        Self(element as usize)
    }

    fn as_ref(&self) -> AxUiElementRef {
        self.0 as AxUiElementRef
    }
}

impl Drop for SendableElement {
    fn drop(&mut self) {
        // SAFETY: retained 里 CFRetain 过一次，这里配对释放。
        unsafe { CFRelease(self.0 as CFTypeRef) };
    }
}

/// 观察线程持有的全部状态。回调通过 `refcon` 拿到它。
struct WatchContext {
    element: SendableElement,
    /// 停止 flag，与 [`run_edit_watch_loop`] 那个是同一个。
    ///
    /// 回调也得看它，不能只有循环看。解除信号到达时，观察线程可能正卡在
    /// `CFRunLoop::run_in_mode` 里（最长 1 秒），而这一秒内排队的 AX 通知**照样会派发
    /// 到回调**——循环末尾那道 `if !stop.load(..)` 覆盖不到这条路径。
    ///
    /// 这不是唯一防线（协调方那边还有观察器代次和「听写进行中不弹卡片」两道），但它是
    /// 最早、最便宜的一道：对不上就直接不做那次跨进程 AX 全文读取和比对。
    stop: Arc<AtomicBool>,
    /// 比对基线：**我们插完字之后**该控件的全文。
    ///
    /// 不能在武装的那一刻就定死。`inserter.insert()` 返回只代表事件发出去了，目标 app
    /// 把字放进文档要晚几十到几百毫秒；那一刻读到的是**插入之前**的文档。拿它当基线，
    /// 第一次比对出来的差异就是我们自己插的那一整段，会被当成「纯插入」直接丢掉，
    /// 用户真正改的那个词永远轮不到被看见。所以基线是「落字生效后才锚定」的。
    baseline: std::cell::RefCell<String>,
    /// 基线是否已经锚定到「落字生效后」的状态。
    anchored: std::cell::Cell<bool>,
    /// 我们本次插入内容在基线全文中的 char 范围。拿不到可靠光标或存在多个同文片段时
    /// 保持 None，后续只允许低置信度的内容回落。
    anchor: std::cell::RefCell<Option<TextAnchor>>,
    /// 武装时刻，用于给锚定兜底一个时限。
    armed_at: Instant,
    /// 我们这次实际打出去的文本。只有落在这段文字里的改动才算「用户改了我们插的东西」。
    typed_text: String,
    on_edit: Box<dyn Fn(EditObservation) + Send + Sync>,
    on_selection: Box<dyn Fn(Option<SelectionObservation>) + Send + Sync>,
    /// 最近一次已上报选区，用于过滤同一组选区通知。`None` 既表示当前无候选，也表示
    /// 下一次有效选区必须重新上报。
    last_selection: std::cell::RefCell<Option<(usize, usize, String)>>,
    /// 已上报过的 `(source, target)`。用户改一个词要敲好几下，每一下都发一次通知，
    /// 不去重会把同一处改动刷成一串日志。
    reported: std::cell::RefCell<std::collections::HashSet<(String, String)>>,
    /// 本次武装期间上报了几处改动。
    reports: std::cell::Cell<u64>,
    /// 上一次通知时看到的文本。
    ///
    /// 用来把两种通知分开 —— 这是「一次编辑结束了没有」的**主判据**：
    ///
    /// | 用户在干什么 | 文本变了 | 光标动了 |
    /// |---|---|---|
    /// | 打字 / 删字 | ✅ | ✅（跟着走） |
    /// | 点到别处、按方向键、选中别的 | ❌ | ✅ |
    ///
    /// 「光标动了但文本没变」就是他离开了这一处 —— 那一刻这次改动才算定稿。这不是
    /// 时间上的猜测，是语义信号，而且用的是本来就在收的 `AXSelectedTextChanged`。
    last_text: std::cell::RefCell<String>,
    /// 可测试的定稿状态机。回调只登记最后一次文本变化；光标离开、静默超时或观察器自然
    /// 结束才触发分析。回调和监听循环在同一线程，`Cell` 足够。
    settlement: std::cell::Cell<EditSettlementState>,
    /// 本次武装期间收到了几次通知。
    ///
    /// 解除时和「学到了几条」一起打出来 —— 逐事件的诊断日志都降到了 debug（这个 app
    /// 只记 info 以上），日常使用里一次听写只留 armed/disarmed 两行，而这两个数字足够
    /// 判断「这个 app 到底发不发通知」，那正是要逐 app 收集的覆盖率数据。
    ///
    /// 解除时打出来。这一个数字就能把「观察器压根没工作」（0）和「通知收到了但被后面
    /// 某一步过滤掉了」（>0）分开 —— 没有它，两种情况在日志里完全一样。
    notifications: std::cell::Cell<u64>,
}

/// `AXValueChanged` 回调 shim：把 `refcon` 还原成 `WatchContext` 并比对文本。
///
/// # Safety
/// `refcon` 必须是 `run_edit_watch_loop` 注册时传入、且在观察器存活期间一直有效的
/// `*const WatchContext`（由观察线程的栈持有，反注册在其之前完成）。
unsafe extern "C" fn value_changed_shim(
    _observer: AxObserverRef,
    _element: AxUiElementRef,
    notification: CFStringRef,
    refcon: *mut c_void,
) {
    if refcon.is_null() {
        return;
    }
    let ctx = &*(refcon as *const WatchContext);
    ctx.notifications.set(ctx.notifications.get() + 1);
    // 已经解除就什么都别做。**这一刀必须在读 AXValue 之前。**
    //
    // 解除信号到达时观察线程可能正卡在 `run_in_mode` 里（最长 1 秒），这一秒内排队的
    // AX 通知照样派发到这里 —— 循环末尾那道 `if !stop.load(..)` 覆盖不到回调这条路。
    // 不挡的话，一次已经作废的观察还会再去跨进程读一遍宿主 app 的全文。
    if ctx.stop.load(Ordering::Relaxed) {
        return;
    }
    // 每一条 early return 都要留痕。否则「回调没被调用」和「回调被调用但被过滤掉了」
    // 在日志里长得一模一样 —— 第一次真机排查就卡在这个盲点上。
    let Some(current) = copy_string_attr(ctx.element.as_ref(), b"AXValue\0") else {
        log::debug!("[cursor-context] notified but AXValue is unreadable");
        return;
    };
    // 第一阶段：等我们自己的落字生效，把基线锚在那之后。
    if !ctx.anchored.get() {
        // 正常情况：文档里出现了我们刚打出去的那段文字 —— 插入生效了。
        // 兜底：目标 app 可能对文本做了加工（智能引号、自动补全），contains 永远匹配
        // 不上。等到这个时限就直接以当前状态为准 —— 落字早已生效，再等只会一直瞎等。
        let inserted = current.contains(&ctx.typed_text);
        if inserted || ctx.armed_at.elapsed() >= BASELINE_ANCHOR_TIMEOUT {
            let caret = copy_caret_offset(ctx.element.as_ref())
                .map(|utf16| utf16_offset_to_char_offset(&current, utf16));
            *ctx.anchor.borrow_mut() = locate_typed_span(&current, &ctx.typed_text, caret);
            log::debug!(
                "[cursor-context] baseline anchored at {} chars ({}, position={:?})",
                current.chars().count(),
                if inserted {
                    "insertion landed"
                } else {
                    "timeout"
                },
                *ctx.anchor.borrow()
            );
            // 两者必须一起推进：`baseline` 是比对起点，`last_text` 是「上次看到的样子」。
            // 只更新前者的话，锚定后第一条通知会把「插入生效」当成一次用户编辑。
            *ctx.last_text.borrow_mut() = current.clone();
            *ctx.baseline.borrow_mut() = current;
            ctx.anchored.set(true);
        }
        return;
    }

    // 第二阶段：把「打字」和「光标移开」分开 —— 全程只记录，边界到了才分析。
    if *ctx.last_text.borrow() != current {
        clear_selection_candidate(ctx);
        // 还在改。登记一笔，不判定：中间态怎么都可能变。
        *ctx.last_text.borrow_mut() = current;
        let mut settlement = ctx.settlement.get();
        settlement.note_text_change(Instant::now());
        ctx.settlement.set(settlement);
        return;
    }

    // 文本没变。可能是用户把光标移开了（边界），也可能只是刚才那次打字带出来的配对
    // 通知 —— 后者必须挡掉，否则每敲一个键都判定一次。
    if !is_caret_notification(notification) {
        return;
    }
    report_selection_candidate(ctx, &current);
    log::debug!("[cursor-context] caret moved away; settling the pending edit");
    settle_pending_edit(ctx, SettlementBoundary::CaretMoved);
}

unsafe fn clear_selection_candidate(ctx: &WatchContext) {
    if ctx.last_selection.borrow_mut().take().is_some() {
        (ctx.on_selection)(None);
    }
}

/// 把 AX 的 UTF-16 选区转换为可交给纠错流程的稳定快照，并用最近一次落字的 char 范围
/// 做位置围栏。这个纯函数也覆盖 emoji 等 UTF-16/Unicode 标量下标不一致的情况。
fn selection_observation_within_anchor(
    current: &str,
    start_utf16: usize,
    length_utf16: usize,
    anchor: TextAnchor,
) -> Option<SelectionObservation> {
    if length_utf16 == 0 {
        return None;
    }
    let end_utf16 = start_utf16.checked_add(length_utf16)?;
    if end_utf16 > current.encode_utf16().count() {
        return None;
    }
    let start = utf16_offset_to_char_offset(current, start_utf16);
    let end = utf16_offset_to_char_offset(current, end_utf16);
    if start < anchor.start || end > anchor.end || start >= end {
        return None;
    }
    let selected_text: String = current.chars().skip(start).take(end - start).collect();
    if selected_text.trim().is_empty() {
        return None;
    }
    Some(SelectionObservation {
        selected_text,
        document_text: current.to_string(),
        selection_start_utf16: start_utf16,
        selection_length_utf16: length_utf16,
        whole_field_selected: start_utf16 == 0 && end_utf16 == current.encode_utf16().count(),
    })
}

/// 只把有高置信位置锚的选区交给纠错气泡。内容 contains 回落在这里不够安全：同一个词
/// 可能同时出现在邮件标题、引用和本轮听写里，气泡必须能证明用户点的是最后那一段。
unsafe fn report_selection_candidate(ctx: &WatchContext, current: &str) {
    if !ctx.anchored.get() {
        clear_selection_candidate(ctx);
        return;
    }
    let Some(anchor) = *ctx.anchor.borrow() else {
        clear_selection_candidate(ctx);
        return;
    };
    let Some(range) = copy_selected_range(ctx.element.as_ref()) else {
        clear_selection_candidate(ctx);
        return;
    };
    if range.location < 0 || range.length <= 0 {
        clear_selection_candidate(ctx);
        return;
    }
    let start_utf16 = range.location as usize;
    let length_utf16 = range.length as usize;
    let Some(observation) =
        selection_observation_within_anchor(current, start_utf16, length_utf16, anchor)
    else {
        clear_selection_candidate(ctx);
        return;
    };
    let dedupe = (start_utf16, length_utf16, observation.selected_text.clone());
    if ctx.last_selection.borrow().as_ref() == Some(&dedupe) {
        return;
    }
    *ctx.last_selection.borrow_mut() = Some(dedupe);
    (ctx.on_selection)(Some(observation));
}

/// 这条通知是不是 `AXSelectedTextChanged`（光标/选区变化）。
unsafe fn is_caret_notification(notification: CFStringRef) -> bool {
    cfstring_to_rust(notification).as_deref() == Some("AXSelectedTextChanged")
}

/// 一处改动定稿了，比对一次并上报。
///
/// 边界判定统一交给 [`EditSettlementState`]，避免回调和监听循环各自维护一套时间规则。
unsafe fn settle_pending_edit(ctx: &WatchContext, boundary: SettlementBoundary) {
    if !ctx.settlement.get().should_settle(boundary, Instant::now()) {
        return;
    }
    ctx.settlement.set(EditSettlementState::Idle);

    let Some(current) = copy_string_attr(ctx.element.as_ref(), b"AXValue\0") else {
        return;
    };
    let baseline = ctx.baseline.borrow().clone();
    if let Some(edits) = multiple_edits(&baseline, &current) {
        let old_anchor = *ctx.anchor.borrow();
        // 精确差异成功后始终推进全文基线；外部编辑也会同步重映射 anchor，下一次不会把
        // 已处理的文档变化重复卷进来。只有纯删除/纯插入这种尚未形成候选的单一编辑保留
        // 原基线，支持「删掉错词 → 停顿 → 重打正确词」。
        let has_only_unfinished_edit = !edits.is_empty()
            && edits
                .iter()
                .all(|item| item.edit.target.trim().is_empty() && !is_vocab_worthy(&item.edit));
        if !has_only_unfinished_edit {
            *ctx.baseline.borrow_mut() = current.clone();
            if let Some(anchor) = old_anchor {
                *ctx.anchor.borrow_mut() = Some(remap_anchor(anchor, &edits));
            }
        }

        let mut reported_any = false;
        for positioned in edits {
            let Some(anchor) = old_anchor else {
                break;
            };
            if !positioned_edit_is_within_anchor(&positioned, anchor)
                || !is_vocab_worthy(&positioned.edit)
            {
                continue;
            }
            let edit = positioned.edit;
            let key = (edit.source.clone(), edit.target.clone());
            if !ctx.reported.borrow_mut().insert(key) {
                continue;
            }
            reported_any = true;
            ctx.reports.set(ctx.reports.get() + 1);
            (ctx.on_edit)(EditObservation {
                edit,
                confidence: crate::types::CorrectionConfidence::High,
            });
        }
        if reported_any || old_anchor.is_some() {
            return;
        }
        // 没有位置锚才继续走下面的低置信度内容回落。
    }

    let Some(edit) = minimal_edit(&baseline, &current) else {
        return;
    };
    if !edit_is_within_typed_text(&edit, &ctx.typed_text) || !is_vocab_worthy(&edit) {
        return;
    }
    *ctx.baseline.borrow_mut() = current;
    let key = (edit.source.clone(), edit.target.clone());
    if !ctx.reported.borrow_mut().insert(key) {
        return;
    }
    ctx.reports.set(ctx.reports.get() + 1);
    (ctx.on_edit)(EditObservation {
        edit,
        confidence: crate::types::CorrectionConfidence::Low,
    });
}

/// 观察器愿意盯的文档上限（UTF-16 code unit）。
///
/// 每收到一条通知就要整份读一次 `AXValue` 再做 O(n) 比对，而观察窗口最长 60 秒、
/// 用户每敲一个键都可能来一条。文档大到一定程度，这个代价就变成「用户改一个词，
/// 每次击键都跨进程拷贝一份文档」—— 卡顿、甚至把 AX 消息拖超时。
///
/// 与 [`FULL_TEXT_MAX_UTF16`] 同一量级：一次性读不下的文档，也不值得逐键盯着。
/// 超过就干脆不武装 —— 学不到词可以接受，让用户打字变卡不行。
const EDIT_WATCH_MAX_UTF16: usize = 20_000;

/// 武装手改监听。成功返回停止开关，失败返回 `None`（只 warn，绝不影响主链路）。
///
/// `typed_text` 是用户实际看到落到屏幕上的那段文字 —— 流式路径下它是真正打出去的内容
/// 而非完整 LLM 输出，两者可能不同。
///
/// **抓焦点元素和读基线都在新线程里做，不在调用线程上。** 调用方 `arm_edit_watch` 位于
/// `end_session` 这条 async 路径上，也就是 tokio worker —— 而这几次 AX 调用每次都可能
/// 耗到 [`AX_MESSAGING_TIMEOUT_SECS`]，对着一个 AX 无响应的 app（正是设这个超时要防的
/// 那种）能把一个 worker 卡住几百毫秒。本模块开头第 2 条硬约束写的就是这件事。
///
/// 代价是「趁焦点还没跑」这个窗口从零变成一次线程启动（几十微秒）。这比放进
/// `spawn_blocking` 好 —— 那个要排 tokio 阻塞池的队，负载高时反而更晚。
pub(super) fn spawn_edit_watcher(
    typed_text: String,
    on_edit: Box<dyn Fn(EditObservation) + Send + Sync>,
    on_selection: Box<dyn Fn(Option<SelectionObservation>) + Send + Sync>,
) -> Option<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let spawn_result = std::thread::Builder::new()
        .name("openless-cursor-edit-watch".into())
        .spawn(move || {
            let Some((element, baseline, pid, caret)) = grab_focused_element() else {
                return;
            };
            // 兜底。主判定在 `grab_focused_element` 里靠 `AXNumberOfCharacters` 完成，
            // 那一道能在整篇拷回来**之前**就拦住；这里防目标 app 报出与 AXValue 不一致
            // 的长度，避免观察器在错误元数据下继续工作。
            let baseline_utf16 = baseline.encode_utf16().count();
            if baseline_utf16 > EDIT_WATCH_MAX_UTF16 {
                log::info!(
                    "[cursor-context] edit watch skipped: AXValue is {baseline_utf16} UTF-16 units (limit {EDIT_WATCH_MAX_UTF16})"
                );
                return;
            }
            let (_, bundle_id) = crate::selection::current_front_app_parts();
            let baseline_for_last_text = baseline.clone();
            let anchor = locate_typed_span(&baseline, &typed_text, caret);
            let anchored = baseline.contains(&typed_text);
            run_edit_watch_loop(
                WatchContext {
                    element,
                    stop: Arc::clone(&thread_stop),
                    // 武装时若文档里已经有我们插的字，说明落字已经生效，基线直接可用。
                    anchored: std::cell::Cell::new(anchored),
                    anchor: std::cell::RefCell::new(anchor),
                    baseline: std::cell::RefCell::new(baseline),
                    armed_at: Instant::now(),
                    last_text: std::cell::RefCell::new(baseline_for_last_text),
                    settlement: std::cell::Cell::new(EditSettlementState::Idle),
                    typed_text,
                    on_edit,
                    on_selection,
                    last_selection: std::cell::RefCell::new(None),
                    reported: std::cell::RefCell::new(std::collections::HashSet::new()),
                    reports: std::cell::Cell::new(0),
                    notifications: std::cell::Cell::new(0),
                },
                pid,
                bundle_id,
                thread_stop,
            );
        });

    if let Err(err) = spawn_result {
        log::warn!("[cursor-context] spawn edit watch thread failed: {err}");
        return None;
    }
    Some(stop)
}

/// 抓当前焦点元素 + 读一次基线全文 + 取 pid。**只在观察线程上调用。**
///
/// ## 安全闸门必须在这里再过一遍
///
/// 观察器读的是和 [`read_around_cursor_blocking`] 完全相同的东西 —— 焦点元素的
/// `AXValue` 全文 —— 只是读得更频繁（整个观察窗口内每条通知一次），而且读到的差异会
/// 进日志、还可能变成一张词条建议卡片。
///
/// 两条路径是**分别**到达 AX 的：读取那条走 `probe_around_cursor`，观察这条走
/// `arm_edit_watch`。闸门只装在前者身上时，后者就是一个绕过口 —— 在终端里听写，上下文
/// 读取被正确拦住，落字之后观察器却照样武装、照样把终端全文读走。这个功能敢默认存在
/// 的全部前提就是「密码框 / Secure Input / 密码管理器 / 终端一律不读」，两条路径必须
/// 给出同一个答案。
///
/// 走的是和读取路径同一个 [`focused_element_passing_the_gate`]，不另开一条路。
fn grab_focused_element() -> Option<(SendableElement, String, i32, Option<usize>)> {
    let (_, bundle_id) = crate::selection::current_front_app_parts();
    let gate = GateInputs {
        secure_input: crate::unicode_keystroke::is_secure_input_enabled(),
        bundle_id,
        role: None,
        subrole: None,
    };

    unsafe {
        let focused = match focused_element_passing_the_gate(gate) {
            GatedElement::Ready(el) => el,
            GatedElement::Blocked(reason) => {
                log::info!("[cursor-context] edit watch blocked: {reason:?}");
                return None;
            }
            GatedElement::Unavailable(why) => {
                log::info!("[cursor-context] edit watch skipped: {why}");
                return None;
            }
        };

        // 先问长度再决定要不要整篇拷回来 —— 与 `read_document` 同一套做法。
        // `AXValue` 会把整篇文档跨进程拷过来，在一个十万字的文件上光 marshalling 就够
        // 撞上超时；而超限的文档我们本来就不观察（见 `EDIT_WATCH_MAX_UTF16`），白拷一次
        // 纯属浪费。
        match classify_document_length(
            copy_index_attr(focused, b"AXNumberOfCharacters\0"),
            EDIT_WATCH_MAX_UTF16,
        ) {
            DocumentLength::Unknown => {
                log::info!(
                    "[cursor-context] edit watch skipped: AXNumberOfCharacters unavailable; refusing an unbounded AXValue read"
                );
                CFRelease(focused as CFTypeRef);
                return None;
            }
            DocumentLength::OverLimit(total) => {
                log::info!(
                    "[cursor-context] edit watch skipped: document is {total} UTF-16 units (limit {EDIT_WATCH_MAX_UTF16})"
                );
                CFRelease(focused as CFTypeRef);
                return None;
            }
            DocumentLength::WithinLimit(_) => {}
        }

        let baseline = copy_string_attr(focused, b"AXValue\0");
        let caret_utf16 = copy_caret_offset(focused);
        let mut pid: i32 = 0;
        let pid_err = AXUIElementGetPid(focused, &mut pid);
        let element = SendableElement::retained(focused);
        CFRelease(focused as CFTypeRef);

        let Some(baseline) = baseline else {
            log::info!("[cursor-context] edit watch skipped: focused element has no AXValue");
            return None;
        };
        if pid_err != AX_ERROR_SUCCESS || pid <= 0 {
            log::info!("[cursor-context] edit watch skipped: AXUIElementGetPid failed");
            return None;
        }
        let caret = caret_utf16.map(|offset| utf16_offset_to_char_offset(&baseline, offset));
        Some((element, baseline, pid, caret))
    }
}

fn run_edit_watch_loop(
    ctx: WatchContext,
    pid: i32,
    bundle_id: Option<String>,
    stop: Arc<AtomicBool>,
) {
    unsafe {
        let mut observer: AxObserverRef = std::ptr::null_mut();
        let err = AXObserverCreate(pid, value_changed_shim, &mut observer);
        if err != AX_ERROR_SUCCESS || observer.is_null() {
            log::warn!("[cursor-context] AXObserverCreate failed: AXError={err}");
            return;
        }
        // 注册两种通知，不是一种。
        //
        // `AXValueChanged` 是「文本内容变了」的标准信号，但不是每个文本控件都发它。
        // `AXSelectedTextChanged` 是「选区/光标动了」—— 用户改一个词必然会移动光标，
        // 所以它是同一件事的另一条证据路径。收到任意一个都去比对一次文本，代价只是
        // 一次 AX 读；漏掉一种通知的代价是整个功能在那个 app 里静默失效。
        let mut registered: Vec<(CFStringRef, &str)> = Vec::new();
        for name in [&b"AXValueChanged\0"[..], &b"AXSelectedTextChanged\0"[..]] {
            let Some(notification) = cfstring_from_static(name) else {
                continue;
            };
            // SAFETY: &ctx 在本函数返回前一直有效，而反注册发生在返回之前，C 侧拿不到
            // 悬垂指针。
            let add_err = AXObserverAddNotification(
                observer,
                ctx.element.as_ref(),
                notification,
                &ctx as *const _ as *mut c_void,
            );
            let label = std::str::from_utf8(&name[..name.len() - 1]).unwrap_or("?");
            if add_err == AX_ERROR_SUCCESS {
                registered.push((notification, label));
            } else {
                log::info!(
                    "[cursor-context] {label} not registered: AXError={add_err} (app does not emit it)"
                );
                CFRelease(notification);
            }
        }
        if registered.is_empty() {
            log::info!(
                "[cursor-context] no usable AX notification on this element; edit watch off"
            );
            CFRelease(observer as CFTypeRef);
            return;
        }

        // runloop 这一段走 core_foundation 的封装而不是自己再声明一遍 extern：
        // `hotkey.rs` 已经声明过 CFRunLoopGetCurrent / CFRunLoopAddSource，重复声明
        // 会触发 clashing_extern_declarations（ABI 上兼容，但那是靠运气）。
        let source = CFRunLoopSource::wrap_under_get_rule(AXObserverGetRunLoopSource(observer));
        let runloop = CFRunLoop::get_current();
        // SAFETY: kCFRunLoopDefaultMode 是 CoreFoundation 的 'static 常量字符串。
        let mode = kCFRunLoopDefaultMode;
        runloop.add_source(&source, mode);
        log::info!(
            "[cursor-context] edit watch armed (pid={pid} bundle={bundle_id:?} notifications=[{}])",
            registered
                .iter()
                .map(|(_, l)| *l)
                .collect::<Vec<_>>()
                .join(", ")
        );

        let started = Instant::now();
        let mut end_reason = "disarmed";
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // 60 秒硬上限：过了这么久还在改，多半是在写新东西而不是纠我们插的词。
            if started.elapsed() >= EDIT_WATCH_MAX_LIFETIME {
                end_reason = "timeout";
                break;
            }
            // 前台 app 一换就收工 —— 继续盯着别人的窗口既没意义也不该做。
            let (_, current_bundle) = crate::selection::current_front_app_parts();
            if current_bundle != bundle_id {
                end_reason = "front app changed";
                break;
            }
            let result = CFRunLoop::run_in_mode(mode, Duration::from_secs(1), false);
            // 解除信号可能正好在这 1 秒里到达。先看一眼再判定 —— 否则会上报一条属于
            // 上一轮的改动（见下面收尾处的长注释）。
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // 每转一圈问一次「停手够久了吗」。判定发生在这里而不是回调里。
            settle_pending_edit(&ctx, SettlementBoundary::QuietTimeout);
            // Finished 表示 runloop 里没有任何 input source —— 观察器的 source 已经装上，
            // 正常走不到这里；真到了就说明焦点元素没了，收工。
            if matches!(result, CFRunLoopRunResult::Finished) {
                end_reason = "focused element gone";
                break;
            }
        }

        // 收工前兜一次：用户改完就直接切走 app 的话，停手计时还没到就已经退出循环了，
        // 那次改动不该白丢。
        //
        // **但被主动解除时不补。** `stop` 被置位只有两个来源：新一轮听写开始
        //（`begin_session_as`）或用户关掉了开关（`disarm_edit_watch`）。两种情况下协调方
        // 都已经把建议卡片收掉了 —— 这时再上报一条属于上一轮的改动，卡片会在**新会话
        // 进行中**弹出来。而卡片会把胶囊窗口缩到自己那么大，等于把正在进行的那次听写的
        // 胶囊弄没了（这个坑真机上踩过一次，表现是「热键像是坏了」）。
        //
        // 自然结束（超时 / 切走 app / 焦点元素没了）才补 —— 那几种情况下没有新会话在跑，
        // 用户那次改动是真的还没被判定过。
        if !stop.load(Ordering::Relaxed) {
            settle_pending_edit(&ctx, SettlementBoundary::ObserverEnding);
            clear_selection_candidate(&ctx);
        }

        // 无论怎么退出的，反注册这一段都必须跑到。
        runloop.remove_source(&source, mode);
        for (notification, label) in registered {
            let remove_err =
                AXObserverRemoveNotification(observer, ctx.element.as_ref(), notification);
            if remove_err != AX_ERROR_SUCCESS {
                // -25202 = notification not registered，通常意味着元素已经被目标 app
                // 销毁重建（Electron 每次输入都这样）——那也解释了为什么通知收不到。
                log::warn!(
                    "[cursor-context] remove {label} failed: AXError={remove_err} (element gone?)"
                );
            }
            CFRelease(notification);
        }
        CFRelease(observer as CFTypeRef);
        log::info!(
            "[cursor-context] edit watch disarmed after {}ms ({end_reason}, {} notifications, {} edits)",
            started.elapsed().as_millis(),
            ctx.notifications.get(),
            ctx.reports.get()
        );
        // ctx 在此 drop —— 此时观察器已移除，C 侧不再回调，安全。
        drop(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        caret_offset_from_location, classify_document_length, locate_typed_span,
        positioned_edit_is_within_anchor, remap_anchor, selection_observation_within_anchor,
        DocumentLength, EditSettlementState, SettlementBoundary, TextAnchor, CARET_MOVE_QUIET,
        EDIT_SETTLE_TIMEOUT,
    };
    use crate::host_document::{EditPair, PositionedEdit};

    #[test]
    fn unknown_document_length_is_not_safe_for_a_full_value_read() {
        assert_eq!(
            classify_document_length(None, 20_000),
            DocumentLength::Unknown
        );
    }

    #[test]
    fn small_document_length_allows_a_full_value_read() {
        assert_eq!(
            classify_document_length(Some(20_000), 20_000),
            DocumentLength::WithinLimit(20_000)
        );
    }

    #[test]
    fn large_document_length_requires_a_bounded_range_read() {
        assert_eq!(
            classify_document_length(Some(20_001), 20_000),
            DocumentLength::OverLimit(20_001)
        );
    }

    /// 负数 location 是「没有光标」的哨兵，必须和「光标在开头」区分开。
    ///
    /// 真机上 Electron 类 app 反复出现 `before=0 after=N`，一直被当成「这个 app 读不到
    /// 上文」；实际上是 `AXSelectedTextRange` 返回了 kCFNotFound(-1)，被钳成 0 之后
    /// 我们读了文档开头，还当成光标附近发给了 LLM。错的上下文比没有上下文更糟 ——
    /// 它看起来是对的。
    #[test]
    fn a_negative_caret_location_is_not_the_start_of_the_document() {
        assert_eq!(caret_offset_from_location(0), Some(0), "光标真在开头");
        assert_eq!(caret_offset_from_location(42), Some(42));
        assert_eq!(
            caret_offset_from_location(-1),
            None,
            "kCFNotFound：没有光标"
        );
        assert_eq!(caret_offset_from_location(isize::MIN), None);
    }

    #[test]
    fn duplicate_inserted_text_is_anchored_nearest_the_caret() {
        let text = "第一段 OpenLess。第二段 OpenLess。";
        let caret = text.chars().count() - 1;
        let span = locate_typed_span(text, "OpenLess", Some(caret)).expect("span");
        assert!(span.start > 10, "应选择靠近第二段光标的匹配");
    }

    #[test]
    fn position_fence_rejects_the_same_word_elsewhere() {
        let anchor = TextAnchor { start: 20, end: 30 };
        let outside = PositionedEdit {
            edit: EditPair {
                source: "好的".into(),
                target: "好滴".into(),
                before: String::new(),
                after: String::new(),
            },
            source_start: 2,
            source_end: 4,
            target_start: 2,
            target_end: 4,
        };
        assert!(!positioned_edit_is_within_anchor(&outside, anchor));
    }

    #[test]
    fn selection_candidate_uses_utf16_offsets_inside_the_latest_anchor() {
        let text = "🙂 甲Codex乙";
        let observation =
            selection_observation_within_anchor(text, 4, 5, TextAnchor { start: 2, end: 9 })
                .expect("selection inside latest insertion");
        assert_eq!(observation.selected_text, "Codex");
        assert_eq!(observation.selection_start_utf16, 4);
    }

    #[test]
    fn selection_candidate_rejects_identical_text_outside_the_latest_anchor() {
        let text = "Codex / Codex";
        assert!(
            selection_observation_within_anchor(text, 0, 5, TextAnchor { start: 8, end: 13 },)
                .is_none()
        );
    }

    #[test]
    fn anchor_moves_when_text_before_it_changes() {
        let anchor = TextAnchor { start: 10, end: 20 };
        let edit = PositionedEdit {
            edit: EditPair {
                source: "甲".into(),
                target: "甲乙丙".into(),
                before: String::new(),
                after: String::new(),
            },
            source_start: 2,
            source_end: 3,
            target_start: 2,
            target_end: 5,
        };
        assert_eq!(
            remap_anchor(anchor, &[edit]),
            TextAnchor { start: 12, end: 22 }
        );
    }

    #[test]
    fn settlement_state_resets_the_quiet_deadline_on_each_keystroke() {
        let now = std::time::Instant::now();
        let mut state = EditSettlementState::Idle;
        state.note_text_change(now - EDIT_SETTLE_TIMEOUT);
        assert!(state.should_settle(SettlementBoundary::QuietTimeout, now));
        state.note_text_change(now);
        assert!(!state.should_settle(SettlementBoundary::QuietTimeout, now));
    }

    #[test]
    fn caret_move_requires_quiet_but_observer_end_is_immediate() {
        let now = std::time::Instant::now();
        let state = EditSettlementState::Editing { last_change: now };
        assert!(!state.should_settle(SettlementBoundary::CaretMoved, now));
        assert!(state.should_settle(SettlementBoundary::CaretMoved, now + CARET_MOVE_QUIET));
        assert!(state.should_settle(SettlementBoundary::ObserverEnding, now));
    }
}
