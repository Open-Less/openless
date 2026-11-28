import { inputMethodEngine, InputMethodExtensionContext } from '@kit.IMEKit';
import { BusinessError } from '@kit.BasicServicesKit';
import { initializeRime, rimeRequest, RimeState, cloudCandidates, refreshPublicDictionary, PublicDictionaryInfo } from './RimeClient';
import { preferences } from '@kit.ArkData';
import { VoiceClient } from './VoiceClient';
import { KeySound } from './KeySound';
import { pasteboard } from '@kit.BasicServicesKit';
import { ClipboardEntry, ClipboardHistory } from './ClipboardHistory';

class InputPanelState {
  connected: boolean = false;
  status: string = '等待输入框';
  enterLabel: string = '换行';
  chinese: boolean = true;
  preedit: string = '';
  candidates: string[] = [];
  candidatesLoading: boolean = false;
  toolbarActions: string[] = ['layout', 'emoji', 'voice', 'settings'];
  keyFontSize: number = 22;
  swipeSymbols: boolean = true;
  keySound: boolean = false;
  clipboardItems: ClipboardEntry[] = [];
  clipboardStatus: string = '';
  clipboardAutoRead: boolean = false;
  rimeReady: boolean = false;
  page: number = 0;
  hasNext: boolean = false;
  password: boolean = false;
  editorVerified: boolean = false;
  voicePhase: string = 'idle';
  voiceHolding: boolean = false;
  voiceText: string = '';
  voiceLevel: number = 0;
  pendingCommit: boolean = false;
  writeInFlight: boolean = false;
  commitNeedsConfirmation: boolean = false;
  cloudEnabled: boolean = false;
  symbolFeedback: string = '';
  keyboardWidth: number = 574;
  keyWidth: number = 52;
  dictionaryStatus: string = '公共词库：自动更新';
  dictionaryAuto: boolean = true;
}

export const imeStorage: LocalStorage = new LocalStorage(new InputPanelState());

// Only the system-provided client may edit the target app. Never retain a
// queued operation across inputStop/inputStart or inspect another app's text.
class InputSession {
  private client?: inputMethodEngine.InputClient;
  private keyboard?: inputMethodEngine.KeyboardController;
  private context?: InputMethodExtensionContext;
  private action: number = inputMethodEngine.ENTER_KEY_TYPE_UNSPECIFIED;
  private queue: Promise<void> = Promise.resolve();
  private generation: number = 0;
  private rimeSession: string = '';
  private encoding: string = '';
  private candidateRevision: number = 0;
  private voice: VoiceClient = new VoiceClient(true);
  private voiceGeneration: number = 0;
  private voiceReleaseRequested: boolean = false;
  private observedText?: string;
  private selectionBegin: number = -1;
  private selectionEnd: number = -1;
  private expectedText?: string;
  private confirmWrite?: () => void;
  private compatibilityEditor: boolean = false;
  private queueEpoch: number = 0;
  private writeId: number = 0;
  private cloudEpoch: number = 0;
  private cloudTimer: number = -1;
  private cloudInFlight: boolean = false;
  private feedbackTimer: number = -1;
  private localCandidates: string[] = [];
  private candidatePageSize: number = 0;
  private candidateBase: number = 0;
  private candidatesAppended: boolean = false;
  private candidateTimer: number = -1;
  private cloudChoices: string[] = [];
  private cloudSettings?: preferences.Preferences;
  private sound?: KeySound;
  private clipboard?: ClipboardHistory;
  private clipboardSaving: Promise<void> = Promise.resolve();
  private clipboardBoard?: pasteboard.SystemPasteboard;
  private clipboardChanged: () => void = () => { this.refreshClipboard(); };
  private clipboardRead: number = 0;

