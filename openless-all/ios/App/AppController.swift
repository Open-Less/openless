import AVFoundation
import Foundation
import Combine
import OpenLessShared
import Speech
import SwiftUI
import UIKit
import WidgetKit

final class AppController: ObservableObject {
  static let shared = AppController()
  @Published var preferences = KeyboardPreferences() {
    didSet {
      if let data = try? SharedContract.encode(preferences) {
        UserDefaults.standard.set(data, forKey: SharedContract.preferencesKey)
      }
      do { try mailbox?.set(preferences, for: SharedContract.preferencesKey) } catch {
        errorMessage = error.localizedDescription
      }
      SharedContainer.post(SharedContainer.stateNotification)
      if oldValue.backgroundKeepAlive != preferences.backgroundKeepAlive {
        if preferences.backgroundKeepAlive {
          if keyboardAudio.hasActivationIntent {
            keepAlive.arm()
            keepAlive.startIfPossible()
          }
        } else {
          keepAlive.stop()
          keyboardAudio.deactivate()
        }
        publish(force: true)
      }
    }
  }
  @Published var snapshot = VoiceSnapshot(hostEpoch: UUID())
  @Published var pending: [InsertionRequest] = []
  @Published var resultText = ""
  @Published var keyboardTestText = ""
  @Published var errorMessage: String?
  @Published var coreReady = false
  @Published var groupAvailable = false
  @Published var speechPrepared = false
  var speechAuthorizationDenied: Bool {
    MicrophoneAccess.isDenied
      || (currentProviderType == "apple-speech" && (SFSpeechRecognizer.authorizationStatus() == .denied
      || SFSpeechRecognizer.authorizationStatus() == .restricted))
  }
  @Published var preparing = false
  @Published var needsDownload = false
  @Published var diagnostics = ""
  @Published var hostActivation: HostVoiceActivation?
  @Published private(set) var developerMode = UserDefaults.standard.bool(forKey: "ol.ios.developerMode")
  @Published private(set) var runtimeID = UUID()
  @Published var restarting = false
  let developer = DeveloperSettings()
  let library = ProductLibrary()
  private(set) var vocabularyBridge: VocabularyBridge?
  private var versionTapCount = 0
  private var lastVersionTap: TimeInterval = 0
  var currentProviderType: String? {
    guard developerMode else { return "ios-sensevoice" }
    guard let configuration = developer.configuration else { return nil }
    return configuration.isOmni ? "omni" : configuration.activeASR?.providerType
  }
  var asrTitle: String {
    guard developerMode else { return "SenseVoice Small · 本机识别" }
    guard let configuration = developer.configuration else { return "正在读取提供商" }
    if configuration.isOmni { return "全模态 · " + configuration.active.omni }
    return configuration.activeASR?.title ?? "请选择 ASR 提供商"
  }
  let keepAlive = ExperimentalPiPKeepAlive()
  private let keyboardAudio = NoJumpAudioSession.shared
  var keyboardVoiceReady: Bool { backgroundVoiceReady }
  var keyboardVoiceEnabled: Bool { keyboardAudio.hasActivationIntent }

  private var developerObservation: AnyCancellable?
  let interfacePreferences = InterfacePreferences()
  var styleLibraryCore: CoreBridge? { coreReady && !restarting ? core : nil }
  var styleLibraryActivityBusy: Bool { active != nil || snapshot.phase.isBusy || preparing || restarting }
  private var core: CoreBridge?
  private var mailbox: Mailbox?
  private var observer: MailboxObserver?
  private var settingsObserver: MailboxObserver?
  private let capture = AudioCapture()
  private let speech = NativeSpeechService()
  private var inForeground = UIApplication.shared.applicationState == .active
  private var processingBackgroundTask: UIBackgroundTaskIdentifier = .invalid
  private var systemRecordingAdmission: UUID?
  private var recordingActivity: (any RecordingActivitySession)?
  private var recordingActivityOwner: UUID?
  private var publishedControlRecording = false
  private var activeBackgroundAudio: Bool {
    active != nil && snapshot.phase == .recording && snapshot.captureReady
  }
  private var backgroundVoiceReady: Bool {
    preferences.backgroundKeepAlive && keyboardAudio.isReady && keepAlive.isReady
  }
  private var active: ActiveSession?
  private var nativeTasks: [String: Task<Void, Never>] = [:]
  private var spaceProcessingTask: Task<Void, Never>?
  private var insertionDelivery = KeyboardInsertionDelivery()
  private var notificationTokens: [NSObjectProtocol] = []
  private var lastPublish = Date.distantPast
  private var lastTranscriptPublish = Date.distantPast
  private var lastPreview = ""
  private var automaticActivationID: UUID?
  private var automaticActivationTask: Task<Void, Never>?
  private var automaticReturnID: UUID?
  private var automaticReturnTarget: OLHostApplicationReturnTarget?
  private var automaticReturnTask: Task<Void, Never>?
  private var automaticReturnStarted = false
  private var startTask: Task<Void, Never>?
  private var deferredKeyboardCommand: VoiceCommand?
  private var deferredKeyboardTask: Task<Void, Never>?
  private var keyboardLease: KeyboardLease?
  private var leaseTask: Task<Void, Never>?
  private var startupDeadline: Task<Void, Never>?
  private var durationDeadline: Task<Void, Never>?
  private var previewExpiryTask: Task<Void, Never>?
  private var capabilityTask: Task<Void, Never>?
  private var capabilityRevision = UUID()
  private var speechPreparationRevision = UUID()
  private var keyboardPreparationPending = false
  private var keyboardPreparationTask: Task<Void, Never>?

  private func clearDeadlines() {
    startupDeadline?.cancel()
    durationDeadline?.cancel()
    leaseTask?.cancel()
    startupDeadline = nil
    durationDeadline = nil
    leaseTask = nil
    keyboardLease = nil
  }

  private struct ActiveSession {
    let client: UUID
    let target: InputTarget?
    let language: KeyboardLanguage
    var voiceStart: KeyboardVoiceStart?
    var activationID: UUID?
    var systemAuthorized = false
    var audioGeneration: UUID?
    var spaceDelivery: SpaceVoiceDeliveryState?
    var spaceTranslationTargets: Set<String> = []
    var coreFinished = false
    var generatedInsertionID: UUID?
    var coreID: String?
    var cancelled = false
    var stopping = false
    var createdAt = Date()
  }

  init() {
    developerObservation = developer.objectWillChange.sink { [weak self] _ in
      MainActor.assumeIsolated { self?.objectWillChange.send() }
    }
    if let data = UserDefaults.standard.data(forKey: SharedContract.preferencesKey),
      let saved = try? SharedContract.decode(KeyboardPreferences.self, from: data),
      saved.version == 1
    {
      preferences = saved
    }
    do {
      mailbox = try SharedContainer.openMailbox()
      groupAvailable = true
      if let saved = try mailbox?.value(
        KeyboardPreferences.self, for: SharedContract.preferencesKey), saved.version == 1
      {
        preferences = saved
      }
      try mailbox?.set(preferences, for: SharedContract.preferencesKey)
      try mailbox?.recoverUncertainInsertions()
      pending = try mailbox?.pendingInsertions() ?? []
    } catch {
      groupAvailable = false
    }
    interfacePreferences.connect(mailbox: mailbox) {
      SharedContainer.post(SharedContainer.stateNotification)
    }
    observer = MailboxObserver(name: SharedContainer.commandsNotification) { [weak self] in
      self?.drainCommands()
    }
    settingsObserver = MailboxObserver(name: SharedContainer.stateNotification) { [weak self] in
      guard let self,
        let saved = try? self.mailbox?.value(
          KeyboardPreferences.self, for: SharedContract.preferencesKey),
        saved.version == 1, saved != self.preferences
      else { return }
      self.preferences = saved
    }
    keyboardAudio.isBackgroundCaptureAvailable = { [weak self] in
      self?.preferences.backgroundKeepAlive == true && self?.keepAlive.isReady == true
    }
    keyboardAudio.onChange = { [weak self] in self?.publish(force: true) }
    keepAlive.onPrepareStart = { [weak self] in
      guard let self else { throw VoiceFailure.cancelled }
      try self.keyboardAudio.preparePiPStart()
    }
    keepAlive.onSettled = { [weak self] in self?.keyboardAudio.settlePiPStart() }
    keepAlive.onChange = { [weak self] in self?.keyboardAudio.backgroundCapabilityDidChange() }
    keyboardAudio.onInterruption = { [weak self] in
      guard let self else { return }
      self.cancelDeferredKeyboardStart()
      if self.active?.stopping == false, self.active?.cancelled == false { self.stop() }
      self.snapshot.message = "音频设备或系统会话发生变化，已停止录音，不会自动重开。"
      self.publish(force: true)
    }
    startTask = Task { await loadCore() }
  }

