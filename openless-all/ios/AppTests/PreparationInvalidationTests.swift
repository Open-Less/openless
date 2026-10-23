import Testing
@testable import OpenLess

@MainActor struct PreparationInvalidationTests {
  @Test func modelAndPolishChangesDoNotInvalidatePreparedMicrophoneOrASR() {
    #expect(!DeveloperSettings.affectsSpeechPreparation(["action":"saveChannel","kind":"llm"], currentPipeline:"traditional"))
    #expect(!DeveloperSettings.affectsSpeechPreparation(["action":"setActive","kind":"llm"], currentPipeline:"traditional"))
    #expect(!DeveloperSettings.affectsSpeechPreparation(["action":"updateSettings","pipelineMode":"traditional","polishEnabled":true], currentPipeline:"traditional"))
    #expect(!DeveloperSettings.affectsSpeechPreparation(["action":"updateSettings","pipelineMode":"traditional","polishEnabled":false], currentPipeline:"traditional"))
  }
  @Test func asrOrPipelineChangesStillRequirePreparation() {
    #expect(DeveloperSettings.affectsSpeechPreparation(["action":"saveChannel","kind":"asr"], currentPipeline:"traditional"))
    #expect(DeveloperSettings.affectsSpeechPreparation(["action":"updateSettings","pipelineMode":"multimodal"], currentPipeline:"traditional"))
    #expect(DeveloperSettings.affectsSpeechPreparation(["action":"setActive","kind":"omni"], currentPipeline:"multimodal"))
  }
}
