import { inputMethod } from '@kit.IMEKit';
import { imeStorage, inputSession } from '../inputmethod/InputSession';
import { panelLayout } from '../inputmethod/PanelLayout';
import { SymbolGesture, letterSymbol, digitSymbol, symbolRows, localizedSymbol, liveSymbolHint } from '../inputmethod/SymbolGesture';
import { ClipboardEntry } from '../inputmethod/ClipboardHistory';

const BACKGROUND: Resource = $r('app.color.ime_panel_background');
const KEY: Resource = $r('app.color.ime_key_background');
const FUNCTION_KEY: Resource = $r('app.color.ime_function_background');
const INK: Resource = $r('app.color.ime_foreground');
const MUTED: Resource = $r('app.color.ime_secondary');
const ACCENT: Resource = $r('app.color.ime_accent');
const KEY_SHADOW: Resource = $r('app.color.ime_key_shadow');

@Entry(imeStorage)
@Component
struct InputMethodPanel {
  @LocalStorageLink('connected') connected: boolean = false;
  @LocalStorageLink('status') status: string = '等待输入框';
  @LocalStorageLink('enterLabel') enterLabel: string = '换行';
  @LocalStorageLink('chinese') chinese: boolean = true;
  @LocalStorageLink('preedit') @Watch('resetCandidateScroll') preedit: string = '';
  @LocalStorageLink('candidates') candidates: string[] = [];
  @LocalStorageLink('candidatesLoading') candidatesLoading: boolean = false;
  @LocalStorageLink('toolbarActions') toolbarActions: string[] = ['layout', 'emoji', 'voice', 'settings'];
  @LocalStorageLink('keyFontSize') keyFontSize: number = 22;
  @LocalStorageLink('swipeSymbols') swipeSymbols: boolean = true;
  @LocalStorageLink('keySound') keySound: boolean = false;
  @LocalStorageLink('clipboardItems') clipboardItems: ClipboardEntry[] = [];
  @LocalStorageLink('clipboardStatus') clipboardStatus: string = '';
  @LocalStorageLink('clipboardAutoRead') clipboardAutoRead: boolean = false;
  @LocalStorageLink('cloudEnabled') cloudEnabled: boolean = false;
  @LocalStorageLink('symbolFeedback') symbolFeedback: string = '';
  @LocalStorageLink('dictionaryStatus') dictionaryStatus: string = '公共词库：自动更新';
  @LocalStorageLink('dictionaryAuto') dictionaryAuto: boolean = true;
  @LocalStorageLink('rimeReady') rimeReady: boolean = false;
  @LocalStorageLink('page') page: number = 0;
  @LocalStorageLink('hasNext') hasNext: boolean = false;
  @LocalStorageLink('password') password: boolean = false;
  @LocalStorageLink('editorVerified') editorVerified: boolean = false;
  @LocalStorageLink('voicePhase') voicePhase: string = 'idle';
  @LocalStorageLink('voiceHolding') voiceHolding: boolean = false;
  @LocalStorageLink('voiceText') voiceText: string = '';
  @LocalStorageLink('voiceLevel') voiceLevel: number = 0;
  @LocalStorageLink('pendingCommit') pendingCommit: boolean = false;
  @LocalStorageLink('writeInFlight') writeInFlight: boolean = false;
  @LocalStorageLink('commitNeedsConfirmation') commitNeedsConfirmation: boolean = false;
  @LocalStorageLink('keyboardLayout') keyboardLayout: string = 'standard';
  @LocalStorageLink('keyHeight') keyHeight: number = 52;
  @LocalStorageLink('keyWidth') keyWidth: number = 52;
  @LocalStorageLink('keyboardWidth') keyboardWidth: number = 574;
  @LocalStorageLink('showNumberRow') showNumberRow: boolean = true;
  @State uppercase: boolean = false;
  @State capsLock: boolean = false;
  @State layer: number = 0;
  @State utility: number = 0;
  @State toolTab: number = 0;
  @State toolPosition: number = 0;
  @State shortcutSlot: number = 0;
  @State expanded: boolean = false;
  @State symbolPage: number = 0;
  @State previewId: string = '';
  @State previewSymbol: string = '';
  @State previewArmed: boolean = false;
  private symbolGesture: SymbolGesture = new SymbolGesture();
  private toolSwiper: SwiperController = new SwiperController();
  private candidateScroller: Scroller = new Scroller();
  private expandedScroller: Scroller = new Scroller();

  private resetCandidateScroll(): void {
    this.candidateScroller.scrollTo({ xOffset: 0, yOffset: 0 });
    this.expandedScroller.scrollTo({ xOffset: 0, yOffset: 0 });
  }

  @Styles
  keySurface() {
    .borderRadius(10)
    .shadow({ radius: 1.25, color: KEY_SHADOW, offsetX: 0, offsetY: 2 })
  }

  @Styles
  pressMotion() {
    .stateStyles({
      normal: { .scale({ x: 1, y: 1 }).opacity(1) },
      pressed: { .scale({ x: 0.94, y: 0.94 }).opacity(0.78) }
    })
    .animation({ duration: 90, curve: Curve.EaseOut })
  }

  private row(index: number): string[] {
    if (this.layer === 2) {
      return symbolRows(this.symbolPage, this.chinese)[index];
    }
    const letters: string[] = this.keyboardLayout === 'split' ?
      ['qwertyuiop', 'asdfgghjkl', 'zxcvvbnm'] : ['qwertyuiop', 'asdfghjkl', 'zxcvbnm'];
    return (!this.chinese && this.uppercase ? letters[index].toUpperCase() : letters[index]).split('');
  }

  private hint(index: number, position: number): string {
    if (this.layer !== 0) return '';
    return letterSymbol(index, position, this.keyboardLayout === 'split', this.chinese);
  }

  private keyContext(): string {
    return inputSession.bindingVersion().toString() + ':' + this.chinese + ':' + this.layer + ':' + this.symbolPage + ':' + this.keyboardLayout + ':' + this.utility + ':' + this.liveSwipeEnabled();
  }

  private liveSwipeEnabled(): boolean { return imeStorage.get<boolean>('swipeSymbols') !== false; }

  private clearPreview(id: string): void {
    if (this.previewId === id) { this.previewId = ''; this.previewSymbol = ''; this.previewArmed = false; }
  }

  private armSymbol(id: string, symbol: string, mode: string): void {
    const current = liveSymbolHint(id, symbol, this.chinese);
    if (this.symbolGesture.arm(id, current, this.keyContext(), mode, this.liveSwipeEnabled())) {
      this.previewId = id; this.previewSymbol = current; this.previewArmed = true;
    }
  }

  private moveSymbol(id: string, event: GestureEvent): void {
    const armed = this.symbolGesture.move(id, event.offsetX, event.offsetY, this.swipeDistance());
    if (this.previewId === id) this.previewArmed = armed;
  }

  private swipeDistance(): number { return Math.max(22, Math.min(30, this.keyHeight * 0.48)); }

  private keyGap(): number { return this.keyboardLayout === 'standard' ? 7 : 5; }

  private finishSymbol(id: string, mode: string): void {
    const symbol = this.symbolGesture.finish(id, this.keyContext(), mode, this.liveSwipeEnabled());
    this.clearPreview(id);
    if (symbol) this.insertSymbol(symbol);
  }

