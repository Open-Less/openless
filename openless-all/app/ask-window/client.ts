export type AskError = { code: string; message: string };
export type AttachmentKind = 'image' | 'pdf' | 'selection';

export type AttachmentInfo = {
  attachmentId: string;
  kind: AttachmentKind;
  displayName: string;
  state: 'processing' | 'ready' | 'error';
  pageCount?: number;
  sourceApp?: string;
  preview?:
    | { kind: 'image'; width: number; height: number; thumbnailAvailable: boolean }
    | { kind: 'selection'; text: string; hasMore: boolean };
  error?: AskError;
};

export type AttachmentPreview =
  | { kind: 'image'; mimeType: 'image/jpeg' | 'image/png'; dataBase64: string }
  | { kind: 'selection'; text: string };

export type SearchStatus =
  'idle' | 'searching' | 'reading' | 'done' | 'no_results' | 'error' | 'cancelled';

export type AskSource = {
  sourceId: string;
  url: string;
  title: string;
  snippet?: string;
  readStatus: 'not_attempted' | 'reading' | 'read' | 'blocked' | 'failed' | 'cancelled';
};

export type AskOptions = {
  deepThinking: boolean;
  webSearch: boolean;
  publicSearchQuery?: string;
};

export type SubmitAsk = {
  conversationId: string;
  clientRequestId: string;
  text: string;
  attachmentIds: string[];
  options: AskOptions;
  voiceRecordingId?: string;
};

export type AskEditor = {
  conversationId: string;
  mountId: string;
  editorId: string;
  revision: number;
  editable: boolean;
  selectionStart: number;
  selectionEnd: number;
  attachmentIds: string[];
  options: AskOptions;
};

export type WorkingStatus = 'queued' | 'thinking' | 'writing' | 'compacting';
export type TerminalStatus = 'completed' | 'cancelled' | 'failed';
export type AskMessage = { messageId: string; text: string };

export type AskTurn = {
  turnId: string;
  userMessage: AskMessage;
  assistantMessage?: AskMessage;
  attachments: Pick<AttachmentInfo, 'attachmentId' | 'kind' | 'displayName'>[];
  deepThinking: boolean;
  status: WorkingStatus | TerminalStatus;
  searchStatus: SearchStatus;
  searchError?: AskError;
  sources: AskSource[];
  error?: AskError;
};

export type AskSnapshot = {
  conversationId: string;
  sequence: number;
  activeTurnId: string | null;
  turns: AskTurn[];
  attachments: AttachmentInfo[];
};

export type AskEvent = { conversationId: string; sequence: number } & (
  | { type: 'turnAccepted'; turn: AskTurn }
  | { type: 'answerDelta'; turnId: string; messageId: string; text: string }
  | { type: 'turnStatus'; turnId: string; status: WorkingStatus }
  | {
      type: 'searchChanged';
      turnId: string;
      toolCallId?: string;
      status: SearchStatus;
      sources?: AskSource[];
      error?: AskError;
    }
  | { type: 'sourceChanged'; turnId: string; source: AskSource }
  | { type: 'usageChanged'; turnId: string }
  | { type: 'turnFinished'; turnId: string; status: TerminalStatus; error?: AskError }
  | { type: 'attachmentChanged'; attachment: AttachmentInfo }
  | { type: 'attachmentRemoved'; attachmentId: string }
);

export type AskReplay = { events: AskEvent[]; latestSequence: number; truncated: boolean };
export type AskTheme = 'light' | 'dark' | 'system';
export type AskWindowEvent =
  | { type: 'activated' | 'hidden' }
  | { type: 'themeChanged'; themeMode: AskTheme }
  | { type: 'notice'; error: AskError };
export type AskVoiceEvent = {
  conversationId: string;
  recordingId: string;
  mode: 'insert' | 'submit';
} & ({ type: 'started' | 'cancelled' } | { type: 'transcribed'; text: string });

export interface AskClient {
  current(): Promise<{
    conversationId: string | null;
    themeMode?: AskTheme;
    platform?: string;
    notice?: AskError | null;
  }>;
  newConversation(): Promise<{ conversationId: string }>;
  endConversation(conversationId: string): Promise<void>;
  snapshot(conversationId: string): Promise<AskSnapshot>;
  events(conversationId: string, afterSequence: number): Promise<AskReplay>;
  submit(request: SubmitAsk): Promise<{ turnId: string }>;
  cancel(conversationId: string, turnId: string): Promise<void>;
  addAttachment(conversationId: string): Promise<AttachmentInfo | null>;
  addDroppedImage(
    conversationId: string,
    displayName: string,
    dataBase64: string,
  ): Promise<AttachmentInfo>;
  removeAttachment(conversationId: string, attachmentId: string): Promise<void>;
  getAttachmentPreview(conversationId: string, attachmentId: string): Promise<AttachmentPreview | null>;
  openSource(conversationId: string, turnId: string, sourceId: string): Promise<void>;
  hide(): Promise<void>;
  syncEditor?(editor: AskEditor): Promise<boolean>;
  acknowledgeVoice?(recordingId: string): Promise<unknown>;
  onEvent(handler: (event: AskEvent) => void): Promise<() => void>;
  onWindowEvent(handler: (event: AskWindowEvent) => void): Promise<() => void>;
  onVoiceEvent(handler: (event: AskVoiceEvent) => void): Promise<() => void>;
}

export function errorMessage(error: unknown, fallback: string): string {
  if (
    typeof error === 'object' &&
    error &&
    'message' in error &&
    typeof error.message === 'string'
  ) {
    return error.message || fallback;
  }
  return typeof error === 'string' && error ? error : fallback;
}

export function newRequestId(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(16)), (byte) =>
    byte.toString(16).padStart(2, '0'),
  ).join('');
}

export function requestFingerprint({
  text,
  attachmentIds,
  options,
}: Omit<SubmitAsk, 'clientRequestId'>) {
  return JSON.stringify([
    text,
    attachmentIds,
    options.deepThinking,
    options.webSearch,
    options.publicSearchQuery,
  ]);
}