  private func loadCore() async {
    do {
      if #available(iOS 18.0, *) { await SystemRecordingActivitySession.finishOrphans() }
      let directory = try FileManager.default.url(
        for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
      ).appendingPathComponent(developerMode ? "CoreDeveloper" : "Core", isDirectory: true)
      core = try await CoreBridge.open(directory: directory, developerMode: developerMode)
      let speech = self.speech
      core?.setAudioSink { id, data in speech.consumeFromCore(data, id: id) }
      core?.onAudioFailure = { [weak self] id, message in
        guard let self, self.active?.coreID == id else { return }
        self.fail(message)
      }
      core?.onEvent = { [weak self] event in self?.handle(event) }
      core?.onConfigurationMutation = { [weak self] in
        self?.invalidateKeyboardCapabilities()
        self?.vocabularyBridge?.clearPublishedState()
        SharedContainer.post(SharedContainer.stateNotification)
        self?.refreshVocabularyBridge()
      }
      core?.onTransportFailure = { [weak self] message in
        guard self?.active != nil else { return }
        self?.fail(message)
      }
      core?.onAudio = { [weak self] id, data in
        guard let self, let active = self.active, !active.cancelled, active.coreID == id else {
          return
        }
        do { try self.speech.consume(data, id: id) } catch { self.fail(error.localizedDescription) }
      }
      library.connect(core)
      if let core {
        vocabularyBridge = VocabularyBridge(control: { [weak self, weak core] action in
          guard let core, self?.core === core else { throw VocabularyBridgeError.stale }
          let data = try await core.control(action)
          guard self?.core === core else { throw VocabularyBridgeError.stale }
          return data
        }, mailbox: mailbox)
        refreshVocabularyBridge()
      }
      developer.connect(core)
      developer.onConfigurationChange = { [weak self] affectsSpeech in
        if affectsSpeech { self?.invalidateSpeechPreparation() }
        self?.refreshHistory()
      }
      if developerMode { await developer.refresh() }
      coreReady = true
      recoverInterruptedHostActivation()
      await refreshKeyboardCapabilities()
      snapshot.message = "准备识别后即可在应用内试用"
      refreshHistory()
      drainCommands()
      presentPendingHostActivation()
      scheduleAutomaticHostActivation()
      publish(force: true)
    } catch {
      errorMessage = error.localizedDescription
      snapshot.message = error.localizedDescription
    }
  }

  func prepareSpeech(allowDownload: Bool, activateKeyboardSession: Bool = true) async {
    guard !Task.isCancelled, active == nil, !preparing, !restarting else { return }
    let revision = speechPreparationRevision
    preparing = true
    keyboardPreparationPending = false
    needsDownload = false
    errorMessage = nil
    publish(force: true)
    defer {
      preparing = false
      if revision != speechPreparationRevision { scheduleKeyboardPreparation() }
      scheduleAutomaticHostActivation()
      publish(force: true)
    }
    do {
      guard let provider = currentProviderType else { throw DeveloperError.rejected("请先选择 ASR 提供商。") }
      if provider == "omni", developer.configuration?.credentialsStatus.omniConfigured != true {
        throw DeveloperError.rejected("请先配置全模态提供商和模型。")
      }
      try await speech.prepare(provider: provider, allowDownload: allowDownload)
      // Provider changes may arrive while an
      // actor loads the model. Never publish the old preparation as ready.
      guard revision == speechPreparationRevision, !Task.isCancelled else { return }
      speechPrepared = true
      snapshot.phase = .ready
      snapshot.message = "语音已就绪"
      if activateKeyboardSession, systemRecordingAdmission == nil {
        for _ in 0..<40 {
          if inForeground && UIApplication.shared.applicationState == .active { break }
          guard !Task.isCancelled else { throw CancellationError() }
          try await Task.sleep(for: .milliseconds(50))
        }
        do {
          try await keyboardAudio.activate()
          if preferences.backgroundKeepAlive {
            keepAlive.arm()
            keepAlive.startIfPossible()
          }
        }
        catch { snapshot.message = "语音已就绪；请重新启用键盘语音。" }
      }
    } catch VoiceFailure.assetsMissing {
      guard revision == speechPreparationRevision else { return }
      needsDownload = true
      speechPrepared = false
      snapshot.message = "需要下载系统语音资源"
    } catch {
      guard revision == speechPreparationRevision else { return }
      errorMessage = error.localizedDescription
      snapshot.message = error.localizedDescription
      speechPrepared = false
    }
  }

  private func invalidateSpeechPreparation() {
    speechPreparationRevision = UUID()
    keyboardPreparationPending = speechPrepared || preparing || keyboardPreparationPending
    speechPrepared = false
    publish(force: true)
    scheduleKeyboardPreparation()
  }

  private func scheduleKeyboardPreparation() {
    guard keyboardPreparationPending, keyboardPreparationTask == nil, !speechPrepared,
      !preparing, !restarting, active == nil, coreReady, keyboardAudio.isReady else { return }
    let revision = speechPreparationRevision
    keyboardPreparationTask = Task { [weak self] in
      guard let self else { return }
      defer {
        self.keyboardPreparationTask = nil
        if revision != self.speechPreparationRevision { self.scheduleKeyboardPreparation() }
      }
      guard !Task.isCancelled, revision == self.speechPreparationRevision, !self.restarting else { return }
      // This refresh owns no capture engine and does not reactivate audio.
      // A language switch therefore keeps the already activated background
      // session usable instead of leaving canStart=false until another jump.
      await self.prepareSpeech(allowDownload: false, activateKeyboardSession: false)
    }
  }

  func toggleKeyboardVoice() async {
    guard active == nil, !preparing, !restarting, inForeground else { return }
    cancelDeferredKeyboardStart()
    if keyboardAudio.hasActivationIntent {
      keepAlive.stop()
      keyboardAudio.deactivate()
      publish(force: true)
    }
    else { await prepareSpeech(allowDownload: false) }
  }

  func registerVersionTap() -> Bool {
    let now = ProcessInfo.processInfo.systemUptime
    if now - lastVersionTap > 1.5 { versionTapCount = 0 }
    lastVersionTap = now
    versionTapCount += 1
    if versionTapCount >= 3 { versionTapCount = 0; return true }
    return false
  }
  func reloadDeveloperMode(_ enabled: Bool) async {
    guard active == nil, systemRecordingAdmission == nil, !preparing, !restarting, !developer.busy, !library.busy else {
      errorMessage = "请先结束录音、识别或提供商请求。"; return
    }
    restarting = true
    cancelDeferredKeyboardStart()
    defer { restarting = false }
    startTask?.cancel()
    let previousKeyboardPreparation = keyboardPreparationTask
    previousKeyboardPreparation?.cancel(); keyboardPreparationTask = nil
    speechPreparationRevision = UUID(); keyboardPreparationPending = false
    capabilityTask?.cancel(); previewExpiryTask?.cancel()
    capabilityRevision = UUID()
    let pendingNativeTasks = Array(nativeTasks.values)
    for task in pendingNativeTasks { task.cancel() }
    for task in pendingNativeTasks { await task.value }
    nativeTasks.removeAll()
    // Drain a queued preparation before releasing its recognizer or replacing
    // Core. Its cancelled continuation must not mutate the replacement runtime.
    await previousKeyboardPreparation?.value
    keepAlive.stop()
    keyboardAudio.deactivate()
    await speech.release()
    if let core { await core.close() }
    core = nil
    insertionDelivery = KeyboardInsertionDelivery()
    developer.connect(nil)
    library.connect(nil)
    vocabularyBridge?.clearPublishedState(); vocabularyBridge = nil
    SharedContainer.post(SharedContainer.stateNotification)
    coreReady = false; speechPrepared = false; errorMessage = nil
    developerMode = enabled
    UserDefaults.standard.set(enabled, forKey: "ol.ios.developerMode")
    snapshot = VoiceSnapshot(hostEpoch: UUID())
    await loadCore()
    runtimeID = UUID()
    publish(force: true)
  }

