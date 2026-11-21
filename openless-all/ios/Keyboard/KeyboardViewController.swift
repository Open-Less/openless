@preconcurrency import KeyboardKit
import OpenLessShared
import SwiftUI
import UIKit

final class KeyboardViewController: UIInputViewController, UIGestureRecognizerDelegate, @preconcurrency KeyboardController {
  let state = KeyboardKit.Keyboard.State()
  lazy var services = KeyboardKit.Keyboard.Services(state: state)
  private var typingHost: UIHostingController<TypingKeyboard>?
  private let keyboardID = UUID()
  private var revision: UInt64 = 0
  private var lastDocumentID: UUID?
  private var libraryDocumentID: UUID?
  private var preferences = KeyboardPreferences()
  private var deferredPreferences: KeyboardPreferences?
  private var mailbox: Mailbox?
  private var lastFullAccess: Bool?
  private var connectionRetryAfter = Date.distantPast
  private var observer: MailboxObserver?
  private var snapshot: VoiceSnapshot = {
    var unavailable = VoiceSnapshot(hostEpoch: UUID())
    unavailable.updatedAt = .distantPast
    unavailable.inForeground = false
    return unavailable
  }()
  private let urlBridge = KeyboardOpenURLBridge()
  private var cachedConfiguredCapabilities: KeyboardVoiceCapabilities?
  private var rime: OLRimeSession?
  private var rimeError: String?
  private var preedit = ""
  private var rawInput = ""
  private var candidates: [String] = []
  private enum CompositionChoice { case native(Int), supplementary(String) }
  private var compositionChoices: [CompositionChoice] = []
  private var nativeCandidates: [String] = []
  private var lexiconRequestPending = false
  private var lastLexiconRequest = Date.distantPast
  private lazy var predictionEngine = KeyboardPredictionEngine(
    url: Bundle.main.url(forResource: "lexicon", withExtension: "sqlite3", subdirectory: "Prediction"))
  private struct CandidateAnchor: Equatable {
    let document: UUID
    let before: String
    let after: String
  }
  private var candidateAnchor: CandidateAnchor?
  private var hasNextPage = false
  private var hasMarkedText = false
  private var selfEditing = false
  private var activeClient: UUID?
  private var voiceProgress = KeyboardVoiceProgress()
  private var activeTarget: InputTarget?
  private var localPhase: VoicePhase = .inactive {
    didSet {
      if localPhase == .error && oldValue != .error { voiceFailureFeedback() }
    }
  }
  private var pendingStart: UUID?
  private var startHandshake: KeyboardStartHandshake?
  private var activeStart: KeyboardVoiceStart?
  private var activeHostEpoch: UUID?
  private var activeTranslationTitle: String?
  private var selectedStyleID = KeyboardStyleSelection.load() {
    didSet { KeyboardStyleSelection.save(selectedStyleID) }
  }
  private var styleMenuCapabilities: KeyboardVoiceCapabilities?
  private var styleMenuSelection: String?
  private var styleMenuLanguage: InterfaceLanguage?
  private var pendingGeneratedPreview: KeyboardGeneratedPreview?
  private var returnedHostActivation: HostVoiceActivation?
  private var liveHostActivation: HostVoiceActivation?
  // Frozen when attaching. A transient mailbox miss must not turn a host
  // recording into a direct keyboard session for lifecycle cancellation.
  private var activeHostActivationID: UUID?
  private var ignoredHostActivationID: UUID?
  private var keyboardIsVisible = false
  private var returnedInsertionScheduled = false
  private var deliveringReturnedInsertion = false
  private struct HandoffInputAnchor {
    let target: InputTarget
    let before: String?
    let after: String?
    let selection: String?
  }
  private var handoffInputAnchor: HandoffInputAnchor?
  private var foregroundHandoffID: UUID?
  private var confirmedPreviewID: UUID?
  private let helpWriteButton = UIButton(type: .system)
  private let translationButton = UIButton(type: .system)
  private let atButton = UIButton(type: .system)
  private let styleButton = UIButton(type: .system)
  private let proposalButton = UIButton(type: .system)
  private let undoButton = UIButton(type: .system)
  private var undoRecord: KeyboardUndoRecord?
  private var interfacePreferences = SharedInterfacePreferences()
  private var lastVocabularyRevision: String?
  private var lastVocabularyPhrases: [String] = []
  private var vocabularyRetryAfter = Date.distantPast
  private var rimeVocabularyError: String?
  private var uiText: InterfaceLocalizer { InterfaceLocalizer(language: interfacePreferences.language) }
  private let translationArc = TranslationArcView()
  private let helpPrompt = UILabel()
  private let voiceTools = UIStackView()
  private let previewPanel = UIStackView()
  private let previewText = UITextView()
  private let insertPreview = UIButton(type: .system)
  private let dismissButton = UIButton(type: .system)
  private var speakCenterX: NSLayoutConstraint?
  private var speakCenterY: NSLayoutConstraint?
  private var pendingPreparation = false
  private var requestTimeout: Timer?
  private var heartbeat: Timer?
  private var idleHeartbeat: Timer?
  private var startAcknowledgementDeadline: Date?
  private var deleteTimer: Timer?
  private var deleteDocumentID: UUID?
  private enum DeleteTiming {
    static let delay: TimeInterval = 0.38
    static let interval: TimeInterval = 0.07
  }
  private let spaceVoiceOverlay = SpaceVoiceOverlay()
  private var spaceHoldArmed = false
  private var spaceFinishScheduled = false
  private var spaceTerminalPoint: CGPoint?
  private var spaceSessionID: UUID?
  private var startingSpaceVoice = false
  private var awaitingSpaceTranslation = false
  private var spaceFinish: SpaceVoiceFinish?
  private var spaceTranslationTargets: [KeyboardVoiceOption] = []
  private var spaceOutputTarget: KeyboardVoiceOption?
  private var spaceUsesFixedOutput = false
  private var submitIntent: KeyboardSubmitIntent?
  private var lastEnglishPredictionAnchor: CandidateAnchor?
  private struct CandidateRenderState: Equatable {
    let input: String; let preedit: String; let words: [String]
    let anchor: CandidateAnchor?; let nextPage: Bool; let language: KeyboardLanguage
    let interfaceLanguage: InterfaceLanguage; let voicePage: Bool
    let expanded: Bool
  }
  private var candidateRenderState: CandidateRenderState?
  private var voicePage = true
  private var paging = false
  private var panOrigin: CGFloat = 0
  private var lastLayoutWidth: CGFloat = 0
  private var lastReturnKeyType: UIReturnKeyType?
  private var lastPrimaryKeyEnabled: Bool?
  private let checker = UITextChecker()
  private let keyFeedback = UIImpactFeedbackGenerator(style: .rigid)
  private let voiceStartFeedback = UIImpactFeedbackGenerator(style: .heavy)
  private let voiceStopFeedback = UIImpactFeedbackGenerator(style: .medium)
  private let voiceErrorFeedback = UINotificationFeedbackGenerator()
  private let translationSelectionFeedback = UISelectionFeedbackGenerator()
  private var englishWord = ""

  private let root = UIStackView()
  private let top = UIStackView()
  private let header = UIView()
  private var showingCandidatesInHeader = false
  private var headerMotion: UIViewPropertyAnimator?
  private let pageControl = KeyboardModeControl()
  private let status = UILabel()
  private let voicePrompt = UILabel()
  private let body = UIView()
  private let voice = UIView()
  private let speak = SpeakButton()
  private let typingMeter = UIProgressView(progressViewStyle: .bar)
  private let typingPreview = UILabel()
  private let typingCancel = UIButton(type: .system)
  private var showingVoiceStrip = false
  private let transcript = UILabel()
  private let cancelButton = UIButton(type: .system)
  private let keys = UIView()
  private let candidateScroll = UIScrollView()
  private let candidateRow = UIStackView()
  private let candidateToggle = UIButton(type: .system)
  private let candidatePanel = UIScrollView()
  private let candidateGrid = UIStackView()
  private var candidatePanelExpanded = false
  private let globe = UIButton(type: .system)
  private let voiceGlobe = UIButton(type: .system)
  private let typingHint = UILabel()
  private let typingVoiceButton = UIButton(type: .system)
  private let outputLanguageButton = UIButton(type: .system)
  private var outputMenuTargets: [KeyboardVoiceOption] = []
  private var outputMenuSelection: String?
  private var outputMenuLanguage: InterfaceLanguage?
  private let voiceReturn = KeyboardKey(title: "换行", role: .primary)
  private let voiceDelete = KeyboardKey(title: "⌫", role: .auxiliary)
  private var bodyHeight: NSLayoutConstraint?
  private var candidateHeight: NSLayoutConstraint?
  private var voiceControlHeight: NSLayoutConstraint?
  private var notice: String?
  private var rootHeight: NSLayoutConstraint?
  private var rootLeading: NSLayoutConstraint?
  private var rootTrailing: NSLayoutConstraint?
  private var motion: UIViewPropertyAnimator?

  private var target: InputTarget? {
    guard let documentID = DocumentIdentity.read(from: textDocumentProxy) else { return nil }
    return InputTarget(keyboardID: keyboardID, documentID: documentID, revision: revision)
  }

  override init(nibName: String?, bundle: Bundle?) {
    super.init(nibName: nibName, bundle: bundle)
    hasDictationKey = true
  }
  required init?(coder: NSCoder) {
    super.init(coder: coder)
    hasDictationKey = true
  }
  override func loadView() {
    let keyboard = KeyboardAudioInputView(frame: .zero, inputViewStyle: .keyboard)
    keyboard.allowsSelfSizing = true
    inputView = keyboard
  }
  override func viewDidLoad() {
    super.viewDidLoad()
    // UIInputView already supplies the native keyboard material and outer shape.
    // An opaque fill here creates seams against the system's top and bottom areas.
    view.backgroundColor = .clear
    view.isOpaque = false
    services.feedbackService = SystemKeyboardFeedbackService()
    services.actionHandler = OpenLessKeyboardActionHandler(owner: self)
    services.spaceDragGestureHandler.action = { [weak self] offset in self?.adjustTextPosition(by: offset) }
    setupLayout()
    urlBridge.install(in: self)
    installTypingKeyboard()
    loadRime()
    reconnect()
    rebuildKeys()
    setPage(preferences.opensVoicePage, animated: false)
  }
  override func viewWillAppear(_ animated: Bool) {
    super.viewWillAppear(animated)
    // The host queries keyboard capabilities before the view is loaded and
    // can reconnect the extension later. Supply it at both lifecycle points.
    hasDictationKey = true
    libraryDocumentID = nil
    reconnect()
    ping()
    startIdleHeartbeat()
    updateReturnKey()
  }
  override func viewDidAppear(_ animated: Bool) {
    super.viewDidAppear(animated)
    keyboardIsVisible = true
    refreshSystemLexicon()
    prepareFeedback()
    refreshConnectionIfNeeded(force: true)
    readState()
    if pendingStart == nil { ping() }
    UIAccessibility.post(notification: .layoutChanged, argument: nil)
  }
  override func viewWillDisappear(_ animated: Bool) {
    keyboardIsVisible = false
    commitComposition()
    stopForDestinationChange()
    libraryDocumentID = nil
    endDelete()
    cancelSpaceGesture()
    services.actionHandler.handle(.end, on: .backspace)
    requestTimeout?.invalidate()
    heartbeat?.invalidate()
    idleHeartbeat?.invalidate()
    idleHeartbeat = nil
    motion?.stopAnimation(true)
    headerMotion?.stopAnimation(true)
    headerMotion = nil
    top.isHidden = showingCandidatesInHeader
    candidateScroll.isHidden = !showingCandidatesInHeader
    observer = nil
    super.viewWillDisappear(animated)
  }
  override func viewDidLayoutSubviews() {
    super.viewDidLayoutSubviews()
    let inset: CGFloat = preferences.experience == .singleKeyboard ? 3 : 6
    rootLeading?.constant = inset
    rootTrailing?.constant = -inset
    syncLibraryContext()
    candidateHeight?.constant = 44
    // All modes share one viewport; TypingKeyboard scales its contents to fit.
    let contentHeight = CGFloat(KeyboardSizing.bodyHeight(scale: preferences.keyboardScale, landscape: view.bounds.width > 600, experience: preferences.experience))
    bodyHeight?.constant = contentHeight
    voiceControlHeight?.constant = 112
    status.font = KeyboardStyle.detailFont
    transcript.font = KeyboardStyle.detailFont
    helpPrompt.font = KeyboardStyle.detailFont
    updateVoiceGeometry()
    transcript.numberOfLines = 1
    status.isHidden = !translationArc.isHidden || pendingGeneratedPreview != nil || status.text?.isEmpty != false
    rootHeight?.constant = contentHeight + 63 + (typingHint.isHidden ? 0 : 30) + view.safeAreaInsets.bottom
    if body.bounds.width > 1, body.bounds.width != lastLayoutWidth, !paging {
      if !translationArc.isHidden {
        if awaitingSpaceTranslation { cancelSpaceVoice() } else { hideTranslationArc() }
      }
      lastLayoutWidth = body.bounds.width
      setPage(voicePage, animated: false)
    }
  }
  override func viewWillLayoutSubviews() {
    super.viewWillLayoutSubviews()
    // The host can detach before UIKit finishes its final layout callbacks.
    guard view.window != nil, DocumentIdentity.read(from: textDocumentProxy) != nil else {
      globe.isHidden = true
      voiceGlobe.isHidden = true
      return
    }
    globe.isHidden = preferences.experience != .singleKeyboard || !needsInputModeSwitchKey
    voiceGlobe.isHidden = !needsInputModeSwitchKey
  }
  override func textDidChange(_ textInput: (any UITextInput)?) {
    super.textDidChange(textInput)
    changedInput()
    updateReturnKey()
    syncLibraryContext()
    refreshTypingCandidates()
  }
  override func selectionDidChange(_ textInput: (any UITextInput)?) {
    super.selectionDidChange(textInput)
    changedInput()
    if !selfEditing { refreshTypingCandidates() }
  }
  private func changedInput() {
    guard !selfEditing else { return }
    guard let documentID = DocumentIdentity.read(from: textDocumentProxy) else {
      endDelete()
      stopForDestinationChange()
      lastDocumentID = nil
      _ = rime?.clearComposition()
      preedit = ""; rawInput = ""; candidates = []; hasMarkedText = false; hasNextPage = false
      updateCandidates()
      refreshVoice()
      return
    }
    // Reattaching the same field after the voice handoff emits UIKit change
    // callbacks even when neither text nor caret moved. Preserve its revision.
    let unchangedHandoff = handoffInputAnchor.map { anchor in
      anchor.target == target && anchor.before == textDocumentProxy.documentContextBeforeInput
        && anchor.after == textDocumentProxy.documentContextAfterInput
        && anchor.selection == textDocumentProxy.selectedText
    } ?? false
    if !unchangedHandoff { revision &+= 1; handoffInputAnchor = nil }
    let newlyAttached = lastDocumentID != documentID
    if newlyAttached {
      endDelete()
      NSLog("OPENLESS_DESTINATION attached previous=%@ current=%@ busy=%d backgroundReady=%d fresh=%d",
        lastDocumentID?.uuidString ?? "nil", documentID.uuidString, localPhase.isBusy,
        snapshot.backgroundReady, snapshot.isFresh())
      lastDocumentID = documentID
      _ = rime?.clearComposition()
      preedit = ""
      rawInput = ""
      candidates = []
      hasMarkedText = false
      hasNextPage = false
      updateCandidates()
    }
    if let activeTarget, activeTarget != target { stopForDestinationChange() }
    if newlyAttached, keyboardIsVisible, pendingStart == nil { ping() }
    scheduleReturnedVoiceInsertion()
  }

  private func refreshConnectionIfNeeded(force: Bool = false) {
    let accessChanged = lastFullAccess != hasFullAccess
    let retry = hasFullAccess && mailbox == nil && (force || Date() >= connectionRetryAfter)
    guard accessChanged || retry else { return }
    if !hasFullAccess {
      stopForDestinationChange()
      snapshot = VoiceSnapshot(hostEpoch: UUID())
      snapshot.updatedAt = .distantPast
      snapshot.inForeground = false
    }
    reconnect()
    ping()
  }

