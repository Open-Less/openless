import Foundation
import Testing
@testable import OpenLess

@MainActor
struct StyleLibraryTests {
  @MainActor final class Fixture {
    var actions: [[String: Any]] = []
    var fail: String?
    var selected = "builtin.raw"
    var target = ""
    var targets: [String]?
    var directory = "/tmp/unused-style-staging"
    var inspectImport: ((String) throws -> Void)?
    var developerMode = false
    func data(_ value: Any) throws -> Data { try JSONSerialization.data(withJSONObject: value) }
    func style(_ id: String = "builtin.raw", kind: String = "builtin") -> [String: Any] {
      ["id": id, "name": "<untrusted **name**>", "description": "pack text", "version": "1", "kind": kind,
       "baseMode": "raw", "enabled": true, "active": selected == id]
    }
    func translation() -> [String: Any] {
      var result: [String: Any] = [
        "catalog": [["code": "en", "nativeName": "English"], ["code": "zh", "nativeName": "中文"]],
        "targetLanguage": target, "workingLanguages": ["中文"], "maxTargets": targets == nil ? 1 : 5,
        "multipleTargetsSupported": targets != nil, "targetOrderingSupported": targets != nil,
        "providerLanguageSupport": "notVerified", "effective": !target.isEmpty]
      if let targets { result["targetLanguages"] = targets }
      return result
    }
    func response(_ request: [String: Any]) async throws -> Data {
      actions.append(request)
      let action = request["action"] as? String ?? ""
      if action == fail { throw DeveloperError.rejected("Core rejected the request") }
      switch action {
      case "styleLibrary":
        return try data(["packs": [style(), style("imported", kind: "imported")], "activeStylePackId": selected,
                         "archive": ["format": "zip", "maxCompressedBytes": 524288, "stagingDirectory": directory],
                         "editorSupported": false, "updateSupported": false, "repeatImportBehavior": "uniqueId"])
      case "capabilitySnapshot":
        return try data(["developerMode": developerMode, "pipelineMode": "traditional", "llm": [
          "enabled": developerMode, "configured": true, "providerSupported": true,
          "providerType": "openai", "model": "", "availability": developerMode ? "configuredNotVerified" : "disabled",
          "canAttemptPolish": developerMode, "canAttemptTranslation": developerMode && !target.isEmpty,
          "verification": "notRecorded", "modelAvailability": "unknown", "entitlement": "unknown"]])
      case "translationConfiguration": return try data(translation())
      case "setTranslationTarget": target = request["targetLanguage"] as! String; return try data(translation())
      // Tests supply the next Core snapshot; this fixture does not implement target mutations.
      case "addTranslationTarget", "deleteTranslationTarget", "reorderTranslationTargets", "selectTranslationTarget":
        return try data(translation())
      case "marketplaceList": return try data(["items": []])
      case "activateStyle": selected = request["id"] as! String; return try data(["pack": style(selected)])
      case "removeStyle": selected = "builtin.polish"; return try data(["removedId": "imported"])
      case "importStyleArchive": try inspectImport?(request["stagedFileName"] as! String); return try data(["pack": style("new")])
      default: return try data([:])
      }
    }
    func library() -> StyleLibrary { StyleLibrary(request: { try await self.response($0) }) }
    var names: [String] { actions.compactMap { $0["action"] as? String } }
  }

  @Test func localLoadNeverRequestsMarketplaceAndPreservesCapabilityHonesty() async throws {
    let fixture = Fixture(), library: StyleLibrary
    library = fixture.library()
    await library.load()
    #expect(fixture.names == ["styleLibrary", "capabilitySnapshot", "translationConfiguration"])
    #expect(library.snapshot?.packs.count == 2)
    #expect(library.snapshot?.updateSupported == false)
    #expect(library.snapshot?.repeatImportBehavior == "uniqueId")
    #expect(library.capabilities?.llm.configured == true)
    #expect(library.capabilities?.llm.canAttemptPolish == false)
    #expect(library.capabilities?.llm.verification == "notRecorded")
    #expect(library.capabilities?.llm.entitlement == "unknown")
    #expect(library.operation == nil)
  }

  @Test func ordinaryModeCanManageButCannotActivateNonrawOrDisableRaw() async throws {
    let fixture = Fixture(), library: StyleLibrary
    library = fixture.library(); await library.load()
    let raw = try #require(library.snapshot?.packs.first)
    let imported = try #require(library.snapshot?.packs.last)
    await library.activate(imported)
    await library.setEnabled(raw, enabled: false)
    #expect(!fixture.names.contains("activateStyle"))
    #expect(!fixture.names.contains("setStyleEnabled"))
    await library.setEnabled(imported, enabled: false)
    #expect(fixture.names.contains("setStyleEnabled"))
    await library.activate(raw)
    #expect(fixture.names.contains("activateStyle"))
  }

