import { invokeOrMock } from "./shared"

// ── Remote input (局域网手机录音) ──────────────────────────────────────
export interface RemoteInputStatus {
    running: boolean
    port: number
    pin: string
    urls: string[]
}

export function getRemoteInputStatus(): Promise<RemoteInputStatus> {
    return invokeOrMock("get_remote_input_status", undefined, () => ({
        running: false,
        port: 8443,
        pin: "000000",
        urls: [],
    }))
}

export function listLocalIps(): Promise<string[]> {
    return invokeOrMock("list_local_ips", undefined, () => ["192.168.1.100"])
}

export function regenerateRemotePin(): Promise<string> {
    return invokeOrMock("regenerate_remote_pin", undefined, () => "123456")
}

/** 把 PC 端界面语言同步给远程输入服务，H5 录音页据此显示对应语言。 */
export function setRemoteLocale(locale: string): Promise<void> {
    return invokeOrMock("set_remote_locale", { locale }, () => undefined)
}
