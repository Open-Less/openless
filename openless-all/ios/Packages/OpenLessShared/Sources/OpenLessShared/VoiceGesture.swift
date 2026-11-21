import Foundation

/// Pure input policy; one physical press can never produce both tap and hold.
public struct VoiceGesture: Sendable {
  public enum Action: Equatable, Sendable { case start, stop, cancel }
  private var beganAt: TimeInterval?
  private var held = false
  private var cancelled = false
  private var initialPhase: VoicePhase = .inactive
  public static let holdDelay: TimeInterval = 0.3
  public init() {}

  public mutating func down(at time: TimeInterval, phase: VoicePhase) {
    beganAt = time
    held = false
    cancelled = false
    initialPhase = phase
  }
  public mutating func tick(at time: TimeInterval) -> Action? {
    guard let beganAt, !held, !cancelled,
      time - beganAt >= Self.holdDelay,
      [.ready, .done, .cancelled, .error].contains(initialPhase)
    else { return nil }
    held = true
    return .start
  }
  public mutating func up(phase: VoicePhase, inside: Bool = true) -> Action? {
    guard beganAt != nil else { return nil }
    defer {
      beganAt = nil
      held = false
      cancelled = false
    }
    guard !cancelled else { return nil }
    if !inside { return held || phase == .recording || phase == .starting ? .cancel : nil }
    if held { return phase == .starting ? .cancel : .stop }
    switch initialPhase {
    case .ready, .done, .cancelled, .error: return .start
    case .recording: return .stop
    case .starting: return .cancel
    default: return nil
    }
  }
  public mutating func cancel() -> Action? {
    guard beganAt != nil else { return nil }
    let action: Action? = held ? .cancel : nil
    cancelled = true
    beganAt = nil
    return action
  }
}

/// Speech modules revise volatile ranges: replace by audio timestamp, never
/// concatenate every partial as if it were final.
public struct TranscriptAccumulator: Sendable {
  public struct Segment: Sendable, Equatable {
    public let start: Double
    public let end: Double
    public let text: String
    public let final: Bool
    public init(start: Double, end: Double, text: String, final: Bool) {
      self.start = start
      self.end = end
      self.text = text
      self.final = final
    }
  }
  public private(set) var segments: [Segment] = []
  public init() {}
  public mutating func apply(_ segment: Segment) {
    guard segment.start.isFinite, segment.end.isFinite, segment.end >= segment.start else { return }
    // A late volatile result must not overwrite an already-final segment.
    if !segment.final
      && segments.contains(where: { $0.final && $0.start <= segment.start && $0.end >= segment.end }
      )
    {
      return
    }
    segments.removeAll {
      $0.start < segment.end && $0.end > segment.start || $0.start == segment.start
    }
    segments.append(segment)
    segments.sort { $0.start < $1.start }
  }
  public var text: String { segments.map(\.text).joined() }
}

public struct KeepAliveEvidence: Codable, Equatable, Sendable {
  public let systemBuild: String
  public let deviceModel: String
  public let backgroundRestartPassed: Bool
  public let idleMicrophoneReleased: Bool
  public let noVisibleSurface: Bool
  public let evidenceID: String
  public var accepted: Bool {
    backgroundRestartPassed && idleMicrophoneReleased && noVisibleSurface && !evidenceID.isEmpty
  }
}
public enum KeepAliveGate {
  public static func allows(
    systemBuild: String, deviceModel: String,
    records: [KeepAliveEvidence], experimentEnabled: Bool
  ) -> Bool {
    experimentEnabled
      && records.contains {
        $0.systemBuild == systemBuild && $0.deviceModel == deviceModel && $0.accepted
      }
  }
}
