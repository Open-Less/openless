import Combine
import Foundation
import OpenLessShared

/// Main-App owner. The transport is CoreBridge.control; the keyboard receives
/// only bounded projections and sends an explicit VocabularyDecision via parent IPC.
@MainActor final class VocabularyBridge: ObservableObject {
  typealias Control = @MainActor ([String: Any]) async throws -> Data
  @Published private(set) var snapshot = VocabularySnapshot(generation: 0, entries: [])
  @Published private(set) var proposals = VocabularyProposals(generation: 0, proposals: [])
  @Published private(set) var isBusy = false
  @Published private(set) var errorMessage: String?
  private let control: Control
  private let mailbox: Mailbox?

  init(control: @escaping Control, mailbox: Mailbox?) {
    self.control = control
    self.mailbox = mailbox
  }
  func refresh() async throws {
    guard !isBusy else { throw VocabularyBridgeError.busy }
    isBusy = true
    defer { isBusy = false }
    do { try await load(); errorMessage = nil }
    catch { clearPublishedState(); throw error }
  }
  /// Call only from an explicit confirmation UI (including manual add/update).
  /// Never invoke this for all rows returned by a vocabulary refresh.
  func confirmEntry(id: String, phrase: String) async throws {
    try await mutate(["command": "confirmEntry", "id": id, "phrase": phrase,
                      "expectedGeneration": snapshot.generation])
  }
  func apply(_ decision: VocabularyDecision) async throws {
    guard decision.expectedGeneration == proposals.generation else { throw VocabularyBridgeError.stale }
    var request: [String: Any] = ["command": decision.action.rawValue]
    if decision.action != .dismiss {
      guard let id = decision.proposalID,
        proposals.visible().contains(where: { $0.id == id })
      else { throw VocabularyBridgeError.stale }
      request["id"] = id
    }
    if decision.action == .accept { request["expectedGeneration"] = decision.expectedGeneration }
    try await mutate(request)
  }
  private func mutate(_ request: [String: Any]) async throws {
    guard !isBusy else { throw VocabularyBridgeError.busy }
    isBusy = true
    defer { isBusy = false }
    do {
      _ = try await send(request)
      try await load()
      errorMessage = nil
    } catch { clearPublishedState(); throw error }
  }
  private func send(_ request: [String: Any]) async throws -> Data {
    let data = try await control(["action": "vocabularyBridge", "request": request])
    guard data.count <= VocabularySnapshot.maxBytes else { throw VocabularyValidation.invalid }
    return data
  }
  private func load() async throws {
    let next = try VocabularySnapshot.decode(await send(["command": "snapshot"]))
    let candidates = try JSONDecoder().decode(VocabularyProposals.self, from: await send(["command": "proposals"]))
    try candidates.validate()
    guard candidates.generation == next.generation else { throw VocabularyBridgeError.stale }
    // Shared writes contain neither private ledger nor account identifiers.
    try mailbox?.set(next, for: VocabularySnapshot.mailboxKey)
    try mailbox?.set(candidates, for: VocabularyProposals.mailboxKey)
    snapshot = next
    proposals = candidates
  }
  /// Call on account transition as well. Authenticated export is unsupported;
  /// the parent must not relabel local consent with a login/account identity.
  func clearPublishedState() {
    snapshot = VocabularySnapshot(generation: 0, entries: [])
    proposals = VocabularyProposals(generation: 0, proposals: [])
    try? mailbox?.set(snapshot, for: VocabularySnapshot.mailboxKey)
    try? mailbox?.set(proposals, for: VocabularyProposals.mailboxKey)
    errorMessage = "词典确认状态不可用，请刷新后重试。"
  }
}
enum VocabularyBridgeError: Error { case busy, stale }
