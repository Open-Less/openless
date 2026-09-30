import type { CodingAgentPermissionMode } from '../types';
export type { CodingAgentPermissionMode };
import { invokeOrMock } from './shared';

export type McpHealth = 'connected' | 'failed' | 'needs_auth' | 'unknown';

export interface McpServerStatus {
  name: string;
  detail: string;
  health: McpHealth;
}

export interface ClaudeDetection {
  installed: boolean;
  version: string | null;
  exe: string;
  mcpServers: McpServerStatus[];
  hasComputerUse: boolean;
}

/** OpenCode CLI detection result (issue #579). */
export interface OpenCodeDetection {
  installed: boolean;
  version: string | null;
  exe: string;
}

/** Detects whether `opencode` is installed (the settings page uses this to hint when the voice agent selects the OpenCode backend). */
export function codingAgentDetectOpencode(exe?: string): Promise<OpenCodeDetection> {
  return invokeOrMock('coding_agent_detect_opencode', { exe }, () => ({
    installed: false,
    version: null,
    exe: exe || 'opencode',
  }));
}

/**
 * Detects whether Codex / dsh is installed. Shares the OpenCode detection result shape.
 * `provider` takes the backend id from prefs (only `codex-cli` / `dsh-cli` are accepted).
 */
export function codingAgentDetectCli(provider: string, exe?: string): Promise<OpenCodeDetection> {
  return invokeOrMock('coding_agent_detect_cli', { provider, exe }, () => ({
    installed: false,
    version: null,
    exe: exe || (provider === 'dsh-cli' ? 'dsh' : 'codex'),
  }));
}

/** Fetches the `provider/model` list available in the current OpenCode config. */
export function codingAgentListOpencodeModels(exe?: string, refresh = true): Promise<string[]> {
  return invokeOrMock('coding_agent_list_opencode_models', { exe, refresh }, () => []);
}

/** Headless Claude run events, streamed by the backend's `coding-agent:test` (tagged by `kind`). */
export type CodingAgentEvent =
  | { kind: 'started'; sessionId: string }
  | { kind: 'delta'; sessionId: string; text: string }
  | { kind: 'tool_use'; sessionId: string; name: string }
  | {
      kind: 'completed';
      sessionId: string;
      text: string;
      costUsd: number | null;
      durationMs: number | null;
    }
  | { kind: 'cancelled'; sessionId: string }
  | { kind: 'error'; sessionId: string; message: string };

export function codingAgentDetect(exe?: string): Promise<ClaudeDetection> {
  return invokeOrMock('coding_agent_detect', { exe }, () => ({
    installed: false,
    version: null,
    exe: exe || 'claude',
    mcpServers: [],
    hasComputerUse: false,
  }));
}

export interface CodingAgentRunTestArgs {
  prompt: string;
  exe?: string;
  permissionMode?: CodingAgentPermissionMode;
  workdir?: string;
  model?: string;
  maxBudgetUsd?: number;
}

export function codingAgentRunTest(args: CodingAgentRunTestArgs): Promise<void> {
  return invokeOrMock('coding_agent_run_test', { ...args }, () => undefined);
}

export function codingAgentCancelTest(): Promise<void> {
  return invokeOrMock('coding_agent_cancel_test', undefined, () => undefined);
}

export function codingAgentCommandRisk(command: string): Promise<string | null> {
  return invokeOrMock('coding_agent_command_risk', { command }, () => null);
}
