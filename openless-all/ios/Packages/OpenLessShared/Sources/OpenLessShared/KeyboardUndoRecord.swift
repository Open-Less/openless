import Foundation

/// Memory-only undo for one keyboard-owned insertion. Never guesses at an edited document.
public struct KeyboardUndoRecord: Equatable, Sendable {
  public let target: InputTarget
  public let text: String
  public let before: String
  public let after: String
  public init?(target: InputTarget, text: String, before: String?, after: String?, replacedSelection: Bool) {
    guard !replacedSelection, !text.isEmpty, text.count <= 128,
      let before, let after, before.hasSuffix(text) else { return nil }
    self.target = target; self.text = text; self.before = before; self.after = after
  }
  public func matches(target: InputTarget?, before: String?, after: String?, selection: String?) -> Bool {
    target == self.target && before == self.before && after == self.after && (selection?.isEmpty != false)
  }
}
