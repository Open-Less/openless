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
  'idle' | 'disabled' | 'unsupported' | 'rejected' | 'snippet_only' | 'searching' | 'reading' | 'done' | 'no_results' | 'error' | 'cancelled';

export type AskSource = {
  sourceId: string;
  url: string;
  title: string;
  snippet?: string;
  readStatus: 'not_attempted' | 'reading' | 'read' | 'not_relevant' | 'blocked' | 'failed' | 'cancelled';
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

export type WorkingStatus = 'queued' | 'thinking' | 'writing' | 'compacting' | 'polishing';
export type TerminalStatus = 'completed' | 'cancelled' | 'failed';
export type AskMessage = { messageId: string; text: string };
export type AskRoute = 'answer' | 'selectionAnswer' | 'write' | 'edit' | 'clarify';

export type AskTool = {
  toolCallId: string;
  name: string;
  status: 'running' | 'completed' | 'failed' | 'cancelled';
  error?: AskError;
};

export type AskTurn = {
  clientRequestId?: string;
  tools?: AskTool[];
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
  clientRequestId?: string;
  tools?: AskTool[];
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

export type AskEvent = { conversationId: string; clientRequestId?: string; sequence: number } & (
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
  | ({ type: 'toolChanged'; turnId: string } & AskTool)
  | { type: 'sourceChanged'; turnId: string; source: AskSource }
  | { type: 'usageChanged'; turnId: string }
  | { type: 'turnFinished'; turnId: string; status: TerminalStatus; text?: string | null; error?: AskError }
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
} & ({ type: 'started' } | { type: 'cancelled' } | {
  type: 'failed'; stage: 'start' | 'processing';
} | { type: 'transcribed'; text: string });

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
  expandChat?(conversationId: string, turnId: string): Promise<void>;
  syncEditor?(focus: AskFocus, editor: AskEditor): Promise<boolean>;
  acknowledgeVoice?(recordingId: string): Promise<boolean>;
  onEvent(handler: (event: AskEvent) => void): Promise<() => void>;
  onWindowEvent(handler: (event: AskWindowEvent) => void): Promise<() => void>;
  onVoiceEvent(handler: (event: AskVoiceEvent) => void): Promise<() => void>;
}

// Public codes are deliberately independent of provider messages and internal error text.
const ERROR_CODES: Record<string, string> = {
  not_configured: 'ASK-1001', busy: 'ASK-1002', invalid_argument: 'ASK-1003',
  askImageUnsupported: 'ASK-1101', askDraftUnusable: 'ASK-1102',
  askAnswerUnusable: 'ASK-1102', askDraftReviewUnavailable: 'ASK-1103',
  askAnswerReviewUnavailable: 'ASK-1103', askDraftSearchUnavailable: 'ASK-1104',
  askSelectionUnavailable: 'ASK-1105', askSelectionRequired: 'ASK-1105',
  askIntentInvalid: 'ASK-1201', askIntentToolUnsupported: 'ASK-1202',
  askIntentTimeout: 'ASK-1203', askEntrySettingsUnavailable: 'ASK-1204',
  askIntentClarificationRequired: 'ASK-1205',
  askEmptyAnswer: 'ASK-1102', askEmptyModelOutput: 'ASK-1102',
  askContextLimit: 'ASK-1301', askCompactNotSmaller: 'ASK-1301',
  askContextDenied: 'ASK-1302', permission_denied: 'ASK-1302',
  askContextUnavailable: 'ASK-1303', askContextFailed: 'ASK-1303',
  askToolTimeout: 'ASK-1401', askToolLimit: 'ASK-1402',
  askToolFailed: 'ASK-1403', askSearchDisabled: 'ASK-1404',
  askPageBlocked: 'ASK-1405', askPageReadFailed: 'ASK-1405',
  askReleaseInvalidRepository: 'ASK-1406', askReleaseInvalidResponse: 'ASK-1406',
  askReleaseNetworkError: 'ASK-1406', askReleaseNotFound: 'ASK-1406',
  askReleaseRateLimited: 'ASK-1406', askReleaseRequestFailed: 'ASK-1406', askReleaseTooLarge: 'ASK-1406',
  'ask-connection': 'ASK-2001', 'ask-voice': 'ASK-2002',
  'ask-attachment': 'ASK-2003', 'ask-source': 'ASK-2004',
  'ask-stop': 'ASK-2005', 'ask-hide': 'ASK-2006', 'ask-expand': 'ASK-2007',
  'ask-copy': 'ASK-2008', 'ask-delivery': 'ASK-2009',
};

export function askErrorCode(error: unknown, operation?: string): string {
  if (typeof error === 'object' && error !== null) {
    const { reason, code, message } = error as Partial<AskError>;
    // Older native builds put a symbolic reason in message. Only exact known tokens qualify.
    for (const token of [reason, code, message]) {
      if (typeof token === 'string' && Object.hasOwn(ERROR_CODES, token)) return ERROR_CODES[token];
    }
  }
  return (operation && ERROR_CODES[operation]) || 'ASK-0000';
}

export function askErrorLabel(code: string, locale = askLocale()): string {
  const label = /^zh-(TW|HK|Hant)\b/i.test(locale) ? '出現了一點錯誤'
    : /^zh\b/i.test(locale) ? '出现了一点错误'
    : /^de\b/i.test(locale) ? 'Ein Fehler ist aufgetreten'
    : 'Something went wrong';
  return `${label} · ${code}`;
}

export function errorMessage(error: unknown, _fallback?: string): string {
  return askErrorLabel(askErrorCode(error));
}

export function askLocale() {
  let language = navigator.language;
  try {
    language = window.localStorage.getItem('ol.locale') ?? language;
  } catch {}
  return language;
}

export function askIdleLabel(locale = askLocale()) {
  if (/^zh-(TW|HK|Hant)\b/i.test(locale)) return '問任何問題';
  const language = locale.toLowerCase().split('-')[0];
  return ({
    zh: '问任何问题', ja: '何でも聞いてください', ko: '무엇이든 물어보세요',
    de: 'Frag mich alles', es: 'Pregunta lo que quieras', fr: 'Posez toutes vos questions',
  } as Record<string, string>)[language] ?? 'Ask anything';
}

export function submissionErrorMessage(error?: AskError | null) {
  return error ? errorMessage(error) : undefined;
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