  attach(context: InputMethodExtensionContext, keyboard: inputMethodEngine.KeyboardController,
    client: inputMethodEngine.InputClient): void {
    if (this.client) this.detach();
    this.generation++;
    const generation = this.generation;
    this.context = context;
    this.keyboard = keyboard;
    this.client = client;
    this.cloudSettings = undefined;
    this.clipboard = undefined;
    let clipboardAutoRead = false;
    try { clipboardAutoRead = context.resourceManager.getBooleanByName('ime_clipboard_auto_read'); } catch (_) {}
    imeStorage.setOrCreate('clipboardAutoRead', clipboardAutoRead);
    imeStorage.setOrCreate('clipboardStatus', clipboardAutoRead ? '' : '自动读取待权限开放；当前可主动粘贴并保存本地记录');
    imeStorage.setOrCreate('cloudEnabled', false);
    preferences.getPreferences(context, { name: 'ime_cloud_candidates' }).then(async (settings: preferences.Preferences) => {
      if (generation !== this.generation) return;
      this.cloudSettings = settings;
      const enabled = await settings.get('enabled', false);
      const publicUpdates = await settings.get('public_updates', true);
      const toolbar = await settings.get('toolbar_actions', 'layout,emoji,voice,settings');
      const font = Number(await settings.get('key_font_size', 22));
      const swipe = await settings.get('swipe_symbols', true);
      const sound = await settings.get('key_sound', false);
      await this.clipboardSaving;
      const clipboard = await settings.get('clipboard_history', '[]');
      if (generation === this.generation) {
        if (typeof clipboard === 'string') {
          this.clipboard = new ClipboardHistory();
          this.clipboard.load(clipboard);
          imeStorage.setOrCreate('clipboardItems', this.clipboard.list());
          this.refreshClipboard();
        }
        if ([20, 22, 24, 26].includes(font)) imeStorage.setOrCreate('keyFontSize', font);
        imeStorage.setOrCreate('swipeSymbols', swipe !== false);
        imeStorage.setOrCreate('keySound', sound === true);
        if (typeof toolbar === 'string') {
          const actions = toolbar.split(',');
          if (actions.length === 4 && actions.every((action: string) => this.toolbarActionAllowed(action))) {
            imeStorage.setOrCreate('toolbarActions', actions);
          }
        }
        imeStorage.setOrCreate('cloudEnabled', enabled === true);
        imeStorage.setOrCreate('dictionaryAuto', publicUpdates === true);
        if (imeStorage.get<boolean>('rimeReady')) this.checkPublicDictionary(false);
      }
    }).catch(() => {});
    this.action = inputMethodEngine.ENTER_KEY_TYPE_UNSPECIFIED;
    this.resetObservation();
    this.compatibilityEditor = false;
    imeStorage.setOrCreate('connected', true);
    imeStorage.setOrCreate('status', '可输入');
    imeStorage.setOrCreate('enterLabel', '换行');
    imeStorage.setOrCreate('rimeReady', false);
    imeStorage.setOrCreate('password', true);
    imeStorage.setOrCreate('editorVerified', false);
    this.resetCandidates();
    this.readAttributes(client).then((attribute: inputMethodEngine.EditorAttribute) => {
      if (this.client !== client || generation !== this.generation) return;
      const password = [inputMethodEngine.PATTERN_PASSWORD, inputMethodEngine.PATTERN_PASSWORD_NUMBER,
        inputMethodEngine.PATTERN_PASSWORD_SCREEN_LOCK, inputMethodEngine.PATTERN_NEW_PASSWORD].includes(attribute.inputPattern);
      imeStorage.setOrCreate('chinese', !password);
      imeStorage.setOrCreate('password', password);
      imeStorage.setOrCreate('editorVerified', true);
      if (!password) this.watchClipboard();
      this.compatibilityEditor = !attribute.bundleName || attribute.bundleName === 'com.droi.tong';
      console.info('[OpenLess IME] editor transport=async-observed' +
        ' bundle=' + (attribute.bundleName || 'unknown'));
      if (!password) this.prepareRime(context, generation, true);
      this.action = attribute.enterKeyType;
      let label = '换行';
      if (this.action === inputMethodEngine.ENTER_KEY_TYPE_SEND) label = '发送';
      else if (this.action === inputMethodEngine.ENTER_KEY_TYPE_SEARCH) label = '搜索';
      else if (this.action === inputMethodEngine.ENTER_KEY_TYPE_DONE) label = '完成';
      else if (this.action === inputMethodEngine.ENTER_KEY_TYPE_NEXT) label = '下一项';
      else if (this.action === inputMethodEngine.ENTER_KEY_TYPE_GO) label = '前往';
      imeStorage.setOrCreate('enterLabel', label);
    }).catch((error: BusinessError) => {
      if (this.client !== client || generation !== this.generation) return;
      // Compatibility runtimes may not implement editor attributes. Keep
      // typing available, but never learn or record audio in an unknown field.
      console.warn('[OpenLess IME] editor attributes unavailable code=' + error.code);
      imeStorage.setOrCreate('password', false);
      imeStorage.setOrCreate('chinese', true);
      this.prepareRime(context, generation, false);
    });
  }

  private async readAttributes(client: inputMethodEngine.InputClient): Promise<inputMethodEngine.EditorAttribute> {
    let timer: number = -1;
    const deadline = new Promise<inputMethodEngine.EditorAttribute>((_resolve, reject) => {
      timer = setTimeout(() => { reject(new Error('editor attributes timeout')); }, 1200);
    });
    try {
      return await Promise.race([Promise.resolve().then(() => client.getEditorAttribute()), deadline]);
    } finally { clearTimeout(timer); }
  }

  private prepareRime(context: InputMethodExtensionContext, generation: number, learning: boolean): void {
    const epoch = this.queueEpoch;
    this.queue = this.queue.then(async (): Promise<void> => {
      if (generation !== this.generation || epoch !== this.queueEpoch) return;
      await initializeRime(context.filesDir, context.resourceManager, 'rime-user', context.cacheDir);
      if (generation !== this.generation || epoch !== this.queueEpoch) return;
      const state = await rimeRequest('create', '', learning ? 'learning' : 'private');
      if (generation !== this.generation || epoch !== this.queueEpoch) { await rimeRequest('destroy', state.session); return; }
      this.rimeSession = state.session;
      imeStorage.setOrCreate('rimeReady', true);
      this.publish(state);
      console.info('[OpenLess IME] editor ready learning=' + learning);
      this.checkPublicDictionary(false);
    }).catch(() => { if (generation === this.generation && epoch === this.queueEpoch) imeStorage.setOrCreate('status', '中文引擎加载失败，可切换英文'); });
  }

  detach(): void {
    this.clipboardRead++;
    try { this.clipboardBoard?.off('update', this.clipboardChanged); } catch (_) {}
    this.clipboardBoard = undefined;
    clearTimeout(this.feedbackTimer);
    imeStorage.setOrCreate('symbolFeedback', '');
    this.cancelVoice();
    this.resetObservation();
    this.generation++;
    this.client = undefined;
    this.keyboard = undefined;
    this.context = undefined;
    const session = this.rimeSession;
    this.rimeSession = '';
    // A compatibility client's unfinished IPC must not block the next field.
    this.queueEpoch++;
    this.writeId++;
    this.queue = Promise.resolve();
    imeStorage.setOrCreate('writeInFlight', false);
    if (session) rimeRequest('destroy', session).catch(() => {});
    this.resetCandidates();
    imeStorage.setOrCreate('connected', false);
    imeStorage.setOrCreate('editorVerified', false);
    imeStorage.setOrCreate('status', '等待输入框');
  }

