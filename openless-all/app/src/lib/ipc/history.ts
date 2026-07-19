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

/** 读取某次会话的原始麦克风 WAV 的 data URL（base64）。
 *  仅当 session.hasAudioRecording === true 时调用，避免无效 IPC。
 *  返回 `data:audio/wav;base64,...` 格式，前端 `<audio>` 和导出按钮直接使用。 */
export function readAudioRecording(sessionId: string): Promise<string> {
    return invokeOrMock(
        "read_audio_recording",
        { sessionId },
        () => "data:audio/wav;base64,",
    )
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