  func foregroundChanged(_ foreground: Bool) {
    NSLog("OPENLESS_VOICE_HANDOFF lifecycle foreground=%d appState=%ld return=%@ %@",
      foreground, UIApplication.shared.applicationState.rawValue,
      automaticReturnID?.uuidString ?? "nil", keyboardAudio.diagnosticReport)
    inForeground = foreground
    keyboardAudio.foregroundChanged()
    if preferences.backgroundKeepAlive, keyboardAudio.hasActivationIntent {
      keepAlive.foregroundChanged(foreground)
    }
    if !foreground, UIApplication.shared.applicationState == .background {
      automaticReturnID = nil; automaticReturnTarget = nil
    }
    if foreground {
      presentPendingHostActivation()
      scheduleAutomaticHostActivation()
      // The first PCM callback can arrive during scene activation. Recheck on
      // activation as well, so return does not depend on another level event.
      returnToKeyboardAfterCaptureStarted()
    }
    if !foreground, active?.coreFinished == true { cancel() }
    else if !foreground, active != nil, !activeBackgroundAudio, systemRecordingAdmission != active?.client,
      !keyboardAudio.admits(active?.audioGeneration) {
      if active?.stopping == true { beginProcessingBackgroundTime() }
      else { stop() }
    }
    publish(force: true)
  }
  func start() { begin(client: UUID(), target: nil, language: preferences.language) }

