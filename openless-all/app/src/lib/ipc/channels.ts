// Typed IPC for channel configuration: ordering, enable/disable, naming and manual test results.
// Core picks the first enabled channel after sorting; credentials are read/written per channel id
// via readCredential/setCredential.

import { invokeOrMock } from './shared';
import { mockCredentialValues } from './mock-data';

export type ChannelKind = 'llm' | 'asr';

export interface ChannelTestResult {
  ok: boolean;
  latencyMs: number | null;
  /** Unix seconds (backend clock; the frontend must not generate it itself). */
  at: number;
  error: string | null;
}

export interface Channel {
  id: string;
  /** User-chosen name; empty string means unnamed, with the UI falling back to the preset display name. */
  name: string;
  /** Vendor id — decides the protocol and form shape, independent of id. */
  providerType: string;
  enabled: boolean;
  order: number;
  lastTest: ChannelTestResult | null;
}

// Sample data for the browser (non-Tauri) case, letting `npm run dev` preview the list's four
// states: active / backup / failed test in red / disabled sunk to the bottom.
const mockChannels: Record<ChannelKind, Channel[]> = {
  llm: [
    {
      id: 'siliconflow',
      name: '硅基流动-主号',
      providerType: 'siliconflow',
      enabled: true,
      order: 0,
      lastTest: { ok: true, latencyMs: 284, at: Math.floor(Date.now() / 1000) - 90, error: null },
    },
    {
      id: 'ark',
      name: '',
      providerType: 'ark',
      enabled: true,
      order: 1,
      lastTest: null,
    },
    {
      id: 'openai',
      name: 'OpenAI-备用',
      providerType: 'openai',
      enabled: false,
      order: 2,
      lastTest: {
        ok: false,
        latencyMs: null,
        at: Math.floor(Date.now() / 1000) - 3600,
        error: 'providerHttpStatus:401',
      },
    },
  ],
  asr: [
    {
      id: 'volcengine',
      name: '',
      providerType: 'volcengine',
      enabled: true,
      order: 0,
      lastTest: { ok: true, latencyMs: 143, at: Math.floor(Date.now() / 1000) - 20, error: null },
    },
    {
      id: 'groq',
      name: 'Groq-白嫖号',
      providerType: 'groq',
      enabled: true,
      order: 1,
      lastTest: null,
    },
    {
      id: 'orcarouter-asr',
      name: 'OrcaRouter-ASR',
      providerType: 'orcarouter',
      enabled: false,
      order: 2,
      lastTest: null,
    },
  ],
};

export function listChannels(kind: ChannelKind): Promise<Channel[]> {
  return invokeOrMock('list_channels', { kind }, () => mockChannels[kind]);
}

export function invalidateMockChannelTest(id: string): void {
  const channel = mockChannels.llm.find((channel) => channel.id === id);
  if (channel) channel.lastTest = null;
}

export function invalidateMockChannelTests(kind: ChannelKind): void {
  for (const channel of mockChannels[kind]) channel.lastTest = null;
}

/** Returns the backend-assigned channel id. */
export function createChannel(
  kind: ChannelKind,
  providerType: string,
  name: string,
): Promise<string> {
  return invokeOrMock('create_channel', { kind, providerType, name }, () => {
    const id = `${providerType}-${Date.now()}-${mockChannels[kind].length}`;
    mockChannels[kind].push({
      id,
      name,
      providerType,
      enabled: true,
      order: mockChannels[kind].length,
      lastTest: null,
    });
    return id;
  });
}

/** Switch provider on an existing draft card (the normal operation in the single-dialog add flow). */
export function setChannelProviderType(
  kind: ChannelKind,
  id: string,
  providerType: string,
): Promise<void> {
  return invokeOrMock('set_channel_provider_type', { kind, id, providerType }, () => {
    const channel = mockChannels[kind].find((channel) => channel.id === id);
    if (channel && channel.providerType !== providerType) {
      channel.providerType = providerType;
      channel.lastTest = null;
      if (kind === 'llm') {
        mockCredentialValues.delete(`${id}:ark.request_format`);
        mockCredentialValues.delete(`${id}:ark.service_tier`);
      }
    }
  });
}

/** Reclaim a draft with nothing filled in when the add dialog closes; returns whether it was actually deleted. */
export function deleteChannelIfBlank(kind: ChannelKind, id: string): Promise<boolean> {
  return invokeOrMock('delete_channel_if_blank', { kind, id }, () => {
    const channel = mockChannels[kind].find((channel) => channel.id === id);
    const prefix = `${id}:`;
    const hasCredentials = [...mockCredentialValues].some(
      ([key, value]) => key.startsWith(prefix) && value.length > 0,
    );
    if (!channel || channel.name.trim() || hasCredentials) return false;
    mockChannels[kind] = mockChannels[kind].filter((channel) => channel.id !== id);
    for (const key of mockCredentialValues.keys()) {
      if (key.startsWith(prefix)) mockCredentialValues.delete(key);
    }
    return true;
  });
}

export function renameChannel(kind: ChannelKind, id: string, name: string): Promise<void> {
  return invokeOrMock('rename_channel', { kind, id, name }, () => {
    const channel = mockChannels[kind].find((channel) => channel.id === id);
    if (channel) channel.name = name;
    return undefined;
  });
}

export function deleteChannel(kind: ChannelKind, id: string): Promise<void> {
  return invokeOrMock('delete_channel', { kind, id }, () => {
    mockChannels[kind] = mockChannels[kind].filter((channel) => channel.id !== id);
    for (const key of mockCredentialValues.keys())
      if (key.startsWith(`${id}:`)) mockCredentialValues.delete(key);
  });
}

export function setChannelEnabled(kind: ChannelKind, id: string, enabled: boolean): Promise<void> {
  return invokeOrMock('set_channel_enabled', { kind, id, enabled }, () => {
    const channel = mockChannels[kind].find((channel) => channel.id === id);
    if (channel) channel.enabled = enabled;
    return undefined;
  });
}

/** ids is the full post-drag order; the backend sinks unmentioned channels to the end. */
export function reorderChannels(kind: ChannelKind, ids: string[]): Promise<void> {
  return invokeOrMock('reorder_channels', { kind, ids }, () => {
    // The mock must really reorder too: otherwise the browser preview snaps back to the old
    // order via listChannels after release, looking like "drag is broken" while the real app works.
    const list = mockChannels[kind];
    const ordered = ids
      .map((id) => list.find((c) => c.id === id))
      .filter((c): c is Channel => Boolean(c));
    const rest = list.filter((c) => !ids.includes(c.id));
    mockChannels[kind] = [...ordered, ...rest].map((c, index) => ({
      ...c,
      order: index,
    }));
    return undefined;
  });
}

export function recordChannelTest(
  kind: ChannelKind,
  id: string,
  ok: boolean,
  latencyMs: number | null,
  error: string | null,
): Promise<void> {
  return invokeOrMock('record_channel_test', { kind, id, ok, latencyMs, error }, () => {
    const channel = mockChannels[kind].find((channel) => channel.id === id);
    if (channel) channel.lastTest = { ok, latencyMs, error, at: Math.floor(Date.now() / 1000) };
  });
}
