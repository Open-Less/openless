export type AskError = { code: string; message: string; reason?: string };
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
};

export type SubmitAsk = {
  conversationId: string;
  clientRequestId: string;
  text: string;
  attachmentIds: string[];
  options: AskOptions;
};

export type AskFocus = { focusEpoch: number; focusRequest: number };

export type AskEditor = {
  conversationId: string;
  mountId: string;
  editorId: string;
  revision: number;
  editable: boolean;
  selectionStart: number;
  selectionEnd: number;
};

export type WorkingStatus = 'queued' | 'thinking' | 'writing' | 'compacting';
export type TerminalStatus = 'completed' | 'cancelled' | 'failed';
export type AskMessage = { messageId: string; text: string };
export type AskRoute = 'answer' | 'selectionAnswer' | 'write' | 'edit' | 'clarify';

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
  submission?: AskSubmission | null;
};

export type AskSubmission = {
  currentTurnId: string;
  route?: AskRoute | null;
  status: WorkingStatus | TerminalStatus;
  searchStatus: SearchStatus;
  searchError?: AskError | null;
  sources: AskSource[];
  resultText?: string | null;
  delivery?: 'pending' | 'written' | 'retained' | 'unknown' | null;
  copyStatus: 'notNeeded' | 'pending' | 'copied' | 'failed';
  error?: AskError | null;
};

export type AskEvent = { conversationId: string; sequence: number } & (
  | { type: 'submitted'; submission: AskSubmission; attachmentIds?: string[] }
  | { type: 'routeResolved'; turnId: string; route: AskRoute }
  | { type: 'delivery'; submission: AskSubmission }
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
  | ({ type: 'activated' | 'sessionChanged'; conversationId?: string | null } & Partial<AskFocus>)
  | { type: 'hidden' }
  | { type: 'themeChanged'; themeMode: AskTheme }
  | { type: 'notice'; error: AskError };
export type AskVoiceEvent = Omit<AskEditor, 'editable'> & {
  recordingId: string;
  focusEpoch: number;
} & ({ type: 'started' } | { type: 'cancelled' } | { type: 'transcribed'; text: string });

export interface AskClient {
  current(): Promise<
    Partial<AskFocus> & {
      conversationId: string | null;
      themeMode?: AskTheme;
      platform?: string;
      notice?: AskError | null;
    }
  >;
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
  getAttachmentPreview(
    conversationId: string,
    attachmentId: string,
  ): Promise<AttachmentPreview | null>;
  openSource(conversationId: string, turnId: string, sourceId: string): Promise<void>;
  hide(): Promise<void>;
  syncEditor?(focus: AskFocus, editor: AskEditor): Promise<boolean>;
  acknowledgeVoice?(recordingId: string): Promise<boolean>;
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

const INTENT_ERRORS: Record<string, [english: string, chinese: string]> = {
  askIntentInvalid: [
    'Ask could not determine what to do. Try making your request more specific.',
    'Ask 无法确定该怎么处理。请试着把要求说得更具体一些。',
  ],
  askIntentToolUnsupported: [
    'The configured model does not support tool-based intent detection. Change the model or the Ask intent mode in Settings.',
    '当前模型不支持通过工具识别意图。请在设置中更换模型或调整 Ask 意图识别模式。',
  ],
  askIntentTimeout: [
    'Ask took too long to determine what to do. Try again.',
    'Ask 判断请求意图超时，请重试。',
  ],
  askEntrySettingsUnavailable: [
    'Ask could not read its settings. Open Ask settings and save your preferences again.',
    'Ask 无法读取设置。请打开 Ask 设置并重新保存偏好。',
  ],
};

export function submissionErrorMessage(error?: AskError | null) {
  if (!error) return undefined;
  const localized = error.reason && INTENT_ERRORS[error.reason];
  if (!localized) return error.message;
  let language = navigator.language;
  try {
    language = window.localStorage.getItem('ol.locale') ?? language;
  } catch {}
  return localized[/^zh\b/i.test(language) ? 1 : 0];
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
  return JSON.stringify([text, attachmentIds, options.deepThinking]);
}
