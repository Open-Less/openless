import Foundation
import Testing
@testable import OpenLessShared

struct KeyboardUndoTests {
  @Test func onlyExactUneditedKeyboardInsertionCanBeUndone() throws {
    let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 4)
    let record = try #require(KeyboardUndoRecord(target: target, text: "你好👩🏽‍💻", before: "prefix你好👩🏽‍💻", after: "tail", replacedSelection: false))
    #expect(record.matches(target: target, before: record.before, after: record.after, selection: nil))
    #expect(!record.matches(target: InputTarget(keyboardID: target.keyboardID, documentID: target.documentID, revision: 5), before: record.before, after: record.after, selection: nil))
    #expect(!record.matches(target: target, before: "edited", after: record.after, selection: nil))
    #expect(!record.matches(target: target, before: record.before, after: "moved", selection: nil))
    #expect(!record.matches(target: target, before: record.before, after: record.after, selection: "selected"))
    #expect(KeyboardUndoRecord(target: target, text: "a", before: "a", after: "", replacedSelection: true) == nil)
    #expect(KeyboardUndoRecord(target: target, text: "missing", before: "prefix", after: "", replacedSelection: false) == nil)
    #expect(KeyboardUndoRecord(target: target, text: String(repeating: "a", count: 129), before: String(repeating: "a", count: 129), after: "", replacedSelection: false) == nil)
  }
}
