import Foundation

/// A directional takeover of one speech press. It does not carry a language or
/// mutate preferences; the visible arc resolves the final release separately.
public struct VoiceTranslationGesture: Sendable {
  public static let upwardThreshold: Double = 24
  private var beganAt: TimeInterval?
  public private(set) var isSelecting = false
  public init() {}

  public mutating func down(at time: TimeInterval, allowed: Bool) {
    beganAt = allowed ? time : nil
    isSelecting = false
  }

  /// Returns true exactly once, including an early upward move held until the
  /// hold timer fires. Horizontal paging and downward cancel never enter it.
  public mutating func move(at time: TimeInterval, horizontal: Double, vertical: Double) -> Bool {
    guard let beganAt, !isSelecting, time.isFinite, horizontal.isFinite, vertical.isFinite,
      time - beganAt >= VoiceGesture.holdDelay,
      vertical <= -Self.upwardThreshold, abs(vertical) > abs(horizontal) else { return false }
    isSelecting = true
    return true
  }

  @discardableResult public mutating func end() -> Bool {
    let selecting = isSelecting
    beganAt = nil; isSelecting = false
    return selecting
  }
}