  private perform(kind: string, operation: (client: inputMethodEngine.InputClient) => Promise<boolean>): void {
    const client = this.client;
    const generation = this.generation;
    const epoch = this.queueEpoch;
    if (!client) return;
    const task = this.queue.then(async (): Promise<void> => {
      if (this.client !== client || this.generation !== generation || epoch !== this.queueEpoch) return;
      try {
        const ok = await operation(client);
        console.info('[OpenLess IME] ' + kind + ' result=' + ok);
        if (this.client === client && this.generation === generation && epoch === this.queueEpoch) {
          if (!ok && imeStorage.get<boolean>('pendingCommit')) imeStorage.setOrCreate('commitNeedsConfirmation', true);
          imeStorage.setOrCreate('status', ok ? '可输入' : '输入未成功，请重新选择输入框');
        }
      } catch (error) {
        if (this.client === client && this.generation === generation && epoch === this.queueEpoch) {
          if (imeStorage.get<boolean>('pendingCommit')) imeStorage.setOrCreate('commitNeedsConfirmation', true);
          imeStorage.setOrCreate('status', '输入未成功，请重新选择输入框');
        }
        console.error('[OpenLess IME] ' + kind + ' failed code=' + (error as BusinessError).code);
      }
    });
    this.queue = task.catch(() => {});
  }

  private resetObservation(): void {
    this.observedText = undefined;
    this.selectionBegin = -1;
    this.selectionEnd = -1;
    this.expectedText = undefined;
    this.confirmWrite = undefined;
  }

  observeSelection(begin: number, end: number): void {
    if (!this.client || !imeStorage.get<boolean>('editorVerified') || imeStorage.get<boolean>('password')) return;
    this.selectionBegin = Math.min(begin, end);
    this.selectionEnd = Math.max(begin, end);
  }

  observeText(text: string): void {
    if (!this.client || !imeStorage.get<boolean>('editorVerified') || imeStorage.get<boolean>('password')) return;
    // Only a bounded, transient copy of the active editor's notification is
    // retained. Never log text or infer success from cursor movement alone.
    if (this.expectedText !== undefined && text === this.expectedText) this.confirmWrite?.();
    this.observedText = text.length <= 4096 ? text : undefined;
  }

  private async writeText(client: inputMethodEngine.InputClient, text: string): Promise<boolean> {
    if (this.compatibilityEditor &&
      imeStorage.get<boolean>('editorVerified') && !imeStorage.get<boolean>('password')) {
      // Advancing Rime means the commit was dispatched, not that the host
      // acknowledged it. A missing bridge reply must not lock composition.
      // Dispatch once and never replay a late or failed operation.
      this.observeDispatched(client, client.insertText(text), 'write');
      console.info('[OpenLess IME] compatibility write dispatched without acknowledgement');
      return true;
    }
    const writeId = ++this.writeId;
    const baseline = this.observedText;
    const begin = this.selectionBegin;
    const end = this.selectionEnd;
    this.expectedText = baseline !== undefined && begin >= 0 && end <= baseline.length ?
      baseline.slice(0, begin) + text + baseline.slice(end) : undefined;
    if (this.expectedText === baseline) this.expectedText = undefined;
    console.info('[OpenLess IME] write observation available=' + (this.expectedText !== undefined));
    let timer: number = -1;
    const observed = new Promise<boolean>((resolve) => { this.confirmWrite = () => resolve(true); });
    const deadline = new Promise<boolean>((resolve) => { timer = setTimeout(() => resolve(false), 800); });
    imeStorage.setOrCreate('writeInFlight', true);
    try {
      // Send once. The bridge may apply the edit without returning its IPC
      // acknowledgement. Exact text notifications can independently confirm it.
      const sent = Promise.resolve().then(() => client.insertText(text)).then((ok: boolean) => {
        if (ok) return true;
        return observed;
      }).catch(() => observed);
      const ok = await Promise.race([sent, observed, deadline]);
      console.info('[OpenLess IME] write transport=async-observed result=' + ok);
      if (!ok && writeId === this.writeId && imeStorage.get<boolean>('pendingCommit')) {
        imeStorage.setOrCreate('commitNeedsConfirmation', true);
      }
      return ok;
    } catch (error) {
      console.error('[OpenLess IME] write transport=async-observed' +
        ' failed code=' + (error as BusinessError).code);
      throw new Error('文字提交未确认；若已出现在输入框，请点“已写入”');
    } finally {
      clearTimeout(timer);
      if (writeId === this.writeId) {
        this.expectedText = undefined;
        this.confirmWrite = undefined;
        imeStorage.setOrCreate('writeInFlight', false);
      }
    }
  }

  private async deleteText(client: inputMethodEngine.InputClient): Promise<boolean> {
    if (this.compatibilityEditor && imeStorage.get<boolean>('editorVerified') &&
      !imeStorage.get<boolean>('password')) {
      this.observeDispatched(client, client.deleteForward(1), 'delete');
      return true;
    }
    let timer: number = -1;
    const deadline = new Promise<boolean>((resolve) => { timer = setTimeout(() => resolve(false), 800); });
    try { return await Promise.race([Promise.resolve().then(() => client.deleteForward(1)), deadline]); }
    finally { clearTimeout(timer); }
  }