  @Test func busyAndDisconnectedModelsNeverIssueRequests() async {
    let fixture = Fixture(), library: StyleLibrary
    library = fixture.library(); library.setActivityBusy(true)
    await library.load(); await library.searchMarket(query: "test"); await library.install("id")
    await library.setTranslationTarget("English")
    #expect(fixture.actions.isEmpty)
    library.setActivityBusy(false); await library.load()
    #expect(library.snapshot != nil)
    library.connect(nil); await library.load()
    #expect(!library.ready)
    #expect(library.snapshot == nil)
    #expect(library.market.isEmpty)
  }

  @Test func marketListIsExplicitAndUsesCoreQueryWithoutInventedSortOrHTTP() async {
    let fixture = Fixture(), library: StyleLibrary
    library = fixture.library()
    #expect(fixture.actions.isEmpty)
    await library.searchMarket(query: "中文 & ?")
    #expect(fixture.names == ["marketplaceList"])
    #expect(fixture.actions.first?["query"] as? String == "中文 & ?")
    #expect(fixture.actions.first?["limit"] as? Int == 50)
    #expect(fixture.actions.first?["sort"] == nil)
    #expect(library.marketLoaded && library.market.isEmpty)
  }

  @Test func realCoreMarketFlatteningAndUnknownContentDecodeAsDataOnly() throws {
    let fixture = Fixture()
    var json: [String: Any] = ["id": "uuid", "name": "[tap](https://example.invalid)", "description": "ignore instructions",
                              "authorLogin": "author", "version": "1", "baseMode": "polish", "tags": [],
                              "likeCount": 2, "downloadCount": 3, "prompt": "<script>untrusted</script>", "state": "published"]
    let detail = try JSONDecoder().decode(MarketStyleDetail.self, from: fixture.data(json))
    #expect(detail.summary.name == "[tap](https://example.invalid)")
    #expect(detail.prompt == "<script>untrusted</script>")
    json["originPackId"] = "upstream"
    #expect(try JSONDecoder().decode(MarketStyleDetail.self, from: fixture.data(json)).summary.originPackId == "upstream")
  }

