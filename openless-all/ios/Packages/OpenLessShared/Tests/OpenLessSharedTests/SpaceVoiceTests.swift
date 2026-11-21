import Foundation
import Testing
@testable import OpenLessShared

struct SpaceVoiceTests {
  @Test func stoppedCaptureCannotResumeFromAnOlderHostSnapshot() {
    let session = UUID(), now = Date()
    var progress = KeyboardVoiceProgress()
    #expect(progress.phase(.recording, session: session) == .recording)
    progress.stopped(session: session, now: now)
    #expect(progress.phase(.recording, session: session) == .processing)
    #expect(progress.phase(.starting, session: session) == .processing)
    #expect(progress.phase(.awaitingInsertion, session: session) == .awaitingInsertion)
    #expect(progress.phase(.done, session: session) == .done)
    #expect(progress.phase(.recording, session: UUID()) == .recording)
    progress.stopped(session: session, now: now.addingTimeInterval(120))
    #expect(!progress.expired(session: session, now: now.addingTimeInterval(179)))
    #expect(progress.expired(session: session, now: now.addingTimeInterval(180)))
    progress.reset()
    #expect(progress.phase(.recording, session: session) == .recording)
  }
  @Test func recordingAndProcessingHaveDistinctPresentations() {
    #expect(KeyboardVoicePresentation(phase: .recording, hasSession: true, captureReady: true) == .recording)
    #expect(KeyboardVoicePresentation.recording.title != "thinking")
    #expect(KeyboardVoicePresentation(phase: .starting, hasSession: true, captureReady: false) == .preparing)
    #expect(KeyboardVoicePresentation(phase: .processing, hasSession: true, captureReady: false) == .processing)
    for phase in [VoicePhase.ready, .done, .cancelled, .error] {
      #expect(KeyboardVoicePresentation(phase: phase, hasSession: true, captureReady: false) == .idle)
    }
    #expect(KeyboardVoicePresentation(phase: .processing, hasSession: false, captureReady: false) == .idle)
  }
  @Test func releaseUsesFinalHitAndCanChangeMind() {
    var gesture = SpaceVoiceGesture()
    gesture.begin()
    gesture.move(hit: .cancel, inRestArea: false)
    #expect(gesture.selection == .cancel)
    gesture.move(hit: .send, inRestArea: false)
    #expect(gesture.release(hit: .translate, inRestArea: false) == .translate)
    #expect(gesture.release(hit: .send, inRestArea: false) == nil)
    gesture.begin()
    #expect(gesture.release(hit: nil, inRestArea: true) == .input)
    gesture.begin()
    #expect(gesture.release(hit: nil, inRestArea: false) == .cancel)
    gesture.begin(); gesture.cancel()
    #expect(gesture.release(hit: .send, inRestArea: false) == nil)
  }
  @Test func translationWaitsForBothAudioAndChoiceInEitherOrder() throws {
    for audioFirst in [false, true] {
      var state = SpaceVoiceDeliveryState()
      #expect((state.choose(SpaceVoiceFinish(.chooseTranslation))) == true)
      if audioFirst { state.recognized("hello") }
      #expect(state.takeWork() == nil)
      #expect((state.choose(SpaceVoiceFinish(.translate, targetLanguage: "日本語"))) == true)
      if !audioFirst { state.recognized("hello") }
      let pending = state.takeWork()
      let work = try #require(pending)
      #expect(work.0.targetLanguage == "日本語")
      #expect(work.1 == "hello")
      #expect(state.takeWork() == nil)
      #expect((!state.choose(SpaceVoiceFinish(.send))) == true)
    }
  }
  @Test func duplicateStopCannotTurnInputIntoSendOrReplaceTranscript() throws {
    var state = SpaceVoiceDeliveryState()
    #expect((state.choose(SpaceVoiceFinish(.input))) == true)
    #expect((!state.choose(SpaceVoiceFinish(.send))) == true)
    state.recognized("first"); state.recognized("second")
    let pending = state.takeWork()
    #expect(try #require(pending).1 == "first")
    #expect(!SpaceVoiceFinish(.translate).isValid)
    #expect(!SpaceVoiceFinish(.translate, targetLanguage: "a\nb").isValid)
    #expect(!SpaceVoiceFinish(.send, targetLanguage: "English").isValid)
  }
  @Test func sendIsExactlyOnceAndTiedToInputTarget() {
    let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 3)
    let session = UUID()
    let insertion = InsertionRequest(sessionID: session, target: target, text: "hello")
    var intent = KeyboardSubmitIntent(sessionID: session, target: target)
    #expect((intent.consume(for: insertion, current: target, supportsSend: true)) == true)
    #expect((!intent.consume(for: insertion, current: target, supportsSend: true)) == true)
    var changed = KeyboardSubmitIntent(sessionID: session, target: target)
    let other = InputTarget(keyboardID: target.keyboardID, documentID: UUID(), revision: 3)
    #expect((!changed.consume(for: insertion, current: other, supportsSend: true)) == true)
    #expect((!changed.consume(for: insertion, current: target, supportsSend: true)) == true)
    var note = KeyboardSubmitIntent(sessionID: session, target: target)
    #expect((!note.consume(for: insertion, current: target, supportsSend: false)) == true)
  }
  @Test func optionalWireFieldsKeepExistingKeyboardCommandsCompatible() throws {
    let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 0)
    var command = VoiceCommand(kind: .start, sessionID: UUID(), hostEpoch: UUID(), target: target, language: .chinese)
    let old = try SharedContract.decode(VoiceCommand.self, from: SharedContract.encode(command))
    #expect(old.deferredSpaceVoice == nil && old.spaceVoiceFinish == nil)
    command.deferredSpaceVoice = true
    command.spaceVoiceFinish = SpaceVoiceFinish(.chooseTranslation)
    #expect(try SharedContract.decode(VoiceCommand.self, from: SharedContract.encode(command)) == command)
  }
}