  private observeDispatched(client: inputMethodEngine.InputClient, operation: Promise<boolean>, kind: string): void {
    const generation = this.generation;
    const epoch = this.queueEpoch;
    operation.then((ok: boolean) => {
      if (!ok && this.client === client && generation === this.generation && epoch === this.queueEpoch) {
        imeStorage.setOrCreate('status', '兼容编辑框未确认操作，请检查输入框');
      }
    }).catch((error: BusinessError) => {
      // This bridge reports 12800003 even for edits the user confirmed were
      // applied. Do not replay them or let a missing reply stall the queue.
      console.warn('[OpenLess IME] compatibility ' + kind + ' reply unavailable code=' + error.code);
      if (error.code !== 12800003 && this.client === client && generation === this.generation && epoch === this.queueEpoch) {
        imeStorage.setOrCreate('status', '兼容编辑框操作失败，请检查输入框');
      }
    });
  }

  private async sendAction(client: inputMethodEngine.InputClient, action: number): Promise<boolean> {
    if (this.compatibilityEditor && imeStorage.get<boolean>('editorVerified') &&
      !imeStorage.get<boolean>('password')) {
      this.observeDispatched(client, client.sendKeyFunction(action), 'action');
      return true;
    }
    return client.sendKeyFunction(action);
  }

  insert(text: string): void {
    this.playKeySound();
    if (imeStorage.get<boolean>('chinese') && /^[a-z]$/i.test(text)) {
      this.rimeOperation('key', text.toLowerCase().charCodeAt(0).toString());
      return;
    }
    if (imeStorage.get<boolean>('chinese')) { this.rimeOperation('literal', text); return; }
    if (text.length) this.perform('insert', (client: inputMethodEngine.InputClient) => this.writeText(client, text));
  }

  bindingVersion(): number { return this.generation; }

  private symbolMessage(message: string): void {
    clearTimeout(this.feedbackTimer);
    imeStorage.setOrCreate('symbolFeedback', message);
    this.feedbackTimer = setTimeout(() => { imeStorage.setOrCreate('symbolFeedback', ''); }, 1400);
  }

  insertSymbol(text: string): void {
    this.playKeySound();
    if (!this.client) { this.symbolMessage('请先选择输入框'); return; }
    this.symbolMessage('正在输入「' + text + '」');
    if (imeStorage.get<boolean>('chinese')) { this.rimeOperation('literal', text, true); return; }
    const generation = this.generation;
    this.perform('symbol', async (client: inputMethodEngine.InputClient): Promise<boolean> => {
      let ok = false;
      try { ok = await this.writeText(client, text); } catch (_) {}
      if (generation === this.generation && client === this.client) {
        this.symbolMessage(ok ? (this.compatibilityEditor ? '已发送「' : '已输入「') + text + '」' : '未能确认，请检查输入框');
      }
      return ok;
    });
  }

  backspace(): void {
    this.playKeySound();
    // Check encoding inside the serial queue; fast typing may still be pending.
    this.rimeOperation('backspace', '');
  }

  private resetCandidates(): void {
    clearTimeout(this.candidateTimer);
    this.cloudEpoch++;
    clearTimeout(this.cloudTimer);
    this.cloudChoices = [];
    this.localCandidates = [];
    this.candidatePageSize = 0;
    this.candidateBase = 0;
    this.candidatesAppended = false;
    imeStorage.setOrCreate('candidatesLoading', false);
    this.encoding = '';
    imeStorage.setOrCreate('preedit', '');
    imeStorage.setOrCreate('candidates', [] as string[]);
    imeStorage.setOrCreate('pendingCommit', false);
    imeStorage.setOrCreate('commitNeedsConfirmation', false);
  }

  private publish(state: RimeState): void {
    clearTimeout(this.candidateTimer);
    this.cloudEpoch++;
    clearTimeout(this.cloudTimer);
    this.cloudChoices = [];
    this.localCandidates = state.candidates;
    if (state.page === 0 && state.candidates.length > 0) this.candidatePageSize = state.candidates.length;
    this.candidateBase = state.candidateOffset ?? state.page * this.candidatePageSize;
    this.candidatesAppended = false;
    this.candidateRevision = state.revision;
    this.encoding = state.input;
    imeStorage.setOrCreate('preedit', state.preedit || state.input);
    imeStorage.setOrCreate('candidates', state.candidates);
    imeStorage.setOrCreate('page', state.page);
    imeStorage.setOrCreate('hasNext', state.hasNext);
    imeStorage.setOrCreate('pendingCommit', !!state.commit);
    // A normal asynchronous commit is not an error recovery screen.
    imeStorage.setOrCreate('commitNeedsConfirmation', false);
    imeStorage.setOrCreate('status', 'Rime 拼音');
    this.scheduleCloud();
    if (state.input && !state.commit && state.hasNext && state.page === 0) {
      this.candidateTimer = setTimeout(() => { this.loadMoreCandidates(); }, 60);
    }
  }

