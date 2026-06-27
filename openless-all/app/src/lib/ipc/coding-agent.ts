import type { CodingAgentPermissionMode } from "../types"
export type { CodingAgentPermissionMode }
import { invokeOrMock } from "./shared"

export type McpHealth = "connected" | "failed" | "needs_auth" | "unknown"

export interface McpServerStatus {
    name: string
    detail: string
    health: McpHealth
}

export interface ClaudeDetection {
    installed: boolean
    version: string | null
    exe: string
    mcpServers: McpServerStatus[]
    hasComputerUse: boolean
}

/** OpenCode CLI 检测结果（issue #579）。 */
export interface OpenCodeDetection {
    installed: boolean
    version: string | null
    exe: string
}

/** 检测 `opencode` 是否安装（语音 Agent 选 OpenCode 后端时设置页据此提示）。 */
export function codingAgentDetectOpencode(exe?: string): Promise<OpenCodeDetection> {
    return invokeOrMock(
        "coding_agent_detect_opencode",
        { exe },
        () => ({
            installed: false,
            version: null,
            exe: exe || "opencode",
        }),
    )
}

/** 无头 Claude 运行事件，由后端 `coding-agent:test` 流式推送（tag 为 `kind`）。 */
export type CodingAgentEvent =
    | { kind: "started"; session_id: string }
    | { kind: "delta"; session_id: string; text: string }
    | { kind: "tool_use"; session_id: string; name: string }
    | {
          kind: "completed"
          session_id: string
          text: string
          cost_usd: number | null
          duration_ms: number | null
      }
    | { kind: "cancelled"; session_id: string }
    | { kind: "error"; session_id: string; message: string }

export function codingAgentDetect(exe?: string): Promise<ClaudeDetection> {
    return invokeOrMock(
        "coding_agent_detect",
        { exe },
        () => ({
            installed: false,
            version: null,
            exe: exe || "claude",
            mcpServers: [],
            hasComputerUse: false,
        }),
    )
}

export interface CodingAgentRunTestArgs {
    prompt: string
    exe?: string
    permissionMode?: CodingAgentPermissionMode
    workdir?: string
    model?: string
    maxBudgetUsd?: number
}

export function codingAgentRunTest(args: CodingAgentRunTestArgs): Promise<void> {
    return invokeOrMock("coding_agent_run_test", { ...args }, () => undefined)
}

export function codingAgentCancelTest(): Promise<void> {
    return invokeOrMock("coding_agent_cancel_test", undefined, () => undefined)
}

export function codingAgentCommandRisk(command: string): Promise<string | null> {
    return invokeOrMock("coding_agent_command_risk", { command }, () => null)
}
