import Foundation
import Testing
@testable import OpenLessShared

struct ColdConfigurationRoutingTests {
  private let now = Date(timeIntervalSince1970: 1_800_000_000)
  private func configured() -> KeyboardVoiceCapabilities {
    KeyboardVoiceCapabilities(revision: UUID(), sourceLanguage: "zh-CN",
      translationTargets: [.init(id: "English", title: "English")],
      styles: [.init(id: "builtin.raw", title: "原文")], canTranslate: true, canHelpWrite: false)
  }
  @Test func coldReaderRecoversConfiguredTranslationAfterHostLeaseExpires() throws {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    defer { try? FileManager.default.removeItem(at: directory) }
    let url = directory.appendingPathComponent("mailbox.sqlite")
    let writer = try Mailbox(url: url)
    var saved = VoiceSnapshot(hostEpoch: UUID())
    saved.updatedAt = now.addingTimeInterval(-60)
    saved.capabilities = configured()
    saved.canStart = true
    saved.phase = .recording
    try writer.set(saved, for: SharedContract.snapshotKey)
    let coldReader = try Mailbox(url: url)
    let loaded = try #require(try coldReader.value(VoiceSnapshot.self, for: SharedContract.snapshotKey))
    let choices = try #require(loaded.configuredVoiceCapabilities)
    #expect(choices.allows(.init(kind: .translation, targetLanguage: "English")))
    #expect(!loaded.isFresh(at: now))
    #expect(!(loaded.isFresh(at: now) && loaded.canStart))
    #expect(choices == saved.capabilities)
  }
  @Test func leaseExpiryDoesNotReinterpretOrEnableConfiguration() {
    var saved = VoiceSnapshot(hostEpoch: UUID())
    saved.capabilities = configured()
    saved.updatedAt = now.addingTimeInterval(-4)
    #expect(saved.isFresh(at: now))
    #expect(saved.configuredVoiceCapabilities == saved.capabilities)
    #expect(!saved.isFresh(at: now.addingTimeInterval(0.001)))
    #expect(saved.configuredVoiceCapabilities == saved.capabilities)
    saved.capabilities = KeyboardVoiceCapabilities(revision: UUID(), sourceLanguage: "zh-CN",
      translationTargets: [.init(id: "English", title: "English")], styles: [],
      canTranslate: false, canHelpWrite: false)
    #expect(saved.configuredVoiceCapabilities?.canTranslate == false)
  }
  @Test func malformedAndUnsupportedConfigurationsCannotOfferHandoff() {
    var saved = VoiceSnapshot(hostEpoch: UUID())
    saved.updatedAt = now.addingTimeInterval(-60)
    #expect(saved.configuredVoiceCapabilities == nil)
    saved.capabilities = configured()
    saved.schemaVersion = 99
    #expect(saved.configuredVoiceCapabilities == nil)
    saved.schemaVersion = 2
    saved.capabilities = KeyboardVoiceCapabilities(revision: UUID(), sourceLanguage: "zh-CN",
      translationTargets: [.init(id: "same", title: "one"), .init(id: "same", title: "two")],
      styles: [], canTranslate: true, canHelpWrite: false)
    #expect(saved.configuredVoiceCapabilities == nil)
  }
  @Test func freshRevocationDoesNotKeepOldConfiguredChoices() {
    var saved = VoiceSnapshot(hostEpoch: UUID())
    saved.capabilities = configured()
    saved.updatedAt = now.addingTimeInterval(-60)
    var cached = saved.configuredVoiceCapabilities
    #expect(cached?.canTranslate == true)
    saved.capabilities = nil
    saved.updatedAt = now
    cached = saved.configuredVoiceCapabilities
    #expect(cached == nil)
  }
}
