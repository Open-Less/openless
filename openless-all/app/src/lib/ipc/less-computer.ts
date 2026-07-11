import { invokeOrMock } from "./shared"
import type { LessComputerEvent } from "../types"

/** 用户点 ✕ / 按 Esc 关闭 Less Computer 浮窗（隐藏窗口）。 */
export function lessComputerWindowDismiss(): Promise<void> {
    return invokeOrMock("less_computer_window_dismiss", undefined, () => undefined)
}

/** 内联审批卡的 Approve / Deny 回执。token 关联到等待中的拦截动作。 */
export function lessComputerApprove(
    token: string,
    approved: boolean,
): Promise<void> {
    return invokeOrMock(
        "less_computer_approve",
        { token, approved },
        () => undefined,
    )
}

/** 浮窗打字输入：文字指令直接进入 Less Computer 执行链（与语音同护栏/审批/连续会话）。 */
export function lessComputerSubmitText(text: string): Promise<void> {
    return invokeOrMock(
        "less_computer_submit_text",
        { text },
        () => undefined,
    )
}

/** 浮窗 mount 时拉取当前会话的事件缓冲（seq 升序），重放 webview 冷加载期间
 *  丢掉的事件（尤其首条 user —— 用户说的话）。 */
export function lessComputerSync(): Promise<LessComputerEvent[]> {
    return invokeOrMock("less_computer_sync", undefined, () => [])
}
