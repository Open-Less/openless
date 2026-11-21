import Foundation
import Testing
@testable import OpenLessShared

struct KeyboardGeneratedPreviewTests {
  let now = Date(timeIntervalSince1970: 10_000)
  let epoch = UUID()
  let session = UUID()
  let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 10)
  func start() -> KeyboardVoiceStart {
    .init(intent: .init(kind: .helpWrite), capabilities: .init(revision: UUID(), sourceLanguage: "auto",
      translationTargets: [], styles: [], canTranslate: false, canHelpWrite: true), language: .chinese)
  }
  func preview(text: String = "真正生成的预览") -> KeyboardGeneratedPreview {
    .init(sessionID: session, target: target, text: text, createdAt: now, expiresAt: now.addingTimeInterval(30))
  }
  func confirmation(_ preview: KeyboardGeneratedPreview, start: KeyboardVoiceStart) -> VoiceCommand {
    var command = VoiceCommand(kind: .confirmGeneratedPreview, sessionID: session, hostEpoch: epoch,
      target: target, language: start.language, now: now)
    command.previewID = preview.id; command.intent = start.intent
    command.capabilityRevision = start.capabilityRevision; command.sourceLanguage = start.sourceLanguage
    return command
  }
  @Test func previewIsSeparateFromDurableInsertionAndRoundTrips() throws {
    var snapshot = VoiceSnapshot(hostEpoch: epoch)
    snapshot.generatedPreview = preview()
    snapshot.activeIntent = .init(kind: .helpWrite)
    snapshot.phase = .awaitingInsertion
    #expect(snapshot.insertion == nil)
    #expect(try SharedContract.decode(VoiceSnapshot.self, from: SharedContract.encode(snapshot)) == snapshot)
    let legacy = VoiceSnapshot(hostEpoch: epoch)
    #expect(try SharedContract.decode(VoiceSnapshot.self, from: SharedContract.encode(legacy)).generatedPreview == nil)
  }
  @Test func confirmationContainsOnlyIdentifierAndCapturedMetadata() throws {
    let preview = preview(); let start = start()
    let command = confirmation(preview, start: start)
    #expect(preview.acceptsConfirmation(command, start: start, epoch: epoch, sessionActive: true, now: now))
    let json = String(decoding: try SharedContract.encode(command), as: UTF8.self)
    #expect(!json.contains(preview.text) && !json.contains("prompt") && !json.contains("text"))
    #expect(try SharedContract.decode(VoiceCommand.self, from: SharedContract.encode(command)) == command)
  }
  @Test func cancellationEpochAndCapturedRevisionRejectConfirmation() {
    let preview = preview(); let start = start()
    var command = confirmation(preview, start: start)
    #expect(!preview.acceptsConfirmation(command, start: start, epoch: epoch, sessionActive: false, now: now))
    #expect(!preview.acceptsConfirmation(command, start: start, epoch: UUID(), sessionActive: true, now: now))
    command.capabilityRevision = UUID()
    #expect(!preview.acceptsConfirmation(command, start: start, epoch: epoch, sessionActive: true, now: now))
    command = confirmation(preview, start: start); command.sourceLanguage = "en"
    #expect(!preview.acceptsConfirmation(command, start: start, epoch: epoch, sessionActive: true, now: now))
  }
  @Test func staleDocumentRevisionSessionAndPreviewIDRejectConfirmation() {
    let preview = preview(); let start = start()
    var command = VoiceCommand(kind: .confirmGeneratedPreview, sessionID: session, hostEpoch: epoch,
      target: .init(keyboardID: target.keyboardID, documentID: target.documentID, revision: 11),
      language: .chinese, now: now)
    command.previewID = preview.id; command.intent = start.intent
    command.capabilityRevision = start.capabilityRevision; command.sourceLanguage = start.sourceLanguage
    #expect(!preview.acceptsConfirmation(command, start: start, epoch: epoch, sessionActive: true, now: now))
    command = confirmation(preview, start: start); command.previewID = UUID()
    #expect(!preview.acceptsConfirmation(command, start: start, epoch: epoch, sessionActive: true, now: now))
    #expect(!preview.isUsable(sessionID: UUID(), target: target, now: now))
    let wrongSession = KeyboardGeneratedPreview(sessionID: UUID(), target: target, text: preview.text,
      createdAt: now, expiresAt: now.addingTimeInterval(30))
    #expect(!wrongSession.acceptsConfirmation(confirmation(wrongSession, start: start), start: start,
      epoch: epoch, sessionActive: true, now: now))
  }
  @Test func expiredEmptyOversizeAndUnboundedPreviewReject() {
    #expect(!preview().isUsable(sessionID: session, target: target, now: now.addingTimeInterval(30)))
    #expect(!preview(text: " \n ").isUsable(sessionID: session, target: target, now: now))
    #expect(!preview(text: String(repeating: "a", count: SharedContract.maxTextBytes + 1))
      .isUsable(sessionID: session, target: target, now: now))
    let unbounded = KeyboardGeneratedPreview(sessionID: session, target: target, text: "生成内容",
      createdAt: now, expiresAt: now.addingTimeInterval(301))
    #expect(!unbounded.isUsable(sessionID: session, target: target, now: now))
  }
  @Test func durableOfferMustMatchConfirmedGeneratedTextAndTarget() {
    let preview = preview()
    let exact = InsertionRequest(sessionID: session, target: target, text: preview.text, now: now)
    #expect(preview.matches(exact, now: now))
    #expect(!preview.matches(.init(sessionID: session, target: target, text: "换成别的内容", now: now), now: now))
    #expect(!preview.matches(.init(sessionID: UUID(), target: target, text: preview.text, now: now), now: now))
    let start = start()
    #expect(KeyboardVoiceDelivery.disposition(start: start, echoedIntent: start.intent) != .insert)
    #expect(KeyboardVoiceDelivery.disposition(start: start, echoedIntent: start.intent, confirmedHelpWrite: true) == .insert)
  }
  @Test func capturedSessionDoesNotDependOnMutableGlobalCapability() {
    let preview = preview(); let captured = start()
    let changedGlobal = KeyboardVoiceCapabilities(revision: UUID(), sourceLanguage: "fr",
      translationTargets: [], styles: [], canTranslate: false, canHelpWrite: false)
    #expect(changedGlobal.revision != captured.capabilityRevision)
    #expect(preview.acceptsConfirmation(confirmation(preview, start: captured), start: captured,
      epoch: epoch, sessionActive: true, now: now))
  }
}