  private cancelSymbol(id: string, mode: string = ''): void {
    if (this.symbolGesture.cancel(id, mode)) this.clearPreview(id);
  }

  private insertSymbol(symbol: string): void {
    inputSession.insertSymbol(localizedSymbol(symbol, this.chinese));
    if (this.uppercase && !this.capsLock) this.uppercase = false;
  }

  private tapKey(id: string, key: string, letter: boolean): void {
    if (!this.symbolGesture.tap(id, this.keyContext())) return;
    this.clearPreview(id);
    if (letter) this.insert(key); else this.insertSymbol(key);
  }

  @Builder
  characterKey(key: string, hint: string, id: string, weight: number, letter: boolean) {
    Stack({ alignContent: Alignment.Top }) {
      Button({ type: ButtonType.Normal }) {
        Stack({ alignContent: Alignment.Center }) {
          Text(letter ? (this.chinese ? key.toUpperCase() : key) : localizedSymbol(key, this.chinese)).fontSize(letter ? this.keyFontSize : this.keyFontSize + 2)
            .fontColor(this.previewId === id && this.previewArmed ? '#FFFFFF' : INK)
          if (hint) {
            Column() {
              Text(liveSymbolHint(id, hint, this.chinese)).fontSize(10).fontColor(this.previewId === id && this.previewArmed ? '#FFFFFF' : MUTED)
                .height(13).width('100%').textAlign(TextAlign.End)
            }.width('100%').height('100%').padding({ top: 3, right: 6 })
              .hitTestBehavior(HitTestMode.None)
          }
        }.width('100%').height('100%')
      }.width('100%').height('100%').keySurface().padding(0)
        .backgroundColor(this.previewId === id && this.previewArmed ? ACCENT : KEY)
        .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).pressMotion().focusable(false)
        .accessibilityText(hint ? localizedSymbol(key, this.chinese) + (this.swipeSymbols ? '，长按或上滑输入' : '，长按输入') + liveSymbolHint(id, hint, this.chinese) : localizedSymbol(key, this.chinese))
        .onTouch((event: TouchEvent) => {
          if (event.type === TouchType.Down && event.touches.length) this.symbolGesture.begin(id, this.keyContext(), event.touches[0].x, event.touches[0].y);
          else if (event.type === TouchType.Move && event.touches.length) {
            if (this.symbolGesture.moveTouch(id, event.touches[0].x, event.touches[0].y)) this.clearPreview(id);
          }
          else if (event.type === TouchType.Cancel) this.cancelSymbol(id);
        })
        .gesture(GestureGroup(GestureMode.Exclusive,
          PanGesture({ direction: PanDirection.Up, distance: this.swipeDistance() })
            .onActionStart((event: GestureEvent) => { this.armSymbol(id, hint, 'swipe'); this.moveSymbol(id, event); })
            .onActionUpdate((event: GestureEvent) => { this.moveSymbol(id, event); })
            .onActionEnd((event: GestureEvent) => { this.moveSymbol(id, event); this.finishSymbol(id, 'swipe'); })
            .onActionCancel(() => { this.cancelSymbol(id, 'swipe'); }),
          LongPressGesture({ duration: 350, repeat: false })
            .onAction(() => { this.armSymbol(id, hint || key, 'hold'); })
            .onActionEnd(() => { this.finishSymbol(id, 'hold'); })
            .onActionCancel(() => { this.cancelSymbol(id, 'hold'); }),
          TapGesture().onAction(() => { this.tapKey(id, key, letter); })
        ))
      if (this.previewId === id && this.previewSymbol) {
        Column({ space: 1 }) {
          Text(this.previewSymbol).fontSize(27).fontColor(this.previewArmed ? ACCENT : MUTED)
          Text(this.previewArmed ? '松开输入' : '松开取消').fontSize(10).fontColor(MUTED)
        }.height(50).constraintSize({ minWidth: 60 }).padding({ left: 8, right: 8 })
          .justifyContent(FlexAlign.Center).backgroundColor(KEY).borderRadius(9)
          .shadow({ radius: 8, color: '#33000000', offsetY: 2 }).translate({ y: -50 })
          .hitTestBehavior(HitTestMode.None)
      }
    }.height(this.keyHeight).width(this.keyWidth * weight).zIndex(this.previewId === id ? 10 : 0)
  }

  private insert(text: string): void {
    inputSession.insert(text);
    if (this.uppercase && !this.capsLock) this.uppercase = false;
  }

  private language(): void {
    inputSession.toggleLanguage();
    this.uppercase = false;
    this.capsLock = false;
  }

  private shift(): void {
    if (this.layer === 2) { this.symbolPage = this.symbolPage === 0 ? 1 : 0; return; }
    if (this.chinese) inputSession.toggleLanguage();
    this.uppercase = !this.uppercase;
    this.capsLock = false;
  }

  private inputMethods(): void {
    inputMethod.getSetting().showOptionalInputMethods().catch(() => {
      this.status = '请在系统输入法菜单切换';
    });
  }

  private voice(): void {
    this.utility = 0;
    this.expanded = false;
    inputSession.startVoice();
  }

  private openTools(tab: number): void {
    if (this.utility === 1 && this.toolTab === tab) this.utility = 0;
    else { this.toolTab = tab; this.toolPosition = tab; this.utility = 1; this.expanded = false; }
  }

  private inputMode(chinese: boolean): void {
    if (this.chinese !== chinese) this.language();
    this.layer = 0; this.utility = 0; this.expanded = false;
  }

  @Builder
  toolbarButton(icon: Resource, description: string, action: () => void) {
    Button({ type: ButtonType.Normal }) {
      Image(icon).width(25).height(25).objectFit(ImageFit.Contain)
    }.height(34).layoutWeight(1)
      .backgroundColor(Color.Transparent).borderRadius(12).padding(0).pressMotion().focusable(false)
      .accessibilityText(description).onClick(() => { action(); })
  }

  @Builder
  keyboardToolbar() {
    Row({ space: 6 }) {
      Button({ type: ButtonType.Normal }) {
        Image($r('app.media.ime_tools')).width(25).height(25)
      }.width(38).height(34).padding(0).borderRadius(999)
        .backgroundColor(Color.Transparent).pressMotion().focusable(false)
        .accessibilityText('输入法工具').onClick(() => { this.openTools(0); })
      ForEach([0, 1, 2, 3], (slot: number) => {
        Button({ type: ButtonType.Normal }) {
          Image(this.shortcutIcon(this.toolbarActions[slot])).width(25).height(25).objectFit(ImageFit.Contain)
        }.height(34).layoutWeight(1).borderRadius(999).padding(0).backgroundColor(Color.Transparent)
          .pressMotion().focusable(false).accessibilityText(this.shortcutLabel(this.toolbarActions[slot]) + '，长按编辑')
          .enabled(this.toolbarActions[slot] !== 'voice' ||
            (this.connected && this.editorVerified && !this.password && !this.pendingCommit && this.voicePhase === 'idle'))
          .gesture(GestureGroup(GestureMode.Exclusive,
            LongPressGesture({ duration: 450 }).onAction(() => { this.shortcutSlot = slot; this.utility = 3; }),
            TapGesture().onAction(() => { this.runShortcut(this.toolbarActions[slot]); })))
      }, (slot: number) => slot.toString())
      Button({ type: ButtonType.Normal }) {
        Image($r('app.media.ime_chevron_down')).width(25).height(25)
      }.width(38).height(34).padding(0).borderRadius(999).backgroundColor(Color.Transparent)
        .pressMotion().focusable(false).accessibilityText('收起键盘').onClick(() => { inputSession.hide(); })
    }.width('100%').height(34)
  }

  private shortcutIcon(action: string): Resource {
    switch (action) {
      case 'emoji': return $r('app.media.ime_emoji');
      case 'voice': return $r('app.media.ime_microphone');
      case 'settings': return $r('app.media.ime_settings');
      case 'symbols': return $r('app.media.ime_symbols');
      case 'numbers': return $r('app.media.ime_number');
      case 'language': return $r('app.media.ime_language');
      case 'inputMethods': return $r('app.media.ime_switch');
      default: return $r('app.media.ime_keyboard');
    }
  }

  private shortcutLabel(action: string): string {
    switch (action) {
      case 'emoji': return '表情';
      case 'voice': return '语音输入';
      case 'settings': return '应用设置';
      case 'symbols': return '符号键盘';
      case 'numbers': return '数字键盘';
      case 'language': return '中英切换';
      case 'inputMethods': return '切换输入法';
      default: return '键盘模式';
    }
  }

  private runShortcut(action: string): void {
    switch (action) {
      case 'emoji': this.utility = 2; break;
      case 'voice': this.voice(); break;
      case 'settings': inputSession.settings(); break;
      case 'symbols': this.layer = 2; this.symbolPage = 0; this.utility = 0; break;
      case 'numbers': this.layer = 1; this.utility = 0; break;
      case 'language': this.language(); break;
      case 'inputMethods': this.inputMethods(); break;
      default: this.openTools(2); break;
    }
  }

  @Builder
  shortcutEditor() {
    Column({ space: 12 }) {
      Row({ space: 8 }) {
        ForEach([0, 1, 2, 3], (slot: number) => {
          Button({ type: ButtonType.Normal }) {
            Column({ space: 4 }) {
              Image(this.shortcutIcon(this.toolbarActions[slot])).width(25).height(25)
              Text('位置 ' + (slot + 1).toString()).fontSize(12).fontColor(INK)
            }
          }.layoutWeight(1).height(64).borderRadius(12).backgroundColor(this.shortcutSlot === slot ? FUNCTION_KEY : KEY)
            .pressMotion().focusable(false).onClick(() => { this.shortcutSlot = slot; })
        }, (slot: number) => slot.toString())
      }.width('100%')
      Text('选择要放到第 ' + (this.shortcutSlot + 1).toString() + ' 个位置的功能').fontSize(13).fontColor(MUTED)
      Scroll() {
        Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Center }) {
          ForEach(['layout', 'emoji', 'voice', 'settings', 'symbols', 'numbers', 'language', 'inputMethods'], (action: string) => {
            this.menuTile(this.shortcutIcon(action), this.shortcutLabel(action), () => {
              inputSession.setToolbarAction(this.shortcutSlot, action); this.utility = 0;
            })
          }, (action: string) => action)
        }.width('100%')
      }.width('100%').layoutWeight(1).scrollBar(BarState.Auto)
    }.width('100%').layoutWeight(1)
  }

  @Builder
  shortcutEditorHeader() {
    Row() {
      Button({ type: ButtonType.Normal }) {
        Image($r('app.media.ime_chevron_down')).width(22).height(22).rotate({ angle: 90 })
      }.width(38).height(34).padding(0).borderRadius(999).backgroundColor(KEY).pressMotion().focusable(false)
        .accessibilityText('返回键盘').onClick(() => { this.utility = 0; })
      Text(this.utilityTitle()).layoutWeight(1).textAlign(TextAlign.Center).fontSize(14).fontColor(INK)
      Blank().width(38)
    }.width('100%').height(34)
  }

  private utilityTitle(): string {
    switch (this.utility) {
      case 4: return '词库与云候选';
      case 5: return '剪贴板';
      case 6: return '润色模式';
      case 7: return '字体大小';
      case 8: return '常用语';
      default: return '编辑快捷入口';
    }
  }

  @Builder
  keyboardSettings() {
    Scroll() {
      Column({ space: 10 }) {
        Button('打开应用设置').width('100%').height(40).backgroundColor(KEY).fontColor(INK)
          .pressMotion().focusable(false).onClick(() => { inputSession.settings(); })
        Button(this.cloudEnabled ? '云候选：开（Google）' : '云候选：关').width('100%').height(40).backgroundColor(KEY).fontColor(INK)
          .pressMotion().focusable(false).onClick(() => { inputSession.toggleCloudCandidates(); })
        Text('开启后将当前拼音发送给 Google，密码框不查询').fontSize(12).fontColor(MUTED)
        Button(this.dictionaryAuto ? '公共词库自动更新：开' : '公共词库自动更新：关').width('100%').height(40).backgroundColor(KEY).fontColor(INK)
          .pressMotion().focusable(false).onClick(() => { inputSession.togglePublicDictionaryUpdates(); })
        Button(this.dictionaryStatus).width('100%').height(40).backgroundColor(KEY).fontColor(INK)
          .pressMotion().focusable(false).onClick(() => { inputSession.checkPublicDictionary(); })
        Text('公共词库每周检查；学习词仅保存在本机。按键音跟随媒体音量。').fontSize(12).fontColor(MUTED)
        Button('切换系统输入法').width('100%').height(40).backgroundColor(KEY).fontColor(INK)
          .pressMotion().focusable(false).onClick(() => { this.inputMethods(); })
      }.width('100%').padding({ top: 6, bottom: 8 })
    }.width('100%').layoutWeight(1).scrollBar(BarState.Auto)
  }

  @Builder
  clipboardPanel() {
    Column({ space: 6 }) {
      Row({ space: 10 }) {
        if (this.clipboardAutoRead) {
        Button('粘贴当前内容').height(32).fontSize(13).fontColor(INK).backgroundColor(KEY).pressMotion().focusable(false)
          .enabled(this.connected && !this.pendingCommit).onClick(() => { inputSession.pasteClipboard(); })
        Button('刷新').height(32).fontSize(13).fontColor(INK).backgroundColor(KEY).pressMotion().focusable(false)
          .onClick(() => { inputSession.refreshClipboard(); })
        Button('剪贴板授权').height(32).fontSize(13).fontColor(INK).backgroundColor(KEY).pressMotion().focusable(false)
          .onClick(() => { inputSession.authorizeClipboard(); })
        } else {
          PasteButton({ icon: PasteIconStyle.LINES, text: PasteDescription.PASTE }).width(140).height(40)
            .enabled(this.connected && !this.pendingCommit)
            .onClick((_event: ClickEvent, result: PasteButtonOnClickResult) => {
              if (result === PasteButtonOnClickResult.SUCCESS) inputSession.pasteClipboard();
            })
        }
        Text('本地记录 ' + this.clipboardItems.length.toString() + '/30').layoutWeight(1).fontSize(12).fontColor(MUTED)
        Button('清空').height(32).fontSize(13).fontColor(INK).backgroundColor(KEY).pressMotion().focusable(false)
          .enabled(this.clipboardItems.length > 0).accessibilityText('清空本地剪贴板记录')
          .onClick(() => { inputSession.clearClipboardHistory(); })
      }.width('100%')
      Text(this.clipboardStatus || '自动读取复制的文字，仅存本机；点击记录粘贴，私密输入不记录。').fontSize(11).fontColor(MUTED).width('100%')
      if (this.clipboardItems.length === 0) {
        Text(this.clipboardAutoRead ? '复制文字后自动显示在这里；首次使用请授权读取剪贴板' : '暂无本地记录，自动收录待权限开放')
          .fontSize(14).fontColor(MUTED).layoutWeight(1)
      } else {
        Scroll() {
          Column({ space: 8 }) {
            ForEach(this.clipboardItems, (item: ClipboardEntry) => {
              Row({ space: 6 }) {
                Button({ type: ButtonType.Normal }) {
                  Text(item.text).fontSize(15).fontColor(INK).maxLines(3).textAlign(TextAlign.Start)
                    .textOverflow({ overflow: TextOverflow.Ellipsis }).width('100%')
                }.layoutWeight(1).height(76).padding(8).backgroundColor(Color.Transparent).pressMotion().focusable(false)
                  .enabled(this.connected && !this.pendingCommit).onClick(() => { inputSession.pasteClipboardItem(item.id); })
                this.clipboardAction($r('app.media.ime_pin'), item.pinned ? '取消固定' : '固定记录',
                  () => { inputSession.pinClipboardItem(item.id); }, item.pinned)
                this.clipboardAction($r('app.media.ime_clipboard'), '复制记录', () => { inputSession.copyClipboardItem(item.id); })
                this.clipboardAction($r('app.media.ime_trash'), '删除记录', () => { inputSession.deleteClipboardItem(item.id); })
              }.width('100%').backgroundColor(KEY).borderRadius(12).padding({ left: 4, right: 4 })
            }, (item: ClipboardEntry) => item.id + ':' + item.pinned + ':' + item.usedAt)
          }.width('100%').padding({ top: 2, bottom: 6 })
        }.width('100%').layoutWeight(1).scrollBar(BarState.Auto)
      }
    }.width('100%').layoutWeight(1).onAppear(() => { inputSession.refreshClipboard(); })
  }

  @Builder
  clipboardAction(icon: Resource, label: string, action: () => void, active: boolean = false) {
    Button({ type: ButtonType.Normal }) { Image(icon).width(20).height(20) }
      .width(32).height(32).padding(0).borderRadius(999).backgroundColor(active ? FUNCTION_KEY : Color.Transparent)
      .pressMotion().focusable(false).accessibilityText(label).onClick(() => { action(); })
  }

  @Builder
  polishPanel() {
    Column({ space: 6 }) {
      Text('选择语音输入使用的润色模式').fontSize(13).fontColor(MUTED)
      Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Center }) {
        this.menuTile($r('app.media.ime_polish'), '保留原文', () => { inputSession.setPolishMode('raw'); this.utility = 0; })
        this.menuTile($r('app.media.ime_polish'), '轻度润色', () => { inputSession.setPolishMode('light'); this.utility = 0; })
        this.menuTile($r('app.media.ime_polish'), '结构整理', () => { inputSession.setPolishMode('structured'); this.utility = 0; })
        this.menuTile($r('app.media.ime_polish'), '正式表达', () => { inputSession.setPolishMode('formal'); this.utility = 0; })
      }.width('100%')
    }.width('100%').layoutWeight(1)
  }

  @Builder
  fontPanel() {
    Column({ space: 16 }) {
      Text('中英文 Aa').fontSize(this.keyFontSize).fontColor(INK)
      Row() {
        ForEach([20, 22, 24, 26], (size: number, index: number) => {
          Button(['小', '标准', '大', '特大'][index]).layoutWeight(1).height(48)
            .fontColor(INK).backgroundColor(this.keyFontSize === size ? FUNCTION_KEY : KEY).pressMotion().focusable(false)
            .onClick(() => { inputSession.setKeyFontSize(size); })
        }, (size: number) => size.toString())
      }.width('100%')
    }.width('100%').layoutWeight(1).justifyContent(FlexAlign.Center).padding(10)
  }

  @Builder
  phrasePanel() {
    Scroll() {
      Column({ space: 8 }) {
        ForEach(['你好！', '谢谢！', '好的，收到。', '请稍等一下。', '稍后回复你。', '辛苦了！'], (phrase: string) => {
          Button(phrase).width('100%').height(42).fontColor(INK).backgroundColor(KEY).pressMotion().focusable(false)
            .enabled(this.connected && !this.pendingCommit).onClick(() => { inputSession.insert(phrase); this.utility = 0; })
        }, (phrase: string) => phrase)
      }.width('100%').padding({ top: 4, bottom: 8 })
    }.width('100%').layoutWeight(1).scrollBar(BarState.Auto)
  }

  @Builder
  pendingBar() {
    Row({ space: 8 }) {
      Button('已写入', { type: ButtonType.Normal }).height(32).layoutWeight(1).fontSize(14)
        .fontColor(ACCENT).backgroundColor(KEY).padding(0).pressMotion().focusable(false)
        .accessibilityText('文字已出现在输入框，确认并继续输入，不重复写入')
        .onClick(() => { inputSession.acknowledgePendingCommit(); })
      Button('重试写入', { type: ButtonType.Normal }).height(32).layoutWeight(1).fontSize(14)
        .fontColor(INK).backgroundColor(KEY).padding(0).pressMotion().focusable(false)
        .enabled(!this.writeInFlight)
        .onClick(() => { inputSession.retryCommit(); })
      Button('取消待提交', { type: ButtonType.Normal }).height(32).layoutWeight(1).fontSize(14)
        .fontColor(INK).backgroundColor(KEY).padding(0).pressMotion().focusable(false)
        .onClick(() => { inputSession.cancelPendingCommit(); })
    }.height(34).width('100%')
  }

  @Builder
  candidateBar() {
    Row({ space: 4 }) {
      Button({ type: ButtonType.Normal }) {
        Image($r('app.media.ime_tools')).width(25).height(25)
      }.width(38).height(34).padding(0).borderRadius(999)
        .backgroundColor(Color.Transparent).pressMotion().focusable(false)
        .accessibilityText('输入法工具').onClick(() => { this.openTools(0); })
      Scroll(this.candidateScroller) {
        Row({ space: 2 }) {
          ForEach(this.candidates, (candidate: string, index: number) => {
            Button(candidate, { type: ButtonType.Normal }).height(32).fontSize(21)
              .fontWeight(index === 0 ? FontWeight.Medium : FontWeight.Regular)
              .fontColor(index === 0 ? ACCENT : INK).backgroundColor(Color.Transparent)
              .padding({ left: 12, right: 12 }).pressMotion().focusable(false)
              .enabled(this.connected && !this.pendingCommit && !this.candidatesLoading && (!this.chinese || this.rimeReady))
              .onClick(() => { inputSession.selectCandidate(index, candidate); this.expanded = false; this.utility = 0; })
          }, (candidate: string, index: number) => index.toString() + ':' + candidate)
        }.justifyContent(FlexAlign.Start)
      }.scrollable(ScrollDirection.Horizontal).scrollBar(BarState.Off).layoutWeight(1).height(34)
        .align(Alignment.Start).onReachEnd(() => { inputSession.loadMoreCandidates(); })
        .onScrollStop(() => { if (this.candidateScroller.isAtEnd()) inputSession.loadMoreCandidates(); })
      Button({ type: ButtonType.Normal }) {
        Image(this.expanded ? $r('app.media.ime_chevron_up') : $r('app.media.ime_chevron_down')).width(25).height(25)
      }.width(38).height(32).padding(0).borderRadius(12)
        .backgroundColor(Color.Transparent).pressMotion().focusable(false)
        .accessibilityText(this.expanded ? '收起候选词' : '展开候选词')
        .onClick(() => {
          this.expanded = !this.expanded; this.utility = 0;
          if (this.expanded) inputSession.loadMoreCandidates();
        })
    }.width('100%').height(34)
  }

  @Builder
  keyRow(index: number) {
    Row({ space: this.keyGap() }) {
      if (index === 1 && this.layer === 0 && this.keyboardLayout !== 'split') { Blank().layoutWeight(0.5) }
      if (index === 2) {
        Button(this.layer === 2 ? (this.symbolPage === 0 ? '更多' : '常用') : this.capsLock ? '⇪' : '⇧', { type: ButtonType.Normal })
          .height(this.keyHeight).layoutWeight(this.keyboardLayout === 'split' ? 1 : 1.5).fontSize(this.layer > 0 ? 15 : 25)
          .fontColor(this.uppercase && this.layer === 0 ? ACCENT : INK)
          .backgroundColor(FUNCTION_KEY).keySurface().padding(0).pressMotion().focusable(false).enabled(this.connected)
          .accessibilityText(this.layer > 0 ? '切换符号页' : '切换大写')
          .onClick(() => { this.shift(); })
          .gesture(LongPressGesture().onAction(() => {
            if (this.layer === 0) {
              if (this.chinese) inputSession.toggleLanguage();
              this.uppercase = true; this.capsLock = true;
            }
          }))
      }
      ForEach(this.row(index), (key: string, position: number) => {
        if (this.keyboardLayout === 'split' && position === (index === 2 ? 4 : 5)) { Blank().layoutWeight(2) }
        this.characterKey(key, this.hint(index, position), 'letter:' + index + ':' + position + ':' + key, 1, this.layer === 0)
      }, (key: string, position: number) => position.toString() + ':' + key)
      if (index === 1 && this.layer === 0 && this.keyboardLayout !== 'split') { Blank().layoutWeight(0.5) }
      if (index === 2) {
        Button('⌫', { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(this.keyboardLayout === 'split' ? 1 : 1.5).fontSize(25)
          .fontColor(INK).backgroundColor(FUNCTION_KEY).keySurface().padding(0).pressMotion().focusable(false)
          .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).accessibilityText('删除光标前的文字')
          .onClick(() => { inputSession.backspace(); })
          .gesture(LongPressGesture({ repeat: true }).onAction(() => { inputSession.backspace(); }))
      }
    }.width('100%').height(this.keyHeight).justifyContent(FlexAlign.Center)
  }

  @Builder
  bottomKey(label: string, weight: number, action: () => void) {
    Button(label, { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(weight)
      .fontSize(15).fontColor(INK).backgroundColor(FUNCTION_KEY).keySurface().padding(0).pressMotion().focusable(false)
      .onClick(() => { action(); })
  }

  @Builder
  punctuation(comma: boolean) {
    this.characterKey(comma ? (this.chinese ? '，' : ',') : (this.chinese ? '。' : '.'),
      comma ? (this.chinese ? '！' : '!') : (this.chinese ? '？' : '?'),
      comma ? 'comma' : 'period', 0.85, false)
  }

  @Builder
  spaceKey(weight: number) {
    Button({ type: ButtonType.Normal }) {
      if (this.keyboardLayout === 'split') {
        Column({ space: 0 }) {
          Text(this.voiceHolding ? '松开输入' : '语音').fontSize(10).fontColor(MUTED)
          Text('⎵').fontSize(28).fontColor(INK)
        }.width('100%').height('100%').justifyContent(FlexAlign.Center)
      } else {
        Column({ space: 1 }) {
          Text(this.chinese ? '空格' : 'space').fontSize(15).fontColor(MUTED)
          if (!this.password) { Text(this.voiceHolding ? '松开输入' : '长按语音').fontSize(9).fontColor(MUTED) }
        }.width('100%').height('100%').justifyContent(FlexAlign.Center)
      }
    }.height(this.keyHeight).layoutWeight(weight).backgroundColor(KEY).keySurface().padding(0).pressMotion().focusable(false)
      .accessibilityText('空格，长按语音输入')
      .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady))
      .gesture(GestureGroup(GestureMode.Exclusive,
        LongPressGesture({ duration: 450, repeat: false })
          .onAction(() => { if (!this.password && this.voicePhase === 'idle') inputSession.startVoice(true); })
          .onActionEnd(() => { inputSession.releaseVoiceHold(); })
          .onActionCancel(() => { inputSession.cancelVoiceHold(); }),
        TapGesture().onAction(() => { if (this.voicePhase === 'idle') inputSession.space(); })
      ))
  }

  @Builder
  bottomRow() {
    Row({ space: this.keyGap() }) {
      this.bottomKey(this.layer > 0 ? '返回' : '符号', this.keyboardLayout === 'split' ? 1.2 : 1.1,
        () => { this.layer = this.layer > 0 ? 0 : 2; this.symbolPage = 0; })
      Button(this.chinese ? '中/英' : '英/中', { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(0.95)
        .fontSize(14).fontColor(INK).backgroundColor(FUNCTION_KEY).keySurface().padding(0).pressMotion().focusable(false)
        .enabled(this.connected && !this.password).onClick(() => { this.language(); })
        .gesture(LongPressGesture().onAction(() => { this.inputMethods(); }))
      if (this.keyboardLayout === 'split') { this.bottomKey('◎', 0.8, () => { this.inputMethods(); }) }
      else { this.bottomKey('123', 0.9, () => { this.layer = this.layer === 1 ? 0 : 1; }) }
      this.punctuation(true)
      if (this.keyboardLayout === 'split') {
        this.spaceKey(1.2)
        Blank().layoutWeight(2)
        this.spaceKey(1.2)
      } else { this.spaceKey(3) }
      this.punctuation(false)
      if (this.keyboardLayout === 'split') {
        this.bottomKey('☺', 0.8, () => { this.utility = this.utility === 2 ? 0 : 2; })
        this.bottomKey('123', 0.9, () => { this.layer = this.layer === 1 ? 0 : 1; })
      }
      Button(this.enterLabel, { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(1.25)
        .fontSize(16).fontColor(INK)
        .backgroundColor(FUNCTION_KEY).keySurface().padding(0).pressMotion().focusable(false)
        .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { inputSession.enter(); })
    }.width('100%').height(this.keyHeight)
  }

  @Builder
  digitRow() {
    Row({ space: this.keyGap() }) {
      ForEach(['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'], (digit: string, index: number) => {
        if (this.keyboardLayout === 'split' && index === 5) { Blank().layoutWeight(2) }
        this.characterKey(digit, digitSymbol(digit, this.chinese), 'digit:' + digit, 1, false)
      }, (digit: string) => digit)
    }.height(this.keyHeight).width('100%')
  }

  @Builder
  layoutOptions() {
    Column({ space: 0 }) {
      Row() {
        ForEach([0, 1, 2, 3].slice(0, this.menuSplit(4)), (index: number) => { this.layoutTool(index) }, (index: number) => index.toString())
      }
      if (this.menuSplit(4) < 4) {
        Row() { ForEach([0, 1, 2, 3].slice(this.menuSplit(4)), (index: number) => { this.layoutTool(index) }, (index: number) => index.toString()) }
      }
    }.width('100%').padding({ top: 8, bottom: 8 })
  }

  @Builder
  layoutTool(index: number) {
    if (index === 0) { this.menuTile($r('app.media.ime_split'), '拇指模式', () => { this.selectLayout('split'); }) }
    else if (index === 1) { this.menuTile($r('app.media.ime_standard'), '普通模式', () => { this.selectLayout('standard'); }) }
    else if (index === 2) { this.menuTile($r('app.media.ime_floating'), '悬浮模式', () => { this.selectLayout('floating'); }) }
    else { this.menuTile($r('app.media.ime_onehand'), '单手模式', () => { this.selectLayout('onehand'); }) }
  }

  private menuSplit(count: number): number {
    return count * (this.menuTileSize() + 24) <= this.keyboardWidth ? count : Math.ceil(count / 2);
  }

  private selectLayout(mode: string): void {
    this.layer = 0; this.expanded = false; this.utility = 0; panelLayout.select(mode);
  }

  private menuTileSize(): number {
    return Math.min(92, Math.max(64, (this.keyboardWidth - 72) / 4));
  }

  @Builder
  menuTile(icon: Resource, label: string, action: () => void, available: boolean = true, note: string = '') {
    Button({ type: ButtonType.Normal }) {
      Column({ space: note ? 4 : 9 }) {
        Image(icon).width(note ? 22 : 28).height(note ? 22 : 28).objectFit(ImageFit.Contain)
        Text(label).fontSize(label.length > 4 ? 11 : 13).fontColor(INK).maxLines(1)
        if (note) {
          Text(label === '上滑输入' ? (this.swipeSymbols ? '已开启' : '已关闭') :
            label === '按键音' ? (this.keySound ? '已开启' : '已关闭') : note).fontSize(10).fontColor(MUTED).maxLines(1)
        }
      }.width('100%').height('100%').justifyContent(FlexAlign.Center)
    }.width(this.menuTileSize()).height(this.menuTileSize()).margin({ left: 12, right: 12, top: 6, bottom: 6 }).padding(4)
      .backgroundColor(KEY).borderRadius(12).pressMotion().focusable(false).enabled(available)
      .shadow({ radius: 1.25, color: KEY_SHADOW, offsetX: 0, offsetY: 2 })
      .accessibilityText(label).onClick(() => { action(); })
  }

  @Builder
  commonTool(index: number) {
    if (index === 0) { this.menuTile($r('app.media.ime_settings'), '设置', () => { inputSession.settings(); }) }
    else if (index === 1) { this.menuTile($r('app.media.ime_clipboard'), '剪贴板', () => { this.utility = 5; }) }
    else if (index === 2) { this.menuTile($r('app.media.ime_polish'), '润色模式', () => { this.utility = 6; }) }
    else if (index === 3) { this.menuTile($r('app.media.ime_font'), '字体大小', () => { this.utility = 7; }) }
    else if (index === 4) { this.menuTile($r('app.media.ime_sound'), '按键音', () => { inputSession.toggleKeyOption('keySound'); }, true, '状态') }
    else if (index === 5) { this.menuTile($r('app.media.ime_tools'), '定制工具栏', () => { this.shortcutSlot = 0; this.utility = 3; }) }
    else if (index === 6) { this.menuTile($r('app.media.ime_phrase'), '常用语', () => { this.utility = 8; }) }
    else if (index === 7) { this.menuTile($r('app.media.ime_emoji'), '表情', () => { this.utility = 2; }) }
    else if (index === 8) { this.menuTile($r('app.media.ime_camera'), '拍照输入', () => {}, false, '待接入') }
    else if (index === 9) { this.menuTile($r('app.media.ime_language'), '繁体输入', () => {}, false, '待接入') }
    else if (index === 10) { this.menuTile($r('app.media.ime_swipe'), '上滑输入', () => { inputSession.toggleKeyOption('swipeSymbols'); }, true, '状态') }
    else { this.menuTile($r('app.media.ime_language'), '英文联想', () => {}, false, '待接入') }
  }

  @Builder
  commonTools() {
    Scroll() {
      Column({ space: 0 }) {
        Row() { ForEach([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11].slice(0, this.menuSplit(12)), (index: number) => { this.commonTool(index) }, (index: number) => index.toString()) }
        if (this.menuSplit(12) < 12) {
          Row() { ForEach([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11].slice(this.menuSplit(12)), (index: number) => { this.commonTool(index) }, (index: number) => index.toString()) }
        }
      }.width(Math.max(this.keyboardWidth, (this.menuTileSize() + 24) * this.menuSplit(12)))
    }.width('100%').height((this.menuTileSize() + 12) * (this.menuSplit(12) === 12 ? 1 : 2)).scrollable(ScrollDirection.Horizontal).scrollBar(BarState.Off)
      .align(Alignment.TopStart)
  }

  @Builder
  inputTool(index: number) {
    if (index === 0) { this.menuTile($r('app.media.ime_keyboard'), '26键', () => { this.inputMode(true); }, this.connected && !this.password && !this.pendingCommit) }
    else if (index === 1) { this.menuTile($r('app.media.ime_number'), '9键', () => {}, false, '待接入') }
    else { this.menuTile($r('app.media.ime_handwrite'), '手写输入法', () => {}, false, '待接入') }
  }

  @Builder
  floatingHandle() {
    Row({ space: 4 }) {
      Button('−').height(24).layoutWeight(1).fontSize(18).padding(0).pressMotion().focusable(false)
        .fontColor(INK).backgroundColor(Color.Transparent).accessibilityText('缩小悬浮键盘')
        .onClick(() => { panelLayout.resize(-0.1); })
      Row() { Text('━━━━').fontSize(16).fontColor(MUTED) }
        .height(24).layoutWeight(5).justifyContent(FlexAlign.Center).accessibilityText('拖动悬浮键盘')
        .gesture(PanGesture().onActionStart(() => { panelLayout.beginMove(); })
          .onActionUpdate((event: GestureEvent) => { panelLayout.drag(event.offsetX, event.offsetY); })
          .onActionEnd(() => { panelLayout.endMove(); })
          .onActionCancel(() => { panelLayout.endMove(); }))
      Button('+').height(24).layoutWeight(1).fontSize(18).padding(0).pressMotion().focusable(false)
        .fontColor(INK).backgroundColor(Color.Transparent).accessibilityText('放大悬浮键盘')
        .onClick(() => { panelLayout.resize(0.1); })
      Button('↙').height(24).layoutWeight(1).fontSize(18).padding(0).pressMotion().focusable(false)
        .fontColor(INK).backgroundColor(Color.Transparent).accessibilityText('恢复固定键盘')
        .onClick(() => { panelLayout.select('standard'); })
    }.height(24).width('100%')
  }

  @Builder
  numberPad() {
    Row({ space: 6 }) {
      Column({ space: 6 }) {
        ForEach(['+', '-', '×', '÷'], (value: string) => {
          Button(value, { type: ButtonType.Normal }).height(this.keyHeight).width('100%').fontSize(22)
            .fontColor(INK).backgroundColor(FUNCTION_KEY).keySurface().padding(0).pressMotion().focusable(false)
            .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { this.insert(value); })
        }, (value: string) => value)
      }.layoutWeight(1)
      Column({ space: 6 }) {
        ForEach([['1', '2', '3'], ['4', '5', '6'], ['7', '8', '9']], (row: string[], index: number) => {
          Row({ space: 6 }) {
            ForEach(row, (value: string) => {
              Button(value, { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(1).fontSize(25)
                .fontColor(INK).backgroundColor(KEY).keySurface().padding(0).pressMotion().focusable(false)
                .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { this.insert(value); })
            }, (value: string) => value)
          }.width('100%').height(this.keyHeight)
        }, (row: string[], index: number) => index.toString())
        Row({ space: 6 }) {
          Button('返回', { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(1).fontSize(14)
            .fontColor(INK).backgroundColor(FUNCTION_KEY).keySurface().padding(0).pressMotion().focusable(false)
            .onClick(() => { this.layer = 0; })
          Button('0', { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(1).fontSize(25)
            .fontColor(INK).backgroundColor(KEY).keySurface().padding(0).pressMotion().focusable(false)
            .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { this.insert('0'); })
          Button('00', { type: ButtonType.Normal }).height(this.keyHeight).layoutWeight(1).fontSize(22)
            .fontColor(INK).backgroundColor(KEY).keySurface().padding(0).pressMotion().focusable(false)
            .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { this.insert('00'); })
        }.height(this.keyHeight).width('100%')
      }.layoutWeight(3.6)
      Column({ space: 6 }) {
        Button('⌫', { type: ButtonType.Normal }).height(this.keyHeight).width('100%').fontSize(25)
          .fontColor(INK).backgroundColor(FUNCTION_KEY).keySurface().padding(0).pressMotion().focusable(false)
          .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).accessibilityText('删除光标前的文字')
          .onClick(() => { inputSession.backspace(); })
          .gesture(LongPressGesture({ repeat: true }).onAction(() => { inputSession.backspace(); }))
        Button('.', { type: ButtonType.Normal }).height(this.keyHeight).width('100%').fontSize(25)
          .fontColor(INK).backgroundColor(KEY).keySurface().padding(0).pressMotion().focusable(false)
          .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { this.insert('.'); })
        Button(this.enterLabel, { type: ButtonType.Normal }).height(this.keyHeight * 2 + 6).width('100%').fontSize(17)
          .fontColor(INK).backgroundColor(FUNCTION_KEY).keySurface().padding(0).pressMotion().focusable(false)
          .enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady)).onClick(() => { inputSession.enter(); })
      }.layoutWeight(1.2)
    }.width('100%').layoutWeight(1).alignItems(VerticalAlign.Center)
  }

  @Builder
  expandedCandidates() {
    Column({ space: 8 }) {
      Grid(this.expandedScroller) {
        ForEach(this.candidates, (candidate: string, index: number) => {
          GridItem() {
            Button({ type: ButtonType.Normal }) {
              Text(candidate).fontSize(20).fontColor(index === 0 ? ACCENT : INK).maxLines(2)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
            }.height(54).width('100%').backgroundColor(KEY).keySurface()
              .padding({ left: 8, right: 8 }).pressMotion().focusable(false).accessibilityText(candidate)
              .enabled(this.connected && !this.pendingCommit && !this.candidatesLoading && (!this.chinese || this.rimeReady))
              .onClick(() => { inputSession.selectCandidate(index, candidate); this.expanded = false; this.utility = 0; })
          }
        }, (candidate: string, index: number) => index.toString() + ':' + candidate)
      }.columnsTemplate('1fr '.repeat(Math.max(2, Math.min(6, Math.floor(this.keyboardWidth / 152)))).trim())
        .columnsGap(6).rowsGap(6).width('100%').layoutWeight(1).scrollBar(BarState.Auto)
        .onReachEnd(() => { inputSession.loadMoreCandidates(); })
        .onScrollStop(() => { if (this.expandedScroller.isAtEnd()) inputSession.loadMoreCandidates(); })
      Text(this.candidatesLoading ? '正在加载候选…' : this.hasNext ? '向上滑动查看更多候选' : '全部候选已显示')
        .fontSize(12).fontColor(MUTED).height(18)
    }.width('100%').layoutWeight(1).padding({ top: 8 })
  }

  @Builder
  toolTabs() {
    Row() {
      Button({ type: ButtonType.Normal }) {
        Image($r('app.media.ime_chevron_down')).width(22).height(22).rotate({ angle: 90 })
      }.width(38).height(34).borderRadius(999).padding(0)
        .backgroundColor(KEY).pressMotion().focusable(false).accessibilityText('返回键盘')
        .onClick(() => { this.utility = 0; })
      Blank().layoutWeight(1)
      Stack({ alignContent: Alignment.Start }) {
        Row() {
          Blank().width(this.toolTabWidth()).height(28).backgroundColor(KEY).borderRadius(999)
            .translate({ x: this.toolPosition * (this.toolTabWidth() + 6) })
        }.width('100%').height(34).padding({ left: 4, right: 4 })
        Row({ space: 6 }) {
          ForEach(['常用功能', '输入方式', '键盘模式'], (label: string, index: number) => {
            Button(label, { type: ButtonType.Normal }).height(34).width(this.toolTabWidth()).fontSize(14)
              .fontWeight(this.toolTab === index ? FontWeight.Medium : FontWeight.Regular)
              .fontColor(this.toolTab === index ? INK : MUTED).backgroundColor(Color.Transparent)
              .borderRadius(999).padding(0).pressMotion().focusable(false)
              .accessibilityText(label).onClick(() => { this.toolSwiper.changeIndex(index, true); })
          }, (label: string) => label)
        }.padding({ left: 4, right: 4 })
      }.width(this.toolTabWidth() * 3 + 20).height(34).backgroundColor(FUNCTION_KEY).borderRadius(999)
        .shadow({ radius: 1.25, color: KEY_SHADOW, offsetX: 0, offsetY: 2 })
      Blank().layoutWeight(1)
      Blank().width(38)
    }.width('100%').height(34)
  }

  private toolTabWidth(): number {
    return Math.min(112, (this.keyboardWidth - 108) / 3);
  }

  @Builder
  toolPage(tab: number) {
    Column({ space: 8 }) {
      Scroll() {
        Column({ space: 10 }) {
          if (tab === 0) {
            this.commonTools()
          } else if (tab === 1) {
            Column({ space: 0 }) {
              Row() {
                ForEach([0, 1, 2].slice(0, this.menuSplit(3)), (index: number) => { this.inputTool(index) }, (index: number) => index.toString())
              }
              if (this.menuSplit(3) < 3) {
                Row() { ForEach([0, 1, 2].slice(this.menuSplit(3)), (index: number) => { this.inputTool(index) }, (index: number) => index.toString()) }
              }
              Button('词库与云候选').height(32).backgroundColor(Color.Transparent).fontColor(MUTED)
                .pressMotion().focusable(false).onClick(() => { this.utility = 4; })
            }.width('100%')
          } else {
            this.layoutOptions()
          }
        }.width('100%').padding({ top: 4, bottom: 8 })
      }.width('100%').layoutWeight(1).scrollBar(BarState.Off)
    }.width('100%').layoutWeight(1)
  }

  @Builder
  tools() {
    Swiper(this.toolSwiper) {
      this.toolPage(0)
      this.toolPage(1)
      this.toolPage(2)
    }.width('100%').layoutWeight(1).index(this.toolTab).indicator(false).loop(false)
      .duration(220).curve(Curve.EaseOut).onChange((index: number) => { this.toolTab = index; })
      .onContentDidScroll((_selected: number, index: number, position: number, _length: number) => {
        this.toolPosition = Math.max(0, Math.min(2, index - position));
      })
      .onAnimationEnd((index: number) => { this.toolPosition = index; })
  }

  @Builder
  emoji() {
    Column({ space: 8 }) {
      Scroll() {
        Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.Center }) {
          ForEach(['😀', '😄', '😊', '🥰', '😍', '😎', '🤔', '😂', '🥹', '😭', '😅', '😴', '👍', '👏', '🙏', '💪', '❤️', '🎉', '🌹', '✨', '✅', '💡', '☀️', '🎂'], (value: string) => {
            Button(value, { type: ButtonType.Normal }).width(52).height(50).fontSize(27)
              .backgroundColor(Color.Transparent).pressMotion().focusable(false).enabled(this.connected && !this.pendingCommit && (!this.chinese || this.rimeReady))
              .onClick(() => { inputSession.insert(value); })
          }, (value: string) => value)
        }.width('100%')
      }.width('100%').layoutWeight(1).scrollBar(BarState.Off)
      Button('返回键盘').pressMotion().focusable(false).fontColor(ACCENT).backgroundColor(KEY)
        .onClick(() => { this.utility = 0; })
    }.width('100%').layoutWeight(1)
  }

  @Builder
  voicePanel() {
    Column({ space: 16 }) {
      if (this.voicePhase === 'review' || this.voicePhase === 'inserting') {
        Scroll() { Text(this.voiceText).fontSize(22).fontColor(INK).width('100%') }
          .width('100%').layoutWeight(1).padding(12)
        Row({ space: 12 }) {
          Button('重新输入').pressMotion().focusable(false).enabled(this.voicePhase === 'review')
            .fontColor(INK).backgroundColor(KEY).onClick(() => { inputSession.cancelVoice(); })
          Button(this.voicePhase === 'inserting' ? '正在写入' : '确认写入').pressMotion().focusable(false)
            .enabled(this.voicePhase === 'review').fontColor('#FFFFFF').backgroundColor(ACCENT)
            .onClick(() => { inputSession.confirmVoice(); })
        }
      } else {
        Text(this.voicePhase === 'recording' ? '正在听你说' : this.voicePhase === 'processing' ? '正在识别…' : '正在准备…')
          .fontSize(22).fontColor(INK)
        if (this.voicePhase === 'recording') {
          Progress({ value: this.voiceLevel * 100, total: 100, type: ProgressType.Linear })
            .width('64%').color(ACCENT).backgroundColor(FUNCTION_KEY)
          Button('说完了').pressMotion().focusable(false).fontColor('#FFFFFF').backgroundColor(ACCENT)
            .onClick(() => { inputSession.stopVoice(); })
        }
        Button('返回键盘').pressMotion().focusable(false).fontColor(MUTED).backgroundColor(Color.Transparent)
          .onClick(() => { inputSession.cancelVoice(); })
      }
    }.width('100%').layoutWeight(1).justifyContent(FlexAlign.Center).padding(12)
  }

  private headerHint(): string {
    if (this.previewId) return (this.previewArmed ? '松开输入「' : '松开取消「') + this.previewSymbol + '」';
    if (this.symbolFeedback) return this.symbolFeedback;
    if (this.preedit) return this.preedit;
    if (this.connected && this.chinese && !this.rimeReady && this.status === '可输入') return '正在准备中文输入…';
    if (this.status === '可输入' || this.status === 'Rime 拼音') return this.swipeSymbols ? '长按或上滑按键，输入上方符号' : '长按按键，输入上方符号';
    return this.status;
  }

  build() {
    Column({ space: 6 }) {
      if (this.keyboardLayout === 'floating') { this.floatingHandle() }
      Column({ space: 6 }) {
      Column({ space: 0 }) {
        if (this.previewId || this.symbolFeedback || this.preedit || this.pendingCommit ||
          (this.connected && this.voicePhase === 'idle' && this.utility === 0 && this.layer === 0 && this.rimeReady) ||
          (this.status !== '可输入' && this.status !== 'Rime 拼音' && this.status !== '等待输入框') ||
          (this.connected && this.chinese && !this.rimeReady)) {
          Row() {
            Text(this.headerHint())
              .fontSize(12).fontColor(MUTED).maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis }).layoutWeight(1)
          }.height(14).width('100%').padding({ left: 8, right: 8 })
        }
        if (this.utility >= 3) { this.shortcutEditorHeader() }
        else if (this.utility === 1) { this.toolTabs() }
        else if (this.commitNeedsConfirmation) { this.pendingBar() }
        else if (this.candidates.length > 0 && this.voicePhase === 'idle') { this.candidateBar() }
        else { this.keyboardToolbar() }
      }.height(48).width(this.keyboardWidth)
        .justifyContent(FlexAlign.End)
      if (this.voicePhase !== 'idle' && !this.voiceHolding) { this.voicePanel() }
      else if (this.utility === 3) { this.shortcutEditor() }
      else if (this.utility === 4) { this.keyboardSettings() }
      else if (this.utility === 5) { this.clipboardPanel() }
      else if (this.utility === 6) { this.polishPanel() }
      else if (this.utility === 7) { this.fontPanel() }
      else if (this.utility === 8) { this.phrasePanel() }
      else if (this.utility === 1) { this.tools() }
      else if (this.utility === 2) { this.emoji() }
      else if (this.expanded && this.candidates.length > 0) { this.expandedCandidates() }
      else if (this.layer === 1) { this.numberPad() }
      else {
        Column({ space: 7 }) {
          if (this.showNumberRow) { this.digitRow() }
          this.keyRow(0)
          this.keyRow(1)
          this.keyRow(2)
          this.bottomRow()
        }.width(this.keyboardWidth)
          .layoutWeight(1).justifyContent(FlexAlign.End)
      }
      }.width(this.keyboardWidth).layoutWeight(1)
    }.width('100%').height('100%').padding({ left: 6, right: 6, top: 4, bottom: 8 }).backgroundColor(BACKGROUND)
      .alignItems(this.keyboardLayout === 'onehand' ? HorizontalAlign.End : HorizontalAlign.Center)
      .borderRadius(this.keyboardLayout === 'floating' ? 12 : 0).clip(true)
  }
}