  private scheduleCloud(): void {
    clearTimeout(this.cloudTimer);
    const context = this.context;
    const generation = this.generation;
    const epoch = this.cloudEpoch;
    const input = this.encoding;
    if (context && input.length >= 2 && this.candidateBase === 0 && !imeStorage.get<boolean>('pendingCommit') &&
      imeStorage.get<boolean>('chinese') &&
      imeStorage.get<boolean>('cloudEnabled') && imeStorage.get<boolean>('editorVerified') &&
      !imeStorage.get<boolean>('password') && /^[a-z']{2,64}$/.test(input)) {
      this.cloudTimer = setTimeout(() => {
        if (this.cloudInFlight || epoch !== this.cloudEpoch || generation !== this.generation || !imeStorage.get<boolean>('cloudEnabled')) return;
        this.cloudInFlight = true;
        cloudCandidates(context.filesDir, context.cacheDir, input).then((rows: string[]) => {
          console.info('[OpenLess IME] cloud response candidates=' + rows.length);
          if (epoch !== this.cloudEpoch || generation !== this.generation || !imeStorage.get<boolean>('cloudEnabled')) return;
          this.cloudChoices = rows.filter((text: string) => !this.localCandidates.includes(text));
          imeStorage.setOrCreate('candidates', this.localCandidates.slice(0, 1).concat(this.cloudChoices).concat(this.localCandidates.slice(1)));
        }).catch(() => {}).finally(() => {
          this.cloudInFlight = false;
          if (epoch !== this.cloudEpoch) this.scheduleCloud();
        });
      }, 180);
    }
  }

  toggleCloudCandidates(): void {
    if (!this.cloudSettings) { imeStorage.setOrCreate('status', '云候选设置暂不可用'); return; }
    const enabled = !imeStorage.get<boolean>('cloudEnabled');
    imeStorage.setOrCreate('cloudEnabled', enabled);
    this.cloudEpoch++;
    clearTimeout(this.cloudTimer);
    this.cloudChoices = [];
    imeStorage.setOrCreate('candidates', this.localCandidates);
    if (this.cloudSettings) {
      this.cloudSettings.put('enabled', enabled).then(() => this.cloudSettings?.flush()).catch(() => {});
    }
  }

  checkPublicDictionary(force: boolean = true): void {
    if (!force && (!this.cloudSettings || !imeStorage.get<boolean>('dictionaryAuto'))) return;
    const context = this.context;
    if (!context) return;
    imeStorage.setOrCreate('dictionaryStatus', '公共词库：正在检查');
    refreshPublicDictionary(context.filesDir, context.cacheDir, force).then((info: PublicDictionaryInfo | undefined) => {
      imeStorage.setOrCreate('dictionaryStatus', !info ? '公共词库：离线可用，稍后重试' : info.pending ?
        '公共词库：已更新，下次启动生效' : '公共词库：已检查');
    }).catch(() => { imeStorage.setOrCreate('dictionaryStatus', '公共词库：离线可用，稍后重试'); });
  }

  togglePublicDictionaryUpdates(): void {
    const settings = this.cloudSettings;
    if (!settings) return;
    const enabled = !imeStorage.get<boolean>('dictionaryAuto');
    imeStorage.setOrCreate('dictionaryAuto', enabled);
    settings.put('public_updates', enabled).then(() => settings.flush()).catch(() => {});
    if (enabled) this.checkPublicDictionary(false);
  }

  private toolbarActionAllowed(action: string): boolean {
    return ['layout', 'emoji', 'voice', 'settings', 'symbols', 'numbers', 'language', 'inputMethods'].includes(action);
  }

  setToolbarAction(slot: number, action: string): void {
    const settings = this.cloudSettings;
    if (!settings || slot < 0 || slot > 3 || !Number.isInteger(slot) || !this.toolbarActionAllowed(action)) return;
    const actions = (imeStorage.get<string[]>('toolbarActions') || ['layout', 'emoji', 'voice', 'settings']).slice();
    actions[slot] = action;
    imeStorage.setOrCreate('toolbarActions', actions);
    settings.put('toolbar_actions', actions.join(',')).then(() => settings.flush()).catch(() => {
      imeStorage.setOrCreate('status', '快捷入口保存失败，请重试');
    });
  }

  setKeyFontSize(size: number): void {
    if (![20, 22, 24, 26].includes(size)) return;
    imeStorage.setOrCreate('keyFontSize', size);
    this.cloudSettings?.put('key_font_size', size).then(() => this.cloudSettings?.flush()).catch(() => {});
  }

  toggleKeyOption(option: string): void {
    if (option !== 'swipeSymbols' && option !== 'keySound') return;
    const enabled = !imeStorage.get<boolean>(option);
    imeStorage.setOrCreate(option, enabled);
    this.cloudSettings?.put(option === 'swipeSymbols' ? 'swipe_symbols' : 'key_sound', enabled)
      .then(() => this.cloudSettings?.flush()).catch(() => {});
    if (option === 'keySound' && enabled) this.playKeySound();
  }

  private playKeySound(): void {
    if (!imeStorage.get<boolean>('keySound') || !this.context) return;
    if (!this.sound) this.sound = new KeySound();
    this.sound.play(this.context);
  }

  pasteClipboard(): void {
    if (!this.client || imeStorage.get<boolean>('pendingCommit')) return;
    const generation = this.generation;
    pasteboard.getSystemPasteboard().getData().then((data: pasteboard.PasteData) => {
      if (generation !== this.generation) return;
      const text = data.getPrimaryText();
      if (text) {
        if (imeStorage.get<boolean>('editorVerified') && !imeStorage.get<boolean>('password')) {
          this.clipboard?.add(text);
          this.saveClipboard();
        }
        this.insert(text);
      } else imeStorage.setOrCreate('status', '剪贴板中没有文字');
    }).catch(() => {
      if (generation === this.generation) imeStorage.setOrCreate('clipboardStatus', '无法读取剪贴板，请检查剪贴板授权后重试');
    });
  }

  private watchClipboard(): void {
    if (!imeStorage.get<boolean>('clipboardAutoRead')) return;
    if (!this.clipboardBoard) {
      try {
        this.clipboardBoard = pasteboard.getSystemPasteboard();
        this.clipboardBoard.on('update', this.clipboardChanged);
      } catch (_) { this.clipboardBoard = undefined; }
    }
    this.refreshClipboard();
  }

  refreshClipboard(): void {
    if (!imeStorage.get<boolean>('clipboardAutoRead') || !this.client || !this.clipboard || !imeStorage.get<boolean>('editorVerified') ||
      imeStorage.get<boolean>('password')) return;
    const generation = this.generation;
    const read = ++this.clipboardRead;
    pasteboard.getSystemPasteboard().getData().then((data: pasteboard.PasteData) => {
      if (generation !== this.generation || read !== this.clipboardRead) return;
      const text = data.getPrimaryText();
      if (text) { this.clipboard?.add(text); this.saveClipboard(); }
      imeStorage.setOrCreate('clipboardStatus', text ? '已读取系统剪贴板，复制新文字后自动更新' : '系统剪贴板中没有文字');
    }).catch((error: BusinessError) => {
      if (generation !== this.generation || read !== this.clipboardRead) return;
      imeStorage.setOrCreate('clipboardStatus', error.code === 201 ?
        '请先授权读取剪贴板，再返回输入框' : '暂时无法读取系统剪贴板，请刷新重试');
      console.warn('[OpenLess IME] clipboard read failed code=' + error.code);
    });
  }

  authorizeClipboard(): void {
    if (!imeStorage.get<boolean>('clipboardAutoRead')) return;
    this.context?.startAbility({ bundleName: 'openless.hm.com', abilityName: 'EntryAbility',
      parameters: { requestClipboardPermission: true } }).catch(() => {
      imeStorage.setOrCreate('clipboardStatus', '无法打开授权页面，请打开 OpenLess 应用重试');
    });
  }

  private saveClipboard(): void {
    if (!this.clipboard) return;
    imeStorage.setOrCreate('clipboardItems', this.clipboard.list());
    const settings = this.cloudSettings;
    const generation = this.generation;
    if (!settings) return;
    const saved = this.clipboard.serialize();
    this.clipboardSaving = this.clipboardSaving.then(() => settings.put('clipboard_history', saved)).then(() => settings.flush()).catch(() => {
      if (generation === this.generation) imeStorage.setOrCreate('status', '剪贴板记录保存失败');
    });
  }

  pasteClipboardItem(id: string): void {
    const text = this.clipboard?.get(id);
    if (!text || !this.client || imeStorage.get<boolean>('pendingCommit')) return;
    this.clipboard?.add(text); this.saveClipboard(); this.insert(text);
  }

  copyClipboardItem(id: string): void {
    const text = this.clipboard?.get(id);
    if (!text) return;
    const generation = this.generation;
    pasteboard.getSystemPasteboard().setData(pasteboard.createData(pasteboard.MIMETYPE_TEXT_PLAIN, text)).then(() => {
      if (generation === this.generation) imeStorage.setOrCreate('status', '已复制到系统剪贴板');
    }).catch(() => {
      if (generation === this.generation) imeStorage.setOrCreate('status', '复制失败，请重试');
    });
  }

  pinClipboardItem(id: string): void { this.clipboard?.pin(id); this.saveClipboard(); }
  deleteClipboardItem(id: string): void { this.clipboardRead++; this.clipboard?.remove(id); this.saveClipboard(); }
  clearClipboardHistory(): void { this.clipboardRead++; this.clipboard?.clear(); this.saveClipboard(); }

  setPolishMode(mode: string): void {
    if (!this.context || !['raw', 'light', 'structured', 'formal'].includes(mode)) return;
    const generation = this.generation;
    this.voice.setPolishMode(this.context, mode).then(() => {
      if (generation === this.generation) imeStorage.setOrCreate('status', '已更新语音润色模式');
    }).catch((error: Error) => {
      if (generation === this.generation) imeStorage.setOrCreate('status', error.message);
    });
  }

  private rimeOperation(operation: string, argument: string, feedback: boolean = false): void {
    const generation = this.generation;
    const epoch = this.queueEpoch;
    const client = this.client;
    if (!client) return;
    this.queue = this.queue.then(async (): Promise<void> => {
      if (generation !== this.generation || client !== this.client || epoch !== this.queueEpoch) return;
      if (operation === 'backspace' && (!this.rimeSession || !this.encoding)) {
        const ok = await this.deleteText(client);
        console.info('[OpenLess IME] backspace ' + (this.compatibilityEditor ? 'dispatched=' : 'result=') + ok);
        return;
      }
      if (operation === 'space' && !this.encoding) { await this.writeText(client, ' '); return; }
      if (!this.rimeSession) throw new Error('中文引擎未就绪');
      const delimiter = operation === 'literal' && argument === "'" && this.encoding.length > 0;
      const key = delimiter ? '39' : operation === 'backspace' ? '65288' : operation === 'space' ? '32' : argument;
      let state = await rimeRequest(operation === 'backspace' || operation === 'space' ? 'key' : delimiter ? 'key' : operation === 'literal' ? 'confirm' : operation, this.rimeSession, key);
      if (generation !== this.generation || client !== this.client || epoch !== this.queueEpoch) return;
      if (state.commit) {
        this.publish(state);
        if (!await this.writeText(client, state.commit)) throw new Error('文字写入未确认，请重试或取消待提交');
        if (generation !== this.generation || epoch !== this.queueEpoch) return;
        state = await rimeRequest('ack', this.rimeSession);
        if (generation !== this.generation || epoch !== this.queueEpoch) return;
        this.publish(state);
      }
      if (operation === 'literal' && !delimiter && argument && !await this.writeText(client, argument)) {
        throw new Error('标点写入失败，请重新选择输入框');
      }
      if (generation === this.generation && client === this.client && epoch === this.queueEpoch) {
        this.publish(state);
        if (feedback) this.symbolMessage((delimiter ? '已加入拼音「' : this.compatibilityEditor ? '已发送「' : '已输入「') + argument + '」');
      }
    }).catch((error: Error) => {
      console.error('[OpenLess IME] operation=' + operation + ' failed code=' + (error as BusinessError).code);
      if (generation === this.generation && epoch === this.queueEpoch) {
        if (imeStorage.get<boolean>('pendingCommit')) imeStorage.setOrCreate('commitNeedsConfirmation', true);
        imeStorage.setOrCreate('status', error.message);
        if (feedback) this.symbolMessage('未能确认，请检查输入框');
      }
    });
  }

  selectCandidate(index: number, expectedText: string = ''): void {
    if (imeStorage.get<boolean>('candidatesLoading')) return;
    const displayed = imeStorage.get<string[]>('candidates') || [];
    const text = expectedText || displayed[index];
    if (text === undefined || !displayed.includes(text)) return;
    this.playKeySound();
    const local = this.localCandidates.indexOf(text);
    if (local >= 0) this.rimeOperation(this.candidatesAppended ? 'select_absolute' : 'select',
      (this.candidatesAppended ? this.candidateBase + local : local).toString() + '@' + this.candidateRevision.toString());
    else if (this.cloudChoices.includes(text)) this.rimeOperation('external', this.candidateRevision.toString() + '@' + text);
  }

  loadMoreCandidates(): void {
    if (!this.client || !this.rimeSession || !this.encoding || !imeStorage.get<boolean>('hasNext') ||
      imeStorage.get<boolean>('pendingCommit') || imeStorage.get<boolean>('candidatesLoading')) return;
    const generation = this.generation;
    const epoch = this.queueEpoch;
    const revision = this.candidateRevision;
    const input = this.encoding;
    const session = this.rimeSession;
    imeStorage.setOrCreate('candidatesLoading', true);
    this.queue = this.queue.then(async (): Promise<void> => {
      if (generation !== this.generation || epoch !== this.queueEpoch || revision !== this.candidateRevision) return;
      const offset = this.candidateBase + this.localCandidates.length;
      const state = await rimeRequest('candidates', session, offset.toString() + '@' + revision.toString());
      if (generation !== this.generation || epoch !== this.queueEpoch || session !== this.rimeSession) return;
      if (state.input !== input || state.commit) {
        this.publish(state);
        return;
      }
      if (state.revision !== revision || state.candidateOffset !== offset) throw new Error('候选版本已变化');
      this.localCandidates = this.localCandidates.concat(state.candidates);
      this.candidatesAppended = true;
      this.cloudChoices = this.cloudChoices.filter((text: string) => !this.localCandidates.includes(text));
      imeStorage.setOrCreate('candidates', this.localCandidates.slice(0, 1).concat(this.cloudChoices).concat(this.localCandidates.slice(1)));
      imeStorage.setOrCreate('hasNext', state.candidateHasNext === true && state.candidates.length > 0);
    }).catch(() => {
      if (generation === this.generation && epoch === this.queueEpoch) imeStorage.setOrCreate('status', '候选加载失败，可再次滑动重试');
    }).finally(() => {
      if (generation === this.generation && epoch === this.queueEpoch) imeStorage.setOrCreate('candidatesLoading', false);
    });
  }
  space(): void { this.playKeySound(); this.rimeOperation('space', ''); }
  page(next: boolean): void { this.rimeOperation('key', next ? '65366' : '65365'); }
  toggleLanguage(): void {
    this.cloudEpoch++;
    clearTimeout(this.cloudTimer);
    const chinese = !imeStorage.get<boolean>('chinese');
    imeStorage.setOrCreate('chinese', chinese);
    if (this.rimeSession) this.rimeOperation('clear', '');
  }

  enter(): void {
    this.playKeySound();
    const action = this.action;
    const generation = this.generation;
    const epoch = this.queueEpoch;
    this.perform('enter', async (client: inputMethodEngine.InputClient): Promise<boolean> => {
      if (this.encoding && this.rimeSession) {
        const state = await rimeRequest('key', this.rimeSession, '65293');
        if (generation !== this.generation || client !== this.client || epoch !== this.queueEpoch) return false;
        this.publish(state);
        if (state.commit) {
          if (!await this.writeText(client, state.commit)) return false;
          if (generation !== this.generation || client !== this.client || epoch !== this.queueEpoch) return false;
          const acknowledged = await rimeRequest('ack', this.rimeSession);
          if (generation === this.generation && client === this.client && epoch === this.queueEpoch) this.publish(acknowledged);
        }
        return true;
      }
      return action === inputMethodEngine.ENTER_KEY_TYPE_UNSPECIFIED || action === inputMethodEngine.ENTER_KEY_TYPE_NEWLINE ?
        await this.writeText(client, '\n') : await this.sendAction(client, action);
    });
  }

  retryCommit(): void { this.rimeOperation('state', ''); }
  cancelPendingCommit(): void { this.recoverPending('clear'); }
  acknowledgePendingCommit(): void { this.recoverPending('ack'); }

  private recoverPending(operation: string): void {
    if (!imeStorage.get<boolean>('pendingCommit') || !this.client || !this.rimeSession) return;
    const generation = this.generation;
    const session = this.rimeSession;
    const epoch = ++this.queueEpoch;
    this.writeId++;
    this.expectedText = undefined;
    this.confirmWrite = undefined;
    imeStorage.setOrCreate('writeInFlight', false);
    // Only these explicit user decisions can release an uncertain submission.
    // They never resend text, and late IPC results cannot ack a newer word.
    this.queue = rimeRequest(operation, session).then((state: RimeState): void => {
      if (generation === this.generation && session === this.rimeSession && epoch === this.queueEpoch) this.publish(state);
    }).catch(() => {
      if (generation === this.generation && epoch === this.queueEpoch) imeStorage.setOrCreate('status', '恢复失败，请重新选择输入框');
    });
  }

  async startVoice(holdToTalk: boolean = false): Promise<void> {
    const context = this.context;
    const client = this.client;
    if (!context || !client || imeStorage.get<boolean>('password') || imeStorage.get<string>('voicePhase') !== 'idle') return;
    if (!imeStorage.get<boolean>('editorVerified')) {
      imeStorage.setOrCreate('status', '当前输入框不支持语音，请在系统原生应用中使用');
      return;
    }
    if (imeStorage.get<boolean>('pendingCommit')) { imeStorage.setOrCreate('status', '请先重新确认待写入的候选词'); return; }
    const generation = this.generation;
    const voiceGeneration = ++this.voiceGeneration;
    this.voiceReleaseRequested = false;
    imeStorage.setOrCreate('voiceHolding', holdToTalk);
    imeStorage.setOrCreate('voicePhase', 'starting');
    imeStorage.setOrCreate('voiceText', '');
    imeStorage.setOrCreate('status', '正在准备麦克风');
    try {
      // Clear the composition in the same queue as normal keyboard operations.
      await this.queue;
      if (generation !== this.generation || voiceGeneration !== this.voiceGeneration) return;
      if (this.rimeSession) this.publish(await rimeRequest('clear', this.rimeSession));
      if (generation !== this.generation || voiceGeneration !== this.voiceGeneration) return;
      await this.voice.start(context, (level: number) => {
        if (generation === this.generation && voiceGeneration === this.voiceGeneration) imeStorage.setOrCreate('voiceLevel', level);
      }, (message: string) => {
        if (generation === this.generation && voiceGeneration === this.voiceGeneration) {
          imeStorage.setOrCreate('voicePhase', 'idle');
          imeStorage.setOrCreate('voiceHolding', false);
          imeStorage.setOrCreate('status', message);
        }
      });
      if (generation !== this.generation || voiceGeneration !== this.voiceGeneration) return;
      imeStorage.setOrCreate('voicePhase', 'recording');
      imeStorage.setOrCreate('status', holdToTalk ? '正在录音，松开空格识别并输入' : '正在录音，点结束识别并输入');
      if (this.voiceReleaseRequested) await this.stopVoice();
    } catch (error) {
      if (generation === this.generation && voiceGeneration === this.voiceGeneration) {
        imeStorage.setOrCreate('voicePhase', 'idle');
        imeStorage.setOrCreate('voiceHolding', false);
        imeStorage.setOrCreate('status', (error as Error).message);
      }
    }
  }

  releaseVoiceHold(): void {
    if (!imeStorage.get<boolean>('voiceHolding')) return;
    imeStorage.setOrCreate('voiceHolding', false);
    this.voiceReleaseRequested = true;
    if (imeStorage.get<string>('voicePhase') === 'recording') this.stopVoice();
  }

  cancelVoiceHold(): void {
    if (imeStorage.get<boolean>('voiceHolding')) this.cancelVoice();
  }

  async stopVoice(): Promise<void> {
    if (imeStorage.get<string>('voicePhase') !== 'recording') return;
    const generation = this.generation;
    const voiceGeneration = this.voiceGeneration;
    imeStorage.setOrCreate('voicePhase', 'processing');
    imeStorage.setOrCreate('voiceLevel', 0);
    imeStorage.setOrCreate('status', '正在识别');
    try {
      const result = await this.voice.stop();
      if (generation !== this.generation || voiceGeneration !== this.voiceGeneration) return;
      const text = result.polishedText || result.rawText;
      imeStorage.setOrCreate('voiceText', text);
      imeStorage.setOrCreate('voicePhase', text ? 'review' : 'idle');
      imeStorage.setOrCreate('status', text ? '正在写入识别结果' : '未识别到文字，请重新录音');
      if (text) this.confirmVoice();
    } catch (error) {
      if (generation === this.generation && voiceGeneration === this.voiceGeneration) {
        imeStorage.setOrCreate('voicePhase', 'idle');
        imeStorage.setOrCreate('status', (error as Error).message);
      }
    }
  }

  confirmVoice(): void {
    if (imeStorage.get<string>('voicePhase') !== 'review') return;
    const text = imeStorage.get<string>('voiceText') || '';
    const generation = this.generation;
    const voiceGeneration = this.voiceGeneration;
    imeStorage.setOrCreate('voicePhase', 'inserting');
    this.perform('voice insert', async (client: inputMethodEngine.InputClient): Promise<boolean> => {
      if (voiceGeneration !== this.voiceGeneration || imeStorage.get<string>('voicePhase') !== 'inserting') return false;
      let ok = false;
      try { ok = await this.writeText(client, text); return ok; } finally {
        if (generation === this.generation && voiceGeneration === this.voiceGeneration) {
          imeStorage.setOrCreate('voicePhase', ok ? 'idle' : 'review');
          if (ok) imeStorage.setOrCreate('voiceText', '');
        }
      }
    });
  }

  cancelVoice(): void {
    const phase = imeStorage.get<string>('voicePhase');
    this.voiceGeneration++;
    this.voiceReleaseRequested = false;
    this.voice.cancel().catch(() => {});
    imeStorage.setOrCreate('voicePhase', 'idle');
    imeStorage.setOrCreate('voiceHolding', false);
    imeStorage.setOrCreate('voiceText', '');
    imeStorage.setOrCreate('voiceLevel', 0);
    if (phase !== 'idle') imeStorage.setOrCreate('status', '录音已取消');
  }

  hide(): void {
    this.cancelVoice();
    this.keyboard?.hide().catch(() => {});
  }

  settings(): void {
    this.cancelVoice();
    this.context?.startAbility({ bundleName: 'openless.hm.com', abilityName: 'EntryAbility' })
      .catch(() => { imeStorage.setOrCreate('status', '无法打开设置'); });
  }
}

export const inputSession: InputSession = new InputSession();
