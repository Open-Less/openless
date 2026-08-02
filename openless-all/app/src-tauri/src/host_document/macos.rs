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

use super::{
    evaluate_gate, plan_window, utf16_offset_to_char_offset, window_around_cursor, GateInputs,
    ReadOutcome, AX_MESSAGING_TIMEOUT_SECS,
};

/// 超过这个 UTF-16 长度就不整篇 `AXValue` 读回来，改走 `AXStringForRange` 只取光标附近。
///
/// 在一篇十万字的文档上 `AXValue` 会把整篇跨进程拷过来，光是 marshalling 就够撞上
/// 超时；而我们最终只要几百字。阈值取得比任何合理预算都大得多，正常文档仍走简单路径。
const FULL_TEXT_MAX_UTF16: usize = 20_000;

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

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AxUiElementRef;
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
    fn CFGetTypeID(cf: CFTypeRef) -> CFTypeId;
    fn CFStringGetTypeID() -> CFTypeId;
    fn CFNumberGetTypeID() -> CFTypeId;
    fn CFStringCreateWithCString(
        allocator: CFAllocatorRef,
        cstr: *const c_char,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetCStringPtr(s: CFStringRef, encoding: u32) -> *const c_char;
    fn CFStringGetCString(
        s: CFStringRef,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> bool;
    fn CFStringGetLength(s: CFStringRef) -> isize;
    fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
    fn CFNumberGetValue(number: CFTypeRef, number_type: i32, value_ptr: *mut c_void) -> bool;
}

/// 同步读取光标周围的文档。**只允许在 `spawn_blocking` 上下文里调用。**
///
/// `gate` 带着调用方已经填好的 `secure_input` / `bundle_id`；本函数补上需要一次 AX 读
/// 的 `role` / `subrole`，再做最终判定 —— 拿到焦点元素之后、读正文之前。
pub(super) fn read_around_cursor_blocking(budget_chars: usize, mut gate: GateInputs) -> ReadOutcome {
    unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return ReadOutcome::Unavailable("system-wide AX element unavailable");
        }
        // 这一行是整个模块最重要的一行：不设就继承 AX 默认的 ~6 秒，对着一个卡死的
        // app 就是 6 秒冻结。系统级 element 上的设置会成为本进程的默认值。
        AXUIElementSetMessagingTimeout(system, AX_MESSAGING_TIMEOUT_SECS);

        let focused = copy_element_attr(system, b"AXFocusedUIElement\0");
        CFRelease(system as CFTypeRef);

        let Some(focused) = focused else {
            return ReadOutcome::Unavailable("no focused UI element (AX permission or no focus)");
        };
        // 显式再设一次：进程默认值只对「之后创建」的 ref 生效，对已有 ref 补一刀更稳。
        AXUIElementSetMessagingTimeout(focused, AX_MESSAGING_TIMEOUT_SECS);

        gate.role = copy_string_attr(focused, b"AXRole\0");
        gate.subrole = copy_string_attr(focused, b"AXSubrole\0");
        if let Some(reason) = evaluate_gate(&gate) {
            CFRelease(focused as CFTypeRef);
            return ReadOutcome::Blocked(reason);
        }

        let outcome = read_document(focused, budget_chars);
        CFRelease(focused as CFTypeRef);
        outcome
    }
}

unsafe fn read_document(focused: AxUiElementRef, budget_chars: usize) -> ReadOutcome {
    let Some(cursor_utf16) = copy_caret_offset(focused) else {
        return ReadOutcome::Unavailable("AXSelectedTextRange unavailable (not a text element?)");
    };
    let total_utf16 = copy_index_attr(focused, b"AXNumberOfCharacters\0");

    // 小文档（绝大多数情况）：整篇读回来，按 char 精确截窗。
    let full_text = match total_utf16 {
        Some(total) if total > FULL_TEXT_MAX_UTF16 => None,
        _ => copy_string_attr(focused, b"AXValue\0"),
    };
    if let Some(text) = full_text {
        let cursor = utf16_offset_to_char_offset(&text, cursor_utf16);
        return ReadOutcome::Window(window_around_cursor(&text, cursor, budget_chars));
    }

    // 回落：文档太大，或者该控件压根不给 AXValue（Electron 类常见）。改成只跟它要
    // 光标附近的一段。UTF-16 预算给两倍 —— 宁可多要一点回来自己裁，也不要因为
    // char/UTF-16 换算差把上文截秃。
    let Some(total) = total_utf16 else {
        return ReadOutcome::Unavailable("neither AXValue nor AXNumberOfCharacters is readable");
    };
    let span = plan_window(total, cursor_utf16, budget_chars.saturating_mul(2));
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
    Some(range.location.max(0) as usize)
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
    if ok && out >= 0 {
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
    if !ok {
        return None;
    }
    CStr::from_ptr(buf.as_ptr() as *const c_char)
        .to_str()
        .ok()
        .map(str::to_string)
}
