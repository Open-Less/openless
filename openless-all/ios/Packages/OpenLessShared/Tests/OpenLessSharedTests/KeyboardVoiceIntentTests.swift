import Foundation
import Testing
@testable import OpenLessShared

struct KeyboardVoiceIntentTests {
  private func capabilities(targets: [KeyboardVoiceOption] = [.init(id: "en-US", title: "英语（美国）")],
    styles: [KeyboardVoiceOption] = [.init(id: "builtin.raw", title: "原文"), .init(id: "style.one", title: "简洁")],
    translate: Bool = true, help: Bool = true) -> KeyboardVoiceCapabilities {
    .init(revision: UUID(), sourceLanguage: "auto", translationTargets: targets, styles: styles,
      canTranslate: translate, canHelpWrite: help)
  }
  private func command() -> VoiceCommand {
    .init(kind: .start, sessionID: UUID(), hostEpoch: UUID(),
      target: .init(keyboardID: UUID(), documentID: UUID(), revision: 3), language: .chinese)
  }
  @Test func legacyV2CommandAndSnapshotDecodeWithoutNewFields() throws {
    let original = command()
    let decoded = try SharedContract.decode(VoiceCommand.self, from: SharedContract.encode(original))
    #expect(decoded.schemaVersion == 2)
    #expect(decoded.intent == nil && decoded.capabilityRevision == nil && decoded.sourceLanguage == nil)
    let snapshot = try SharedContract.decode(VoiceSnapshot.self,
      from: SharedContract.encode(VoiceSnapshot(hostEpoch: UUID())))
    #expect(snapshot.capabilities == nil && snapshot.activeIntent == nil)
  }
  @Test func legacyReaderIgnoresAdditiveIntentFields() throws {
    struct LegacyCommand: Decodable { let schemaVersion: Int; let kind: CommandKind; let requestID: UUID }
    var value = command()
    value.intent = .init(kind: .translation, targetLanguage: "en-US")
    value.capabilityRevision = UUID(); value.sourceLanguage = "auto"
    let legacy = try SharedContract.decode(LegacyCommand.self, from: SharedContract.encode(value))
    #expect(legacy.schemaVersion == 2 && legacy.kind == .start && legacy.requestID == value.requestID)
  }
  @Test func unknownIntentKindIsNotSilentlyDictation() throws {
    var value = command()
    value.intent = .init(kind: .translation, targetLanguage: "en-US")
    let json = String(decoding: try SharedContract.encode(value), as: UTF8.self)
      .replacingOccurrences(of: "translation", with: "future-kind")
    #expect(throws: (any Error).self) { try SharedContract.decode(VoiceCommand.self, from: Data(json.utf8)) }
  }
  @Test func advertisedCapabilitiesGateAllOptionalModes() {
    let cap = capabilities(translate: false, help: false)
    #expect(cap.allows(.init()))
    #expect(!cap.allows(.init(kind: .translation, targetLanguage: "en-US")))
    #expect(!cap.allows(.init(kind: .helpWrite)))
    #expect(!cap.allows(.init(kind: .dictation, targetLanguage: "en-US")))
    #expect(!cap.allows(.init(stylePackID: "missing")))
    #expect(cap.allows(.init(stylePackID: "style.one")))
    #expect(!capabilities().allows(.init(kind: .translation, targetLanguage: "missing")))
  }
  @Test func transportBoundsFailClosedWithoutTruncation() {
    let targets = (0..<257).map { KeyboardVoiceOption(id: "lang.\($0)", title: "语言 \($0)") }
    let cap = capabilities(targets: targets)
    #expect(cap.translationTargets.count == 257)
    #expect(!cap.isValid)
    #expect(!cap.allows(.init(kind: .translation, targetLanguage: "lang.256")))
    #expect(capabilities(targets: Array(targets.prefix(256))).isValid)
    #expect(!capabilities(targets: [.init(id: "same", title: "一"), .init(id: "same", title: "二")]).isValid)
    #expect(!capabilities(styles: [.init(id: "", title: "不合法")]).isValid)
    #expect(!capabilities(targets: [.init(id: "en", title: String(repeating: "a", count: 161))]).isValid)
  }
  @Test func startFreezesTargetSourceStyleRevisionAndTypingLanguage() {
    var current = capabilities()
    let intent = KeyboardVoiceIntent(kind: .translation, targetLanguage: "en-US", stylePackID: "style.one")
    let captured = KeyboardVoiceStart(intent: intent, capabilities: current, language: .chinese)
    let revision = current.revision
    current = capabilities(targets: [.init(id: "fr", title: "法语")], styles: [])
    #expect(captured.intent == intent)
    #expect(captured.sourceLanguage == "auto" && captured.capabilityRevision == revision)
    #expect(captured.language == .chinese)
    #expect(captured.capabilityRevision != current.revision)
    #expect(!current.allows(captured.intent))
  }
  @Test func helpWriteRequiresEchoAndExplicitInsertion() {
    let intent = KeyboardVoiceIntent(kind: .helpWrite)
    let start = KeyboardVoiceStart(intent: intent, capabilities: capabilities(), language: .english)
    #expect(KeyboardVoiceDelivery.disposition(start: start, echoedIntent: nil) == .reject)
    #expect(KeyboardVoiceDelivery.disposition(start: start, echoedIntent: .init()) == .reject)
    #expect(KeyboardVoiceDelivery.disposition(start: start, echoedIntent: intent) == .preview)
    #expect(KeyboardVoiceDelivery.disposition(start: start, echoedIntent: intent, confirmedHelpWrite: true) == .insert)
  }
  @Test func helpWriteRejectsStyleWhileDictationAndTranslationRetainIt() {
    let cap = capabilities()
    #expect(cap.allows(.init(kind: .helpWrite)))
    let forbidden = KeyboardVoiceIntent(kind: .helpWrite, stylePackID: "style.one")
    #expect(!cap.allows(forbidden))
    let start = KeyboardVoiceStart(intent: forbidden, capabilities: cap, language: .chinese)
    #expect(KeyboardVoiceDelivery.disposition(start: start, echoedIntent: forbidden,
      confirmedHelpWrite: true) == .reject)
    #expect(cap.allows(.init(stylePackID: "style.one")))
    #expect(cap.allows(.init(kind: .translation, targetLanguage: "en-US", stylePackID: "style.one")))
  }
  @Test func wrongTranslationTargetAndStyleCannotBeDelivered() {
    let start = KeyboardVoiceStart(intent: .init(kind: .translation, targetLanguage: "en-US", stylePackID: "style.one"),
      capabilities: capabilities(), language: .chinese)
    #expect(KeyboardVoiceDelivery.disposition(start: start,
      echoedIntent: .init(kind: .translation, targetLanguage: "fr", stylePackID: "style.one")) == .reject)
    #expect(KeyboardVoiceDelivery.disposition(start: start,
      echoedIntent: .init(kind: .translation, targetLanguage: "en-US")) == .reject)
    #expect(KeyboardVoiceDelivery.disposition(start: start, echoedIntent: start.intent) == .insert)
  }
  @Test func legacyRawDictationRemainsDeliverableWithoutCapabilities() throws {
    let legacy = try JSONDecoder().decode(KeyboardVoiceIntent.self, from: Data("{\"kind\":\"dictation\"}".utf8))
    let start = KeyboardVoiceStart(intent: legacy, capabilities: nil, language: .english)
    #expect(KeyboardVoiceDelivery.disposition(start: start, echoedIntent: nil) == .insert)
    let modern = KeyboardVoiceStart(intent: .init(), capabilities: capabilities(), language: .english)
    #expect(KeyboardVoiceDelivery.disposition(start: modern, echoedIntent: nil) == .reject)
  }
  @Test func intentSurvivesDurableAdmissionOnce() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let writer = try Mailbox(url: directory.appendingPathComponent("mailbox.sqlite"))
    let reader = try Mailbox(url: directory.appendingPathComponent("mailbox.sqlite"))
    var value = command()
    value.intent = .init(kind: .translation, targetLanguage: "en-US", stylePackID: "style.one")
    value.capabilityRevision = UUID(); value.sourceLanguage = "auto"
    try writer.enqueue(value); try writer.enqueue(value)
    #expect(try reader.claimCommands() == [value])
    #expect(try writer.claimCommands().isEmpty)
  }
  @Test func currentRawSelectionIsExplicitButLegacyDecodeRemainsDistinct() throws {
    let intent = KeyboardVoiceIntent()
    #expect(intent.kind == .dictation)
    #expect(intent.stylePackID == "builtin.raw")
    let encoded = try JSONEncoder().encode(intent)
    #expect(String(decoding: encoded, as: UTF8.self).contains("builtin.raw"))
    let legacy = try JSONDecoder().decode(KeyboardVoiceIntent.self, from: Data("{\"kind\":\"dictation\"}".utf8))
    #expect(legacy.stylePackID == nil)
    #expect(KeyboardVoiceIntent(kind: .translation, targetLanguage: "English").stylePackID == nil)
    #expect(KeyboardVoiceIntent(kind: .helpWrite).stylePackID == nil)
  }
  @Test func actualStartMetadataProducerCannotDowngradeWithoutCapabilities() {
    var without = command()
    let missing = KeyboardVoiceStart(intent: .init(), capabilities: nil, language: .chinese)
    #expect(!missing.attachMetadata(to: &without))
    #expect(without.intent == nil)
    var with = command()
    let current = KeyboardVoiceStart(intent: .init(), capabilities: capabilities(), language: .chinese)
    #expect(current.attachMetadata(to: &with))
    #expect(with.intent?.stylePackID == "builtin.raw")
    #expect(with.capabilityRevision == current.capabilityRevision)
    #expect(with.sourceLanguage == current.sourceLanguage)
    #expect(KeyboardVoiceDelivery.disposition(start: current, echoedIntent: with.intent) == .insert)
  }
  @Test func boundedSnapshotRoundTripContainsPublicMetadataOnly() throws {
    var snapshot = VoiceSnapshot(hostEpoch: UUID())
    snapshot.capabilities = capabilities()
    snapshot.activeIntent = .init(kind: .translation, targetLanguage: "en-US")
    let data = try SharedContract.encode(snapshot)
    #expect(data.count < SharedContract.maxPayloadBytes)
    #expect(try SharedContract.decode(VoiceSnapshot.self, from: data) == snapshot)
    let json = String(decoding: data, as: UTF8.self)
    #expect(!json.contains("prompt") && !json.contains("credential") && !json.contains("apiKey"))
  }
}

