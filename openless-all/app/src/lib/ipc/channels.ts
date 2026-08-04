// 渠道卡片的 IPC 封装。
//
// 一张卡片 = 一份可命名、可排序、可开关的供应商配置；同一家厂商可以有多张卡片
// （多把 key）。列表里**第一个启用的**就是当前生效的渠道 —— 后端不另存"当前选中"，
// 排序即优先级（见 docs/provider-channels-plan.md）。
//
// 凭据不走这里：按渠道 id 调 readCredential/setCredential(account, value, id)。

import { invokeOrMock } from "./shared"

export type ChannelKind = "llm" | "asr"

export interface ChannelTestResult {
    ok: boolean
    latencyMs: number | null
    /** Unix 秒（后端时钟，前端不要自己生成）。 */
    at: number
    error: string | null
}

export interface Channel {
    id: string
    /** 用户取的名字；空串表示未命名，由 UI 回落到 preset 显示名。 */
    name: string
    /** 厂商 id —— 决定协议与表单形状，与 id 相互独立。 */
    providerType: string
    enabled: boolean
    order: number
    lastTest: ChannelTestResult | null
}

// 浏览器（非 Tauri）下的样例数据，让 `npm run dev` 能预览列表的四种状态：
// 生效中 / 备用 / 测试失败标红 / 已关闭沉底。
const mockChannels: Record<ChannelKind, Channel[]> = {
    llm: [
        {
            id: "siliconflow",
            name: "硅基流动-主号",
            providerType: "siliconflow",
            enabled: true,
            order: 0,
            lastTest: { ok: true, latencyMs: 284, at: Math.floor(Date.now() / 1000) - 90, error: null },
        },
        {
            id: "ark",
            name: "",
            providerType: "ark",
            enabled: true,
            order: 1,
            lastTest: null,
        },
        {
            id: "openai",
            name: "OpenAI-备用",
            providerType: "openai",
            enabled: false,
            order: 2,
            lastTest: { ok: false, latencyMs: null, at: Math.floor(Date.now() / 1000) - 3600, error: "providerHttpStatus:401" },
        },
    ],
    asr: [
        {
            id: "volcengine",
            name: "",
            providerType: "volcengine",
            enabled: true,
            order: 0,
            lastTest: { ok: true, latencyMs: 143, at: Math.floor(Date.now() / 1000) - 20, error: null },
        },
        {
            id: "groq",
            name: "Groq-白嫖号",
            providerType: "groq",
            enabled: true,
            order: 1,
            lastTest: null,
        },
    ],
}

export function listChannels(kind: ChannelKind): Promise<Channel[]> {
    return invokeOrMock("list_channels", { kind }, () => mockChannels[kind])
}

/** 返回后端分配的渠道 id。 */
export function createChannel(
    kind: ChannelKind,
    providerType: string,
    name: string,
): Promise<string> {
    return invokeOrMock(
        "create_channel",
        { kind, providerType, name },
        () => providerType,
    )
}

/** 在已建好的草稿卡片上换供应商（单弹窗添加流程的常规操作）。 */
export function setChannelProviderType(
    kind: ChannelKind,
    id: string,
    providerType: string,
): Promise<void> {
    return invokeOrMock(
        "set_channel_provider_type",
        { kind, id, providerType },
        () => undefined,
    )
}

/** 关闭添加弹窗时回收没填任何东西的草稿；返回是否真的删了。 */
export function deleteChannelIfBlank(
    kind: ChannelKind,
    id: string,
): Promise<boolean> {
    return invokeOrMock("delete_channel_if_blank", { kind, id }, () => true)
}

export function renameChannel(
    kind: ChannelKind,
    id: string,
    name: string,
): Promise<void> {
    return invokeOrMock("rename_channel", { kind, id, name }, () => undefined)
}

export function deleteChannel(kind: ChannelKind, id: string): Promise<void> {
    return invokeOrMock("delete_channel", { kind, id }, () => undefined)
}

export function setChannelEnabled(
    kind: ChannelKind,
    id: string,
    enabled: boolean,
): Promise<void> {
    return invokeOrMock(
        "set_channel_enabled",
        { kind, id, enabled },
        () => undefined,
    )
}

/** ids 是拖拽后的完整顺序；后端会把未提及的渠道排到末尾。 */
export function reorderChannels(
    kind: ChannelKind,
    ids: string[],
): Promise<void> {
    return invokeOrMock("reorder_channels", { kind, ids }, () => {
        // mock 也要真的重排：否则浏览器预览里松手后顺序被 listChannels 拉回原样，
        // 看着就像"拖拽坏了"，而真机是好的。
        const list = mockChannels[kind]
        const ordered = ids
            .map(id => list.find(c => c.id === id))
            .filter((c): c is Channel => Boolean(c))
        const rest = list.filter(c => !ids.includes(c.id))
        mockChannels[kind] = [...ordered, ...rest].map((c, index) => ({
            ...c,
            order: index,
        }))
        return undefined
    })
}

export function recordChannelTest(
    kind: ChannelKind,
    id: string,
    ok: boolean,
    latencyMs: number | null,
    error: string | null,
): Promise<void> {
    return invokeOrMock(
        "record_channel_test",
        { kind, id, ok, latencyMs, error },
        () => undefined,
    )
}
