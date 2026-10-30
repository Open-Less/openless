import Foundation
import Testing
@testable import OpenLess

@MainActor
struct VocabularyModelsTests {
  private func entry(_ fields: [String: Any] = [:]) throws -> CoreVocabularyEntry {
    var json: [String: Any] = ["id": "entry", "phrase": "词条", "enabled": true, "hits": 0, "createdAt": "2026-10-16T00:00:00Z"]
    json.merge(fields) { _, value in value }
    return try JSONDecoder().decode(CoreVocabularyEntry.self, from: JSONSerialization.data(withJSONObject: json))
  }

  @Test func sourceUsesDTOBeforeTheExactCoreMarker() throws {
    #expect(try entry(["source": "manual", "note": CoreVocabularyEntry.learnedNote]).classifiedSource == .manual)
    #expect(try entry(["source": "learned"]).classifiedSource == .learned)
    #expect(try entry(["note": "从手改中自动收集"]).classifiedSource == .learned)
    #expect(try entry(["note": "从手改中自动收集 "]).classifiedSource == .manual)
  }

  @Test func legacyFallbackMatchesCoreAndFutureSourcesRemainUnknown() throws {
    #expect(try entry().classifiedSource == .manual)
    #expect(try entry(["note": "旧备注"]).classifiedSource == .manual)
    #expect(try entry(["source": "future", "note": CoreVocabularyEntry.learnedNote]).classifiedSource == .unknown)
    #expect(try entry(["source": "unknown"]).classifiedSource == .unknown)
    #expect(try entry(["phrase": CoreVocabularyEntry.learnedNote]).classifiedSource == .manual)
  }

  @Test func requestAlwaysIncludesSourceAlongsideQueryAndOffset() {
    for source in VocabularySource.filters {
      let selection = VocabularySelection(query: "词典", source: source)
      let request = selection.request(offset: 100)
      #expect(request["action"] as? String == "vocabulary")
      #expect(request["query"] as? String == "词典")
      #expect(request["source"] as? String == source.rawValue)
      #expect(request["offset"] as? Int == 100)
      #expect(selection != VocabularySelection(query: "另一查询", source: source))
    }
    #expect(VocabularySelection().source == .all)
    #expect(VocabularySource.filters == [.all, .learned, .manual])
  }

  @Test func blankAndNewlineInputsAreRejected() {
    for phrase in ["", " \t ", "\n", "one\ntwo", "one\rtwo", "one\r\ntwo", "one\u{2028}two"] {
      #expect(VocabularyInput.validationMessage(phrase) != nil)
    }
    #expect(VocabularyInput.validationMessage(" 中文 👩🏽‍💻 ") == nil)
  }

  @Test func inputLimitUsesUTF8BytesNotVisibleCharacters() {
    #expect(VocabularyInput.validationMessage(String(repeating: "a", count: 512)) == nil)
    #expect(VocabularyInput.validationMessage(String(repeating: "a", count: 513)) != nil)
    #expect(VocabularyInput.validationMessage(String(repeating: "词", count: 170)) == nil)
    #expect(VocabularyInput.validationMessage(String(repeating: "词", count: 171)) != nil)
  }

  @Test func savesUseExistingDuplicateContractWithoutSuggestionOrMarkerWrites() {
    let add = VocabularyInput.saveRequest("  常用术语  ", entryID: nil)
    #expect(add["action"] as? String == "addVocabulary")
    #expect(add["phrase"] as? String == "常用术语")
    #expect(add["note"] == nil)
    #expect(add["id"] == nil)
    #expect(add.count == 2)
    let edit = VocabularyInput.saveRequest(" 修改词条 ", entryID: "existing")
    #expect(edit["action"] as? String == "updateVocabulary")
    #expect(edit["phrase"] as? String == "修改词条")
    #expect(edit["id"] as? String == "existing")
    #expect(edit.count == 3)
  }
}
