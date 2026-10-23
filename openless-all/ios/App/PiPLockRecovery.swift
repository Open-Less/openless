import Foundation

/// Grants one unlock recovery attempt only for work observed at lock time.
/// An ordinary background stop cannot create a recovery grant.
nonisolated struct PiPLockRecovery {
  private var isLocked = false
  private var activeGeneration: UUID?
  private var interrupted = false

  mutating func didLock(generation: UUID, active: Bool, recovering: Bool) {
    guard !isLocked else { return }
    isLocked = true
    activeGeneration = active ? generation : nil
    interrupted = recovering
  }

  mutating func didStop(generation: UUID) {
    if isLocked, activeGeneration == generation { interrupted = true }
  }

  mutating func consumeOnUnlock(generation: UUID, active: Bool) -> Bool {
    // AVKit can flip active before delivering didStop. Match the generation
    // observed at lock so an unrelated replacement cannot claim that stop.
    let needsRecovery = !active && (interrupted || (isLocked && activeGeneration == generation))
    self = Self()
    return needsRecovery
  }

  mutating func reset() { self = Self() }
}
