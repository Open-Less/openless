import Speech
import Testing
@testable import OpenLess

struct NativeAuthorizationTests {
  @Test(.enabled(if: SFSpeechRecognizer.authorizationStatus() != .notDetermined))
  @MainActor func realAuthorizationCallbackReturnsToMainActorWithoutCapture() async {
    // Reuse an existing decision. No microphone, new permission prompt, model
    // download, analyzer or audio session is involved in this regression.
    let existing = SFSpeechRecognizer.authorizationStatus()
    let received = await AppleSpeechService.speechAuthorization()
    MainActor.preconditionIsolated()
    #expect(received == existing)
  }
}