  /// Invoked only by the system's user-operated recording control. The intent
  /// stays alive until real capture arrives; a successful command enqueue is
  /// not evidence that iOS admitted microphone access.
  @available(iOS 18.0, *)
  func setSystemRecording(_ enabled: Bool, sessionID: UUID? = nil) async throws {
    if !enabled {
      if let sessionID, active?.client != sessionID, systemRecordingAdmission != sessionID { return }
      if active != nil { stop() }
      else if systemRecordingAdmission != nil {
        systemRecordingAdmission = nil
        updateHostActivation(phase: .cancelled)
        endRecordingActivity()
      }
      return
    }
    guard active == nil, deferredKeyboardCommand == nil,
      systemRecordingAdmission == nil, !preparing, !restarting else {
      throw SystemRecordingError.unavailable
    }
    guard MicrophoneAccess.isGranted else { throw SystemRecordingError.permissionRequired }
    let reservation = UUID()
    systemRecordingAdmission = reservation
    defer {
      if systemRecordingAdmission == reservation { systemRecordingAdmission = nil }
    }
    await startTask?.value
    try Task.checkCancellation()
    guard systemRecordingAdmission == reservation, active == nil, coreReady,
      let mailbox else { throw SystemRecordingError.unavailable }
    if currentProviderType == "apple-speech", SFSpeechRecognizer.authorizationStatus() != .authorized {
      throw SystemRecordingError.permissionRequired
    }
    await refreshKeyboardCapabilities()
    try Task.checkCancellation()
    guard systemRecordingAdmission == reservation, active == nil else { throw CancellationError() }
    let request: HostVoiceActivation
    if let pending = try mailbox.value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey),
      pending.isValid(), pending.phase == .requested {
      request = pending
    } else {
      request = HostVoiceActivation(target: nil, language: preferences.language)
      try mailbox.requestHostVoice(request)
    }
    guard snapshot.capabilities?.allows(request.intent) == true else {
      throw SystemRecordingError.unavailable
    }
    hostActivation = request
    await prepareSpeech(allowDownload: false, activateKeyboardSession: false)
    try Task.checkCancellation()
    guard systemRecordingAdmission == reservation, active == nil else { throw CancellationError() }
    guard speechPrepared else {
      updateHostActivation(phase: .failed, message: snapshot.message)
      throw SystemRecordingError.startFailed
    }
    do {
      recordingActivity = try SystemRecordingActivitySession(sessionID: request.id) { [weak self] in
        guard let self, self.active?.client == request.id else { return }
        self.stop()
      }
      recordingActivityOwner = request.id
      systemRecordingAdmission = request.id
      begin(client: request.id, target: nil, language: request.language, activation: request)
      guard active?.client == request.id else { throw SystemRecordingError.startFailed }
      updateHostActivation(phase: .starting)
      let deadline = ContinuousClock.now.advanced(by: .seconds(8))
      while active?.client == request.id, active?.cancelled == false, active?.stopping == false,
        !snapshot.captureReady, ContinuousClock.now < deadline {
        try await Task.sleep(for: .milliseconds(50))
      }
      guard active?.client == request.id, snapshot.captureReady, snapshot.phase == .recording else {
        throw SystemRecordingError.startFailed
      }
      systemRecordingAdmission = nil
    } catch {
      if active?.client == request.id, active?.cancelled == false, active?.stopping == false {
        fail(error.localizedDescription)
      } else if active == nil, hostActivation?.id == request.id,
        ![.cancelled, .processing, .ready, .consumed].contains(hostActivation?.phase) {
        updateHostActivation(phase: .failed, message: error.localizedDescription)
      }
      endRecordingActivity(ownedBy: request.id)
      throw error
    }
  }

  private func endRecordingActivity(ownedBy owner: UUID? = nil) {
    if let owner, recordingActivityOwner != owner, systemRecordingAdmission != owner { return }
    if owner == nil || systemRecordingAdmission == owner { systemRecordingAdmission = nil }
    if let owner, recordingActivityOwner != owner { return }
    recordingActivity?.finish()
    recordingActivity = nil
    recordingActivityOwner = nil
  }

  private func begin(client: UUID, target: InputTarget?, language: KeyboardLanguage,
    command: VoiceCommand? = nil, activation: HostVoiceActivation? = nil) {
    guard active == nil, deferredKeyboardCommand == nil, coreReady, !restarting,
      systemRecordingAdmission == nil || systemRecordingAdmission == client, let core else { return }
    guard speechPrepared, !preparing else {
      if let command, preparing || keyboardPreparationPending { deferKeyboardStart(command) }
      return
    }
    guard !core.hasPendingControls else {
      if let command { deferKeyboardStart(command); return }
      snapshot.sessionID = client; snapshot.phase = .error
      snapshot.message = "配置正在更新，请稍后重试。"; publish(force: true); return
    }
    var frozenStart: KeyboardVoiceStart?
    if let command, let intent = command.intent {
      guard let capabilities = snapshot.capabilities, capabilities.isValid,
        command.capabilityRevision == capabilities.revision,
        command.sourceLanguage == capabilities.sourceLanguage,
        capabilities.allows(intent) else {
        snapshot.sessionID = client; snapshot.phase = .error
        snapshot.message = "语音配置已变更，请重新选择。"; publish(force: true); return
      }
      frozenStart = KeyboardVoiceStart(intent: intent, capabilities: capabilities, language: language)
    }
    guard inForeground || systemRecordingAdmission == client || backgroundVoiceReady else {
      snapshot.phase = .inactive
      snapshot.message = "请打开 OpenLess，或使用控制中心的录音按钮。"
      publish(force: true)
      return
    }
    if let activation {
      frozenStart = KeyboardVoiceStart(intent: activation.intent, capabilities: snapshot.capabilities, language: language)
    }
    let deferredSpace = command?.deferredSpaceVoice == true
    if deferredSpace {
      guard target != nil, activation == nil, frozenStart?.intent.kind == .dictation,
        snapshot.capabilities?.supportsSpaceVoice == true else { return }
    }
    active = ActiveSession(client: client, target: target, language: language, voiceStart: frozenStart,
      activationID: activation?.id, systemAuthorized: systemRecordingAdmission == client,
      audioGeneration: keyboardAudio.generation)
    if deferredSpace {
      active?.spaceDelivery = SpaceVoiceDeliveryState()
      if snapshot.capabilities?.canTranslate == true {
        active?.spaceTranslationTargets = Set(snapshot.capabilities?.translationTargets.map(\.id) ?? [])
      }
    }
    snapshot.activeIntent = frozenStart?.intent
    snapshot.generatedPreview = nil
    if let target {
      keyboardLease = KeyboardLease(sessionID: client, target: target)
      leaseTask?.cancel()
      leaseTask = Task { [weak self] in
        while !Task.isCancelled {
          try? await Task.sleep(for: .seconds(1))
          guard !Task.isCancelled, let self, self.active?.client == client,
            self.active?.stopping == false, self.active?.cancelled == false
          else { return }
          if self.keyboardLease?.expired() == true {
            self.stop()
            self.snapshot.message = "键盘连接已中断，录音已停止"
            self.publish(force: true)
            return
          }
        }
      }
    }
    snapshot.sessionID = client
    snapshot.phase = .starting
    snapshot.captureReady = false
    snapshot.preview = ""
    snapshot.insertion = nil
    snapshot.level = 0
    snapshot.elapsedMilliseconds = 0
    snapshot.message = "正在准备麦克风"
    resultText = ""
    lastPreview = ""
    errorMessage = nil
    publish(force: true)
    var request: [String: Any] = ["operation": "start", "clientSessionID": client.uuidString, "insert": target != nil && !deferredSpace]
    if let intent = frozenStart?.intent,
      let data = try? JSONEncoder().encode(intent),
      let value = try? JSONSerialization.jsonObject(with: data) { request["intent"] = value }
    if !core.command(request) {
      active = nil
      snapshot.phase = .error
      snapshot.message = "无法开始录音"
      publish(force: true)
    }
  }
  private func deferKeyboardStart(_ command: VoiceCommand) {
    guard let client = command.sessionID, deferredKeyboardCommand == nil else { return }
    deferredKeyboardCommand = command
    keyboardLease = KeyboardLease(sessionID: client, target: command.target)
    snapshot.sessionID = client; snapshot.phase = .starting; snapshot.captureReady = false
    snapshot.message = "正在准备…"
    NSLog("OPENLESS_START_DEFERRED request=%@", command.requestID.uuidString)
    publish(force: true)
    let deadline = ProcessInfo.processInfo.systemUptime + 4
    deferredKeyboardTask = Task { [weak self] in
      while !Task.isCancelled {
        do { try await Task.sleep(for: .milliseconds(50)) } catch { return }
        guard let self, self.deferredKeyboardCommand?.requestID == command.requestID else { return }
        guard ProcessInfo.processInfo.systemUptime < deadline,
          command.rejection(now: Date(), epoch: self.snapshot.hostEpoch) == nil,
          self.keyboardLease?.expired() == false,
          self.inForeground || self.backgroundVoiceReady else {
          self.cancelDeferredKeyboardStart(failed: true); return
        }
        guard self.active == nil, self.coreReady, !self.restarting,
          self.systemRecordingAdmission == nil || self.systemRecordingAdmission == client else {
          self.cancelDeferredKeyboardStart(failed: true); return
        }
        if self.preparing || self.keyboardPreparationPending { continue }
        guard self.speechPrepared else { self.cancelDeferredKeyboardStart(failed: true); return }
        guard let core = self.core, !core.hasPendingControls else { continue }
        self.deferredKeyboardCommand = nil
        self.deferredKeyboardTask = nil
        self.keyboardLease = nil
        NSLog("OPENLESS_START_DEFERRED_READY request=%@", command.requestID.uuidString)
        self.begin(client: client, target: command.target, language: command.language, command: command)
        return
      }
    }
  }
  private func cancelDeferredKeyboardStart(failed: Bool = false) {
    guard deferredKeyboardCommand != nil else { return }
    deferredKeyboardCommand = nil
    deferredKeyboardTask?.cancel(); deferredKeyboardTask = nil
    keyboardLease = nil
    snapshot.captureReady = false
    snapshot.phase = failed ? .error : .cancelled
    snapshot.message = failed ? "语音仍在处理中，请稍后再试。" : "已取消"
    publish(force: true)
  }
  func stop() {
    if deferredKeyboardCommand != nil { cancelDeferredKeyboardStart(); return }
    guard var active, !active.stopping, !active.cancelled else { return }
    if active.spaceDelivery != nil && active.spaceDelivery?.decision == nil {
      // A lease timeout or system interruption cannot invent a release choice.
      cancel(); return
    }
    clearDeadlines()
    if snapshot.phase == .starting && !snapshot.captureReady {
      cancel()
      return
    }
    active.stopping = true
    self.active = active
    if !inForeground { beginProcessingBackgroundTime() }
    if active.activationID != nil { updateHostActivation(phase: .processing) }
    capture.stopHardware()
    endRecordingActivity()
    snapshot.captureReady = false
    snapshot.level = 0
    snapshot.phase = .processing
    snapshot.message = "正在完成识别"
    publish(force: true)
    core?.command(["operation": "stop", "clientSessionID": active.client.uuidString])
  }
  func cancel() {
    spaceProcessingTask?.cancel(); spaceProcessingTask = nil
    endProcessingBackgroundTime()
    if deferredKeyboardCommand != nil { cancelDeferredKeyboardStart(); return }
    guard var active, !active.cancelled else { return }
    previewExpiryTask?.cancel(); previewExpiryTask = nil
    if active.activationID != nil { updateHostActivation(phase: .cancelled) }
    if let insertion = snapshot.insertion { try? mailbox?.revokeInsertion(insertion.id) }
    if let mailbox, let resolutions = try? insertionDelivery.detach(sessionID: active.client, mailbox: mailbox) {
      resolutions.forEach(resolveKeyboardInsertion)
    }
    snapshot.insertion = nil; snapshot.generatedPreview = nil
    if active.coreFinished {
      finishKeyboardDelivery(sessionID: active.client)
      self.active = nil; snapshot.phase = .cancelled; snapshot.message = "已取消"
      clearDeadlines(); refreshHistory(); publish(force: true); return
    }
    clearDeadlines()
    active.cancelled = true
    self.active = active
    capture.stopHardware()
    endRecordingActivity()
    for task in nativeTasks.values { task.cancel() }
    snapshot.captureReady = false
    snapshot.level = 0
    snapshot.phase = .cancelled
    snapshot.message = "已取消"
    publish(force: true)
    core?.command(["operation": "cancel", "clientSessionID": active.client.uuidString])
    let id = active.coreID
    Task { await speech.cancel(id: id) }
  }
  private func fail(_ message: String) {
    errorMessage = message
    if !lastPreview.isEmpty { resultText = lastPreview }
    cancel()
    snapshot.phase = .error
    snapshot.message = message
    publish(force: true)
  }
  func refreshVocabularyBridge() {
    guard active == nil, let vocabularyBridge, !vocabularyBridge.isBusy else { return }
    Task {
      do { try await vocabularyBridge.refresh() }
      catch { errorMessage = "词典共享暂不可用，请刷新后重试。" }
      SharedContainer.post(SharedContainer.stateNotification)
    }
  }
  func confirmVocabularyEntry(id: String, phrase: String) async -> Bool {
    guard active == nil, let vocabularyBridge else { return false }
    do {
      try await vocabularyBridge.confirmEntry(id: id, phrase: phrase)
      SharedContainer.post(SharedContainer.stateNotification)
      return true
    } catch {
      errorMessage = "词条可能已保存，请刷新后确认键盘词典状态。"
      SharedContainer.post(SharedContainer.stateNotification)
      return false
    }
  }

  func refreshHistory() {
    scheduleKeyboardPreparation()
    refreshVocabularyBridge()
    Task {
      await library.refresh()
      if snapshot.capabilities == nil, active == nil { await refreshKeyboardCapabilities() }
    }
    pending = (try? mailbox?.pendingInsertions()) ?? []
  }
  func discardPending(_ id: UUID) {
    do {
      try mailbox?.discardInsertion(id)
      refreshHistory()
    } catch { errorMessage = error.localizedDescription }
  }
  func collectDiagnostics() { diagnostics = keyboardAudio.diagnosticReport + "\n\n" + keepAlive.collectDiagnostics() }
  func stopExperimentalSession() {
    cancelDeferredKeyboardStart()
    keepAlive.stop()
    keyboardAudio.deactivate()
    if active != nil { stop() }
    publish(force: true)
  }

  private func drainCommands() {
    guard let mailbox else { return }
    do {
      for _ in 0..<4 {
        let commands = try mailbox.claimCommands()
        if commands.isEmpty { break }
        for command in commands {
          if let reason = command.rejection(now: Date(), epoch: snapshot.hostEpoch) {
            if active == nil { snapshot.message = reason }
            continue
          }
          switch command.kind {
          case .ping:
            _ = keyboardLease?.observe(command)
            try detachPreviousKeyboardDelivery(for: command, mailbox: mailbox)
            scheduleKeyboardPreparation()
            if active == nil, snapshot.capabilities == nil {
              capabilityTask?.cancel()
              capabilityTask = Task { [weak self] in await self?.refreshKeyboardCapabilities() }
            }
          case .prepare:
            guard inForeground else {
              snapshot.message = "请在 OpenLess 内完成首次语音准备。"; continue
            }
            Task { await self.prepareSpeech(allowDownload: false) }
          case .start:
            if let client = command.sessionID {
              begin(client: client, target: command.target, language: command.language, command: command)
            }
          case .stop:
            if deferredKeyboardCommand?.sessionID == command.sessionID,
              deferredKeyboardCommand?.target == command.target {
              cancelDeferredKeyboardStart(); continue
            }
            if active?.client == command.sessionID,
              active?.target == command.target || active?.activationID == command.sessionID {
              if active?.spaceDelivery != nil {
                guard let choice = command.spaceVoiceFinish, choice.isValid,
                  choice.kind != .translate,
                  active?.spaceDelivery?.choose(choice) == true else { continue }
              }
              if command.spaceVoiceFinish?.kind == .chooseTranslation,
                active?.spaceTranslationTargets.isEmpty == true { cancel(); continue }
              stop()
              completeSpaceVoiceIfReady()
            }
          case .finishSpaceTranslation:
            guard active?.client == command.sessionID, active?.target == command.target,
              let choice = command.spaceVoiceFinish, choice.kind == .translate, choice.isValid,
              let language = choice.targetLanguage,
              active?.spaceTranslationTargets.contains(language) == true,
              command.capabilityRevision == active?.voiceStart?.capabilityRevision,
              active?.spaceDelivery?.choose(choice) == true else { continue }
            completeSpaceVoiceIfReady()
          case .cancel:
            if deferredKeyboardCommand?.sessionID == command.sessionID,
              deferredKeyboardCommand?.target == command.target {
              cancelDeferredKeyboardStart(); continue
            }
            if active?.client == command.sessionID,
              active?.target == command.target || active?.activationID == command.sessionID { cancel() }
          case .vocabularyDecision:
            guard active == nil, let decision = command.vocabularyDecision, let vocabularyBridge else { continue }
            Task {
              do { try await vocabularyBridge.apply(decision) }
              catch { errorMessage = "纠正建议已变更或无法保存，请刷新后重试。" }
              SharedContainer.post(SharedContainer.stateNotification)
            }
          case .confirmGeneratedPreview:
            confirmGeneratedPreview(command)
          case .insertionReceipt:
            if let current = active, current.coreFinished,
              let id = current.generatedInsertionID, command.insertionID == id,
              command.sessionID == current.client, command.target == current.target,
              let record = try mailbox.insertion(id) {
              previewExpiryTask?.cancel(); previewExpiryTask = nil
              let submitted = record.state == .submitted && command.receipt == .submitted
              snapshot.phase = submitted ? .done : .error
              snapshot.message = submitted ? "已提交到当前输入框" : "写入结果未确认，文字保留待核对。"
              snapshot.insertion = nil; snapshot.generatedPreview = nil
              finishKeyboardDelivery(sessionID: current.client)
              active = nil; clearDeadlines(); refreshHistory()
              continue
            }
            if let resolution = try insertionDelivery.receive(command, mailbox: mailbox) {
              resolveKeyboardInsertion(resolution)
            }
          }
        }
      }
      publish(force: true)
    } catch { errorMessage = error.localizedDescription }
  }

  private func resolveKeyboardInsertion(_ resolution: KeyboardInsertionDelivery.Resolution) {
    if resolution.submitted { core?.reply(resolution.requestID, value: ["status": "submitted"]) }
    else { core?.reply(resolution.requestID, error: "输入目标已改变，文字保留待确认") }
  }

  private func detachPreviousKeyboardDelivery(for command: VoiceCommand, mailbox: Mailbox) throws {
    guard let current = active, current.target != nil,
      let resolutions = try insertionDelivery.observeKeyboardOwnership(command, sessionID: current.client,
        startedAt: current.createdAt, recordingHasEnded: current.stopping || current.coreFinished,
        mailbox: mailbox) else { return }
    resolutions.forEach(resolveKeyboardInsertion)
    if current.coreFinished { cancel(); return }
    if snapshot.insertion?.sessionID == current.client { snapshot.insertion = nil }
    pending = try mailbox.pendingInsertions()
  }

  private func finishKeyboardDelivery(sessionID: UUID?) {
    guard let sessionID, let mailbox else { return }
    try? insertionDelivery.finish(sessionID: sessionID, mailbox: mailbox)
  }

  private func handle(_ event: CoreEvent) {
    switch event.kind {
    case "preview":
      guard let id = event.sessionID, active?.coreID == id, active?.cancelled == false else { return }
      snapshot.preview = String((event.text ?? "").suffix(1024))
      if event.stage == "llm" { snapshot.message = "正在进行 LLM 后处理" }
      publish()
    case "nativeRequest":
      guard let request = event.requestID else { return }
      nativeTasks[request] = Task { [weak self] in
        guard let self else { return }
        defer { self.nativeTasks[request] = nil }
        await self.nativeRequest(event)
      }
    case "abortNative":
      if event.sessionID == active?.coreID {
        capture.stopHardware()
        if let request = event.requestID { nativeTasks[request]?.cancel() }
        let id = event.sessionID
        Task { await speech.cancel(id: id) }
      }
    case "started":
      if let active, event.clientSessionID?.lowercased() == active.client.uuidString.lowercased() {
        self.active?.coreID = event.sessionID
      }
    case "level":
      guard let active, !active.cancelled, !active.stopping, event.sessionID == active.coreID else {
        return
      }
      startupDeadline?.cancel()
      startupDeadline = nil
      snapshot.captureReady = true
      snapshot.phase = .recording
      if active.activationID != nil, hostActivation?.phase == .starting {
        updateHostActivation(phase: .recording)
      }
      recordingActivity?.didCaptureAudio()
      snapshot.level = event.level ?? 0
      snapshot.elapsedMilliseconds = event.elapsedMs ?? 0
      let seconds = Int(snapshot.elapsedMilliseconds / 1000)
      snapshot.message =
        seconds >= SharedContract.maxRecordingSeconds - 30
        ? "还剩 \(SharedContract.maxRecordingSeconds - seconds) 秒，到限自动停止" : "正在录音"
      if seconds >= SharedContract.maxRecordingSeconds { stop() } else { publish() }
      returnToKeyboardAfterCaptureStarted()
    case "completed":
      guard event.clientSessionID?.lowercased() == active?.client.uuidString.lowercased() else {
        return
      }
      if active?.spaceDelivery != nil && active?.coreFinished == true { return }
      finishKeyboardDelivery(sessionID: active?.client)
      guard active?.cancelled == false else {
        clearDeadlines(); endProcessingBackgroundTime(); capture.stopHardware(); endRecordingActivity()
        active = nil; snapshot.phase = .cancelled; snapshot.captureReady = false; snapshot.level = 0
        snapshot.generatedPreview = nil; snapshot.insertion = nil; snapshot.message = "已取消"
        refreshHistory(); publish(force: true); return
      }
      clearDeadlines()
      endProcessingBackgroundTime()
      capture.stopHardware()
      endRecordingActivity()
      if active?.spaceDelivery != nil {
        active?.coreFinished = true
        snapshot.captureReady = false
        snapshot.level = 0
        active?.spaceDelivery?.recognized(event.result?.polishedText ?? "")
        resultText = event.result?.polishedText ?? ""
        snapshot.preview = ""
        snapshot.phase = .processing
        snapshot.message = "thinking"
        previewExpiryTask?.cancel()
        let client = active?.client
        previewExpiryTask = Task { [weak self] in
          do { try await Task.sleep(for: .seconds(180)) } catch { return }
          guard let self, self.active?.client == client else { return }
          self.cancel()
        }
        completeSpaceVoiceIfReady()
        publish(force: true)
        return
      }
      if active?.activationID != nil {
        finishHostActivation(event)
        return
      }
      if event.outputDisposition == "preview" {
        receiveGeneratedPreview(event)
        return
      }
      if active?.voiceStart?.intent.kind == .helpWrite {
        snapshot.phase = .error; snapshot.message = "帮写未返回有效预览，未插入文字。"
        active = nil; publish(force: true); return
      }
      resultText = event.result?.polishedText ?? snapshot.preview
      snapshot.preview = resultText
      snapshot.phase = .done
      snapshot.captureReady = false
      snapshot.level = 0
      snapshot.message = active?.target == nil ? "识别完成" : "已提交到当前输入框"
      active = nil
      refreshHistory()
      publish(force: true)
    case "cancelled", "failed":
      guard event.clientSessionID?.lowercased() == active?.client.uuidString.lowercased() else {
        return
      }
      finishKeyboardDelivery(sessionID: active?.client)
      clearDeadlines()
      endProcessingBackgroundTime()
      capture.stopHardware()
      endRecordingActivity()
      let id = active?.coreID
      Task { await speech.cancel(id: id) }
      previewExpiryTask?.cancel(); previewExpiryTask = nil
      snapshot.generatedPreview = nil
      if !lastPreview.isEmpty { resultText = lastPreview }
      let wasCancelled = active?.cancelled == true
      if active?.activationID != nil { updateHostActivation(phase: wasCancelled ? .cancelled : .failed, message: event.message) }
      active = nil
      snapshot.captureReady = false
      snapshot.level = 0
      snapshot.phase = wasCancelled ? .cancelled : .error
      snapshot.message = wasCancelled ? "已取消" : (event.message ?? "识别失败，已有文字保留在预览中")
      refreshHistory()
      publish(force: true)
    case "commandError": errorMessage = event.message
    default: break
    }
  }

  private func nativeRequest(_ event: CoreEvent) async {
    guard let request = event.requestID, let operation = event.operation, let id = event.sessionID
    else { return }
    do {
      if ["speechStart", "captureStart"].contains(operation) {
        guard let current = active, !current.cancelled, !Task.isCancelled else {
          throw VoiceFailure.cancelled
        }
        active?.coreID = id
        guard inForeground || (current.systemAuthorized && systemRecordingAdmission == current.client)
          || (backgroundVoiceReady && keyboardAudio.admits(current.audioGeneration)) else {
          throw VoiceFailure.cancelled
        }
        if operation == "captureStart", current.stopping { throw VoiceFailure.cancelled }
      }
      switch operation {
      case "speechStart":
        guard active != nil else { throw VoiceFailure.cancelled }
        guard let provider = event.payload?.provider else { throw ContractError.invalidTransition }
        try await speech.start(
          id: id, provider: provider,
          preview: { [weak self] text in
            guard let self, self.active?.coreID == id, self.active?.cancelled == false else {
              return
            }
            self.snapshot.preview = String(text.suffix(1024))
            self.lastPreview = text
            if Date().timeIntervalSince(self.lastTranscriptPublish) >= 0.1 {
              self.lastTranscriptPublish = Date()
              self.core?.command(["operation": "transcript", "sessionID": id, "text": text])
              self.publish()
            }
          }, failed: { [weak self] message in self?.fail(message) })
        guard active?.coreID == id, active?.cancelled == false, !Task.isCancelled else {
          throw VoiceFailure.cancelled
        }
        core?.reply(request)
      case "captureStart":
        guard let core, let current = active, current.coreID == id, !current.cancelled, !Task.isCancelled else {
          throw VoiceFailure.cancelled
        }
        let admission: NoJumpAudioSession.CaptureAdmission
        if inForeground { admission = .foreground }
        else if current.systemAuthorized, systemRecordingAdmission == current.client { admission = .system }
        else if let generation = current.audioGeneration, keepAlive.isReady {
          admission = .keyboard(generation)
        }
        else { throw VoiceFailure.cancelled }
        try capture.start(
          sessionID: id, admission: admission,
          feed: { [weak self] data in
            if core.feed(data, session: id) < 0 {
              Task { @MainActor in
                guard self?.active?.coreID == id else { return }
                self?.fail("音频管线发生错误，已停止录音")
              }
            }
          },
          failed: { [weak self] message in
            guard self?.active?.coreID == id else { return }
            self?.fail(message)
          })
        startupDeadline?.cancel()
        startupDeadline = Task { [weak self] in
          try? await Task.sleep(for: .seconds(5))
          guard !Task.isCancelled, let self, self.active?.coreID == id,
            !self.snapshot.captureReady
          else { return }
          self.fail("未收到麦克风音频，已停止录音。请检查输入设备后重试。")
        }
        durationDeadline?.cancel()
        durationDeadline = Task { [weak self] in
          try? await Task.sleep(for: .seconds(SharedContract.maxRecordingSeconds))
          guard !Task.isCancelled, self?.active?.coreID == id else { return }
          self?.stop()
        }
        core.reply(request)
      case "captureStop":
        await capture.stopAndDrain(sessionID: id)
        if active?.coreID == id { endRecordingActivity() }
        core?.reply(request)
      case "speechFinish":
        let output = try await speech.finish(id: id)
        core?.reply(request, value: ["text": output.text, "durationMs": output.duration])
      case "speechCancel":
        await capture.stopAndDrain(sessionID: id)
        if active?.coreID == id { endRecordingActivity() }
        await speech.cancel(id: id)
        core?.reply(request)
      case "insert":
        guard let active, active.spaceDelivery == nil, active.coreID == id, !active.cancelled, !Task.isCancelled, let target = active.target,
          let text = event.payload?.text, let mailbox
        else { throw VoiceFailure.cancelled }
        let insertion = InsertionRequest(sessionID: active.client, target: target, text: text)
        if let resolution = try insertionDelivery.offer(insertion, requestID: request, mailbox: mailbox) {
          resolveKeyboardInsertion(resolution)
          pending = try mailbox.pendingInsertions()
        } else {
          snapshot.insertion = insertion
          snapshot.phase = .awaitingInsertion
          snapshot.message = "正在提交文字"
        }
        publish(force: true)
      // Reply is driven by the keyboard's durable receipt, not a timer
      // pretending the target accepted the text.
      default: throw ContractError.invalidTransition
      }
    } catch {
      if ["speechStart", "captureStart", "speechFinish"].contains(operation) {
        capture.stopHardware(sessionID: id)
        NSLog("OPENLESS_AUDIO_CAPTURE_START_FAILED operation=%@ session=%@ error=%@ %@",
          operation, id, error.localizedDescription, keyboardAudio.diagnosticReport)
        if active?.coreID == id { endRecordingActivity() }
        await speech.cancel(id: id)
        if operation == "speechStart", !(error is CancellationError) {
          if case VoiceFailure.cancelled = error {} else { speechPrepared = false }
        }
      }
      core?.reply(request, error: error.localizedDescription)
    }
  }

  private func beginProcessingBackgroundTime() {
    guard processingBackgroundTask == .invalid else { return }
    processingBackgroundTask = UIApplication.shared.beginBackgroundTask(withName: "Finish requested dictation") { [weak self] in
      Task { @MainActor in
        guard let self else { return }
        self.cancel()
        self.endProcessingBackgroundTime()
        self.snapshot.message = "系统后台处理时间已结束，请在主应用继续。"
        self.publish(force: true)
      }
    }
  }
  private func endProcessingBackgroundTime() {
    let task = processingBackgroundTask
    processingBackgroundTask = .invalid
    if task != .invalid { UIApplication.shared.endBackgroundTask(task) }
  }

  private func recoverInterruptedHostActivation() {
    guard active == nil, let mailbox,
      var request = try? mailbox.value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey),
      [.starting, .recording, .processing].contains(request.phase) else { return }
    request.phase = .failed
    request.message = "主应用会话已更新，录音已中断，请重新开始。"
    try? mailbox.updateHostVoice(request)
  }

  private func presentPendingHostActivation() {
    guard inForeground, active == nil, let mailbox else { return }
    do {
      guard let request = try mailbox.value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey),
        request.isValid() else { hostActivation = nil; return }
      // The keyboard may have consumed A and created B while this App was away.
      // Reconcile with the durable request instead of retaining A's old sheet.
      if request.phase == .requested {
        hostActivation = request
      } else if hostActivation?.id == request.id,
        [.ready, .failed].contains(request.phase) || (request.phase == .starting && automaticActivationTask != nil) {
        hostActivation = request
      } else {
        hostActivation = nil
      }
    } catch {
      errorMessage = "无法读取键盘录音请求，请稍后重试。"
    }
  }
  func receiveHostActivation(_ url: URL, sourceApplication: String? = nil) {
    guard url.scheme == "openless-ios", url.host == "activate",
      let value = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first(where: { $0.name == "request" })?.value,
      let id = UUID(uuidString: value), let mailbox,
      let request = try? mailbox.value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey),
      request.id == id, request.isValid(), request.phase == .requested else { return }
    hostActivation = request
    if request.permitsAutomaticStart(from: url) {
      automaticActivationID = request.id
      automaticReturnStarted = false
      automaticReturnTask?.cancel(); automaticReturnTask = nil
      let source = request.sourceApplication ?? sourceApplication
      let ownBundle = Bundle.main.bundleIdentifier ?? "app.openless.ios"
      let destination = source.flatMap {
        $0 == ownBundle || $0.hasPrefix(ownBundle + ".") ? nil : $0
      }
      automaticReturnTarget = OLHostApplicationCaptureReturnTarget(destination)
      NSLog("OPENLESS_VOICE_HANDOFF source=%@ requested=%@ target=%@", sourceApplication ?? "nil", request.sourceApplication ?? "nil", automaticReturnTarget?.bundleIdentifier ?? "nil")
      scheduleAutomaticHostActivation()
    }
  }

  private func returnToKeyboardAfterCaptureStarted() {
    guard let id = automaticReturnID, active?.activationID == id,
      hostActivation?.id == id, snapshot.captureReady, snapshot.phase == .recording,
      !automaticReturnStarted, inForeground else { return }
    automaticReturnStarted = true
    NSLog("OPENLESS_VOICE_HANDOFF capture-ready request=%@ source=%@", id.uuidString,
      hostActivation?.sourceApplication ?? "nil")
    scheduleReturnToSource(id)
  }

  private func scheduleReturnToSource(_ id: UUID) {
    guard automaticReturnTask == nil, automaticReturnID == id,
      hostActivation?.id == id, inForeground else { return }
    // Keep this request's destination through preparation, UIKit transitions,
    // and (if needed) result completion. A successful API call is not itself
    // proof that the app left the foreground.
    let destination = automaticReturnTarget?.bundleIdentifier ?? hostActivation?.sourceApplication
    automaticReturnTask = Task { [weak self] in
      guard let self else { return }
      defer { if self.hostActivation?.id == id { self.automaticReturnTask = nil } }
      for (attempt, delay) in [0.0, 0.35, 0.8, 1.5].enumerated() {
        if delay > 0 { try? await Task.sleep(for: .seconds(delay)) }
        guard !Task.isCancelled, self.automaticReturnID == id, self.hostActivation?.id == id,
          ![.cancelled, .failed, .consumed].contains(self.hostActivation?.phase) else { return }
        if UIApplication.shared.applicationState == .background {
          self.automaticReturnID = nil; self.automaticReturnTarget = nil
          return
        }
        guard self.inForeground, UIApplication.shared.applicationState == .active else { continue }
        let target = self.automaticReturnTarget ?? OLHostApplicationCaptureReturnTarget(destination)
        self.automaticReturnTarget = nil
        self.publish(force: true)
        let accepted = target.map { OLHostApplicationReturn($0) } ?? false
        NSLog("OPENLESS_VOICE_HANDOFF return request=%@ attempt=%ld target=%@ accepted=%d appState=%ld",
          id.uuidString, attempt + 1, target?.bundleIdentifier ?? "nil", accepted,
          UIApplication.shared.applicationState.rawValue)
      }
      if UIApplication.shared.applicationState == .background {
        self.automaticReturnID = nil
      } else {
        NSLog("OPENLESS_VOICE_HANDOFF return-not-observed request=%@ destination=%@ appState=%ld",
          id.uuidString, destination ?? "nil", UIApplication.shared.applicationState.rawValue)
        // One further bounded attempt is allowed when the result becomes ready.
        self.automaticReturnTarget = OLHostApplicationCaptureReturnTarget(destination)
      }
    }
  }

  private func scheduleAutomaticHostActivation() {
    guard let id = automaticActivationID, hostActivation?.id == id,
      hostActivation?.phase == .requested, coreReady, inForeground,
      active == nil, !preparing, !restarting, systemRecordingAdmission == nil,
      automaticActivationTask == nil else { return }
    automaticActivationID = nil
    automaticReturnID = id
    automaticActivationTask = Task { [weak self] in
      guard let self else { return }
      defer { self.automaticActivationTask = nil }
      await self.startHostActivation()
    }
  }

  func startHostActivation() async {
    guard let request = hostActivation, request.isValid(), active == nil, systemRecordingAdmission == nil,
      coreReady, inForeground, !preparing, !restarting,
      [.requested, .failed, .cancelled, .ready].contains(request.phase) else { return }
    if request.intent.kind != .dictation || request.intent.stylePackID != "builtin.raw" {
      await refreshKeyboardCapabilities()
      guard snapshot.capabilities?.allows(request.intent) == true else {
        updateHostActivation(phase: .failed, message: "所选语音能力尚未配置，请先设置模型与目标语言。")
        return
      }
    }
    updateHostActivation(phase: .starting)
    await prepareSpeech(allowDownload: false)
    guard !Task.isCancelled, hostActivation?.id == request.id, hostActivation?.phase == .starting,
      active == nil, inForeground else { return }
    guard speechPrepared else {
      automaticReturnID = nil
      updateHostActivation(phase: .failed, message: errorMessage ?? snapshot.message)
      return
    }
    begin(client: request.id, target: nil, language: request.language, activation: request)
    if active?.activationID == request.id { updateHostActivation(phase: .starting) }
    else { updateHostActivation(phase: .failed, message: snapshot.message) }
  }

  func closeHostActivation() {
    automaticActivationID = nil; automaticReturnID = nil; automaticReturnTarget = nil
    automaticReturnTask?.cancel(); automaticReturnTask = nil
    automaticActivationTask?.cancel(); automaticActivationTask = nil
    if active?.activationID != nil {
      cancel()
    } else if hostActivation?.phase == .requested || hostActivation?.phase == .starting {
      // Closing an unstarted request must not present it again next foreground.
      updateHostActivation(phase: .cancelled, message: "已取消")
    }
    hostActivation = nil
  }
  private func updateHostActivation(phase: HostVoiceActivation.Phase, message: String? = nil, text: String? = nil) {
    guard var request = hostActivation else { return }
    request.phase = phase; request.message = message
    if let text { request.text = text }
    if phase == .starting || phase == .recording { request.expiresAt = request.createdAt.addingTimeInterval(Double(SharedContract.maxRecordingSeconds + 300)) }
    if phase == .ready { request.expiresAt = min(request.createdAt.addingTimeInterval(Double(SharedContract.maxRecordingSeconds + 300)), Date().addingTimeInterval(300)) }
    hostActivation = request
    do { try mailbox?.updateHostVoice(request) }
    catch { errorMessage = "无法保存返回键盘的结果，文字仍保留在主应用。" }
    SharedContainer.post(SharedContainer.stateNotification)
  }
  private func finishHostActivation(_ event: CoreEvent) {
    guard let current = active, current.activationID == hostActivation?.id, !current.cancelled else { return }
    let text: String
    if current.voiceStart?.intent.kind == .helpWrite {
      guard event.outputDisposition == "preview", event.intent == current.voiceStart?.intent,
        let preview = event.preview, preview.sessionID == current.coreID, preview.promptKind == "voice_compose" else {
        updateHostActivation(phase: .failed, message: "未收到有效帮写结果。")
        active = nil; snapshot.phase = .error; publish(force: true); return
      }
      text = preview.text
    } else { text = event.result?.polishedText ?? "" }
    resultText = text
    snapshot.preview = String(text.suffix(1024)); snapshot.level = 0; snapshot.captureReady = false
    if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || text.utf8.count > SharedContract.maxTextBytes {
      updateHostActivation(phase: .failed, message: "没有可返回的文字，或结果超过键盘容量。")
      snapshot.phase = .error
    } else {
      updateHostActivation(phase: .ready, message: "返回原应用后，结果会自动输入当前输入框。", text: text)
      snapshot.phase = .done
    }
    snapshot.message = "结果已就绪，将自动输入当前输入框。"
    active = nil; refreshHistory(); publish(force: true)
    if hostActivation?.phase == .ready, let id = automaticReturnID { scheduleReturnToSource(id) }
  }

  private func invalidateKeyboardCapabilities() {
    capabilityRevision = UUID()
    snapshot.capabilities = nil
    capabilityTask?.cancel()
    if active == nil {
      capabilityTask = Task { [weak self] in await self?.refreshKeyboardCapabilities() }
    }
  }

  private func refreshKeyboardCapabilities() async {
    guard let core, coreReady, active == nil else { return }
    let revision = capabilityRevision
    do {
      let data = try await core.control(["action": "capabilitySnapshot"])
      let styles = try await core.control(["action": "styleLibrary"])
      try Task.checkCancellation()
      guard self.core === core, revision == capabilityRevision, active == nil else { return }
      let value = try JSONDecoder().decode(KeyboardCapabilityProjection.self, from: data)
      let library = try JSONDecoder().decode(KeyboardCapabilityProjection.Library.self, from: styles)
      snapshot.capabilities = value.project(styles: library, revision: revision)
      publish(force: true)
    } catch is CancellationError {} catch {
      if self.core === core, revision == capabilityRevision { snapshot.capabilities = nil }
    }
  }

  private func receiveGeneratedPreview(_ event: CoreEvent) {
    guard let current = active, !current.cancelled, let start = current.voiceStart,
      start.intent.kind == .helpWrite, event.intent == start.intent,
      let target = current.target, let output = event.preview,
      output.sessionID == current.coreID, output.promptKind == "voice_compose" else {
      active = nil; snapshot.phase = .error; snapshot.message = "帮写预览已失效，未插入文字。"
      publish(force: true); return
    }
    let now = Date()
    let preview = KeyboardGeneratedPreview(sessionID: current.client, target: target, text: output.text,
      createdAt: now, expiresAt: now.addingTimeInterval(180))
    guard preview.isUsable(sessionID: current.client, target: target) else {
      active = nil; snapshot.phase = .error; snapshot.message = "帮写结果为空或过长，未插入文字。"
      publish(force: true); return
    }
    active?.coreFinished = true
    resultText = output.text
    snapshot.preview = ""; snapshot.insertion = nil; snapshot.generatedPreview = preview
    snapshot.captureReady = false; snapshot.level = 0; snapshot.phase = .awaitingInsertion
    snapshot.message = "请确认帮写结果后插入"
    guard (try? SharedContract.encode(snapshot)) != nil else {
      snapshot.generatedPreview = nil; active = nil; snapshot.phase = .error
      snapshot.message = "帮写结果超过共享容量，文字保留在主应用。"; publish(force: true); return
    }
    previewExpiryTask?.cancel()
    previewExpiryTask = Task { [weak self] in
      do { try await Task.sleep(for: .seconds(180)) } catch { return }
      guard let self, self.active?.client == current.client else { return }
      self.cancel()
      self.snapshot.message = "帮写预览已过期，文字保留在主应用。"
      self.publish(force: true)
    }
    refreshHistory(); publish(force: true)
  }

  private func confirmGeneratedPreview(_ command: VoiceCommand) {
    guard let current = active, current.coreFinished, !current.cancelled,
      current.generatedInsertionID == nil, let start = current.voiceStart,
      let preview = snapshot.generatedPreview, let mailbox,
      preview.acceptsConfirmation(command, start: start, epoch: snapshot.hostEpoch, sessionActive: true)
    else { return }
    let insertion = InsertionRequest(sessionID: current.client, target: preview.target, text: preview.text)
    do {
      try mailbox.storeInsertion(insertion)
      active?.generatedInsertionID = insertion.id
      snapshot.generatedPreview = nil
      snapshot.insertion = insertion
      snapshot.phase = .awaitingInsertion; snapshot.message = "正在提交帮写结果"
      publish(force: true)
    } catch {
      // End the failed confirmation explicitly so the keyboard cannot remain
      // locked on an unchanged preview. Retain the generated result in the App.
      try? mailbox.revokeInsertion(insertion.id)
      previewExpiryTask?.cancel(); previewExpiryTask = nil
      snapshot.generatedPreview = nil; snapshot.insertion = nil
      finishKeyboardDelivery(sessionID: current.client)
      active = nil; snapshot.phase = .error
      snapshot.message = "提交失败，帮写结果保留在主应用，请重新操作。"
      publish(force: true)
    }
  }

  private func completeSpaceVoiceIfReady() {
    guard let current = active, current.coreFinished, !current.cancelled,
      spaceProcessingTask == nil, let target = current.target,
      let work = active?.spaceDelivery?.takeWork(), let core else { return }
    let (choice, source) = work
    guard !source.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
      source.utf8.count <= SharedContract.maxTextBytes else {
      cancel(); snapshot.message = "没有识别到可输入的内容"; publish(force: true); return
    }
    if !inForeground { beginProcessingBackgroundTime() }
    spaceProcessingTask = Task { [weak self] in
      guard let self else { return }
      defer {
        if self.active?.client == current.client {
          self.spaceProcessingTask = nil
          self.endProcessingBackgroundTime()
        }
      }
      do {
        var output = source
        if choice.kind == .translate {
          guard let language = choice.targetLanguage,
            current.spaceTranslationTargets.contains(language) else { throw VoiceFailure.cancelled }
          // A separate, explicit supplied-text operation after ASR completion.
          // Core owns the prompt/provider work; recording's frozen intent stays unchanged.
          let data = try await core.control(["action": "rewrite", "text": source,
            "instruction": "Translate the supplied text into \(language). Return only the translated text, without commentary or quotation marks."])
          struct RewriteResult: Decodable { let text: String }
          output = try JSONDecoder().decode(RewriteResult.self, from: data).text
        }
        try Task.checkCancellation()
        guard self.active?.client == current.client, self.active?.cancelled == false,
          self.active?.target == target, self.core === core, let mailbox = self.mailbox,
          !output.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
          output.utf8.count <= SharedContract.maxTextBytes else { throw VoiceFailure.cancelled }
        let insertion = InsertionRequest(sessionID: current.client, target: target, text: output)
        try mailbox.storeInsertion(insertion)
        self.active?.generatedInsertionID = insertion.id
        self.resultText = output
        self.snapshot.preview = ""
        self.snapshot.insertion = insertion
        self.snapshot.phase = .awaitingInsertion
        self.snapshot.message = "正在提交文字"
        self.publish(force: true)
      } catch {
        guard self.active?.client == current.client else { return }
        self.cancel()
        self.snapshot.phase = .error
        self.snapshot.message = "本次处理未完成，未输入文字；原文保留在主应用。"
        self.publish(force: true)
      }
    }
  }

  private func publish(force: Bool = false) {
    let now = Date()
    guard force || now.timeIntervalSince(lastPublish) >= 0.1 else { return }
    lastPublish = now
    snapshot.updatedAt = now
    snapshot.backgroundReady = activeBackgroundAudio || backgroundVoiceReady
    snapshot.backgroundRecovering = keyboardAudio.hasActivationIntent && keepAlive.isRecovering
    snapshot.inForeground = inForeground
    snapshot.canStart =
      coreReady && speechPrepared && active == nil && deferredKeyboardCommand == nil && core?.hasPendingControls == false
      && !preparing && !restarting && snapshot.capabilities?.isValid == true
      && (inForeground || backgroundVoiceReady)
    do { try mailbox?.set(snapshot, for: SharedContract.snapshotKey) } catch {
      errorMessage = error.localizedDescription
    }
    let recording = snapshot.phase == .recording && snapshot.captureReady
    if publishedControlRecording != recording {
      publishedControlRecording = recording
      if #available(iOS 18.0, *) { ControlCenter.shared.reloadControls(ofKind: "app.openless.recording") }
    }
    SharedContainer.post(SharedContainer.stateNotification)
  }
}
