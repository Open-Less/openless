import CSQLite
import Foundation
import Testing
@testable import OpenLessShared

struct MailboxQueueExpiryTests {
  private let now = Date(timeIntervalSince1970: 1_800_000_000)
  private let target = InputTarget(keyboardID: UUID(), documentID: UUID(), revision: 7)

  private func command(at date: Date, kind: CommandKind = .ping) -> VoiceCommand {
    VoiceCommand(kind: kind, sessionID: UUID(), hostEpoch: UUID(),
      target: target, language: .english, now: date)
  }
  private func database() throws -> (Mailbox, URL, URL) {
    let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
    let url = directory.appendingPathComponent("mailbox.sqlite")
    return (try Mailbox(url: url), url, directory)
  }
  // Seed the actual pre-fix on-disk schema, without enqueue's new expiry cleanup.
  // This represents queues left behind by an old extension while the host is absent.
  private func seedLegacyQueue(_ commands: [VoiceCommand], at url: URL) throws {
    var db: OpaquePointer?
    guard sqlite3_open_v2(url.path, &db, SQLITE_OPEN_READWRITE | SQLITE_OPEN_FULLMUTEX, nil) == SQLITE_OK,
      let db else { throw ContractError.database("fixture-open") }
    defer { sqlite3_close(db) }
    guard sqlite3_exec(db, "BEGIN IMMEDIATE", nil, nil, nil) == SQLITE_OK else {
      throw ContractError.database("fixture-begin")
    }
    var committed = false
    defer { if !committed { sqlite3_exec(db, "ROLLBACK", nil, nil, nil) } }
    for command in commands {
      var stmt: OpaquePointer?
      guard sqlite3_prepare_v2(db,
        "INSERT INTO commands(id,payload,consumed,deadline) VALUES(?,?,0,?)", -1, &stmt, nil) == SQLITE_OK,
        let stmt else { throw ContractError.database("fixture-prepare") }
      defer { sqlite3_finalize(stmt) }
      let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)
      let payload = try SharedContract.encode(command)
      _ = command.requestID.uuidString.withCString { sqlite3_bind_text(stmt, 1, $0, -1, transient) }
      _ = payload.withUnsafeBytes { sqlite3_bind_blob(stmt, 2, $0.baseAddress, Int32($0.count), transient) }
      sqlite3_bind_double(stmt, 3, command.deadline.timeIntervalSince1970)
      guard sqlite3_step(stmt) == SQLITE_DONE else { throw ContractError.database("fixture-write") }
    }
    guard sqlite3_exec(db, "COMMIT", nil, nil, nil) == SQLITE_OK else {
      throw ContractError.database("fixture-commit")
    }
    committed = true
  }
  private func drain(_ mailbox: Mailbox, at date: Date) throws -> [VoiceCommand] {
    var result: [VoiceCommand] = []
    while true {
      let batch = try mailbox.claimCommands(now: date)
      if batch.isEmpty { return result }
      result += batch
    }
  }
  private func isQueueFull(_ failure: any Error) -> Bool {
    guard case ContractError.database(let kind) = failure else { return false }
    return kind == "queue-full"
  }

  @Test func expiredLegacyFullQueueAcceptsFreshRequestWithoutHostDrain() throws {
    let (writer, url, directory) = try database()
    defer { try? FileManager.default.removeItem(at: directory) }
    let expired = (0..<256).map { _ in command(at: now.addingTimeInterval(-20)) }
    try seedLegacyQueue(expired, at: url)
    let fresh = command(at: now, kind: .prepare)
    try writer.enqueue(fresh, now: now)
    let reader = try Mailbox(url: url)
    #expect(try drain(reader, at: now) == [fresh])
    #expect(try writer.claimCommands(now: now).isEmpty)
  }

  @Test func unexpiredFullQueueStillRejectsWithoutDeletingCommands() throws {
    let (writer, url, directory) = try database()
    defer { try? FileManager.default.removeItem(at: directory) }
    let live = (0..<256).map { _ in command(at: now) }
    try seedLegacyQueue(live, at: url)
    do {
      try writer.enqueue(command(at: now, kind: .start), now: now)
      Issue.record("A full unexpired queue must reject the new request")
    } catch { #expect(isQueueFull(error)) }
    let reader = try Mailbox(url: url)
    #expect(try drain(reader, at: now) == live)
  }

  @Test func mixedCleanupRetainsEveryLivePayloadAndItsOrder() throws {
    let (writer, url, directory) = try database()
    defer { try? FileManager.default.removeItem(at: directory) }
    let kinds: [CommandKind] = [.prepare, .start, .stop, .cancel, .insertionReceipt]
    var legacy: [VoiceCommand] = []
    var live: [VoiceCommand] = []
    for i in 0..<128 {
      legacy.append(command(at: now.addingTimeInterval(-20)))
      let item = command(at: now, kind: kinds[i % kinds.count])
      legacy.append(item); live.append(item)
    }
    try seedLegacyQueue(legacy, at: url)
    let fresh = command(at: now, kind: .cancel)
    try writer.enqueue(fresh, now: now)
    let reader = try Mailbox(url: url)
    #expect(try drain(reader, at: now) == live + [fresh])
  }

  @Test func exactDeadlineStaysValidAndOnlyExpiresAfterBoundary() throws {
    let (writer, url, directory) = try database()
    defer { try? FileManager.default.removeItem(at: directory) }
    let boundary = command(at: now.addingTimeInterval(-10))
    let live = (0..<255).map { _ in command(at: now) }
    #expect(boundary.deadline == now)
    #expect(boundary.rejection(now: now, epoch: UUID()) == nil)
    try seedLegacyQueue([boundary] + live, at: url)
    do {
      try writer.enqueue(command(at: now), now: now)
      Issue.record("deadline == now is still valid and must count toward capacity")
    } catch { #expect(isQueueFull(error)) }
    let later = now.addingTimeInterval(0.001)
    let fresh = command(at: later, kind: .prepare)
    try writer.enqueue(fresh, now: later)
    let reader = try Mailbox(url: url)
    #expect(try drain(reader, at: later) == live + [fresh])
  }
}