struct TranslationSelectionGestureTests {
  let targets = [KeyboardVoiceOption(id: "fr", title: "法语"), .init(id: "en-US", title: "英语")]
  @Test func releaseWithoutHitCancelsAndDoesNotStartSpeech() {
    var selector = TranslationSelectionGesture()
    selector.begin(targets: targets)
    #expect(selector.isSelecting)
    #expect(selector.release() == nil)
    #expect(!selector.isSelecting)
    #expect(selector.release() == nil)
  }
  @Test func onlyConfiguredHitReleasesExactlyOnce() {
    var selector = TranslationSelectionGesture()
    selector.begin(targets: targets)
    selector.move(hitID: "en-US")
    #expect(selector.release() == "en-US")
    #expect(selector.release() == nil)
  }
  @Test func dragOutsideAndInterruptionCancelSelection() {
    var selector = TranslationSelectionGesture()
    selector.begin(targets: targets); selector.move(hitID: "fr"); selector.move(hitID: nil)
    #expect(selector.release() == nil)
    selector.begin(targets: targets); selector.move(hitID: "fr"); selector.cancel()
    #expect(selector.release() == nil && selector.targets.isEmpty)
    selector.begin(targets: targets); selector.move(hitID: "unconfigured")
    #expect(selector.selectedID == nil && selector.release() == nil)
  }
  @Test func activeTargetsAreFrozenAndEmptyListCannotSelect() {
    var selector = TranslationSelectionGesture()
    var current = targets
    selector.begin(targets: current)
    current.removeAll()
    selector.move(hitID: "fr")
    #expect(selector.release() == "fr")
    selector.begin(targets: current); selector.move(hitID: "fr")
    #expect(!selector.isSelecting && selector.release() == nil)
  }
  @Test func ordinaryStationaryMicHoldStillStartsThenStops() {
    var speech = VoiceGesture()
    speech.down(at: 0, phase: .ready)
    #expect(speech.tick(at: 0.3) == .start)
    #expect(speech.up(phase: .recording) == .stop)
    var selector = TranslationSelectionGesture()
    selector.begin(targets: targets)
    #expect(selector.release() == nil)
  }
}
