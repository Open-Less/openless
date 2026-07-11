import type { ActivityDay, DictationSession } from "../types"
import { invokeOrMock } from "./shared"
import { mockActivityDays, mockHistory } from "./mock-data"

export function listHistory(): Promise<DictationSession[]> {
    return invokeOrMock("list_history", undefined, () => mockHistory)
}

/** 每日听写活动计数（日期升序），概览页年度热力图数据源。与历史保留策略解耦。 */
export function getActivityStats(): Promise<ActivityDay[]> {
    return invokeOrMock("get_activity_stats", undefined, () => mockActivityDays)
}

export function deleteHistoryEntry(id: string): Promise<void> {
    return invokeOrMock("delete_history_entry", { id }, () => undefined)
}

export function clearHistory(): Promise<void> {
    return invokeOrMock("clear_history", undefined, () => undefined)
}

/** 读取某次会话的原始麦克风 wav 字节流。仅当 prefs.recordAudioForDebug 当时打开
 *  并且文件没被 retention 清理掉时才有内容；其他情况后端会返回 "recording not found" 错。
 *  调用方应仅在 session.hasAudioRecording === true 时触发，避免无效 IPC。 */
export function readAudioRecording(sessionId: string): Promise<Uint8Array> {
    return invokeOrMock(
        "read_audio_recording",
        { sessionId },
        () => new Uint8Array(),
    ).then((value) => {
        // Tauri 默认把 Vec<u8> 序列化为 number[]，前端拿到的是普通数组；统一转 Uint8Array。
        if (value instanceof Uint8Array) return value
        if (Array.isArray(value)) return new Uint8Array(value as number[])
        return new Uint8Array(value as ArrayBuffer)
    })
}

/** 用当前 ASR provider 对一条「转录失败」历史条目的归档录音重新转录（issue #613）。
 *  成功时后端原地回写该条历史的 rawTranscript / finalText 并清除错误码，返回更新后的整条记录。
 *  失败时抛出错误（如「重新转录仍未识别到语音」/「recording not found」），录音保留不丢。 */
export function retranscribeRecording(sessionId: string): Promise<DictationSession> {
    return invokeOrMock(
        "retranscribe_recording",
        { sessionId },
        () => mockHistory[0],
    ) as Promise<DictationSession>
}