  private func reconnect() {
    let persistedStyle = KeyboardStyleSelection.load()
    if selectedStyleID != persistedStyle { selectedStyleID = persistedStyle }
    cachedConfiguredCapabilities = nil
    lastVocabularyRevision = nil; vocabularyRetryAfter = .distantPast
    let previous = preferences
    lastFullAccess = hasFullAccess
    connectionRetryAfter = Date().addingTimeInterval(5)
    if hasFullAccess { mailbox = try? SharedContainer.openMailbox() } else { mailbox = nil }
    if let data = UserDefaults.standard.data(forKey: SharedContract.preferencesKey),
      let prefs = try? SharedContract.decode(KeyboardPreferences.self, from: data),
      prefs.version == 1
    {
      preferences = prefs
    }
    if let saved = try? mailbox?.value(
      KeyboardPreferences.self, for: SharedContract.preferencesKey), saved.version == 1
    {
      preferences = saved
    }
    observer = MailboxObserver(name: SharedContainer.stateNotification) { [weak self] in
      self?.readState()
    }
    if previous != preferences {
      rebuildKeys()
      setPage(preferences.opensVoicePage, animated: false)
    }
    notice = nil
    readState()
    refreshVoice()
  }
  private func loadRime() {
    do {
      guard let resources = Bundle.main.resourceURL?.appendingPathComponent("Rime") else {
        throw ContractError.unavailable
      }
      let user = try FileManager.default.url(
        for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
      ).appendingPathComponent("RimeUser")
      // Deliberately stays in this extension's private container, including
      // when Full Access is off. No shared mutable dictionary writer.
      rime = try OLRimeSession(sharedDataPath: resources.path, userDataPath: user.path)
      refreshRimeVocabulary()
    } catch {
      rimeError = error.localizedDescription
      notice = "拼音引擎未能加载，仍可使用英文输入。"
      refreshVoice()
    }
  }
  private func setupLayout() {
    root.axis = .vertical
    root.spacing = 6
    root.translatesAutoresizingMaskIntoConstraints = false
    view.addSubview(root)
    rootLeading = root.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 6)
    rootTrailing = root.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -6)
    NSLayoutConstraint.activate([
      rootLeading!, rootTrailing!,
      root.topAnchor.constraint(equalTo: view.topAnchor, constant: 6),
      root.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -7),
    ])
    rootHeight = view.heightAnchor.constraint(equalToConstant: 305)
    rootHeight?.priority = .defaultHigh
    rootHeight?.isActive = true
    top.axis = .horizontal
    top.alignment = .center
    top.spacing = 6
    top.isLayoutMarginsRelativeArrangement = true
    top.directionalLayoutMargins = NSDirectionalEdgeInsets(top: 0, leading: 6, bottom: 0, trailing: 2)
    top.heightAnchor.constraint(equalToConstant: 44).isActive = true
    let brand = UIButton(type: .system)
    var brandConfiguration = UIButton.Configuration.plain()
    brandConfiguration.title = "OL"
    if let artwork = UIImage(named: "OpenLessBrand") {
      // Render at display scale; a 24-pixel thumbnail is blurry at 3x.
      let size = CGSize(width: 24, height: 24)
      brandConfiguration.title = nil
      brandConfiguration.image = UIGraphicsImageRenderer(size: size).image { _ in
        artwork.draw(in: CGRect(origin: .zero, size: size))
      }.withRenderingMode(.alwaysOriginal)
    }
    brandConfiguration.imagePadding = 5
    brandConfiguration.contentInsets = .zero
    brandConfiguration.baseForegroundColor = .label
    brandConfiguration.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer {
      var attributes = $0
      attributes.font = .systemFont(ofSize: 14, weight: .semibold)
      return attributes
    }
    brand.configuration = brandConfiguration
    brand.accessibilityLabel = "打开 OpenLess 设置"
    brand.accessibilityIdentifier = "keyboardBrand"
    brand.widthAnchor.constraint(equalToConstant: 44).isActive = true
    brand.heightAnchor.constraint(equalToConstant: 44).isActive = true
    brand.setContentCompressionResistancePriority(.required, for: .horizontal)
    brand.addAction(UIAction { [weak self] _ in self?.openApp() }, for: .touchUpInside)
    top.addArrangedSubview(brand)
    top.addArrangedSubview(UIView())
    let modeWidth = pageControl.widthAnchor.constraint(equalToConstant: 144)
    modeWidth.priority = UILayoutPriority(749)
    modeWidth.isActive = true
    let modeMinimumWidth = pageControl.widthAnchor.constraint(greaterThanOrEqualToConstant: 138)
    modeMinimumWidth.priority = UILayoutPriority(999) // Hidden while recording.
    modeMinimumWidth.isActive = true
    pageControl.heightAnchor.constraint(equalToConstant: 44).isActive = true
    pageControl.onSelect = { [weak self] in
      self?.feedback(kind: .control)
      self?.selectMode($0)
    }
    proposalButton.showsMenuAsPrimaryAction = true
    proposalButton.titleLabel?.font = .preferredFont(forTextStyle: .footnote)
    proposalButton.accessibilityIdentifier = "keyboardCorrectionProposals"
    proposalButton.setImage(UIImage(systemName: "checkmark.bubble"), for: .normal)
    proposalButton.titleLabel?.isHidden = true
    let proposalWidth = proposalButton.widthAnchor.constraint(equalToConstant: 44)
    proposalWidth.priority = .defaultHigh
    proposalWidth.isActive = true
    proposalButton.heightAnchor.constraint(equalToConstant: 44).isActive = true
    proposalButton.isHidden = true
    top.addArrangedSubview(proposalButton)
    top.addArrangedSubview(styleButton)
    var styleConfiguration = UIButton.Configuration.filled()
    styleConfiguration.baseBackgroundColor = KeyboardStyle.key
    styleConfiguration.baseForegroundColor = .label
    styleConfiguration.cornerStyle = .capsule
    styleConfiguration.image = UIImage(systemName: "chevron.down")
    styleConfiguration.imagePlacement = .trailing
    styleConfiguration.imagePadding = 4
    styleConfiguration.preferredSymbolConfigurationForImage = UIImage.SymbolConfiguration(pointSize: 9, weight: .semibold)
    styleConfiguration.contentInsets = NSDirectionalEdgeInsets(top: 7, leading: 10, bottom: 7, trailing: 8)
    styleConfiguration.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { attributes in
      var result = attributes; result.font = .systemFont(ofSize: 13, weight: .medium); return result
    }
    styleButton.configuration = styleConfiguration
    styleButton.accessibilityIdentifier = "keyboardStylePicker"
    styleButton.accessibilityLabel = "听写与翻译风格，不用于帮写"
    let styleWidth = styleButton.widthAnchor.constraint(equalToConstant: 96)
    styleWidth.priority = UILayoutPriority(748)
    styleWidth.isActive = true
    let styleMinimumWidth = styleButton.widthAnchor.constraint(greaterThanOrEqualToConstant: 44)
    styleMinimumWidth.priority = UILayoutPriority(999) // UIStackView collapses hidden controls.
    styleMinimumWidth.isActive = true
    styleButton.heightAnchor.constraint(equalToConstant: 44).isActive = true
    styleButton.titleLabel?.lineBreakMode = .byTruncatingTail
    styleButton.showsMenuAsPrimaryAction = true
    top.addArrangedSubview(pageControl)
    top.addArrangedSubview(cancelButton)
    let dismiss = dismissButton
    dismiss.setImage(UIImage(systemName: "keyboard.chevron.compact.down"), for: .normal)
    dismiss.tintColor = .secondaryLabel
    dismiss.accessibilityLabel = "收起键盘"
    let dismissWidth = dismiss.widthAnchor.constraint(equalToConstant: 44)
    dismissWidth.priority = .defaultHigh
    dismissWidth.isActive = true
    dismiss.heightAnchor.constraint(equalToConstant: 44).isActive = true
    dismiss.addAction(UIAction { [weak self] _ in self?.dismissFromUser() }, for: .touchUpInside)
    top.addArrangedSubview(dismiss)
    header.heightAnchor.constraint(equalToConstant: 44).isActive = true
    top.translatesAutoresizingMaskIntoConstraints = false
    header.addSubview(top)
    candidateScroll.translatesAutoresizingMaskIntoConstraints = false
    candidateScroll.accessibilityIdentifier = "keyboardCandidates"
    header.addSubview(candidateScroll)
    candidateToggle.translatesAutoresizingMaskIntoConstraints = false
    candidateToggle.tintColor = .label
    candidateToggle.accessibilityIdentifier = "keyboardExpandCandidates"
    candidateToggle.addAction(UIAction { [weak self] _ in
      guard let self, !self.localPhase.isBusy, !self.candidates.isEmpty else { return }
      self.candidatePanelExpanded.toggle()
      self.candidateRenderState = nil
      self.updateCandidates()
    }, for: .touchUpInside)
    header.addSubview(candidateToggle)
    let divider = UIView()
    divider.backgroundColor = .separator
    divider.translatesAutoresizingMaskIntoConstraints = false
    candidateToggle.addSubview(divider)
    NSLayoutConstraint.activate([
      top.leadingAnchor.constraint(equalTo: header.leadingAnchor),
      top.trailingAnchor.constraint(equalTo: header.trailingAnchor),
      top.topAnchor.constraint(equalTo: header.topAnchor),
      top.bottomAnchor.constraint(equalTo: header.bottomAnchor),
      candidateScroll.leadingAnchor.constraint(equalTo: header.leadingAnchor),
      candidateScroll.trailingAnchor.constraint(equalTo: candidateToggle.leadingAnchor),
      candidateScroll.topAnchor.constraint(equalTo: header.topAnchor),
      candidateScroll.bottomAnchor.constraint(equalTo: header.bottomAnchor),
      candidateToggle.trailingAnchor.constraint(equalTo: header.trailingAnchor),
      candidateToggle.topAnchor.constraint(equalTo: header.topAnchor),
      candidateToggle.bottomAnchor.constraint(equalTo: header.bottomAnchor),
      candidateToggle.widthAnchor.constraint(equalToConstant: 44),
      divider.widthAnchor.constraint(equalToConstant: 0.5),
      divider.leadingAnchor.constraint(equalTo: candidateToggle.leadingAnchor),
      divider.topAnchor.constraint(equalTo: candidateToggle.topAnchor, constant: 6),
      divider.bottomAnchor.constraint(equalTo: candidateToggle.bottomAnchor, constant: -6),
    ])
    candidateScroll.isHidden = true
    candidateScroll.alpha = 0
    root.addArrangedSubview(header)
    body.clipsToBounds = true
    bodyHeight = body.heightAnchor.constraint(equalToConstant: KeyboardStyle.voicePanelHeight)
    bodyHeight?.isActive = true
    root.addArrangedSubview(body)
    voice.translatesAutoresizingMaskIntoConstraints = false
    keys.translatesAutoresizingMaskIntoConstraints = false
    for content in [voice, keys] {
      body.addSubview(content)
      NSLayoutConstraint.activate([
        content.leadingAnchor.constraint(equalTo: body.leadingAnchor),
        content.trailingAnchor.constraint(equalTo: body.trailingAnchor),
        content.topAnchor.constraint(equalTo: body.topAnchor),
        content.bottomAnchor.constraint(equalTo: body.bottomAnchor),
      ])
    }
    candidatePanel.translatesAutoresizingMaskIntoConstraints = false
    candidatePanel.accessibilityIdentifier = "keyboardExpandedCandidates"
    candidatePanel.isHidden = true
    candidatePanel.alwaysBounceVertical = true
    candidateGrid.axis = .vertical
    candidateGrid.spacing = 4
    candidateGrid.translatesAutoresizingMaskIntoConstraints = false
    candidatePanel.addSubview(candidateGrid)
    body.addSubview(candidatePanel)
    NSLayoutConstraint.activate([
      candidatePanel.leadingAnchor.constraint(equalTo: body.leadingAnchor),
      candidatePanel.trailingAnchor.constraint(equalTo: body.trailingAnchor),
      candidatePanel.topAnchor.constraint(equalTo: body.topAnchor),
      candidatePanel.bottomAnchor.constraint(equalTo: body.bottomAnchor),
      candidateGrid.leadingAnchor.constraint(equalTo: candidatePanel.contentLayoutGuide.leadingAnchor, constant: 4),
      candidateGrid.trailingAnchor.constraint(equalTo: candidatePanel.contentLayoutGuide.trailingAnchor, constant: -4),
      candidateGrid.topAnchor.constraint(equalTo: candidatePanel.contentLayoutGuide.topAnchor, constant: 4),
      candidateGrid.bottomAnchor.constraint(equalTo: candidatePanel.contentLayoutGuide.bottomAnchor, constant: -4),
      candidateGrid.widthAnchor.constraint(equalTo: candidatePanel.frameLayoutGuide.widthAnchor, constant: -8),
    ])
    status.font = KeyboardStyle.detailFont
    status.adjustsFontForContentSizeCategory = false
    status.textColor = .secondaryLabel
    status.textAlignment = .center
    status.numberOfLines = 1
    status.lineBreakMode = .byTruncatingTail
    status.translatesAutoresizingMaskIntoConstraints = false
    status.accessibilityIdentifier = "keyboardTranslationStatus"
    voice.addSubview(status)
    voicePrompt.font = .systemFont(ofSize: 15)
    voicePrompt.textColor = .secondaryLabel
    voicePrompt.textAlignment = .center
    voicePrompt.translatesAutoresizingMaskIntoConstraints = false
    voice.addSubview(voicePrompt)
    speak.translatesAutoresizingMaskIntoConstraints = false
    voice.addSubview(speak)
    speak.presentation = .video
    speak.unavailableTitleColor = KeyboardStyle.accent
    speak.onAction = { [weak self] in self?.voiceAction($0) }
    speak.onTranslationTouch = { [weak self] phase, point in
      self?.handleSpeechTranslationTouch(phase, at: point) ?? false
    }
    speak.addAction(UIAction { [weak self] _ in self?.prepareFeedback() }, for: .touchDown)
    speak.onUnavailable = { [weak self] in self?.explainVoiceAvailability() }
    typingMeter.widthAnchor.constraint(equalToConstant: 38).isActive = true
    typingMeter.tintColor = KeyboardStyle.accent
    typingMeter.accessibilityLabel = "当前录音音量"
    typingPreview.font = .systemFont(ofSize: 13)
    typingPreview.lineBreakMode = .byTruncatingHead
    typingCancel.setTitle("取消", for: .normal)
    typingCancel.addAction(UIAction { [weak self] _ in self?.voiceAction(.cancel) }, for: .touchUpInside)
    transcript.font = KeyboardStyle.detailFont
    transcript.adjustsFontForContentSizeCategory = false
    transcript.textAlignment = .center
    transcript.numberOfLines = 1
    transcript.lineBreakMode = .byTruncatingTail
    transcript.textColor = .secondaryLabel
    transcript.translatesAutoresizingMaskIntoConstraints = false
    voice.addSubview(transcript)
    cancelButton.setImage(UIImage(systemName: "xmark"), for: .normal)
    cancelButton.tintColor = .label
    cancelButton.accessibilityLabel = "取消录音"
    cancelButton.isHidden = true
    cancelButton.translatesAutoresizingMaskIntoConstraints = false
    cancelButton.backgroundColor = KeyboardStyle.auxiliary
    cancelButton.layer.cornerRadius = 22
    let cancelWidth = cancelButton.widthAnchor.constraint(equalToConstant: 44)
    cancelWidth.priority = .defaultHigh
    cancelWidth.isActive = true
    cancelButton.addAction(UIAction { [weak self] _ in self?.voiceAction(.cancel) }, for: .touchUpInside)
    // Full-height capsules: the 44pt row is the visual size too, not an inset 36-40pt pill.
    voiceDelete.layer.cornerRadius = 22
    voiceReturn.layer.cornerRadius = 24
    voiceReturn.titleLabel?.font = .systemFont(ofSize: 20, weight: .semibold)
    voiceReturn.accessibilityIdentifier = "keyboardVoiceReturn"
    voiceReturn.titleLabel?.adjustsFontSizeToFitWidth = true
    voiceReturn.titleLabel?.minimumScaleFactor = 0.7
    voiceReturn.titleLabel?.lineBreakMode = .byTruncatingTail
    voiceDelete.onDown = { [weak self] in self?.beginDelete() }
    voiceDelete.onUp = { [weak self] in self?.endDelete() }
    voiceReturn.onDown = { [weak self] in self?.prepareFeedback() }
    voiceReturn.onTap = { [weak self] in
      self?.feedback(kind: .modifier)
      self?.pressReturn()
    }
    for control in [voiceDelete, voiceReturn, voiceGlobe] {
      control.translatesAutoresizingMaskIntoConstraints = false
      voice.addSubview(control)
    }
    for button in [globe, voiceGlobe] {
      button.setImage(UIImage(systemName: "globe"), for: .normal)
      button.tintColor = .label
      button.accessibilityLabel = "切换输入法"
      button.addTarget(self, action: #selector(handleInputModeList(from:with:)), for: .allTouchEvents)
      addButtonFeedback(button, kind: .control, event: .touchDown)
    }
    let globeWidth = globe.widthAnchor.constraint(equalToConstant: 44)
    globeWidth.priority = .defaultHigh
    globeWidth.isActive = true
    globe.heightAnchor.constraint(equalToConstant: 44).isActive = true
    top.insertArrangedSubview(globe, at: 1)
    typingHint.font = .systemFont(ofSize: 12)
    typingHint.textColor = .secondaryLabel
    typingHint.numberOfLines = 2
    typingHint.accessibilityIdentifier = "keyboardTypingHint"
    let hintHeight = typingHint.heightAnchor.constraint(equalToConstant: 24)
    hintHeight.priority = UILayoutPriority(999)
    hintHeight.isActive = true
    root.insertArrangedSubview(typingHint, at: 1)
    typingHint.isHidden = true
    for (button, symbol, identifier) in [(typingVoiceButton, "mic", "keyboardInlineVoice"),
      (outputLanguageButton, "character.bubble", "keyboardOutputLanguage")] {
      var config = UIButton.Configuration.plain()
      config.image = UIImage(systemName: symbol)
      config.imagePadding = 4
      config.baseForegroundColor = .label
      config.contentInsets = .zero
      button.configuration = config
      button.titleLabel?.font = .systemFont(ofSize: 13)
      button.titleLabel?.lineBreakMode = .byTruncatingTail
      button.accessibilityIdentifier = identifier
      button.heightAnchor.constraint(equalToConstant: 44).isActive = true
      let width = button.widthAnchor.constraint(equalToConstant: button === typingVoiceButton ? 44 : 80)
      width.priority = .defaultHigh
      width.isActive = true
      let minimumWidth = button.widthAnchor.constraint(greaterThanOrEqualToConstant: 44)
      minimumWidth.priority = UILayoutPriority(999)
      minimumWidth.isActive = true
      top.insertArrangedSubview(button, at: top.arrangedSubviews.count - 1)
    }
    typingVoiceButton.addAction(UIAction { [weak self] _ in
      guard let self else { return }
      if self.localPhase == .recording { self.voiceAction(.stop) }
      else if !self.localPhase.isBusy { self.explainVoiceAvailability() }
    }, for: .touchUpInside)
    outputLanguageButton.showsMenuAsPrimaryAction = true
    setupVideoControls()
    for button in [brand, dismissButton, styleButton, proposalButton, translationButton,
      undoButton, typingCancel, cancelButton, typingVoiceButton, outputLanguageButton] {
      addButtonFeedback(button, kind: .control)
    }
    for button in [atButton, insertPreview] { addButtonFeedback(button, kind: .input) }
    for control in [voiceGlobe, voiceDelete] {
      let toolWidth = control.widthAnchor.constraint(equalToConstant: 44)
      toolWidth.priority = .defaultHigh
      toolWidth.isActive = true
      control.heightAnchor.constraint(equalToConstant: 44).isActive = true
    }
    // Give the primary action the spare width; compact screens can contract it
    // without shrinking the other touch targets or clipping their icons.
    let returnWidth = voiceReturn.widthAnchor.constraint(equalToConstant: 116)
    returnWidth.priority = UILayoutPriority(749)
    let returnMinimumWidth = voiceReturn.widthAnchor.constraint(greaterThanOrEqualToConstant: 44)
    returnMinimumWidth.priority = .defaultHigh
    NSLayoutConstraint.activate([
      returnWidth, returnMinimumWidth,
      voiceReturn.heightAnchor.constraint(equalToConstant: 48),
    ])
    speakCenterX = speak.centerXAnchor.constraint(equalTo: voice.centerXAnchor)
    speakCenterY = speak.centerYAnchor.constraint(equalTo: voice.centerYAnchor, constant: -25)
    speakCenterX?.isActive = true
    speakCenterY?.isActive = true
    voiceControlHeight = speak.heightAnchor.constraint(equalToConstant: 112)
    voiceControlHeight?.isActive = true
    NSLayoutConstraint.activate([
      status.leadingAnchor.constraint(equalTo: voice.leadingAnchor, constant: 8),
      status.trailingAnchor.constraint(equalTo: voice.trailingAnchor, constant: -8),
      status.topAnchor.constraint(equalTo: transcript.bottomAnchor, constant: 2),
      status.heightAnchor.constraint(equalToConstant: 20),
      voicePrompt.leadingAnchor.constraint(equalTo: voice.leadingAnchor, constant: 12),
      voicePrompt.trailingAnchor.constraint(equalTo: voice.trailingAnchor, constant: -12),
      voicePrompt.bottomAnchor.constraint(equalTo: speak.centerYAnchor, constant: -42),
      voicePrompt.heightAnchor.constraint(equalToConstant: 22),
      speak.widthAnchor.constraint(equalToConstant: 176),
      transcript.leadingAnchor.constraint(equalTo: voice.leadingAnchor, constant: 12),
      transcript.trailingAnchor.constraint(equalTo: voice.trailingAnchor, constant: -12),
      transcript.topAnchor.constraint(equalTo: speak.centerYAnchor, constant: 56),
      transcript.heightAnchor.constraint(equalToConstant: 20),
      cancelButton.heightAnchor.constraint(equalToConstant: 44),
    ])
    spaceVoiceOverlay.translatesAutoresizingMaskIntoConstraints = false
    view.addSubview(spaceVoiceOverlay)
    NSLayoutConstraint.activate([
      spaceVoiceOverlay.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor),
      spaceVoiceOverlay.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor),
      spaceVoiceOverlay.topAnchor.constraint(equalTo: view.topAnchor),
      spaceVoiceOverlay.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor),
    ])
    spaceVoiceOverlay.onAction = { [weak self] action in
      if action == .cancel { self?.cancelSpaceVoice() }
      else { self?.finishSpaceVoice(action) }
    }
    let pan = UIPanGestureRecognizer(target: self, action: #selector(pagePan(_:)))
    // Only the toolbar pans; never intercept keys, candidates or the spacebar.
    pan.delegate = self
    pan.maximumNumberOfTouches = 1
    top.addGestureRecognizer(pan)
    let voicePan = UIPanGestureRecognizer(target: self, action: #selector(pagePan(_:)))
    voicePan.delegate = self
    voicePan.maximumNumberOfTouches = 1
    body.addGestureRecognizer(voicePan)
    candidateScroll.showsHorizontalScrollIndicator = false
    candidateScroll.contentInsetAdjustmentBehavior = .never
    if #available(iOS 26, *) {
      // The compact candidate row has no chrome above or below it. Automatic
      // scroll edge effects otherwise overlap and blur the entire 44pt row.
      candidateScroll.topEdgeEffect.isHidden = true
      candidateScroll.bottomEdgeEffect.isHidden = true
      candidateScroll.leftEdgeEffect.isHidden = true
      candidateScroll.rightEdgeEffect.isHidden = true
    }
    candidateHeight = candidateScroll.heightAnchor.constraint(equalToConstant: 44)
    candidateHeight?.isActive = true
    candidateRow.axis = .horizontal
    candidateRow.spacing = 8
    candidateRow.alignment = .center
    candidateRow.translatesAutoresizingMaskIntoConstraints = false
    candidateScroll.addSubview(candidateRow)
    NSLayoutConstraint.activate([
      candidateRow.leadingAnchor.constraint(equalTo: candidateScroll.contentLayoutGuide.leadingAnchor, constant: 8),
      candidateRow.trailingAnchor.constraint(equalTo: candidateScroll.contentLayoutGuide.trailingAnchor, constant: -8),
      candidateRow.topAnchor.constraint(equalTo: candidateScroll.contentLayoutGuide.topAnchor),
      candidateRow.bottomAnchor.constraint(equalTo: candidateScroll.contentLayoutGuide.bottomAnchor),
      candidateRow.heightAnchor.constraint(equalTo: candidateScroll.frameLayoutGuide.heightAnchor),
    ])
  }

  private func setupVideoControls() {
    for (button, symbol, title) in [(helpWriteButton, "pencil.and.outline", "帮我写"),
      (translationButton, "character.bubble", "翻译：按住并向上滑动选择，也可点按选择"),
      (atButton, "at", "输入 @"), (undoButton, "arrow.uturn.backward", "撤销")] {
      button.setImage(UIImage(systemName: symbol), for: .normal)
      button.tintColor = .label
      button.backgroundColor = KeyboardStyle.key
      button.layer.cornerRadius = 22
      button.accessibilityLabel = title
      button.translatesAutoresizingMaskIntoConstraints = false
      voice.addSubview(button)
      let buttonWidth = button.widthAnchor.constraint(equalToConstant: 44)
      buttonWidth.priority = .defaultHigh // UIStackView can collapse hidden tools.
      buttonWidth.isActive = true
      button.heightAnchor.constraint(equalToConstant: 44).isActive = true
    }
    helpWriteButton.addAction(UIAction { [weak self] _ in self?.startVoice(kind: .helpWrite) }, for: .touchUpInside)
    helpWriteButton.addAction(UIAction { [weak self] _ in self?.prepareFeedback() }, for: .touchDown)
    undoButton.accessibilityIdentifier = "keyboardUndo"
    undoButton.addAction(UIAction { [weak self] _ in self?.undoLastInsertion() }, for: .touchUpInside)
    atButton.accessibilityIdentifier = "keyboardAt"
    atButton.addAction(UIAction { [weak self] _ in self?.insertText("@") }, for: .touchUpInside)
    translationButton.addAction(UIAction { [weak self] _ in self?.showTranslationArc() }, for: .touchUpInside)
    let hold = UILongPressGestureRecognizer(target: self, action: #selector(translationHold(_:)))
    hold.minimumPressDuration = 0.3
    translationButton.addGestureRecognizer(hold)
    helpPrompt.text = "描述要写的内容\n再次点击以完成"
    helpPrompt.font = KeyboardStyle.detailFont
    helpPrompt.adjustsFontForContentSizeCategory = false
    helpPrompt.numberOfLines = 1
    helpPrompt.lineBreakMode = .byTruncatingTail
    helpPrompt.translatesAutoresizingMaskIntoConstraints = false
    voice.addSubview(helpPrompt)
    // Six 44pt targets fit a 320pt screen without stacking edit controls.
    voiceTools.axis = .horizontal
    voiceTools.distribution = .equalSpacing
    voiceTools.alignment = .center
    voiceTools.spacing = 4
    voiceTools.translatesAutoresizingMaskIntoConstraints = false
    voice.addSubview(voiceTools)
    for control in [voiceGlobe, helpWriteButton, voiceReturn, atButton] {
      voiceTools.addArrangedSubview(control)
    }
    undoButton.isHidden = true
    translationArc.translatesAutoresizingMaskIntoConstraints = false
    translationArc.isHidden = true
    translationArc.onChoose = { [weak self] in self?.chooseTranslation($0) }
    translationArc.onCancel = { [weak self] in
      if self?.awaitingSpaceTranslation == true { self?.cancelSpaceVoice() }
      else { self?.hideTranslationArc() }
    }
    body.addSubview(translationArc)
    previewPanel.axis = .vertical
    previewPanel.spacing = 4
    previewPanel.backgroundColor = KeyboardStyle.canvas
    previewPanel.translatesAutoresizingMaskIntoConstraints = false
    previewPanel.isHidden = true
    previewText.isEditable = false
    previewText.font = .preferredFont(forTextStyle: .body)
    previewText.adjustsFontForContentSizeCategory = true
    previewText.backgroundColor = .clear
    previewText.accessibilityLabel = "帮写结果预览"
    insertPreview.setTitle("插入", for: .normal)
    insertPreview.titleLabel?.font = .preferredFont(forTextStyle: .headline)
    insertPreview.addAction(UIAction { [weak self] _ in self?.confirmHelpWriteInsertion() }, for: .touchUpInside)
    previewPanel.addArrangedSubview(previewText)
    previewPanel.addArrangedSubview(insertPreview)
    insertPreview.heightAnchor.constraint(greaterThanOrEqualToConstant: 44).isActive = true
    voice.addSubview(previewPanel)
    NSLayoutConstraint.activate([
      voiceTools.leadingAnchor.constraint(equalTo: voice.leadingAnchor, constant: 8),
      voiceTools.trailingAnchor.constraint(equalTo: voice.trailingAnchor, constant: -8),
      voiceTools.bottomAnchor.constraint(equalTo: voice.bottomAnchor, constant: -4),
      voiceTools.heightAnchor.constraint(equalToConstant: 48),
      translationButton.leadingAnchor.constraint(equalTo: voiceTools.leadingAnchor),
      translationButton.bottomAnchor.constraint(equalTo: voiceTools.topAnchor, constant: -12),
      voiceDelete.trailingAnchor.constraint(equalTo: voiceTools.trailingAnchor),
      voiceDelete.bottomAnchor.constraint(equalTo: voiceTools.topAnchor, constant: -12),
      helpPrompt.leadingAnchor.constraint(equalTo: voice.leadingAnchor, constant: 90),
      helpPrompt.trailingAnchor.constraint(equalTo: voice.trailingAnchor, constant: -12),
      helpPrompt.centerYAnchor.constraint(equalTo: speak.centerYAnchor),
      helpPrompt.heightAnchor.constraint(equalToConstant: 22),
      translationArc.leadingAnchor.constraint(equalTo: body.leadingAnchor),
      translationArc.trailingAnchor.constraint(equalTo: body.trailingAnchor),
      translationArc.topAnchor.constraint(equalTo: body.topAnchor),
      translationArc.bottomAnchor.constraint(equalTo: body.bottomAnchor),
      previewPanel.leadingAnchor.constraint(equalTo: voice.leadingAnchor, constant: 12),
      previewPanel.trailingAnchor.constraint(equalTo: voice.trailingAnchor, constant: -12),
      previewPanel.topAnchor.constraint(equalTo: voice.topAnchor, constant: 8),
      previewPanel.bottomAnchor.constraint(equalTo: voiceTools.topAnchor, constant: -4),
    ])
  }
  private var capabilities: KeyboardVoiceCapabilities? {
    guard hasFullAccess, snapshot.isFresh(), let value = snapshot.capabilities, value.isValid else { return nil }
    return value
  }
  private var configuredCapabilities: KeyboardVoiceCapabilities? {
    guard hasFullAccess else { return nil }
    return cachedConfiguredCapabilities
  }
  private var selectedStyleRequiresSetup: Bool {
    guard let id = selectedStyleID, let capabilities = configuredCapabilities else { return false }
    return capabilities.styleCatalog?.contains(where: { $0.id == id }) == true
      && !capabilities.styles.contains(where: { $0.id == id })
  }
  private func startVoice(kind: KeyboardVoiceIntent.Kind, targetLanguage: String? = nil) {
    let intent = KeyboardVoiceIntent(kind: kind, targetLanguage: targetLanguage,
      stylePackID: kind == .helpWrite ? nil : selectedStyleID)
    let raw = intent.kind == .dictation && intent.targetLanguage == nil && intent.stylePackID == "builtin.raw"
    guard raw || configuredCapabilities?.allows(intent) == true else {
      voiceFailureFeedback()
      notice = selectedStyleRequiresSetup ? "此风格需要文字模型，请在 OpenLess 中配置。" : "请先在 OpenLess 中完成设置。"
      refreshVoice(); return
    }
    if let capabilities, capabilities.allows(intent), snapshot.canStart, snapshot.inForeground || snapshot.backgroundReady {
      beginVoice(intent: intent, capabilities: capabilities)
    } else {
      // A stale snapshot does not prove the host was killed. Ask the running
      // service first; only an unavailable service needs a foreground handoff.
      beginVoice(intent: intent, capabilities: configuredCapabilities)
    }
  }
  private func showTranslationArc() {
    guard !localPhase.isBusy, let capabilities = configuredCapabilities, capabilities.canTranslate,
      !capabilities.translationTargets.isEmpty else {
      notice = "翻译尚未可用，请在 OpenLess 中配置并启用。"; refreshVoice(); return
    }
    translationArc.begin(targets: capabilities.translationTargets)
    translationArc.isHidden = false
    translationSelectionChanged()
    [speak, helpWriteButton, translationButton, atButton, voiceReturn, voiceDelete, status, transcript]
      .forEach { $0.accessibilityElementsHidden = true }
    refreshVoice()
    UIAccessibility.post(notification: .layoutChanged, argument: translationArc)
  }
  private func hideTranslationArc() {
    let wasVisible = !translationArc.isHidden
    translationArc.interrupt()
    translationArc.isHidden = true
    [speak, helpWriteButton, translationButton, atButton, voiceReturn, voiceDelete, status, transcript]
      .forEach { $0.accessibilityElementsHidden = false }
    if wasVisible { refreshVoice() }
  }
  private func chooseTranslation(_ id: String) {
    if awaitingSpaceTranslation {
      guard let client = spaceSessionID, client == activeClient, let target = activeTarget,
        spaceTranslationTargets.contains(where: { $0.id == id }) else { cancelSpaceVoice(); return }
      awaitingSpaceTranslation = false
      hideTranslationArc()
      var command = VoiceCommand(kind: .finishSpaceTranslation, sessionID: client,
        hostEpoch: activeHostEpoch, target: target, language: preferences.language)
      command.capabilityRevision = activeStart?.capabilityRevision
      command.spaceVoiceFinish = SpaceVoiceFinish(.translate, targetLanguage: id)
      spaceFinish = command.spaceVoiceFinish
      do {
        guard let mailbox else { throw ContractError.unavailable }
        try mailbox.enqueue(command); SharedContainer.post(SharedContainer.commandsNotification)
      } catch { cancelSpaceVoice() }
      refreshVoice(); return
    }
    hideTranslationArc()
    startVoice(kind: .translation, targetLanguage: id)
  }
  private func translationSelectionChanged() {
    guard preferences.haptics && hasFullAccess else { return }
    translationSelectionFeedback.selectionChanged()
    translationSelectionFeedback.prepare()
  }
  private func moveTranslationSelection(to point: CGPoint) {
    let previous = translationArc.gesture.selectedID
    translationArc.move(to: point)
    if let selected = translationArc.gesture.selectedID, selected != previous { translationSelectionChanged() }
  }
  private func finishTranslationSelection(at point: CGPoint) {
    guard !translationArc.isHidden else { return }
    let previous = translationArc.gesture.selectedID
    let id = translationArc.release(at: point)
    if let id, id != previous { translationSelectionChanged() }
    hideTranslationArc()
    if let id {
      NSLog("OPENLESS_TRANSLATION_SELECTION confirmed target=%@", id)
      startVoice(kind: .translation, targetLanguage: id)
    } else {
      NSLog("OPENLESS_TRANSLATION_SELECTION cancelled")
    }
  }
  private func handleSpeechTranslationTouch(_ phase: SpeakButton.TranslationTouch, at point: CGPoint) -> Bool {
    switch phase {
    case .began:
      guard configuredCapabilities?.canTranslate == true,
        configuredCapabilities?.translationTargets.isEmpty == false,
        activeStart == nil || activeStart?.intent.kind == .dictation else { return false }
      NSLog("OPENLESS_TRANSLATION_SELECTION takeover client=%@", activeClient?.uuidString ?? "nil")
      // Discard the provisional dictation, including a pending foreground
      // lookup. The selected language starts a separate, immutable request.
      voiceAction(.cancel)
      showTranslationArc()
      guard !translationArc.isHidden else { return false }
      moveTranslationSelection(to: translationArc.convert(point, from: speak))
    case .moved:
      guard !translationArc.isHidden else { return false }
      moveTranslationSelection(to: translationArc.convert(point, from: speak))
    case .ended:
      finishTranslationSelection(at: translationArc.convert(point, from: speak))
    case .cancelled:
      hideTranslationArc()
    }
    return true
  }
  @objc private func translationHold(_ recognizer: UILongPressGestureRecognizer) {
    switch recognizer.state {
    case .began: showTranslationArc()
    case .changed: moveTranslationSelection(to: recognizer.location(in: translationArc))
    case .ended:
      // UIKit may deliver a final location without a matching .changed event.
      // Recheck the release point so moving into the cancel area never commits
      // the target selected by an earlier event.
      finishTranslationSelection(at: recognizer.location(in: translationArc))
    case .cancelled, .failed: hideTranslationArc()
    default: break
    }
  }
  private func updateVoiceGeometry() {
    let help = activeStart?.intent.kind == .helpWrite && localPhase == .recording
    speakCenterX?.constant = help ? -voice.bounds.width / 2 + 46 : 0
    // Ready, preparation and processing share one center above the tool row.
    speakCenterY?.constant = -26
  }
  private func confirmHelpWriteInsertion() {
    guard confirmedPreviewID == nil, let preview = pendingGeneratedPreview,
      let start = activeStart, let client = activeClient, let target,
      target == activeTarget, snapshot.isFresh(), snapshot.hostEpoch == activeHostEpoch,
      snapshot.generatedPreview == preview, snapshot.sessionID == client,
      preview.isUsable(sessionID: client, target: target), start.intent.kind == .helpWrite,
      KeyboardVoiceDelivery.disposition(start: start, echoedIntent: snapshot.activeIntent,
        confirmedHelpWrite: true) == .insert else {
      notice = "帮写会话已失效，未插入文字。请取消后重新开始。"
      UIAccessibility.post(notification: .announcement, argument: notice)
      return
    }
    if send(.confirmGeneratedPreview, session: client, target: target, previewID: preview.id) {
      confirmedPreviewID = preview.id
      refreshVoice()
    }
  }

  private func installTypingKeyboard() {
    let host = UIHostingController(rootView: TypingKeyboard(
      state: state, services: services, context: state.keyboardContext,
      interfaceLanguage: interfacePreferences.language, keyboardScale: preferences.keyboardScale,
      experience: preferences.experience, spaceHoldRecording: preferences.usesSpaceRecording, owner: self))
    host.view.backgroundColor = .clear
    host.view.translatesAutoresizingMaskIntoConstraints = false
    addChild(host)
    keys.addSubview(host.view)
    NSLayoutConstraint.activate([
      host.view.leadingAnchor.constraint(equalTo: keys.leadingAnchor),
      host.view.trailingAnchor.constraint(equalTo: keys.trailingAnchor),
      host.view.topAnchor.constraint(equalTo: keys.topAnchor),
      host.view.bottomAnchor.constraint(equalTo: keys.bottomAnchor),
    ])
    host.didMove(toParent: self)
    typingHost = host
    state.calloutContext.minimumInputDuration = 0
  }

  private func syncLibraryContext() {
    guard view.window != nil, let documentID = DocumentIdentity.read(from: textDocumentProxy) else { return }
    let context = state.keyboardContext
    if libraryDocumentID != documentID {
      context.originalTextDocumentProxy = textDocumentProxy
      libraryDocumentID = documentID
    }
    let orientation: KeyboardKit.InterfaceOrientation = view.bounds.width > 600 ? .landscape : .portrait
    if context.interfaceOrientation != orientation { context.interfaceOrientation = orientation }
    let size = view.window?.screen.bounds.size ?? view.bounds.size
    if context.screenSize != size { context.screenSize = size }
    if context.traitCollection != traitCollection { context.traitCollection = traitCollection }
    if context.hasDictationKey != true { context.hasDictationKey = true }
    if context.hasFullAccess != hasFullAccess { context.hasFullAccess = hasFullAccess }
    if context.needsInputModeSwitchKey != needsInputModeSwitchKey { context.needsInputModeSwitchKey = needsInputModeSwitchKey }
    let locale = Locale(identifier: preferences.language == .chinese ? "zh_Hans" : "en_US")
    if context.locale != locale { context.locale = locale }
    let capitalization: Keyboard.AutocapitalizationType? = preferences.language == .chinese ? Keyboard.AutocapitalizationType.none : nil
    if context.autocapitalizationTypeOverride != capitalization { context.autocapitalizationTypeOverride = capitalization }
    let audioEnabled = preferences.keySound && hasFullAccess
    if state.feedbackContext.settings.isAudioFeedbackEnabled != audioEnabled {
      state.feedbackContext.settings.isAudioFeedbackEnabled = audioEnabled
    }
    let hapticsEnabled = preferences.haptics && hasFullAccess
    if state.feedbackContext.settings.isHapticFeedbackEnabled != hapticsEnabled {
      state.feedbackContext.settings.isHapticFeedbackEnabled = hapticsEnabled
    }
    let hapticConfiguration = Feedback.HapticConfiguration(
      press: .mediumImpact, release: .none, doubleTap: .none, longPress: .heavyImpact,
      repeat: .selectionChanged
    )
    if state.feedbackContext.hapticConfiguration != hapticConfiguration {
      state.feedbackContext.hapticConfiguration = hapticConfiguration
    }
    let audioConfiguration = Feedback.AudioConfiguration.standard
    if state.feedbackContext.audioConfiguration != audioConfiguration {
      state.feedbackContext.audioConfiguration = audioConfiguration
    }
    if context.settings.spaceLongPressBehavior != .moveInputCursor {
      context.settings.spaceLongPressBehavior = .moveInputCursor
    }
  }
  private func rebuildKeys() {
    lastEnglishPredictionAnchor = nil
    candidateRenderState = nil
    syncLibraryContext()
    if let typingHost {
      typingHost.rootView = TypingKeyboard(state: state, services: services,
        context: state.keyboardContext, interfaceLanguage: interfacePreferences.language,
        keyboardScale: preferences.keyboardScale,
        experience: preferences.experience, spaceHoldRecording: preferences.usesSpaceRecording, owner: self)
    }
    view.setNeedsLayout()
    updateReturnKey()
    updateCandidates()
    refreshVoice()
  }
  /// `.control` = ordinary UI action: haptic only, no key-click sound.
  private enum FeedbackKind { case input, delete, modifier, control }
  private func prepareFeedback() {
    guard preferences.haptics && hasFullAccess else { return }
    keyFeedback.prepare()
    voiceStartFeedback.prepare()
    voiceStopFeedback.prepare()
    voiceErrorFeedback.prepare()
  }
  private func addButtonFeedback(_ button: UIButton, kind: FeedbackKind,
    event: UIControl.Event = .touchUpInside) {
    button.addAction(UIAction { [weak self] _ in self?.prepareFeedback() }, for: .touchDown)
    button.addAction(UIAction { [weak self] _ in self?.feedback(kind: kind) }, for: event)
  }
  private func feedback(kind: FeedbackKind) {
    if preferences.haptics && hasFullAccess { keyFeedback.impactOccurred(intensity: 1.0) }
    playKeySound(kind)
  }
  private func playKeySound(_ kind: FeedbackKind) {
    guard kind != .control, preferences.keySound, hasFullAccess else { return }
    KeyboardClickSounds.play()
  }
  private func voiceFeedback(starting: Bool) {
    if preferences.haptics && hasFullAccess {
      let generator = starting ? voiceStartFeedback : voiceStopFeedback
      generator.impactOccurred(intensity: 1.0)
    }
    // Recording start/stop: haptic only, so no click is picked up by the mic.
  }
  private func voiceFailureFeedback() {
    guard preferences.haptics && hasFullAccess else { return }
    voiceErrorFeedback.notificationOccurred(.error)
  }

  private func dismissFromUser() {
    commitComposition()
    super.dismissKeyboard()
  }
  func insertText(_ text: String) {
    if activeClient != nil { stopForDestinationChange() }
    if text == "\n" { pressReturn(); return }
    if text == " " { spaceTapped(); return }
    if preferences.language == .chinese, let rime, text.count == 1,
      let code = text.unicodeScalars.first?.value, code < 128,
      text == text.lowercased()
    {
      let result = rime.processKey(Int32(code))
      apply(result)
      if !result.handled { edit { textDocumentProxy.insertText(text) } }
    } else {
      commitComposition()
      edit { textDocumentProxy.insertText(text) }
    }
    refreshTypingCandidates()
  }
  func deleteBackward() {
    delete()
  }
  func deleteBackward(times: Int) { for _ in 0..<min(1_000, max(0, times)) { delete() } }
  func adjustTextPosition(by characterOffset: Int) {
    commitComposition()
    edit { textDocumentProxy.adjustTextPosition(byCharacterOffset: characterOffset) }
  }
  func endSentence(withText text: String) { edit { textDocumentProxy.endSentence(withText: text) } }
  func insertDiacritic(_ diacritic: Keyboard.Diacritic) { edit { textDocumentProxy.insertDiacritic(diacritic) } }
  func performAutocomplete() { refreshTypingCandidates() }
  func performDictation() { setPage(true, animated: true); voiceAction(.start) }
  func selectNextLocale() { selectMode(preferences.language == .chinese ? 1 : 2) }
  func setKeyboardCase(_ keyboardCase: Keyboard.KeyboardCase) {
    commitComposition()
    state.keyboardContext.keyboardCase = keyboardCase
  }
  func setKeyboardType(_ type: Keyboard.KeyboardType) {
    commitComposition()
    state.keyboardContext.keyboardType = type
  }
  func openUrl(_ url: URL?) {
    guard let url else { return }
    extensionContext?.open(url)
  }

  private func apply(_ result: OLRimeSnapshot) {
    edit {
      if !result.committedText.isEmpty {
        if hasMarkedText {
          textDocumentProxy.setMarkedText(
            result.committedText,
            selectedRange: NSRange(location: result.committedText.utf16.count, length: 0))
          textDocumentProxy.unmarkText()
          hasMarkedText = false
        } else {
          textDocumentProxy.insertText(result.committedText)
        }
      }
      if !result.preedit.isEmpty {
        textDocumentProxy.setMarkedText(
          result.preedit, selectedRange: NSRange(location: result.preedit.utf16.count, length: 0))
        hasMarkedText = true
      } else if hasMarkedText {
        textDocumentProxy.setMarkedText("", selectedRange: NSRange(location: 0, length: 0))
        textDocumentProxy.unmarkText()
        hasMarkedText = false
      }
    }
    preedit = result.preedit
    rawInput = result.rawInput
    candidateScroll.accessibilityValue = rawInput
    nativeCandidates = result.candidates
    hasNextPage = result.hasNextPage
    if preedit.isEmpty { compositionChoices = []; updateChineseCandidates() }
    else { updateCompositionCandidates() }
  }

  private func refreshSystemLexicon() {
    guard !lexiconRequestPending, Date().timeIntervalSince(lastLexiconRequest) > 60 else { return }
    lexiconRequestPending = true
    lastLexiconRequest = Date()
    let completion: @Sendable (UILexicon) -> Void = { [weak self] lexicon in
      let entries = lexicon.entries.prefix(2048).map {
        KeyboardPredictionEngine.SupplementaryEntry(input: $0.userInput, text: $0.documentText)
      }
      Task { @MainActor [weak self] in
        let prepared = await Task.detached(priority: .utility) {
          entries.map { entry in
            var entry = entry
            entry.reading = KeyboardPredictionEngine.reading(for: entry.text)
            return entry
          }
        }.value
        guard let self else { return }
        self.lexiconRequestPending = false
        self.predictionEngine.setSupplementaryEntries(prepared)
        self.lastEnglishPredictionAnchor = nil
        guard self.keyboardIsVisible else { return }
        if self.preedit.isEmpty { self.refreshTypingCandidates() }
        else { self.updateCompositionCandidates() }
      }
    }
    requestSupplementaryLexicon(completion: completion)
  }

  private func updateCompositionCandidates() {
    let personal = predictionAllowed ? predictionEngine.supplementaryCandidates(for: rawInput, chinese: true) : []
    candidates = []; compositionChoices = []
    var seen = Set<String>()
    for word in personal {
      guard seen.insert(word).inserted else { continue }
      candidates.append(word)
      compositionChoices.append(nativeCandidates.firstIndex(of: word).map(CompositionChoice.native) ?? .supplementary(word))
    }
    for (index, word) in nativeCandidates.enumerated() where seen.insert(word).inserted {
      candidates.append(word); compositionChoices.append(.native(index))
    }
    updateCandidates()
  }
  private func edit(_ operation: () -> Void) {
    guard DocumentIdentity.read(from: textDocumentProxy) != nil else {
      stopForDestinationChange()
      return
    }
    undoRecord = nil
    selfEditing = true
    operation()
    selfEditing = false
    revision &+= 1
  }
  private func commitComposition() {
    if !preedit.isEmpty, let rime { apply(rime.commitComposition()) }
  }
  private func delete() {
    if activeClient != nil { stopForDestinationChange() }
    if !preedit.isEmpty, let rime {
      apply(rime.processKey(0xff08))
    } else {
      edit { textDocumentProxy.deleteBackward() }
    }
    refreshTypingCandidates()
  }
  private func beginDelete() {
    endDelete()
    guard let documentID = DocumentIdentity.read(from: textDocumentProxy) else { return }
    deleteDocumentID = documentID
    prepareFeedback()
    feedback(kind: .delete)
    delete()
    let delay = Timer(timeInterval: DeleteTiming.delay, repeats: false) { [weak self] _ in
      MainActor.assumeIsolated {
        guard let self, self.repeatDelete() else { return }
        let repeating = Timer(timeInterval: DeleteTiming.interval, repeats: true) { [weak self] _ in
          MainActor.assumeIsolated { _ = self?.repeatDelete() }
        }
        self.deleteTimer = repeating
        RunLoop.main.add(repeating, forMode: .common)
      }
    }
    deleteTimer = delay
    RunLoop.main.add(delay, forMode: .common)
  }
  private func repeatDelete() -> Bool {
    guard let deleteDocumentID, deleteDocumentID == DocumentIdentity.read(from: textDocumentProxy),
      keyboardIsVisible else { endDelete(); return false }
    feedback(kind: .delete)
    delete()
    return self.deleteDocumentID != nil
  }
  private func endDelete() {
    deleteTimer?.invalidate()
    deleteTimer = nil
    deleteDocumentID = nil
  }
  func handleLibraryDelete(_ gesture: Keyboard.Gesture, action: KeyboardAction) -> Bool {
    guard action == .backspace else { return false }
    switch gesture {
    case .press: beginDelete()
    case .release, .end: endDelete()
    // The shared timer owns all repeats. Consuming these prevents KeyboardKit
    // from deleting again on long press, release or its independent timer.
    default: break
    }
    return true
  }
  private func commitRawComposition() {
    guard !rawInput.isEmpty, let rime else { return }
    apply(rime.commitRawInput())
  }
  private func pressReturn() {
    guard primaryKeyEnabled else { return }
    if !preedit.isEmpty {
      // First confirm the typed letters; a second tap performs the host action.
      commitRawComposition()
      return
    }
    edit { textDocumentProxy.insertText("\n") }
    refreshTypingCandidates()
  }
  var primaryKeyEnabled: Bool {
    let requiresText = textDocumentProxy.returnKeyType == .send || textDocumentProxy.enablesReturnKeyAutomatically == true
    return !requiresText || textDocumentProxy.hasText || !rawInput.isEmpty
  }
  private func updateReturnKey() {
    let type = textDocumentProxy.returnKeyType ?? .default
    let enabled = primaryKeyEnabled
    guard lastReturnKeyType != type || lastPrimaryKeyEnabled != enabled else { return }
    lastReturnKeyType = type
    lastPrimaryKeyEnabled = enabled
    let title: String
    switch type {
    case .go: title = "前往"
    case .search: title = "搜索"
    case .send: title = "发送"
    case .done: title = "完成"
    case .next: title = "下一项"
    case .join: title = "加入"
    default: title = "换行"
    }
    voiceReturn.setTitle(uiText.text(title), for: .normal)
    voiceReturn.accessibilityLabel = uiText.text(title)
    voiceReturn.isEnabled = !localPhase.isBusy && enabled
    state.keyboardContext.objectWillChange.send()
  }
  private func updateEnglishCandidates() {
    guard predictionAllowed, let anchor = currentCandidateAnchor else {
      englishWord = ""
      candidates = []
      candidateAnchor = nil
      updateCandidates()
      return
    }
    if lastEnglishPredictionAnchor == anchor, candidateAnchor == anchor { return }
    lastEnglishPredictionAnchor = anchor
    let input = KeyboardPredictionEngine.EnglishInput.parse(anchor.before)
    englishWord = input.prefix
    candidateAnchor = anchor
    // Do not replace a partial word when the cursor is in its middle.
    if anchor.after.first.map(KeyboardPredictionEngine.EnglishInput.isWordCharacter) == true {
      candidates = []
    } else {
      let context = anchor.before
      let range = NSRange(location: context.utf16.count - englishWord.utf16.count, length: englishWord.utf16.count)
      let completions = englishWord.isEmpty ? [] : checker.completions(
        forPartialWordRange: range, in: context, language: "en_US") ?? []
      let misspelled = englishWord.count >= 3 && textDocumentProxy.autocorrectionType != .no
        && checker.rangeOfMisspelledWord(in: context, range: range, startingAt: range.location,
          wrap: false, language: "en_US").location != NSNotFound
      let corrections = misspelled ? checker.guesses(forWordRange: range, in: context, language: "en_US") ?? [] : []
      candidates = predictionEngine.englishCandidates(
        for: input, corrections: corrections, completions: completions)
    }
    hasNextPage = false
    updateCandidates()
  }
  private var predictionAllowed: Bool {
    guard textDocumentProxy.keyboardType != .emailAddress,
      textDocumentProxy.keyboardType != .URL,
      textDocumentProxy.isSecureTextEntry != true,
      textDocumentProxy.selectedText?.isEmpty != false else { return false }
    return ![UITextContentType.password, .newPassword, .oneTimeCode, .username, .emailAddress, .URL]
      .contains(textDocumentProxy.textContentType ?? .init(rawValue: ""))
  }
  private var currentCandidateAnchor: CandidateAnchor? {
    guard let document = DocumentIdentity.read(from: textDocumentProxy),
      let before = textDocumentProxy.documentContextBeforeInput else { return nil }
    return CandidateAnchor(document: document, before: String(before.suffix(160)),
      after: String((textDocumentProxy.documentContextAfterInput ?? "").prefix(80)))
  }
  private func refreshTypingCandidates() {
    if preferences.language == .english { updateEnglishCandidates() }
    else if preedit.isEmpty { updateChineseCandidates() }
  }
  private func updateChineseCandidates() {
    guard preedit.isEmpty else { return }
    candidateAnchor = predictionAllowed ? currentCandidateAnchor : nil
    candidates = candidateAnchor.map { anchor in
      // A continuation at an insertion point inside an existing Chinese word
      // would duplicate its remaining text.
      let nextIsHan = anchor.after.first?.unicodeScalars.contains {
        (0x3400...0x9fff).contains($0.value)
      } == true
      return nextIsHan ? [] : predictionEngine.chineseCandidates(before: anchor.before)
    } ?? []
    hasNextPage = false
    updateCandidates()
  }
  private func updateCandidates() {
    if candidates.isEmpty { closeCandidatePanel() }
    let rendering = CandidateRenderState(input: rawInput, preedit: preedit, words: candidates,
      anchor: candidateAnchor, nextPage: hasNextPage, language: preferences.language,
      interfaceLanguage: interfacePreferences.language, voicePage: voicePage, expanded: candidatePanelExpanded)
    guard showingVoiceStrip || candidateRenderState != rendering else { return }
    candidateRenderState = rendering
    showingVoiceStrip = false
    candidateRow.arrangedSubviews.forEach {
      candidateRow.removeArrangedSubview($0)
      $0.removeFromSuperview()
    }
    if candidates.isEmpty && rawInput.isEmpty {
      let label = UILabel()
      label.font = .systemFont(ofSize: 12)
      label.textColor = .secondaryLabel
      label.text = !preedit.isEmpty ? preedit : rimeError == nil
        ? (preferences.language == .chinese ? "中文全拼" : "English") + (preferences.usesSpaceRecording ? " · 空格可按住录音" : " · 长按空格移动光标")
        : "拼音引擎未就绪 · 可切换 EN"
      candidateRow.addArrangedSubview(label)
    }
    let isPrediction = preedit.isEmpty
    // Predictions remain useful after a commit, while this fixed first action
    // always restores the language/voice/style toolbar with a 44pt target.
    if isPrediction && !candidates.isEmpty {
      let close = UIButton(type: .system)
      close.setImage(UIImage(systemName: "chevron.backward"), for: .normal)
      close.accessibilityLabel = uiText.text("返回键盘工具栏")
      close.accessibilityIdentifier = "dismissKeyboardPredictions"
      close.widthAnchor.constraint(equalToConstant: 44).isActive = true
      close.heightAnchor.constraint(equalToConstant: 44).isActive = true
      close.addAction(UIAction { [weak self] _ in
        self?.englishWord = ""
        self?.candidates = []
        self?.candidateAnchor = nil
        self?.updateCandidates()
      }, for: .touchUpInside)
      candidateRow.addArrangedSubview(close)
    }
    for (index, text) in candidates.enumerated() {
      candidateRow.addArrangedSubview(candidateButton(text, index: index))
    }
    if !candidatePanelExpanded && hasNextPage && preferences.language == .chinese && !preedit.isEmpty {
      let more = UIButton(type: .system)
      more.setTitle("更多 ›", for: .normal)
      more.heightAnchor.constraint(equalToConstant: 44).isActive = true
      addButtonFeedback(more, kind: .control)
      more.addAction(
        UIAction { [weak self] _ in
          guard let self, let rime = self.rime else { return }
          self.apply(rime.nextPage())
        }, for: .touchUpInside)
      candidateRow.addArrangedSubview(more)
    }
    candidateScroll.setContentOffset(.zero, animated: false)
    updateCandidatePanel()
    updateHeaderPresentation()
  }
  private func candidateButton(_ text: String, index: Int, expanded: Bool = false) -> UIButton {
    let anchor = candidateAnchor
    let composingInput = rawInput
    let document = DocumentIdentity.read(from: textDocumentProxy)
    let choice = compositionChoices.indices.contains(index) ? compositionChoices[index] : nil
    let button = UIButton(type: .system)
    button.setTitle(text, for: .normal)
    button.heightAnchor.constraint(equalToConstant: 44).isActive = true
    button.tintColor = .label
    button.titleLabel?.font = .systemFont(ofSize: 20)
    button.titleLabel?.adjustsFontSizeToFitWidth = true
    button.titleLabel?.minimumScaleFactor = 0.75
    button.accessibilityIdentifier = (expanded ? "expandedCandidate-" : "keyboardCandidate-") + String(index)
    button.configuration = .plain()
    button.configuration?.contentInsets = NSDirectionalEdgeInsets(
      top: 0, leading: 9, bottom: 0, trailing: 9)
    button.addAction(UIAction { [weak self] _ in
      self?.prepareFeedback(); self?.feedback(kind: .input)
    }, for: .touchDown)
    button.addAction(
      UIAction { [weak self] _ in
        guard let self else { return }
        self.closeCandidatePanel()
        if self.preferences.language == .chinese, !self.preedit.isEmpty, let rime = self.rime {
          guard self.rawInput == composingInput, DocumentIdentity.read(from: self.textDocumentProxy) == document,
            self.candidates.contains(text), let choice else { return }
          switch choice {
          case .native(let nativeIndex):
            guard self.nativeCandidates.indices.contains(nativeIndex), self.nativeCandidates[nativeIndex] == text else { return }
            self.apply(rime.selectCandidate(UInt(nativeIndex)))
          case .supplementary(let text):
            self.apply(rime.clearComposition())
            self.edit { self.textDocumentProxy.insertText(text) }
            self.refreshTypingCandidates()
          }
        } else {
          guard let anchor, anchor == self.currentCandidateAnchor, self.predictionAllowed else {
            self.refreshTypingCandidates()
            return
          }
          self.edit {
            if self.preferences.language == .english {
              for _ in self.englishWord { self.textDocumentProxy.deleteBackward() }
              let after = self.textDocumentProxy.documentContextAfterInput?.first
              let space = after == nil || after?.isLetter == true || after?.isNumber == true ? " " : ""
              self.textDocumentProxy.insertText(text + space)
            } else {
              self.textDocumentProxy.insertText(text)
            }
          }
          self.englishWord = ""
          self.refreshTypingCandidates()
        }

      }, for: .touchUpInside)
    return button
  }
  private func closeCandidatePanel() {
    candidatePanelExpanded = false
    candidatePanel.isHidden = true
    candidateToggle.setImage(UIImage(systemName: "chevron.down"), for: .normal)
    candidateToggle.accessibilityLabel = uiText.text("展开候选")
    candidateToggle.accessibilityValue = "collapsed"
    keys.layer.opacity = 1
    keys.accessibilityElementsHidden = voicePage
    candidateRenderState = nil
  }

  private func updateCandidatePanel() {
    candidateToggle.setImage(UIImage(systemName: candidatePanelExpanded ? "chevron.up" : "chevron.down"), for: .normal)
    candidateToggle.accessibilityLabel = uiText.text(candidatePanelExpanded ? "收起候选" : "展开候选")
    candidateToggle.accessibilityValue = candidatePanelExpanded ? "expanded" : "collapsed"
    candidatePanel.isHidden = !candidatePanelExpanded
    keys.layer.opacity = candidatePanelExpanded ? 0 : 1
    keys.accessibilityElementsHidden = candidatePanelExpanded || voicePage
    guard candidatePanelExpanded else { return }
    body.bringSubviewToFront(candidatePanel)
    candidateGrid.arrangedSubviews.forEach { $0.removeFromSuperview() }
    let columns = candidates.allSatisfy { $0.count <= 4 } ? 4 : 2
    for start in stride(from: 0, to: candidates.count, by: columns) {
      let row = UIStackView()
      row.axis = .horizontal; row.spacing = 4; row.distribution = .fillEqually
      for index in start..<min(start + columns, candidates.count) {
        row.addArrangedSubview(candidateButton(candidates[index], index: index, expanded: true))
      }
      for _ in min(start + columns, candidates.count)..<(start + columns) { row.addArrangedSubview(UIView()) }
      candidateGrid.addArrangedSubview(row)
    }
    if hasNextPage && !preedit.isEmpty {
      let more = UIButton(type: .system)
      more.setTitle(uiText.text("更多 ›"), for: .normal)
      more.accessibilityIdentifier = "keyboardMoreCandidates"
      more.heightAnchor.constraint(equalToConstant: 44).isActive = true
      more.addAction(UIAction { [weak self] _ in
        guard let self, let rime = self.rime else { return }
        self.apply(rime.nextPage())
      }, for: .touchUpInside)
      candidateGrid.addArrangedSubview(more)
    }
    candidatePanel.setContentOffset(.zero, animated: false)
  }

  private func updateHeaderPresentation() {
    let composing = !candidates.isEmpty || (preferences.language == .chinese ? !preedit.isEmpty : !englishWord.isEmpty)
    let show = !voicePage && (composing || showingVoiceStrip)
    candidateToggle.isHidden = !show || showingVoiceStrip || candidates.isEmpty
    guard show != showingCandidatesInHeader else { return }
    showingCandidatesInHeader = show
    headerMotion?.stopAnimation(false)
    headerMotion?.finishAnimation(at: .current)
    headerMotion = nil
    top.isHidden = false; candidateScroll.isHidden = false
    top.isUserInteractionEnabled = !show
    candidateScroll.isUserInteractionEnabled = show
    top.accessibilityElementsHidden = show
    candidateScroll.accessibilityElementsHidden = !show
    let change = {
      self.top.alpha = show ? 0 : 1
      self.candidateScroll.alpha = show ? 1 : 0
    }
    let finish = { [weak self] in
      guard let self, self.showingCandidatesInHeader == show else { return }
      self.top.isHidden = show
      self.candidateScroll.isHidden = !show
      self.headerMotion = nil
    }
    guard view.window != nil, !UIAccessibility.isReduceMotionEnabled else {
      change(); finish(); return
    }
    // Animate live surfaces for interruptible transitions without rasterizing
    // candidate glyphs or replacing the active touch targets.
    let animator = UIViewPropertyAnimator(duration: 0.16, curve: .easeInOut, animations: change)
    headerMotion = animator
    animator.addCompletion { _ in finish() }
    animator.startAnimation()
  }

  private func selectMode(_ index: Int) {
    guard index != 0 else { setPage(true, animated: true); return }
    let language: KeyboardLanguage = index == 1 ? .english : .chinese
    if language != preferences.language {
      stopForDestinationChange()
      commitComposition()
      preferences.language = language
      englishWord = ""
      state.keyboardContext.keyboardCase = language == .chinese ? .lowercased : .auto
      state.keyboardContext.keyboardType = .alphabetic
      candidates = []
      savePreferences()
      rebuildKeys()
    }
    notice = nil
    setPage(false, animated: true)
  }
  private func savePreferences() {
    if let data = try? SharedContract.encode(preferences) {
      UserDefaults.standard.set(data, forKey: SharedContract.preferencesKey)
    }
    try? mailbox?.set(preferences, for: SharedContract.preferencesKey)
    SharedContainer.post(SharedContainer.stateNotification)
  }

  func handleSpaceGesture(_ gesture: Keyboard.Gesture, action: KeyboardAction) -> Bool {
    guard action == .space, spaceRecordingEnabled else { return false }
    #if DEBUG
    NSLog("OPENLESS_SPACE_GESTURE %@", String(describing: gesture))
    #endif
    switch gesture {
    case .press:
      spaceTerminalPoint = nil
      return false // Preserve the normal tap feedback path.
    case .longPress:
      spaceHoldArmed = true
      beginSpaceVoice()
      return true
    case .release, .end:
      guard spaceHoldArmed else { return false }
      scheduleSpaceRelease()
      return true // Consume the hold callbacks so no space is inserted.
    default: return spaceHoldArmed
    }
  }
  func spacePointerMoved(_ global: CGPoint) {
    guard spaceHoldArmed else { return }
    moveSpaceVoice(to: view.convert(global, from: view.window))
  }
  func spacePointerEnded(_ global: CGPoint) {
    spaceTerminalPoint = view.convert(global, from: view.window)
    if spaceHoldArmed { scheduleSpaceRelease() }
  }
  private func scheduleSpaceRelease() {
    guard !spaceFinishScheduled else { return }
    spaceFinishScheduled = true
    // Both recognizers finish in the same event. Defer one main-queue turn so
    // the terminal coordinate wins regardless of callback order.
    DispatchQueue.main.async { [weak self] in
      guard let self else { return }
      self.spaceFinishScheduled = false
      guard self.spaceHoldArmed else { return }
      self.spaceHoldArmed = false
      let terminal = self.spaceTerminalPoint
      self.spaceTerminalPoint = nil
      if let terminal { self.endSpaceVoice(at: terminal) }
      else { self.cancelSpaceVoice() }
    }
  }
  var spaceKeyAccessibilityLabel: String { uiText.text("空格，按住说话") }
  var spaceRecordingEnabled: Bool { preferences.usesSpaceRecording }
  var spaceKeyMayBegin: Bool { !localPhase.isBusy && spaceSessionID == nil }
  func beginSpaceVoice() {
    guard spaceKeyMayBegin else { return }
    guard hasFullAccess, let capabilities, capabilities.supportsSpaceVoice == true,
      snapshot.canStart, snapshot.inForeground || snapshot.backgroundReady else {
      notice = "请先启用键盘语音，再按住空格说话"
      setPage(true, animated: true); refreshVoice(); return
    }
    startingSpaceVoice = true
    defer { startingSpaceVoice = false }
    startVoice(kind: .dictation)
    guard let client = activeClient else { return }
    spaceSessionID = client
    spaceTranslationTargets = capabilities.canTranslate ? capabilities.translationTargets : []
    spaceUsesFixedOutput = preferences.experience == .singleKeyboard
    spaceOutputTarget = preferences.translationOutput(in: capabilities)
    spaceFinish = nil; awaitingSpaceTranslation = false; submitIntent = nil
    spaceVoiceOverlay.begin(canTranslate: spaceUsesFixedOutput ? spaceOutputTarget != nil : capabilities.canTranslate,
      canSend: textDocumentProxy.returnKeyType == .send, language: interfacePreferences.language,
      fixedOutput: spaceUsesFixedOutput, outputTitle: spaceOutputTarget?.title)
    refreshVoice()
  }
  func moveSpaceVoice(to point: CGPoint) {
    guard spaceSessionID != nil, !spaceVoiceOverlay.isHidden else { return }
    if spaceVoiceOverlay.move(to: spaceVoiceOverlay.convert(point, from: view)),
      preferences.haptics && hasFullAccess { translationSelectionFeedback.selectionChanged() }
  }
  func endSpaceVoice(at point: CGPoint) {
    guard let action = spaceVoiceOverlay.release(at: spaceVoiceOverlay.convert(point, from: view)) else { return }
    finishSpaceVoice(action)
  }
  private func finishSpaceVoice(_ action: SpaceVoiceGesture.Action) {
    guard spaceFinish == nil else { return }
    guard let client = spaceSessionID, activeClient == client else { cancelSpaceVoice(); return }
    guard localPhase == .recording else { cancelSpaceVoice(); return }
    if action == .cancel { cancelSpaceVoice(); return }
    if action == .translate {
      let targets = spaceTranslationTargets
      guard !targets.isEmpty, !spaceUsesFixedOutput || spaceOutputTarget != nil else {
        cancelSpaceVoice(); notice = "请先配置翻译模型和语言"; refreshVoice(); return
      }
      awaitingSpaceTranslation = true
      spaceFinish = SpaceVoiceFinish(.chooseTranslation)
      voiceAction(.stop)
      guard spaceSessionID == client, activeClient == client, awaitingSpaceTranslation else { return }
      if spaceUsesFixedOutput, let output = spaceOutputTarget {
        // Stop and selection use the same session and the target frozen at start.
        chooseTranslation(output.id)
        return
      }
      translationArc.begin(targets: targets)
      translationArc.isHidden = false
      translationArc.layer.opacity = 1
      body.bringSubviewToFront(translationArc)
      refreshVoice()
    } else {
      let send = action == .send && textDocumentProxy.returnKeyType == .send
      spaceFinish = SpaceVoiceFinish(send ? .send : .input)
      if send, let target = activeTarget { submitIntent = KeyboardSubmitIntent(sessionID: client, target: target) }
      voiceAction(.stop)
    }
  }
  func cancelSpaceVoice() {
    let hadSession = spaceSessionID != nil
    spaceVoiceOverlay.dismiss()
    awaitingSpaceTranslation = false; spaceFinish = nil; submitIntent = nil; spaceSessionID = nil
    if hadSession { voiceAction(.cancel) }
  }
  private func cancelSpaceGesture() {
    spaceHoldArmed = false; spaceTerminalPoint = nil
    cancelSpaceVoice(); state.keyboardContext.isSpaceDragGestureActive = false
  }

  private func spaceTapped() {
    if !preedit.isEmpty { commitComposition() } else { edit { textDocumentProxy.insertText(" ") } }
    refreshTypingCandidates()
  }

  private func voiceAction(_ action: VoiceGesture.Action) {
    switch action {
    case .start:
      startVoice(kind: .dictation)
    case .stop, .cancel:
      if action == .cancel {
        voiceProgress.reset()
        spaceVoiceOverlay.dismiss(); spaceSessionID = nil; spaceFinish = nil
        awaitingSpaceTranslation = false; submitIntent = nil
      }
      // Host resolution may complete after the user cancels preparation.
      // Retire its request ID before any callback can open the containing app.
      if foregroundHandoffID != nil {
        foregroundHandoffID = nil
        handoffInputAnchor = nil
        localPhase = .cancelled
        refreshVoice()
        return
      }
      if action == .cancel {
        if let hostID = activeHostActivationID { ignoredHostActivationID = hostID }
        else if let live = liveHostActivation { ignoredHostActivationID = live.id }
      }
      if action == .cancel, var returned = returnedHostActivation {
        returned.phase = .cancelled
        try? mailbox?.updateHostVoice(returned)
        returnedHostActivation = nil
        refreshVoice(); return
      }
      if action == .cancel { pendingGeneratedPreview = nil; confirmedPreviewID = nil }
      hideTranslationArc()
      if pendingStart != nil {
        clearStartHandshake()
        pendingStart = nil; activeClient = nil; activeHostActivationID = nil; activeTarget = nil; activeStart = nil
        activeHostEpoch = nil
        localPhase = .cancelled
        refreshVoice(); return
      }
      guard let client = activeClient, let target = activeTarget else {
        activeClient = nil; activeHostActivationID = nil; activeStart = nil; activeHostEpoch = nil
        localPhase = .cancelled
        refreshVoice(); return
      }
      let kind: CommandKind = action == .cancel ? .cancel : .stop
      if action == .stop && localPhase == .recording { voiceFeedback(starting: false) }
      guard send(kind, session: client, target: target) else {
        // Best-effort cancellation; the host's missing lease is also terminal.
        if kind != .cancel { _ = send(.cancel, session: client, target: target) }
        spaceVoiceOverlay.dismiss(); spaceSessionID = nil; spaceFinish = nil; submitIntent = nil
        awaitingSpaceTranslation = false
        activeClient = nil; activeTarget = nil; activeStart = nil; activeHostEpoch = nil
        heartbeat?.invalidate(); localPhase = .error
        notice = "暂时无法连接语音，请重试。"; refreshVoice(); return
      }
      if action == .stop { voiceProgress.stopped(session: client) }
      localPhase = action == .cancel ? .cancelled : .processing
      if action == .cancel {
        activeClient = nil; activeHostActivationID = nil; activeTarget = nil; activeStart = nil
        heartbeat?.invalidate()
      }
      refreshVoice()
    }
  }
  private func beginVoice(intent: KeyboardVoiceIntent, capabilities: KeyboardVoiceCapabilities?) {
    guard hasFullAccess, mailbox != nil else { explainVoiceAvailability(); return }
    guard activeClient == nil else { return }
    voiceProgress.reset()
    voiceFeedback(starting: true)
    clearStartHandshake()
    hideTranslationArc()
    commitComposition()
    activeHostActivationID = nil
    activeHostEpoch = nil
    activeStart = KeyboardVoiceStart(intent: intent, capabilities: capabilities, language: preferences.language)
    activeTranslationTitle = capabilities?.translationTargets.first { $0.id == intent.targetLanguage }?.title
    pendingGeneratedPreview = nil; confirmedPreviewID = nil
    // Freeze the destination before yielding to UIKit: a tap must never migrate
    // to a field selected before the asynchronous handshake callback runs.
    let requestedTarget = target
    let client = UUID()
    if startingSpaceVoice { spaceSessionID = client }
    NSLog("OPENLESS_START_REQUEST id=%@ kind=%@ target=%@ fresh=%d backgroundReady=%d",
      client.uuidString, intent.kind.rawValue, requestedTarget?.documentID.uuidString ?? "nil",
      snapshot.isFresh(), snapshot.backgroundReady)
    let requestedAt = ProcessInfo.processInfo.systemUptime
    pendingStart = client
    activeClient = client
    localPhase = .starting
    refreshVoice()
    DispatchQueue.main.async { [weak self] in
      guard let self, self.pendingStart == client else { return }
      guard let current = self.target, current == requestedTarget, self.keyboardIsVisible else {
        self.pendingStart = nil; self.activeClient = nil; self.activeHostActivationID = nil; self.activeTarget = nil
        self.activeStart = nil
        self.localPhase = .inactive; self.notice = "输入框已断开，请重新选择输入框。"; self.refreshVoice()
        return
      }
      self.activeTarget = current
      self.startHandshake = KeyboardStartHandshake(id: client, target: current, uptime: requestedAt)
      self.advancePendingStart()
    }
  }
  // VoiceSnapshot is a four-second liveness lease. Keep it renewed while
  // this keyboard is visible, including the idle interval before a recording.
  private func startIdleHeartbeat() {
    idleHeartbeat?.invalidate()
    idleHeartbeat = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] timer in
      let finished = MainActor.assumeIsolated {
        guard let self else { return true }
        guard self.viewIfLoaded?.window != nil, self.activeClient == nil else { return false }
        self.refreshConnectionIfNeeded()
        // Renew only a live host lease. Explicit entry/tap handshakes may reconnect a stale host.
        if self.snapshot.isFresh(), self.hasFullAccess, self.mailbox != nil, let target = self.target {
          self.send(.ping, session: nil, target: target)
        }
        self.scheduleReturnedVoiceInsertion()
        self.refreshVoice()
        return false
      }
      if finished { timer.invalidate() }
    }
  }
  private func ping() {
    if pendingStart != nil {
      advancePendingStart()
      return
    }
    guard mailbox != nil, let target else { return }
    if let activeClient, let activeTarget {
      // Reattachment probes must preserve ownership of a recording/result that
      // this controller still holds. A nil-session ping explicitly means the
      // keyboard no longer owns the old session, even if its document matches.
      send(.ping, session: activeClient, target: activeTarget)
      return
    }
    send(.ping, session: nil, target: target)
    requestTimeout?.invalidate()
    requestTimeout = Timer.scheduledTimer(withTimeInterval: 1.2, repeats: false) { [weak self] _ in
      MainActor.assumeIsolated {
        guard let self else { return }
        // An absent host is normal while typing. Report timeouts only for an
        // explicit preparation/start handshake, not the idle connection probe.
        if self.pendingPreparation {
          self.pendingPreparation = false
          self.notice = "无法连接语音。请手动打开 OpenLess 检查。"
          self.refreshVoice()
        }
      }
    }
  }
  private func clearStartHandshake() {
    startHandshake = nil
    requestTimeout?.invalidate()
    requestTimeout = nil
  }
  private func finishPendingStart(_ message: String) {
    voiceFailureFeedback()
    clearStartHandshake()
    pendingStart = nil; activeClient = nil; activeHostActivationID = nil
    activeTarget = nil; activeStart = nil; activeHostEpoch = nil
    localPhase = .inactive
    notice = message
    refreshVoice()
  }
  private func scheduleStartRetry(after delay: TimeInterval, probe: Bool = false) {
    guard let handshake = startHandshake else { return }
    let id = handshake.id, destination = handshake.target
    requestTimeout?.invalidate()
    requestTimeout = Timer.scheduledTimer(withTimeInterval: max(0.01, delay), repeats: false) { [weak self] _ in
      MainActor.assumeIsolated {
        guard let self, self.pendingStart == id, self.startHandshake?.id == id else { return }
        guard self.keyboardIsVisible, self.target == destination else {
          self.stopForDestinationChange(); return
        }
        if probe { self.send(.ping, session: nil, target: destination) }
        self.advancePendingStart()
      }
    }
  }
  private func advancePendingStart() {
    guard var handshake = startHandshake, pendingStart == handshake.id,
      let intent = activeStart?.intent else { return }
    let action = handshake.advance(snapshot: snapshot, intent: intent, currentTarget: target,
      date: Date(), uptime: ProcessInfo.processInfo.systemUptime)
    startHandshake = handshake
    switch action {
    case .probe:
      NSLog("OPENLESS_START_PROBE id=%@ age=%.3f", handshake.id.uuidString, Date().timeIntervalSince(snapshot.updatedAt))
      send(.ping, session: nil, target: handshake.target)
      scheduleStartRetry(after: KeyboardStartHandshake.probeTimeout)
    case .awaitResponse(let delay):
      scheduleStartRetry(after: delay)
    case .waitForHost:
      notice = snapshot.phase.isBusy ? "正在等待上一段语音完成…" : "正在准备…"
      scheduleStartRetry(after: 0.5, probe: true)
    case .start:
      guard let capabilities = snapshot.capabilities else { return }
      clearStartHandshake()
      notice = nil
      activeStart = KeyboardVoiceStart(intent: intent, capabilities: capabilities, language: preferences.language)
      sendStart(handshake.id)
    case .openHost:
      if spaceSessionID != nil {
        cancelSpaceVoice()
        notice = "请先启用键盘语音，再按住空格说话"
        refreshVoice(); return
      }
      NSLog("OPENLESS_START_HANDOFF reason=%@ fresh=%d phase=%@ backgroundReady=%d foreground=%d",
        snapshot.isFresh() ? "no-background-session" : "host-probe-timeout",
        snapshot.isFresh(), snapshot.phase.rawValue, snapshot.backgroundReady, snapshot.inForeground)
      fallbackToForegroundVoice()
    case .expired:
      finishPendingStart("语音仍在处理中，请稍后再试。")
    case .unavailable:
      finishPendingStart("语音配置已变更，请重新选择。")
    case .targetChanged:
      finishPendingStart("输入框已断开，请重新选择输入框。")
    case .hostChanged:
      finishPendingStart("主应用会话已更新，请重新开始。")
    case .finished: break
    }
    refreshVoice()
  }
  private func sendStart(_ client: UUID) {
    guard pendingStart == client else { return }
    guard let activeTarget, target == activeTarget, keyboardIsVisible, view.window != nil else {
      finishPendingStart("输入框已断开，请重新选择输入框。")
      return
    }
    guard let start = activeStart, let revision = start.capabilityRevision,
      capabilities?.revision == revision, capabilities?.allows(start.intent) == true else {
      voiceFailureFeedback()
      pendingStart = nil; activeClient = nil; activeHostActivationID = nil; self.activeTarget = nil; activeStart = nil
      localPhase = .inactive; notice = "语音配置已变更，请重新选择。"; refreshVoice(); return
    }
    guard snapshot.isFresh(), snapshot.canStart, snapshot.inForeground || snapshot.backgroundReady else {
      voiceFailureFeedback()
      pendingStart = nil; activeClient = nil; activeHostActivationID = nil; self.activeTarget = nil; activeStart = nil
      localPhase = .inactive; notice = "当前无法开始录音，请手动打开 OpenLess。"; refreshVoice(); return
    }
    activeHostEpoch = snapshot.hostEpoch
    guard send(.start, session: client, target: activeTarget) else {
      voiceFailureFeedback()
      pendingStart = nil; activeClient = nil; activeHostActivationID = nil; self.activeTarget = nil
      activeStart = nil; activeHostEpoch = nil; pendingGeneratedPreview = nil; confirmedPreviewID = nil
      heartbeat?.invalidate(); requestTimeout?.invalidate()
      localPhase = .inactive; notice = "无法发送录音请求，请重试。"; refreshVoice(); return
    }
    pendingStart = nil
    startAcknowledgementDeadline = Date().addingTimeInterval(5)
    startSessionHeartbeat(client, target: activeTarget)
  }
  private func startSessionHeartbeat(_ client: UUID, target activeTarget: InputTarget) {
    heartbeat?.invalidate()
    heartbeat = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] timer in
      let finished = MainActor.assumeIsolated {
        guard let self, self.activeClient == client, self.view.window != nil else { return true }
        if self.voiceProgress.expired(session: client) {
          self.voiceAction(.cancel)
          self.notice = "处理等待超时，请重试；已有结果可在 OpenLess 中查看。"
          self.refreshVoice()
          return true
        }
        if let deadline = self.startAcknowledgementDeadline, Date() >= deadline {
          self.voiceFailureFeedback()
          self.startAcknowledgementDeadline = nil
          self.voiceAction(.cancel)
          self.notice = "主应用未确认本次录音，请重试。"
          self.refreshVoice()
          return true
        }
        let previewExpired = self.pendingGeneratedPreview.map {
          !$0.isUsable(sessionID: client, target: activeTarget)
        } ?? false
        guard self.snapshot.isFresh(), !previewExpired else {
          if self.activeHostActivationID == client {
            // This timer is not a user cancel. Detach only; the App owns capture.
            self.stopForDestinationChange()
            self.notice = "键盘连接已断开，请在主应用或系统录音控制中检查。"
          } else {
            self.voiceAction(.cancel)
            self.notice = "语音连接已中断，请在 OpenLess 中检查。"
          }
          self.refreshVoice()
          return true
        }
        self.send(.ping, session: client, target: activeTarget)
        return false
      }
      if finished { timer.invalidate() }
    }
  }
  @discardableResult
  private func send(_ kind: CommandKind, session: UUID?, target: InputTarget, previewID: UUID? = nil) -> Bool {
    var command = VoiceCommand(
      kind: kind, sessionID: session, hostEpoch: kind == .ping ? nil : activeHostEpoch ?? snapshot.hostEpoch,
      target: target, language: activeStart?.language ?? preferences.language)
    command.previewID = previewID
    if kind == .start, spaceSessionID == session || startingSpaceVoice { command.deferredSpaceVoice = true }
    if kind == .stop, spaceSessionID == session { command.spaceVoiceFinish = spaceFinish ?? SpaceVoiceFinish(.input) }
    if kind == .start || kind == .confirmGeneratedPreview {
      guard let start = activeStart, start.attachMetadata(to: &command) else {
        notice = "语音配置尚未就绪，请在主应用刷新后重试。"
        return false
      }
    }
    do {
      guard let mailbox else { throw ContractError.unavailable }
      try mailbox.enqueue(command)
      SharedContainer.post(SharedContainer.commandsNotification)
      return true
    } catch { notice = "暂时无法连接语音，请重试。"; refreshVoice(); return false }
  }
  private func readInterfacePreferences() {
    let next = mailbox?.interfacePreferences(hasFullAccess: hasFullAccess) ?? SharedInterfacePreferences()
    let changed = next != interfacePreferences
    if changed { lastReturnKeyType = nil }
    interfacePreferences = next
    speak.interfaceLanguage = next.language
    translationArc.interfaceLanguage = next.language
    pageControl.setInterfaceLanguage(next.language)
    helpPrompt.text = uiText.text("描述要写的内容\n再次点击以完成")
    cancelButton.accessibilityLabel = uiText.text("取消录音")
    previewText.accessibilityLabel = uiText.text("帮写结果预览")
    switch next.theme {
    case .system: overrideUserInterfaceStyle = .unspecified
    case .light: overrideUserInterfaceStyle = .light
    case .dark: overrideUserInterfaceStyle = .dark
    }
    typingHost?.overrideUserInterfaceStyle = overrideUserInterfaceStyle
    if changed, let typingHost {
      typingHost.rootView = TypingKeyboard(state: state, services: services,
        context: state.keyboardContext, interfaceLanguage: next.language, keyboardScale: preferences.keyboardScale,
        experience: preferences.experience, spaceHoldRecording: preferences.usesSpaceRecording, owner: self)
    }
  }
  private func refreshRimeVocabulary() {
    guard let rime else { return }
    var phrases: [String] = []
    var revision = "unavailable"
    do {
      if hasFullAccess, let mailbox,
        let value = try mailbox.value(VocabularySnapshot.self, for: VocabularySnapshot.mailboxKey) {
        try value.validate()
        phrases = value.entries.map(\.phrase)
        revision = value.scope + ":" + String(value.generation)
      }
    } catch { /* Revoke unreadable projections; keep base pinyin usable. */ }
    let changed = revision != lastVocabularyRevision || phrases != lastVocabularyPhrases
    guard changed || rime.vocabularyImportError != nil else { return }
    guard Date() >= vocabularyRetryAfter else { return }
    var error: NSError?
    let outcome = rime.importConfirmedPhrases(phrases, revision: revision, error: &error)
    switch outcome {
    case .applied, .unchanged, .deferred:
      lastVocabularyRevision = revision; lastVocabularyPhrases = phrases
      rimeVocabularyError = nil; vocabularyRetryAfter = .distantPast
    case .failed:
      lastVocabularyRevision = nil
      rimeVocabularyError = error?.localizedDescription ?? rime.vocabularyImportError?.localizedDescription
      vocabularyRetryAfter = Date().addingTimeInterval(3)
    @unknown default:
      lastVocabularyRevision = nil
      vocabularyRetryAfter = Date().addingTimeInterval(3)
    }
  }
  private func refreshVocabularyProposals() {
    proposalButton.isHidden = true
    guard hasFullAccess, !localPhase.isBusy, let mailbox,
      let proposals = try? mailbox.value(VocabularyProposals.self, for: VocabularyProposals.mailboxKey),
      let words = try? mailbox.value(VocabularySnapshot.self, for: VocabularySnapshot.mailboxKey),
      (try? proposals.validate()) != nil, (try? words.validate()) != nil,
      proposals.generation == words.generation, !proposals.visible().isEmpty else { return }
    proposalButton.setTitle(uiText.text("纠正建议"), for: .normal)
    let menus: [UIMenuElement] = proposals.visible().map { proposal in
      UIMenu(title: proposal.phrase, children: [
        UIAction(title: uiText.text("添加到词典")) { [weak self] _ in
          self?.sendVocabularyDecision(.init(action: .accept, proposalID: proposal.id, expectedGeneration: proposals.generation))
        },
        UIAction(title: uiText.text("忽略")) { [weak self] _ in
          self?.sendVocabularyDecision(.init(action: .reject, proposalID: proposal.id, expectedGeneration: proposals.generation))
        }
      ])
    }
    let dismiss = UIAction(title: uiText.text("关闭")) { [weak self] _ in
      self?.sendVocabularyDecision(.init(action: .dismiss, proposalID: nil, expectedGeneration: proposals.generation))
    }
    proposalButton.menu = UIMenu(title: uiText.text("纠正建议"), children: menus + [dismiss])
    proposalButton.isHidden = false
  }
  private func sendVocabularyDecision(_ decision: VocabularyDecision) {
    guard hasFullAccess, !localPhase.isBusy, snapshot.isFresh(), let target, let mailbox else { return }
    var command = VoiceCommand(kind: .vocabularyDecision, sessionID: UUID(), hostEpoch: snapshot.hostEpoch,
      target: target, language: preferences.language)
    command.vocabularyDecision = decision
    do { try mailbox.enqueue(command); SharedContainer.post(SharedContainer.commandsNotification); proposalButton.isHidden = true }
    catch { notice = "无法保存纠正建议，请重试。"; refreshVoice() }
  }

  private func readState() {
    readReturnedHostVoice()
    readInterfacePreferences()
    refreshRimeVocabulary()
    refreshVocabularyProposals()
    do {
      if let saved = try mailbox?.value(
        KeyboardPreferences.self, for: SharedContract.preferencesKey), saved.version == 1,
        saved != preferences
      {
        if localPhase.isBusy {
          deferredPreferences = saved
        } else {
          preferences = saved
          rebuildKeys()
          setPage(saved.opensVoicePage, animated: true)
        }
      }
      guard let next = try mailbox?.value(VoiceSnapshot.self, for: SharedContract.snapshotKey) else { return }
      // Public configuration can offer a foreground handoff after the host lease
      // expires. Live phase, insertion and canStart remain behind the freshness gate.
      cachedConfiguredCapabilities = next.configuredVoiceCapabilities
      guard next.isFresh() else { refreshVoice(); return }
      let hostChanged = activeClient != nil && activeHostEpoch != nil && activeHostEpoch != next.hostEpoch
      snapshot = next
      if activeClient == nil, let live = liveHostActivation, live.id != ignoredHostActivationID,
        next.sessionID == live.id, next.phase.isBusy, let target {
        activeClient = live.id; activeHostActivationID = live.id
        activeTarget = target; activeHostEpoch = next.hostEpoch
        activeStart = KeyboardVoiceStart(intent: live.intent, capabilities: nil, language: live.language)
        activeTranslationTitle = live.intent.targetLanguage
        localPhase = next.phase; startAcknowledgementDeadline = nil
        startSessionHeartbeat(live.id, target: target)
      }
      if next.sessionID == activeClient, activeClient != nil { startAcknowledgementDeadline = nil }
      if hostChanged {
        stopForDestinationChange()
        notice = "主应用会话已更新，录音已中断，请重新开始。"
        refreshVoice(); return
      }
      if pendingPreparation {
        pendingPreparation = false
        if next.inForeground, let target {
          send(.prepare, session: UUID(), target: target)
          notice = "请在主应用确认语音授权；准备完成后可以开始。"
        } else {
          notice = "请先打开 OpenLess 完成语音准备。"
        }
      }
      if let client = pendingStart {
        advancePendingStart()
        if pendingStart == client || activeClient != client { refreshVoice(); return }
      }
      if (activeClient != nil && next.sessionID == activeClient)
        || (activeClient == nil && !next.phase.isBusy && ![.error, .cancelled].contains(next.phase))
      {
        localPhase = voiceProgress.phase(next.phase, session: activeClient)
        notice = next.phase == .error ? "本次语音未完成，请重试。" : nil
        if next.phase == .error || next.phase == .cancelled { pendingGeneratedPreview = nil; confirmedPreviewID = nil }
        if let preview = next.generatedPreview, let start = activeStart,
          start.intent.kind == .helpWrite, ![VoicePhase.error, .cancelled].contains(next.phase) {
          let valid = activeClient == preview.sessionID && activeTarget == preview.target
            && target == preview.target && next.activeIntent == start.intent
            && preview.isUsable(sessionID: preview.sessionID, target: preview.target)
          if !valid || (pendingGeneratedPreview != nil && pendingGeneratedPreview != preview) {
            notice = "帮写预览已失效，未插入文字。"
            voiceAction(.cancel)
          } else {
            pendingGeneratedPreview = preview
            localPhase = .awaitingInsertion
          }
        }
        if let insertion = next.insertion, insertion.target.keyboardID == keyboardID,
          insertion.sessionID == activeClient, insertion.target == activeTarget,
          ![VoicePhase.cancelled, .error].contains(next.phase), let start = activeStart {
          let confirmed = pendingGeneratedPreview.map {
            confirmedPreviewID == $0.id && $0.matches(insertion)
          } ?? false
          switch KeyboardVoiceDelivery.disposition(start: start, echoedIntent: next.activeIntent,
            confirmedHelpWrite: confirmed) {
          case .reject:
            notice = "主应用返回的会话意图不匹配，未插入文字。"
            voiceAction(.cancel)
          case .preview:
            notice = "帮写尚未确认，未插入文字。"
            voiceAction(.cancel)
          case .insert:
            if try insert(insertion) {
              pendingGeneratedPreview = nil; confirmedPreviewID = nil
            }
          }
        }
        if [.done, .cancelled, .error].contains(next.phase), pendingGeneratedPreview == nil {
          activeStart = nil
          activeHostEpoch = nil
          activeClient = nil; activeHostActivationID = nil
          activeTarget = nil
          heartbeat?.invalidate()
          if let prefs = deferredPreferences {
            deferredPreferences = nil
            preferences = prefs
            rebuildKeys()
            setPage(prefs.opensVoicePage, animated: true)
          }
        }
      }
      refreshVoice()
    } catch { notice = "暂时无法连接语音，请重试。"; refreshVoice() }
  }
  // True means UIKit received insertText, even if the durable receipt failed.
  // Such a write must never be retried under the same delivery ID.
  private func insert(_ request: InsertionRequest) throws -> Bool {
    guard let mailbox else { return false }
    guard let target else { receipt(request, state: .targetChanged); return false }
    guard let claimed = try mailbox.claimInsertion(request.id, target: target) else {
      if let state = try mailbox.insertion(request.id)?.state,
        [.targetChanged, .uncertain].contains(state)
      {
        receipt(request, state: state)
      }
      return false
    }
    // A delayed UIKit text/selection notification for OUR write must not
    // cancel a successfully delivered Core session.
    activeClient = nil; activeHostActivationID = nil
    activeTarget = nil
    activeStart = nil
    pendingStart = nil
    clearStartHandshake()
    heartbeat?.invalidate()
    let replacedSelection = textDocumentProxy.selectedText?.isEmpty == false
    let shouldSubmit = submitIntent?.consume(for: claimed, current: target,
      supportsSend: textDocumentProxy.returnKeyType == .send) == true
    submitIntent = nil
    var didInsert = false
    edit {
      textDocumentProxy.insertText(claimed.text)
      if shouldSubmit, DocumentIdentity.read(from: textDocumentProxy) == claimed.target.documentID,
        textDocumentProxy.returnKeyType == .send {
        textDocumentProxy.insertText("\n")
      }
      didInsert = true
    }
    guard didInsert else {
      // edit can refuse a detached document after the durable claim succeeded.
      // Revoke conservatively; do not acknowledge a write that never happened.
      try? mailbox.revokeInsertion(claimed.id)
      localPhase = .error
      notice = "输入框已断开，文字未写入；请在主应用复制结果。"
      receipt(claimed, state: .uncertain)
      return false
    }
    if !shouldSubmit, let updatedTarget = self.target {
      undoRecord = KeyboardUndoRecord(target: updatedTarget, text: claimed.text,
        before: textDocumentProxy.documentContextBeforeInput, after: textDocumentProxy.documentContextAfterInput,
        replacedSelection: replacedSelection)
    }
    spaceSessionID = nil; spaceFinish = nil; awaitingSpaceTranslation = false
    voiceProgress.reset()
    // UIKit has already accepted the write. Settle local UI even if durable
    // acknowledgement fails; never retry insertText for this delivery ID.
    pendingGeneratedPreview = nil; confirmedPreviewID = nil; activeHostEpoch = nil
    localPhase = .done
    do {
      try mailbox.acknowledgeInsertion(claimed.id)
      receipt(claimed, state: .submitted)
    } catch {
      try? mailbox.revokeInsertion(claimed.id)
      localPhase = .error
      notice = "文字可能已写入，但回执未保存；请核对输入框，不会自动重试。"
      receipt(claimed, state: .uncertain)
    }
    return true
  }
  private func receipt(_ request: InsertionRequest, state: InsertionState) {
    var command = VoiceCommand(
      kind: .insertionReceipt, sessionID: request.sessionID, hostEpoch: snapshot.hostEpoch,
      target: request.target, language: preferences.language)
    command.insertionID = request.id
    command.receipt = state
    try? mailbox?.enqueue(command)
    SharedContainer.post(SharedContainer.commandsNotification)
  }
  private func refreshVoice() {
    if localPhase.isBusy && candidatePanelExpanded { closeCandidatePanel() }
    let availability = snapshot.keyboardAvailability()
    let canStart = !selectedStyleRequiresSetup && hasFullAccess && mailbox != nil && snapshot.isFresh() && snapshot.canStart
      && (snapshot.inForeground || snapshot.backgroundReady) && target != nil
    let phase = localPhase.isBusy ? localPhase : (canStart ? VoicePhase.ready : .inactive)
    let help = activeStart?.intent.kind == .helpWrite && localPhase == .recording
    let choosingTranslation = !translationArc.isHidden
    let overlayVisible = choosingTranslation || pendingGeneratedPreview != nil
    if activeClient == nil, [.error, .cancelled, .done, .inactive].contains(localPhase) {
      spaceVoiceOverlay.dismiss(); spaceSessionID = nil; spaceFinish = nil; submitIntent = nil
      awaitingSpaceTranslation = false
    }
    let voicePresentation = KeyboardVoicePresentation(phase: localPhase, hasSession: activeClient != nil,
      captureReady: snapshot.isFresh() && snapshot.sessionID == activeClient && snapshot.captureReady)
    let showingSpaceVoice = spaceSessionID != nil && spaceSessionID == activeClient
      && voicePresentation != .idle && !awaitingSpaceTranslation
    if showingSpaceVoice { spaceVoiceOverlay.update(presentation: voicePresentation, level: snapshot.level) }
    else { spaceVoiceOverlay.dismiss() }
    // Keep the original key attached so the held touch receives its terminal event.
    root.layer.opacity = showingSpaceVoice ? 0 : 1
    root.accessibilityElementsHidden = showingSpaceVoice
    speak.presentation = help ? .helpWrite : .video
    speak.presentationIsVisible = voicePage && pendingGeneratedPreview == nil
    let liveLevel = snapshot.isFresh() && snapshot.sessionID == activeClient && snapshot.captureReady
      && localPhase == .recording ? snapshot.level : 0
    let idleLabel: String
    if selectedStyleRequiresSetup { idleLabel = "设置文字模型" }
    else if !hasFullAccess { idleLabel = "启用语音" }
    else {
      switch availability {
      case .ready, .unresponsive: idleLabel = "点击说话"
      case .needsActivation: idleLabel = "启用语音"
      case .preparing, .busy: idleLabel = "请稍候"
      }
    }
    speak.configure(phase: phase, level: liveLevel, available: canStart && !localPhase.isBusy,
      idleLabel: idleLabel)
    voicePrompt.text = uiText.text(idleLabel)
    voicePrompt.isHidden = localPhase.isBusy || overlayVisible
    if let title = activeTranslationTitle, activeStart?.intent.kind == .translation, localPhase.isBusy {
      status.text = snapshot.activeIntent == activeStart?.intent ? "正在翻译为 \(title)" : "正在请求翻译为 \(title)"
    } else {
      status.text = ""
    }
    let translating = activeStart?.intent.kind == .translation && localPhase.isBusy
    status.backgroundColor = .clear
    status.textColor = .secondaryLabel
    status.layer.cornerRadius = 0
    status.clipsToBounds = true
    if !translating { status.text = uiText.text(status.text ?? "") }
    status.accessibilityLabel = status.text
    status.isHidden = overlayVisible || status.text?.isEmpty != false
    helpPrompt.isHidden = !help || overlayVisible
    updateVoiceGeometry()
    updateStyleMenu()
    let canOpenHost = hasFullAccess && mailbox != nil && target != nil && !localPhase.isBusy
    helpWriteButton.isEnabled = (canStart || canOpenHost) && configuredCapabilities?.canHelpWrite == true
    translationButton.isEnabled = (canStart || canOpenHost) && configuredCapabilities?.canTranslate == true
      && configuredCapabilities?.translationTargets.isEmpty == false
    helpWriteButton.accessibilityLabel = helpWriteButton.isEnabled ? "帮我写" : "帮我写不可用，请在 OpenLess 中配置"
    translationButton.accessibilityLabel = translationButton.isEnabled ? "翻译" : "翻译不可用，请在 OpenLess 中配置"
    helpWriteButton.accessibilityHint = helpWriteButton.isEnabled ? "描述要写的内容，不读取选区，不应用听写风格。" : "在 OpenLess 中设置文字模型。"
    translationButton.accessibilityHint = translationButton.isEnabled ? "点按后可逐项选择，或按住向上滑动。" : "在 OpenLess 中添加翻译目标。"
    previewPanel.isHidden = pendingGeneratedPreview == nil
    // Hiding a UIControl cancels its tracked touch. Keep the speech control
    // attached beneath the arc until the selecting finger releases.
    speak.isHidden = pendingGeneratedPreview != nil || (choosingTranslation && !speak.isSelectingTranslation)
    speak.layer.opacity = choosingTranslation ? 0 : 1
    if let preview = pendingGeneratedPreview { previewText.text = preview.text }
    let title = confirmedPreviewID == nil ? "插入" : "正在确认…"
    insertPreview.setTitle(uiText.text(title), for: .normal)
    insertPreview.isEnabled = confirmedPreviewID == nil
    let singleKeyboard = preferences.experience == .singleKeyboard
    pageControl.isHidden = singleKeyboard || localPhase.isBusy
    styleButton.isHidden = localPhase.isBusy || !proposalButton.isHidden
    typingHint.isHidden = !singleKeyboard || notice == nil
    typingHint.text = uiText.text(notice ?? "")
    typingVoiceButton.isHidden = !singleKeyboard || (localPhase.isBusy && localPhase != .recording)
    typingVoiceButton.configuration?.image = UIImage(systemName: localPhase == .recording ? "stop.circle" : "mic")
    typingVoiceButton.accessibilityLabel = uiText.text(localPhase == .recording ? "再次点击以完成" : idleLabel)
    outputLanguageButton.isHidden = !singleKeyboard || localPhase.isBusy
    updateOutputLanguageMenu()
    dismissButton.isHidden = localPhase.isBusy || voicePage || (!singleKeyboard && view.bounds.width < 360 && !styleButton.isHidden)
    if localPhase == .recording && !help {
      transcript.text = "再次点击以完成"
    } else if activeClient != nil && snapshot.sessionID == activeClient && !snapshot.preview.isEmpty {
      transcript.text = String(snapshot.preview.suffix(180))
    } else if let notice {
      transcript.text = notice
    } else if localPhase == .recording {
      transcript.text = "再次点击以完成"
    } else if localPhase == .processing || localPhase == .awaitingInsertion {
      transcript.text = "thinking"
    } else if !hasFullAccess {
      transcript.text = "语音需要“允许完全访问”\n基础打字不受影响"
    } else if let advertised = snapshot.capabilities, !advertised.isValid {
      transcript.text = "请在 OpenLess 中更新语音设置"
    } else if !canStart {
      switch availability {
      case .unresponsive:
        transcript.text = pendingStart != nil ? "正在连接 OpenLess…" : "轻点开始与结束 · 也可以按住说话"
      case .needsActivation: transcript.text = "录音准备完成后将返回原应用。"
      case .preparing: transcript.text = "正在准备…"
      case .busy: transcript.text = "正在等待上一段语音完成…"
      case .ready: transcript.text = "请先选择输入框。"
      }
    } else {
      transcript.text = "轻点开始与结束 · 也可以按住说话"
    }
    if localPhase == .recording || !(activeClient != nil && snapshot.sessionID == activeClient && !snapshot.preview.isEmpty) {
      transcript.text = uiText.text(transcript.text ?? "")
    }
    helpWriteButton.accessibilityLabel = uiText.text(helpWriteButton.accessibilityLabel ?? "")
    helpWriteButton.accessibilityHint = uiText.text(helpWriteButton.accessibilityHint ?? "")
    translationButton.accessibilityLabel = uiText.text(translationButton.accessibilityLabel ?? "")
    translationButton.accessibilityHint = uiText.text(translationButton.accessibilityHint ?? "")
    cancelButton.isHidden = returnedHostActivation == nil && (!localPhase.isBusy || activeClient == nil)
    [voiceDelete, voiceReturn, helpWriteButton]
      .forEach { $0.isHidden = localPhase.isBusy || overlayVisible }
    // Keep the long-press recognizer's view attached and visible to UIKit while
    // its finger is down. Only suppress its drawing beneath the clear overlay.
    translationButton.isHidden = localPhase.isBusy || pendingGeneratedPreview != nil || returnedHostActivation != nil
    translationButton.layer.opacity = choosingTranslation ? 0 : 1
    let invalidCapabilities = snapshot.capabilities.map { !$0.isValid } ?? false
    transcript.isHidden = overlayVisible || help || (localPhase == .processing && notice == nil)
      || (!localPhase.isBusy && notice == nil && !invalidCapabilities && hasFullAccess)
    voiceReturn.isEnabled = !localPhase.isBusy && primaryKeyEnabled
    let canUndo = undoRecord?.matches(target: target, before: textDocumentProxy.documentContextBeforeInput,
      after: textDocumentProxy.documentContextAfterInput, selection: textDocumentProxy.selectedText) == true
    // @ stays available after dictation. Undo is a secondary long-press action
    // on that same edit control, never a replacement for its normal tap.
    atButton.isHidden = localPhase.isBusy || overlayVisible
    atButton.menu = canUndo ? UIMenu(children: [UIAction(title: uiText.text("撤销"),
      image: UIImage(systemName: "arrow.uturn.backward")) { [weak self] _ in self?.undoLastInsertion() }]) : nil
    atButton.showsMenuAsPrimaryAction = false
    undoButton.isHidden = true
    undoButton.isEnabled = canUndo
    undoButton.accessibilityLabel = uiText.text("撤销")
    if !voicePage && voicePresentation != .idle && !showingSpaceVoice {
      if !showingVoiceStrip {
        candidateRow.arrangedSubviews.forEach {
          candidateRow.removeArrangedSubview($0)
          $0.removeFromSuperview()
        }
        [typingMeter, typingPreview, typingCancel].forEach { candidateRow.addArrangedSubview($0) }
        showingVoiceStrip = true
      }
      typingMeter.isHidden = voicePresentation != .recording
      typingMeter.progress = voicePresentation == .recording ? Float(VoiceWaveform.amplitude(rms: snapshot.level)) : 0
      if awaitingSpaceTranslation { typingPreview.text = uiText.text("选择翻译语言") }
      else if !snapshot.preview.isEmpty { typingPreview.text = String(snapshot.preview.suffix(100)) }
      else { typingPreview.text = uiText.text(voicePresentation.title) }
    } else if showingVoiceStrip {
      updateCandidates()
    }
    updateHeaderPresentation()
  }
  private func undoLastInsertion() {
    guard !localPhase.isBusy, let record = undoRecord,
      record.matches(target: target, before: textDocumentProxy.documentContextBeforeInput,
        after: textDocumentProxy.documentContextAfterInput, selection: textDocumentProxy.selectedText) else {
      undoRecord = nil; refreshVoice(); return
    }
    undoRecord = nil
    var expected = record.before
    // Stop if a host transforms context; never delete unrelated text.
    for _ in record.text {
      guard DocumentIdentity.read(from: textDocumentProxy) == record.target.documentID,
        textDocumentProxy.documentContextBeforeInput == expected,
        textDocumentProxy.documentContextAfterInput == record.after,
        textDocumentProxy.selectedText?.isEmpty != false else { break }
      edit { textDocumentProxy.deleteBackward() }
      expected.removeLast()
    }
    refreshVoice()
  }
  private func updateStyleMenu() {
    let capabilities = configuredCapabilities
    guard styleButton.menu == nil || capabilities != styleMenuCapabilities
      || selectedStyleID != styleMenuSelection || interfacePreferences.language != styleMenuLanguage else { return }
    styleMenuCapabilities = capabilities
    styleMenuSelection = selectedStyleID
    styleMenuLanguage = interfacePreferences.language
    let styles = capabilities?.styleCatalog ?? capabilities?.styles ?? []
    let title = styles.first { $0.id == selectedStyleID }?.title ?? (selectedStyleID == nil ? "原文" : "模式不可用")
    styleButton.configuration?.title = uiText.text(title)
    styleButton.accessibilityLabel = uiText.text("选择风格")
    styleButton.accessibilityValue = uiText.text(title)
    styleButton.titleLabel?.lineBreakMode = .byTruncatingTail
    var actions = [UIAction(title: uiText.text("原文"), state: selectedStyleID == nil ? .on : .off) { [weak self] _ in
      self?.selectedStyleID = nil; self?.notice = nil; self?.refreshVoice()
    }]
    actions += styles.filter { $0.id != "builtin.raw" }.map { style in
      let ready = configuredCapabilities?.styles.contains(where: { $0.id == style.id }) == true
      let action = UIAction(title: style.title, state: style.id == selectedStyleID ? .on : .off) { [weak self] _ in
        guard let self else { return }
        self.selectedStyleID = style.id
        self.notice = ready ? nil : "此风格需要文字模型，请在 OpenLess 中配置。"
        self.refreshVoice()
      }
      if !ready { action.subtitle = uiText.text("需设置文字模型") }
      return action
    }
    let manage = UIAction(title: uiText.text("管理风格与模型"), image: UIImage(systemName: "slider.horizontal.3")) { [weak self] _ in
      guard let url = URL(string: "openless-ios://styles") else { return }
      self?.openContainingApp(url)
    }
    styleButton.menu = UIMenu(title: uiText.text("听写与翻译风格（不用于帮写）"), children: [
      UIMenu(options: .displayInline, children: actions),
      UIMenu(options: .displayInline, children: [manage])])
  }

  private func updateOutputLanguageMenu() {
    let targets = configuredCapabilities?.translationTargets ?? []
    let selected = preferences.translationOutputLanguage
    guard outputLanguageButton.menu == nil || targets != outputMenuTargets || selected != outputMenuSelection
      || interfacePreferences.language != outputMenuLanguage else { return }
    outputMenuTargets = targets; outputMenuSelection = selected; outputMenuLanguage = interfacePreferences.language
    let title = targets.first(where: { $0.id == selected })?.title ?? uiText.text("翻译")
    outputLanguageButton.configuration?.title = title
    outputLanguageButton.accessibilityLabel = uiText.text("翻译输出语言")
    outputLanguageButton.accessibilityValue = title
    var actions: [UIMenuElement] = targets.map { option in
      UIAction(title: option.title, state: option.id == selected ? .on : .off) { [weak self] _ in
        guard let self, !self.localPhase.isBusy else { return }
        self.preferences.translationOutputLanguage = option.id
        self.savePreferences(); self.refreshVoice()
      }
    }
    actions.append(UIAction(title: uiText.text("配置翻译"), image: UIImage(systemName: "slider.horizontal.3")) { [weak self] _ in
      guard let url = URL(string: "openless-ios://styles") else { return }
      self?.openContainingApp(url)
    })
    outputLanguageButton.menu = UIMenu(title: uiText.text("翻译输出语言"), children: actions)
  }

  private func explainVoiceAvailability() {
    refreshConnectionIfNeeded(force: true)
    if !hasFullAccess {
      notice = "设置 → OpenLess 键盘 → 允许完全访问\n仅语音需要此权限"
    } else if mailbox == nil {
      notice = "完全访问已开启，但暂时无法连接 OpenLess。请打开主应用后重试。"
    } else if selectedStyleRequiresSetup {
      notice = "此风格需要文字模型，请在 OpenLess 中配置。"
      if let url = URL(string: "openless-ios://styles") { openContainingApp(url) }
    } else {
      startVoice(kind: .dictation)
    }
    refreshVoice()
    UIAccessibility.post(notification: .announcement, argument: notice)
  }
  private func stopForDestinationChange() {
    voiceProgress.reset()
    let wasSpaceSession = spaceSessionID != nil
    spaceVoiceOverlay.dismiss(); spaceSessionID = nil; spaceFinish = nil
    awaitingSpaceTranslation = false; submitIntent = nil
    NSLog("OPENLESS_DESTINATION detach client=%@ pending=%d hostActivation=%d backgroundReady=%d",
      activeClient?.uuidString ?? "nil", pendingStart != nil, activeHostActivationID != nil, snapshot.backgroundReady)
    foregroundHandoffID = nil
    clearStartHandshake()
    // Foreground/system recordings deliver through a later explicit preview.
    // UIKit reattachment and selection callbacks must not stop that independent
    // recording. Direct keyboard sessions still freeze their original target.
    if let client = activeClient, activeHostActivationID != client, let activeTarget, pendingStart == nil {
      let mayFinishDictation = !wasSpaceSession && localPhase == .recording && activeStart?.intent.kind == .dictation
      send(mayFinishDictation ? .stop : .cancel, session: client, target: activeTarget)
    }
    pendingStart = nil
    pendingPreparation = false
    activeClient = nil; activeHostActivationID = nil
    self.activeTarget = nil
    activeStart = nil
    activeHostEpoch = nil
    pendingGeneratedPreview = nil; confirmedPreviewID = nil
    hideTranslationArc()
    localPhase = .inactive
    heartbeat?.invalidate()
    requestTimeout?.invalidate()
  }
  private func fallbackToForegroundVoice() {
    guard pendingStart != nil, let intent = activeStart?.intent else { return }
    clearStartHandshake()
    pendingStart = nil; activeClient = nil; activeHostActivationID = nil
    activeTarget = nil; activeStart = nil; activeHostEpoch = nil
    localPhase = .inactive
    requestForegroundVoice(intent)
  }
  private func requestForegroundVoice(_ intent: KeyboardVoiceIntent) {
    guard hasFullAccess, let mailbox, activeClient == nil, foregroundHandoffID == nil else { return }
    commitComposition()
    guard let target else { notice = "请先选择输入框。"; refreshVoice(); return }
    handoffInputAnchor = HandoffInputAnchor(target: target,
      before: textDocumentProxy.documentContextBeforeInput, after: textDocumentProxy.documentContextAfterInput,
      selection: textDocumentProxy.selectedText)
    let query = UUID()
    let deadline = ProcessInfo.processInfo.systemUptime + 20
    foregroundHandoffID = query
    localPhase = .starting
    notice = "正在准备录音，随后返回原应用。"
    refreshVoice()
    OLKeyboardResolveHostBundleIdentifier(self) { [weak self] source in
      Task { @MainActor in
        guard let self, self.foregroundHandoffID == query else { return }
        self.foregroundHandoffID = nil
        self.localPhase = .inactive
        guard self.keyboardIsVisible, self.hasFullAccess, self.view.window != nil, self.target == target,
          ProcessInfo.processInfo.systemUptime < deadline else {
          self.refreshVoice(); return
        }
        let request = HostVoiceActivation(target: target, language: self.preferences.language,
          intent: intent, startOnOpen: true, sourceApplication: source)
        NSLog("OPENLESS_VOICE_HANDOFF request=%@ source=%@ target=%@", request.id.uuidString,
          source ?? "nil", target.documentID.uuidString)
        self.ignoredHostActivationID = nil
        do {
          try mailbox.requestHostVoice(request, replacingUnresponsiveHost: true)
          SharedContainer.post(SharedContainer.commandsNotification)
          self.openContainingApp(request.activationURL)
        } catch { self.notice = "无法创建录音请求，请稍后重试。" }
        self.refreshVoice()
      }
    }
  }
  private func readReturnedHostVoice() {
    guard hasFullAccess, let mailbox,
      let value = try? mailbox.value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey),
      value.isValid(), value.id != ignoredHostActivationID else {
      liveHostActivation = nil; returnedHostActivation = nil; return
    }
    liveHostActivation = [.starting, .recording, .processing].contains(value.phase) ? value : nil
    guard value.phase == .ready, value.text?.isEmpty == false else {
      returnedHostActivation = nil; return
    }
    returnedHostActivation = value
    scheduleReturnedVoiceInsertion()
  }
  private func scheduleReturnedVoiceInsertion() {
    guard keyboardIsVisible, !returnedInsertionScheduled, !deliveringReturnedInsertion,
      returnedHostActivation != nil else { return }
    returnedInsertionScheduled = true
    DispatchQueue.main.async { [weak self] in
      guard let self else { return }
      self.returnedInsertionScheduled = false
      self.insertReturnedVoiceIntoCurrentInput()
    }
  }
  private func insertReturnedVoiceIntoCurrentInput() {
    guard keyboardIsVisible, view.window != nil, !deliveringReturnedInsertion,
      let returned = returnedHostActivation, let mailbox, returned.isValid(), target != nil else { return }
    deliveringReturnedInsertion = true
    defer { deliveringReturnedInsertion = false }
    // The user's recording is meant for the field available after the return.
    // Rebind here; a new keyboard UUID/revision is normal across an app switch.
    // The durable claim and insertText remain synchronous and at-most-once.
    commitComposition()
    guard let destination = target else { return }
    do {
      let request = try mailbox.prepareHostVoiceInsertion(returned.id, target: destination)
      let didInsert = try insert(request)
      returnedHostActivation = nil
      ignoredHostActivationID = returned.id
      if !didInsert {
        localPhase = .error
        notice = "本次未再次写入；请核对输入框，结果保留在主应用供复制。"
      } else {
        notice = nil
      }
    } catch {
      notice = "暂时无法连接语音，请重试。"
    }
    refreshVoice()
  }
  private func openContainingApp(_ url: URL) {
    let dispatched = urlBridge.open(url) { [weak self] accepted in
      NSLog("OPENLESS_HOST_URL accepted=%d destination=%@", accepted, url.host ?? "")
      if !accepted {
        self?.notice = "请打开 OpenLess 继续录音。"
        self?.refreshVoice()
      }
    }
    if !dispatched {
      notice = "请打开 OpenLess 继续录音。"
      refreshVoice()
    }
  }
  private func openApp() {
    guard let url = URL(string: "openless-ios://settings") else { return }
    openContainingApp(url)
  }
  private func setPage(_ requestedVoicePage: Bool, animated: Bool) {
    closeCandidatePanel()
    let voicePage = preferences.experience == .voiceFocused && requestedVoicePage
    if !voicePage {
      if awaitingSpaceTranslation { cancelSpaceVoice() }
      hideTranslationArc()
    }
    motion?.stopAnimation(false)
    motion?.finishAnimation(at: .current)
    self.voicePage = voicePage
    pageControl.select(voicePage ? 0 : preferences.language == .english ? 1 : 2, animated: animated)
    view.setNeedsLayout()
    let width = max(1, body.bounds.width) + 8 // clear the clipped edge, including antialiasing
    voice.isHidden = false
    keys.isHidden = false
    let changes = {
      self.voice.transform = CGAffineTransform(translationX: voicePage ? 0 : -width, y: 0)
      self.keys.transform = CGAffineTransform(translationX: voicePage ? width : 0, y: 0)
    }
    if animated && !UIAccessibility.isReduceMotionEnabled {
      let animator = UIViewPropertyAnimator(duration: 0.25, dampingRatio: 1, animations: changes)
      motion = animator
      animator.addCompletion { [weak self] _ in
        guard let self, self.voicePage == voicePage else { return }
        self.voice.isHidden = !voicePage
        self.keys.isHidden = voicePage
      }
      animator.startAnimation()
    } else {
      changes()
      voice.isHidden = !voicePage
      keys.isHidden = voicePage
    }
    voice.accessibilityElementsHidden = !voicePage
    keys.accessibilityElementsHidden = voicePage
    refreshVoice()
  }
  func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
    guard preferences.experience == .voiceFocused,
      let pan = gestureRecognizer as? UIPanGestureRecognizer, !localPhase.isBusy, !candidatePanelExpanded,
      translationArc.isHidden else { return false }
    let velocity = pan.velocity(in: body)
    return abs(velocity.x) > abs(velocity.y)
  }
  func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
    if gestureRecognizer.view === body && !voicePage { return false }
    var touched = touch.view
    while let current = touched, current !== view {
      // Never steal mic hold/drag-out, candidate taps, space tracking or keys.
      if current is UIControl || current === keys { return false }
      touched = current.superview
    }
    return true
  }
  @objc private func pagePan(_ recognizer: UIPanGestureRecognizer) {
    guard preferences.experience == .voiceFocused, !localPhase.isBusy else { return }
    let width = max(1, body.bounds.width) + 8 // clear the clipped edge, including antialiasing
    switch recognizer.state {
    case .began:
      motion?.stopAnimation(false)
      motion?.finishAnimation(at: .current)
      voice.isHidden = false
      keys.isHidden = false
      panOrigin = voice.transform.tx
      paging = true
    case .changed:
      let x = min(0, max(-width, panOrigin + recognizer.translation(in: body).x))
      voice.transform = CGAffineTransform(translationX: x, y: 0)
      keys.transform = CGAffineTransform(translationX: x + width, y: 0)
    case .ended:
      paging = false
      let projected = voice.transform.tx + recognizer.velocity(in: body).x * 0.15
      setPage(projected > -width * 0.5, animated: true)
    case .cancelled, .failed:
      paging = false
      setPage(voicePage, animated: true)
    default: break
    }
  }
}