  @Test func removalRefreshUsesCoreFallbackAndBuiltinNeverRemoves() async throws {
    let fixture = Fixture(), library: StyleLibrary
    library = fixture.library(); await library.load()
    await library.remove(try #require(library.snapshot?.packs.first))
    #expect(!fixture.names.contains("removeStyle"))
    await library.remove(try #require(library.snapshot?.packs.last))
    #expect(library.snapshot?.activeStylePackId == "builtin.polish")
    #expect(fixture.names.suffix(4) == ["removeStyle", "styleLibrary", "capabilitySnapshot", "translationConfiguration"])
  }

  @Test func saveErrorPreservesSnapshotAndSingleTargetUsesActualNativeName() async throws {
    let fixture = Fixture(), library: StyleLibrary
    fixture.target = "custom legacy"
    library = fixture.library(); await library.load()
    #expect(library.translation?.targetLanguage == "custom legacy")
    #expect(library.translation?.maxTargets == 1)
    #expect(library.translation?.targetOrderingSupported == false)
    fixture.fail = "setTranslationTarget"
    await library.setTranslationTarget("English")
    #expect(library.translation?.targetLanguage == "custom legacy")
    #expect(library.error != nil)
    fixture.fail = nil; await library.setTranslationTarget("English")
    #expect(library.translation?.targetLanguage == "English")
    #expect(library.capabilities?.llm.canAttemptTranslation == false)
    await library.setTranslationTarget("")
    #expect(library.translation?.targetLanguage == "")
    #expect(!fixture.names.contains("updateSettings"))
  }

  @Test(arguments: ["addTranslationTarget", "deleteTranslationTarget", "reorderTranslationTargets", "selectTranslationTarget"])
  func translationMutationsSendExactPayloadAndPublishCoreOrder(_ action: String) async throws {
    let fixture = Fixture()
    fixture.target = "English"; fixture.targets = ["中文", "English", "日本語"]
    let library = fixture.library(); await library.load()
    #expect(library.translation?.orderedTargets == ["中文", "English", "日本語"])
    #expect(library.capabilities?.llm.canAttemptTranslation == false)
    fixture.actions.removeAll(); fixture.developerMode = true

    let target: String?
    let targets: [String]?
    let expectedRequest: [String: Any]
    switch action {
    case "addTranslationTarget":
      target = "Français"; targets = nil
      fixture.target = "Français"; fixture.targets = ["English", "Français", "日本語", "中文"]
      expectedRequest = ["action": action, "target": "Français"]
    case "deleteTranslationTarget":
      target = "English"; targets = nil
      fixture.target = "中文"; fixture.targets = ["日本語", "中文"]
      expectedRequest = ["action": action, "target": "English"]
    case "reorderTranslationTargets":
      target = nil; targets = ["日本語", "English", "中文"]
      fixture.target = "中文"; fixture.targets = ["English", "中文", "日本語"]
      expectedRequest = ["action": action, "targets": ["日本語", "English", "中文"]]
    default:
      target = "日本語"; targets = nil
      fixture.target = "日本語"; fixture.targets = ["English", "日本語", "中文"]
      expectedRequest = ["action": action, "target": "日本語"]
    }
    await library.changeTranslationTargets(action: action, target: target, targets: targets)

    #expect(fixture.names == [action, "capabilitySnapshot"])
    #expect(NSDictionary(dictionary: try #require(fixture.actions.first)).isEqual(to: expectedRequest))
    #expect(library.translation?.orderedTargets == fixture.targets)
    #expect(library.translation?.targetLanguage == fixture.target)
    #expect(library.translation?.workingLanguages == ["中文"])
    #expect(library.translation?.multipleTargetsSupported == true)
    #expect(library.translation?.targetOrderingSupported == true)
    #expect(library.capabilities?.developerMode == true)
    #expect(library.capabilities?.llm.canAttemptTranslation == true)
    #expect(library.capabilities?.llm.verification == "notRecorded")
    #expect(library.error == nil)
    #expect(library.operation == nil)
  }

  @Test(arguments: ["addTranslationTarget", "deleteTranslationTarget", "reorderTranslationTargets", "selectTranslationTarget"])
  func rejectedTranslationMutationsPreserveTargetsSelectionAndCapabilities(_ action: String) async {
    let fixture = Fixture()
    fixture.target = "English"; fixture.targets = ["中文", "English", "日本語"]
    let library = fixture.library(); await library.load()
    fixture.actions.removeAll(); fixture.fail = action
    fixture.target = "日本語"; fixture.targets = ["日本語", "中文"]; fixture.developerMode = true

    await library.changeTranslationTargets(action: action, target: "日本語", targets: ["日本語", "中文"])

    #expect(fixture.names == [action])
    #expect(library.translation?.orderedTargets == ["中文", "English", "日本語"])
    #expect(library.translation?.targetLanguage == "English")
    #expect(library.capabilities?.developerMode == false)
    #expect(library.capabilities?.llm.canAttemptTranslation == false)
    #expect(library.error?.contains("Core rejected the request") == true)
    #expect(library.operation == nil)
  }

  @Test func translationMutationsNeverWriteWhileActivityBusyOrDisconnected() async {
    let fixture = Fixture()
    fixture.target = "English"; fixture.targets = ["中文", "English"]
    let library = fixture.library(); await library.load()
    fixture.actions.removeAll(); library.setActivityBusy(true)
    for action in ["addTranslationTarget", "deleteTranslationTarget", "reorderTranslationTargets", "selectTranslationTarget"] {
      await library.changeTranslationTargets(action: action, target: "中文", targets: ["English", "中文"])
    }
    #expect(fixture.actions.isEmpty)
    #expect(library.translation?.orderedTargets == ["中文", "English"])
    #expect(library.translation?.targetLanguage == "English")
    #expect(library.operation == nil)
    library.setActivityBusy(false); library.connect(nil)
    await library.changeTranslationTargets(action: "selectTranslationTarget", target: "中文")
    #expect(fixture.actions.isEmpty)
  }

  @Test func pendingTranslationMutationBlocksOtherWritesUntilCoreReturns() async throws {
    let fixture = Fixture()
    fixture.target = "English"; fixture.targets = ["中文", "English"]
    var continuation: CheckedContinuation<Data, any Error>?
    let library = StyleLibrary { request in
      if request["action"] as? String == "reorderTranslationTargets" {
        fixture.actions.append(request)
        return try await withCheckedThrowingContinuation { continuation = $0 }
      }
      return try await fixture.response(request)
    }
    await library.load(); fixture.actions.removeAll()
    let task = Task { await library.changeTranslationTargets(action: "reorderTranslationTargets", targets: ["English", "中文"]) }
    while continuation == nil { await Task.yield() }
    for action in ["addTranslationTarget", "deleteTranslationTarget", "reorderTranslationTargets", "selectTranslationTarget"] {
      await library.changeTranslationTargets(action: action, target: "日本語", targets: ["日本語"])
    }
    #expect(fixture.names == ["reorderTranslationTargets"])
    #expect(library.blocked)
    #expect(library.translation?.orderedTargets == ["中文", "English"])
    fixture.targets = ["English", "中文"]
    continuation?.resume(returning: try fixture.data(fixture.translation()))
    await task.value
    #expect(fixture.names == ["reorderTranslationTargets", "capabilitySnapshot"])
    #expect(library.translation?.orderedTargets == ["English", "中文"])
    #expect(library.operation == nil)
  }

  @Test func deletingLastTranslationTargetRefreshesUnavailableCapability() async {
    let fixture = Fixture()
    fixture.developerMode = true; fixture.target = "English"; fixture.targets = ["English"]
    let library = fixture.library(); await library.load()
    #expect(library.capabilities?.llm.canAttemptTranslation == true)
    fixture.actions.removeAll(); fixture.target = ""; fixture.targets = []
    await library.changeTranslationTargets(action: "deleteTranslationTarget", target: "English")
    #expect(fixture.names == ["deleteTranslationTarget", "capabilitySnapshot"])
    #expect(library.translation?.orderedTargets == [])
    #expect(library.translation?.targetLanguage == "")
    #expect(library.translation?.effective == false)
    #expect(library.capabilities?.llm.canAttemptTranslation == false)
  }

  @Test func translationCapabilityRefreshFailureKeepsCommittedCoreTargets() async {
    let fixture = Fixture()
    fixture.target = "English"; fixture.targets = ["中文", "English"]
    let library = fixture.library(); await library.load()
    fixture.actions.removeAll(); fixture.fail = "capabilitySnapshot"
    fixture.target = "日本語"; fixture.targets = ["中文", "English", "日本語"]
    await library.changeTranslationTargets(action: "addTranslationTarget", target: "日本語")
    #expect(fixture.names == ["addTranslationTarget", "capabilitySnapshot"])
    #expect(library.translation?.orderedTargets == ["中文", "English", "日本語"])
    #expect(library.translation?.targetLanguage == "日本語")
    #expect(library.error != nil)
    #expect(library.operation == nil)
  }

  @Test func translationDecodesLegacyFallbackAndHonorsExplicitEmptyCoreTargets() throws {
    let fixture = Fixture(); fixture.target = "custom legacy"
    let legacy = try JSONDecoder().decode(StyleTranslationConfiguration.self, from: fixture.data(fixture.translation()))
    #expect(legacy.orderedTargets == ["custom legacy"])
    fixture.target = ""
    let disabled = try JSONDecoder().decode(StyleTranslationConfiguration.self, from: fixture.data(fixture.translation()))
    #expect(disabled.orderedTargets == [])
    fixture.target = "legacy value retained in Core"; fixture.targets = []
    let explicitEmpty = try JSONDecoder().decode(StyleTranslationConfiguration.self, from: fixture.data(fixture.translation()))
    #expect(explicitEmpty.orderedTargets == [])
  }

  @Test func cancelledReadDoesNotPublishAndPreventsConcurrentWritesUntilCoreReturns() async throws {
    let fixture = Fixture()
    var continuation: CheckedContinuation<Data, any Error>?
    let library = StyleLibrary { request in
      if request["action"] as? String == "marketplaceList" {
        return try await withCheckedThrowingContinuation { continuation = $0 }
      }
      return try await fixture.response(request)
    }
    let task = Task { await library.searchMarket(query: "query") }
    while continuation == nil { await Task.yield() }
    library.cancelRead()
    await library.install("id")
    #expect(fixture.actions.isEmpty)
    #expect(library.blocked)
    continuation?.resume(returning: try fixture.data(["items": []]))
    await task.value
    #expect(!library.marketLoaded)
    #expect(library.operation == nil)
    #expect(library.error == nil)
    #expect(library.notice == "已取消")
  }

  @Test func reconnectDropsStaleReadWithoutUnlockingNewConnectionOperation() async throws {
    let fixture = Fixture()
    var continuation: CheckedContinuation<Data, any Error>?
    let library = StyleLibrary { _ in try await withCheckedThrowingContinuation { continuation = $0 } }
    let task = Task { await library.load() }
    while continuation == nil { await Task.yield() }
    library.connect(nil)
    continuation?.resume(returning: try fixture.data([:]))
    await task.value
    #expect(library.snapshot == nil)
    #expect(library.error == nil)
    #expect(library.operation == nil)
  }

  @Test func importSendsOnlyGeneratedLeafCleansUpAndNeverActivates() async throws {
    let root = try temporaryDirectory(); defer { try? FileManager.default.removeItem(at: root) }
    let source = root.appendingPathComponent("source.ZIP")
    let staging = root.appendingPathComponent("staging", isDirectory: true)
    try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: false)
    try Data("bounded archive fixture".utf8).write(to: source)
    let fixture = Fixture(); fixture.directory = staging.path
    fixture.inspectImport = { leaf in
      #expect(leaf.hasSuffix(".zip")); #expect(UUID(uuidString: String(leaf.dropLast(4))) != nil)
      #expect(!leaf.contains("/")); #expect(leaf != source.lastPathComponent)
      let copied = try Data(contentsOf: staging.appendingPathComponent(leaf))
      #expect(copied == Data("bounded archive fixture".utf8))
    }
    let library = fixture.library(); await library.load(); library.pendingImport = source
    await library.importPending()
    #expect(library.pendingImport == nil)
    #expect(fixture.names.contains("importStyleArchive"))
    #expect(!fixture.names.contains("activateStyle"))
    #expect(try FileManager.default.contentsOfDirectory(atPath: staging.path).isEmpty)
    #expect(FileManager.default.fileExists(atPath: source.path))
  }

  @Test func rejectedImportPreservesSelectionAndCleansStagedCopy() async throws {
    let root = try temporaryDirectory(); defer { try? FileManager.default.removeItem(at: root) }
    let source = root.appendingPathComponent("bad.zip")
    let staging = root.appendingPathComponent("staging", isDirectory: true)
    try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: false)
    try Data("not a zip; Core validates content".utf8).write(to: source)
    let fixture = Fixture(); fixture.directory = staging.path; fixture.fail = "importStyleArchive"
    let library = fixture.library(); await library.load(); library.pendingImport = source
    await library.importPending()
    #expect(library.pendingImport == source)
    #expect(library.snapshot?.packs.count == 2)
    #expect(library.error != nil)
    #expect(try FileManager.default.contentsOfDirectory(atPath: staging.path).isEmpty)
  }

