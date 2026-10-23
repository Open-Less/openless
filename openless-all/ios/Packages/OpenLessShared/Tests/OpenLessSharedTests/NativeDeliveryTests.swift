import Foundation
import Testing

@testable import OpenLessShared

struct NativeDeliveryTests {
  @Test func stalledUIHasBoundedAudioAndOnlyOneFailure() {
    let queue = NativeDelivery(limit: 2)
    #expect(queue.reservePCM(token: 1, session: "a") == .accept)
    #expect(queue.reservePCM(token: 1, session: "a") == .accept)
    #expect(queue.reservePCM(token: 1, session: "a") == .overflow)
    for _ in 0..<10_000 { #expect(queue.reservePCM(token: 1, session: "a") == .discard) }
    queue.releasePCM(token: 1, session: "a")
    #expect(queue.reservePCM(token: 1, session: "a") == .discard)
    #expect(queue.reservePCM(token: 1, session: "b") == .accept)
  }
  @Test func staleReleaseCannotConsumeANewSessionSlot() {
    let queue = NativeDelivery(limit: 1)
    #expect(queue.reservePCM(token: 1, session: "a") == .accept)
    #expect(queue.reservePCM(token: 1, session: "b") == .accept)
    queue.releasePCM(token: 1, session: "a")
    #expect(queue.reservePCM(token: 1, session: "b") == .overflow)
  }
  @Test func metersCoalesceInsteadOfAccumulatingCallbacks() {
    let queue = NativeDelivery()
    #expect(queue.offerLevel(token: 1, data: Data([0])))
    for value in 1...100 {
      #expect(!queue.offerLevel(token: 1, data: Data([UInt8(value)])))
    }
    #expect(queue.takeLevel(token: 1) == Data([100]))
    #expect(queue.takeLevel(token: 1) == nil)
    #expect(queue.offerLevel(token: 1, data: Data([2])))
    queue.remove(token: 1)
    #expect(queue.takeLevel(token: 1) == nil)
  }
}
