import { invokeOrMock } from "./shared"

/**
 * 让当前聊天面板窗口成为 key window（获得键盘焦点）。
 *
 * QA / Less Computer 浮窗以「不抢前台」方式显示（macOS orderFrontRegardless，
 * 从不 makeKey）——好处是唤起时不打断用户正在用的 app，代价是窗口不是
 * key window 时按键根本进不了 webview（「点了输入框却打不出字」的根因）。
 * 前端在用户点进输入框时调用本命令，此刻抢焦点正是用户想要的。
 */
export function chatPanelFocusKeyboard(): Promise<void> {
    return invokeOrMock("chat_panel_focus_keyboard", undefined, () => undefined)
}
