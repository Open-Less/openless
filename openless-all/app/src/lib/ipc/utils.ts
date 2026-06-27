import { isTauri, invokeOrMock } from "./shared"

export { isTauri }

export async function openExternal(url: string): Promise<void> {
    if (!isTauri) {
        window.open(url, "_blank", "noopener,noreferrer")
        return
    }
    try {
        const { open } = await import("@tauri-apps/plugin-shell")
        await open(url)
        return
    } catch (error) {
        console.warn("[external-open] shell plugin failed", error)
    }
    try {
        const { invoke } = await import("@tauri-apps/api/core")
        await invoke("open_external_url", { url })
        return
    } catch (error) {
        console.warn("[external-open] native fallback failed", error)
    }
    window.open(url, "_blank", "noopener,noreferrer")
}

/**
 * 让用户选 save 路径并把当前会话日志（openless.log）复制过去。
 * 浏览器开发模式下走 mock 不实际写盘。返回最终 save 的绝对路径，取消选择则返回 null。
 */
export async function exportErrorLog(
    suggestedFileName: string,
): Promise<string | null> {
    if (!isTauri) {
        return `~/Downloads/${suggestedFileName}`
    }
    const { save } = await import("@tauri-apps/plugin-dialog")
    const target = await save({
        defaultPath: suggestedFileName,
        filters: [{ name: "Log", extensions: ["log", "txt"] }],
    })
    if (!target) return null
    await invokeOrMock<void>(
        "export_error_log",
        { targetPath: target },
        () => undefined,
    )
    return target
}

/**
 * 把前端关键错误（如自动更新 install 失败）转发到 Rust 文件日志（openless.log）。
 * webview 的 console.error 不会落进 openless.log，单独走 IPC，便于用户「导出日志」
 * 后我们拿到失败的真实原因。永不抛错——日志失败不应再影响调用方的错误处理。
 */
export async function logClientError(message: string): Promise<void> {
    try {
        await invokeOrMock<void>("log_client_error", { message }, () => undefined)
    } catch (error) {
        console.warn("[log-client-error] failed to forward error to app log", error)
    }
}
