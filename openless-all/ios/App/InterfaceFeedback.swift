import UIKit

/// Reuse Apple's generators so consecutive choices can keep the Taptic Engine prepared.
@MainActor
enum InterfaceFeedback {
  private static let selection = UISelectionFeedbackGenerator()
  private static let impact = UIImpactFeedbackGenerator(style: .light)

  static func selectionChanged(enabled: Bool) {
    guard enabled else { return }
    selection.selectionChanged()
    selection.prepare()
  }

  static func expansionChanged(enabled: Bool) {
    guard enabled else { return }
    impact.impactOccurred()
    impact.prepare()
  }
}
