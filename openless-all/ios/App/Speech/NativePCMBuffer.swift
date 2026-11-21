import Foundation
import OpenLessShared

nonisolated enum NativeAudioDisposition: Sendable, Equatable {
  case consumed
  case forwardToMain
  case overflow
}

/// The native callback owns no UI state. A session's PCM is collected here
/// synchronously, even while the main run loop is suspended in the background.
nonisolated final class NativePCMBuffer: @unchecked Sendable {
  enum Failure: Error, Equatable { case activeSession, inactiveSession, capacityExceeded }
  private struct Recording {
    let id: String
    let forwardsToMain: Bool
    var pcm = Data()
    var overflowed = false
  }
  private let lock = NSLock()
  private let capacity: Int
  private var recording: Recording?

  init(capacity: Int = 32_000 * SharedContract.maxRecordingSeconds) {
    precondition(capacity > 0)
    self.capacity = capacity
  }

  func begin(id: String, forwardsToMain: Bool = false) throws {
    lock.lock()
    defer { lock.unlock() }
    guard recording == nil else { throw Failure.activeSession }
    recording = Recording(id: id, forwardsToMain: forwardsToMain)
  }

  func append(_ pcm: Data, id: String) -> NativeAudioDisposition {
    lock.lock()
    defer { lock.unlock() }
    guard recording?.id == id else { return .consumed }
    if recording!.forwardsToMain { return .forwardToMain }
    guard !recording!.overflowed else { return .consumed }
    guard pcm.count <= capacity - recording!.pcm.count else {
      // Never recognize a silently truncated recording; report the first
      // overflow once and retain a terminal error until finish/cancel.
      recording!.overflowed = true
      recording!.pcm = Data()
      return .overflow
    }
    recording!.pcm.append(pcm)
    return .consumed
  }

  /// Call after Core's captureStop/drain barrier. Taking atomically seals the
  /// session so no delayed callback can mutate the recognition input.
  func finish(id: String) throws -> Data {
    lock.lock()
    defer { lock.unlock() }
    guard let current = recording, current.id == id else { throw Failure.inactiveSession }
    recording = nil
    guard !current.overflowed else { throw Failure.capacityExceeded }
    return current.pcm
  }

  func cancel(id: String? = nil) {
    lock.lock()
    defer { lock.unlock() }
    guard id == nil || recording?.id == id else { return }
    recording = nil
  }
}