  @Test func stagingSaveFailurePreservesSelectedFileAndNeverCallsCoreImport() async throws {
    let root = try temporaryDirectory(); defer { try? FileManager.default.removeItem(at: root) }
    let source = root.appendingPathComponent("source.zip")
    try Data("retained source".utf8).write(to: source)
    let fixture = Fixture(); fixture.directory = root.appendingPathComponent("missing-directory").path
    let library = fixture.library(); await library.load(); library.pendingImport = source
    await library.importPending()
    #expect(library.pendingImport == source)
    #expect(library.snapshot?.packs.count == 2)
    #expect(library.error != nil)
    #expect(!fixture.names.contains("importStyleArchive"))
    #expect(try String(contentsOf: source, encoding: .utf8) == "retained source")
  }

  @Test func successfulInstallRefreshesButNeverActivatesOrClaimsVerification() async {
    let fixture = Fixture(), library: StyleLibrary
    library = fixture.library(); await library.load()
    await library.install("market-id")
    #expect(fixture.names.suffix(4) == ["marketplaceInstall", "styleLibrary", "capabilitySnapshot", "translationConfiguration"])
    #expect(!fixture.names.contains("activateStyle"))
    #expect(library.snapshot?.activeStylePackId == "builtin.raw")
    #expect(library.capabilities?.llm.verification == "notRecorded")
  }

  @Test func committedMutationWithRefreshFailureDoesNotPretendSaveFailed() async throws {
    let fixture = Fixture(), library: StyleLibrary
    library = fixture.library(); await library.load(); fixture.fail = "styleLibrary"
    await library.setEnabled(try #require(library.snapshot?.packs.last), enabled: false)
    #expect(library.error?.contains("Core 已保存，但刷新失败") == true)
    #expect(library.snapshot?.packs.count == 2)
    #expect(library.operation == nil)
  }

  private func temporaryDirectory() throws -> URL {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("style-app-tests-" + UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
    return root
  }
}