private final class KeyboardKey: UIButton {
  enum Role { case letter, auxiliary, space, primary }
  var onTap: (() -> Void)?
  var onDown: (() -> Void)?
  var onUp: (() -> Void)?
  var onCancel: (() -> Void)?
  var onMove: ((CGPoint) -> Void)?
  var suppressTap = false
  private let keyRole: Role
  private var origin: CGPoint = .zero
  private var restingColor: UIColor {
    switch keyRole {
    case .auxiliary: KeyboardStyle.auxiliary
    case .primary: .systemBlue
    default: KeyboardStyle.key
    }
  }
  init(title: String, role: Role = .letter) {
    self.keyRole = role
    super.init(frame: .zero)
    setTitleColor(role == .primary ? .white : .label, for: .normal)
    titleLabel?.font = .systemFont(ofSize: role == .letter ? 23 : 14, weight: role == .letter ? .regular : .medium)
    if title == "⌫" || title == "⇧" {
      setImage(UIImage(systemName: title == "⌫" ? "delete.left" : "shift"), for: .normal)
      tintColor = .label
    } else {
      setTitle(title, for: .normal)
    }
    backgroundColor = restingColor
    layer.cornerRadius = 8
    layer.cornerCurve = .continuous
    layer.borderWidth = 0.5
    layer.borderColor = UIColor.black.withAlphaComponent(0.07).cgColor
    accessibilityLabel = title == "⌫" ? "删除" : title == "⇧" ? "切换大小写" : title
  }
  required init?(coder: NSCoder) { fatalError("init(coder:) is not supported") }
  override var isEnabled: Bool {
    didSet { alpha = isEnabled ? 1 : 0.45 }
  }
  override func beginTracking(_ touch: UITouch, with event: UIEvent?) -> Bool {
    suppressTap = false
    origin = touch.location(in: self)
    backgroundColor = keyRole == .primary ? .systemBlue.withAlphaComponent(0.7)
      : keyRole == .auxiliary ? KeyboardStyle.key : KeyboardStyle.auxiliary
    onDown?()
    return true
  }
  override func continueTracking(_ touch: UITouch, with event: UIEvent?) -> Bool {
    let point = touch.location(in: self)
    onMove?(CGPoint(x: point.x - origin.x, y: point.y - origin.y))
    return true
  }
  override func endTracking(_ touch: UITouch?, with event: UIEvent?) {
    backgroundColor = restingColor
    let inside = touch.map { bounds.insetBy(dx: -8, dy: -8).contains($0.location(in: self)) } ?? false
    onUp?()
    if inside && !suppressTap { onTap?() }
  }
  override func cancelTracking(with event: UIEvent?) {
    backgroundColor = restingColor
    suppressTap = true
    onCancel?()
    onUp?()
  }
  override func didMoveToWindow() {
    super.didMoveToWindow()
    if window == nil && isTracking {
      suppressTap = true
      onCancel?()
      onUp?()
    }
  }
  override func accessibilityActivate() -> Bool {
    guard isEnabled else { return false }
    if let onTap { onTap() } else { onDown?(); onUp?() }
    return true
  }
}
