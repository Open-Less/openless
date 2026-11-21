import Foundation

/// Bounds the foreign-callback -> UI hop as well as the audio stream itself.
/// Every mutable field is guarded by lock; no callback is invoked under it.
public final class NativeDelivery: @unchecked Sendable {
  public enum Admission: Equatable, Sendable { case accept, overflow, discard }
  private struct Flow {
    var session = ""
    var queued = 0
    var poisoned = false
    var level: Data?
    var levelScheduled = false
  }
  private let lock = NSLock()
  private var flows: [UInt64: Flow] = [:]
  private let limit: Int
  public init(limit: Int = 16) {
    precondition(limit > 0)
    self.limit = limit
  }
  public func reservePCM(token: UInt64, session: String) -> Admission {
    lock.lock()
    defer { lock.unlock() }
    var flow = flows[token] ?? Flow()
    if flow.session != session {
      flow.session = session
      flow.queued = 0
      flow.poisoned = false
    }
    if flow.poisoned { return .discard }
    if flow.queued >= limit {
      flow.poisoned = true
      flows[token] = flow
      return .overflow
    }
    flow.queued += 1
    flows[token] = flow
    return .accept
  }
  public func releasePCM(token: UInt64, session: String) {
    lock.lock()
    defer { lock.unlock() }
    guard var flow = flows[token], flow.session == session else { return }
    flow.queued = max(0, flow.queued - 1)
    flows[token] = flow
  }
  public func offerLevel(token: UInt64, data: Data) -> Bool {
    lock.lock()
    defer { lock.unlock() }
    var flow = flows[token] ?? Flow()
    let schedule = !flow.levelScheduled
    flow.level = data
    flow.levelScheduled = true
    flows[token] = flow
    return schedule
  }
  public func takeLevel(token: UInt64) -> Data? {
    lock.lock()
    defer { lock.unlock() }
    guard var flow = flows[token] else { return nil }
    let data = flow.level
    flow.level = nil
    flow.levelScheduled = false
    flows[token] = flow
    return data
  }
  public func remove(token: UInt64) {
    lock.lock()
    defer { lock.unlock() }
    flows[token] = nil
  }
}
