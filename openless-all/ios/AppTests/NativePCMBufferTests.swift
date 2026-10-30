import Foundation
import Testing
@testable import OpenLess

nonisolated struct NativePCMBufferTests {
  @Test func backgroundBurstAndDrainedTailAreAvailableWithoutMainQueueDelivery() throws {
    let buffer = NativePCMBuffer(capacity: 4096)
    try buffer.begin(id: "recording")
    // More callbacks than the UI delivery queue can hold. The callback queue
    // completes while the caller is blocked, without pumping the main run loop.
    let expected = Data((0..<128).map(UInt8.init))
    DispatchQueue(label: "native-pcm-test").sync {
      for byte in expected {
        #expect(buffer.append(Data([byte]), id: "recording") == .consumed)
      }
      #expect(buffer.append(Data([250, 251]), id: "recording") == .consumed)
    }
    #expect(try buffer.finish(id: "recording") == expected + Data([250, 251]))
    #expect(buffer.append(Data([252]), id: "recording") == .consumed)
    #expect(throws: NativePCMBuffer.Failure.inactiveSession) {
      try buffer.finish(id: "recording")
    }
  }

  @Test func cancelledCallbacksCannotContaminateReplacementRecording() async throws {
    let buffer = NativePCMBuffer(capacity: 4096)
    try buffer.begin(id: "old")
    _ = buffer.append(Data([1, 2, 3]), id: "old")
    buffer.cancel(id: "old")
    try buffer.begin(id: "new")
    let staleCallbacks = Task.detached {
      for _ in 0..<1000 { _ = buffer.append(Data([255]), id: "old") }
      buffer.cancel(id: "old")
    }
    let expected = Data((0..<128).map(UInt8.init))
    for byte in expected { _ = buffer.append(Data([byte]), id: "new") }
    await staleCallbacks.value
    #expect(try buffer.finish(id: "new") == expected)
  }

  @Test func capacityAllowsExactLimitButNeverReturnsTruncatedAudio() throws {
    let buffer = NativePCMBuffer(capacity: 4)
    try buffer.begin(id: "exact")
    #expect(buffer.append(Data([1, 2, 3, 4]), id: "exact") == .consumed)
    #expect(try buffer.finish(id: "exact") == Data([1, 2, 3, 4]))
    try buffer.begin(id: "overflow")
    #expect(buffer.append(Data([1, 2, 3]), id: "overflow") == .consumed)
    #expect(buffer.append(Data([4, 5]), id: "overflow") == .overflow)
    #expect(buffer.append(Data([6]), id: "overflow") == .consumed)
    #expect(throws: NativePCMBuffer.Failure.capacityExceeded) {
      try buffer.finish(id: "overflow")
    }
    try buffer.begin(id: "recovered")
    #expect(buffer.append(Data([7]), id: "recovered") == .consumed)
    #expect(try buffer.finish(id: "recovered") == Data([7]))
  }

  @Test func wrongSessionCannotFinishOrReplaceActiveRecording() throws {
    let buffer = NativePCMBuffer(capacity: 4)
    try buffer.begin(id: "active")
    #expect(throws: NativePCMBuffer.Failure.activeSession) { try buffer.begin(id: "other") }
    #expect(throws: NativePCMBuffer.Failure.inactiveSession) { try buffer.finish(id: "other") }
    _ = buffer.append(Data([42]), id: "active")
    #expect(try buffer.finish(id: "active") == Data([42]))
  }

  @Test func onlyLiveAppleSessionForwardsToMainActor() throws {
    let buffer = NativePCMBuffer(capacity: 1)
    try buffer.begin(id: "apple", forwardsToMain: true)
    #expect(buffer.append(Data([1, 2]), id: "apple") == .forwardToMain)
    #expect(buffer.append(Data([1]), id: "stale") == .consumed)
    #expect(try buffer.finish(id: "apple").isEmpty)
    #expect(buffer.append(Data([2]), id: "apple") == .consumed)
  }
}
