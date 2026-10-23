import CSQLite
import Foundation

/// Cross-process persistence. This lock protects the SQLite handle; SQLite WAL
/// transactions provide the separate, process-to-process consistency boundary.
public final class Mailbox: @unchecked Sendable {
  private let lock = NSRecursiveLock()
  private var db: OpaquePointer?
  private let transient = unsafeBitCast(-1, to: sqlite3_destructor_type.self)

  public init(url: URL) throws {
    try FileManager.default.createDirectory(
      at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
    guard
      sqlite3_open_v2(
        url.path, &db, SQLITE_OPEN_CREATE | SQLITE_OPEN_READWRITE | SQLITE_OPEN_FULLMUTEX, nil)
        == SQLITE_OK
    else {
      if let db { sqlite3_close(db) }
      db = nil
      throw ContractError.database("open")
    }
    do {
      try execute(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA secure_delete=ON; PRAGMA busy_timeout=1000;"
      )
      try execute(
        """
        CREATE TABLE IF NOT EXISTS state (key TEXT PRIMARY KEY, value BLOB NOT NULL);
        CREATE TABLE IF NOT EXISTS commands (
            sequence INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT UNIQUE NOT NULL,
            payload BLOB NOT NULL, consumed INTEGER NOT NULL DEFAULT 0, deadline REAL NOT NULL);
        CREATE TABLE IF NOT EXISTS insertions (
            id TEXT PRIMARY KEY, payload BLOB NOT NULL, state TEXT NOT NULL, created REAL NOT NULL);
        """)
    } catch {
      sqlite3_close(db)
      db = nil
      throw error
    }
  }
  deinit { if let db { sqlite3_close(db) } }

  public func set<T: Encodable>(_ value: T, for key: String) throws {
    let data = try SharedContract.encode(value)
    try locked {
      let stmt = try prepare(
        "INSERT INTO state(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value"
      )
      defer { sqlite3_finalize(stmt) }
      bind(key, to: stmt, at: 1)
      bind(data, to: stmt, at: 2)
      try done(stmt)
    }
  }
  public func value<T: Decodable>(_ type: T.Type, for key: String) throws -> T? {
    try locked {
      let stmt = try prepare("SELECT value FROM state WHERE key=?")
      defer { sqlite3_finalize(stmt) }
      bind(key, to: stmt, at: 1)
      let result = sqlite3_step(stmt)
      if result == SQLITE_DONE { return nil }
      guard result == SQLITE_ROW else { throw ContractError.database("read") }
      return try SharedContract.decode(type, from: blob(stmt, column: 0))
    }
  }
  public func enqueue(_ command: VoiceCommand, now: Date = Date()) throws {
    let payload = try SharedContract.encode(command)
    try transaction {
      // Match VoiceCommand.rejection's strict deadline boundary. Reclaim stale
      // unclaimed commands even when the host has not run to claim/prune them.
      let prune = try prepare("DELETE FROM commands WHERE consumed=0 AND deadline<?")
      defer { sqlite3_finalize(prune) }
      sqlite3_bind_double(prune, 1, now.timeIntervalSince1970)
      try done(prune)
      let count = try scalar("SELECT count(*) FROM commands WHERE consumed=0")
      guard count < 256 else { throw ContractError.database("queue-full") }
      let stmt = try prepare("INSERT OR IGNORE INTO commands(id,payload,deadline) VALUES(?,?,?)")
      defer { sqlite3_finalize(stmt) }
      bind(command.requestID.uuidString, to: stmt, at: 1)
      bind(payload, to: stmt, at: 2)
      sqlite3_bind_double(stmt, 3, command.deadline.timeIntervalSince1970)
      try done(stmt)
    }
  }
  /// Claims a batch before dispatch. A host crash must lead to a client timeout,
  /// not blind replay of an operation which may already have opened the mic.
  public func claimCommands(now: Date = Date()) throws -> [VoiceCommand] {
    try transaction {
      let stmt = try prepare(
        "SELECT id,payload FROM commands WHERE consumed=0 ORDER BY sequence LIMIT 64")
      defer { sqlite3_finalize(stmt) }
      var rows: [(String, VoiceCommand?)] = []
      while true {
        let result = sqlite3_step(stmt)
        if result == SQLITE_DONE { break }
        guard result == SQLITE_ROW else { throw ContractError.database("commands") }
        rows.append(
          (
            string(stmt, column: 0),
            try? SharedContract.decode(VoiceCommand.self, from: blob(stmt, column: 1))
          ))
      }
      for (id, _) in rows {
        let update = try prepare("UPDATE commands SET consumed=1 WHERE id=?")
        defer { sqlite3_finalize(update) }
        bind(id, to: update, at: 1)
        try done(update)
      }
      let prune = try prepare("DELETE FROM commands WHERE deadline<?")
      defer { sqlite3_finalize(prune) }
      sqlite3_bind_double(prune, 1, now.addingTimeInterval(-60).timeIntervalSince1970)
      try done(prune)
      return rows.compactMap(\.1)
    }
  }
  public func storeInsertion(_ insertion: InsertionRequest) throws {
    guard insertion.text.utf8.count <= SharedContract.maxTextBytes else {
      throw ContractError.tooLarge
    }
    let data = try SharedContract.encode(insertion)
    try locked {
      let stmt = try prepare(
        "INSERT OR IGNORE INTO insertions(id,payload,state,created) VALUES(?,?,?,?)")
      defer { sqlite3_finalize(stmt) }
      bind(insertion.id.uuidString, to: stmt, at: 1)
      bind(data, to: stmt, at: 2)
      bind(InsertionState.pending.rawValue, to: stmt, at: 3)
      sqlite3_bind_double(stmt, 4, insertion.createdAt.timeIntervalSince1970)
      try done(stmt)
    }
  }
  public func insertion(_ id: UUID) throws -> InsertionRequest? {
    try locked {
      let stmt = try prepare("SELECT payload,state FROM insertions WHERE id=?")
      defer { sqlite3_finalize(stmt) }
      bind(id.uuidString, to: stmt, at: 1)
      let result = sqlite3_step(stmt)
      if result == SQLITE_DONE { return nil }
      guard result == SQLITE_ROW else { throw ContractError.database("insertion") }
      var item = try SharedContract.decode(InsertionRequest.self, from: blob(stmt, column: 0))
      item.state = InsertionState(rawValue: string(stmt, column: 1)) ?? .uncertain
      return item
    }
  }
  /// Commits 'attempting' BEFORE UIKit is called. Retrying an uncertain write
  /// requires a separate explicit user action, never the same delivery ID.
  public func claimInsertion(_ id: UUID, target: InputTarget, now: Date = Date()) throws
    -> InsertionRequest?
  {
    try transaction {
      guard var item = try insertion(id), item.state == .pending else { return nil }
      guard item.target == target, now.timeIntervalSince(item.createdAt) <= 300,
        item.createdAt <= now.addingTimeInterval(2)
      else {
        try setInsertionState(id, .targetChanged)
        return nil
      }
      try setInsertionState(id, .attempting)
      item.state = .attempting
      return item
    }
  }
  public func acknowledgeInsertion(_ id: UUID) throws {
    try transaction {
      guard try insertion(id)?.state == .attempting else { throw ContractError.invalidTransition }
      try setInsertionState(id, .submitted)
    }
  }
  /// Revoke an offer without erasing evidence of a write already attempted.
  public func revokeInsertion(_ id: UUID) throws {
    try transaction {
      guard let item = try insertion(id) else { return }
      switch item.state {
      case .pending: try setInsertionState(id, .targetChanged)
      case .attempting: try setInsertionState(id, .uncertain)
      case .submitted, .uncertain, .targetChanged: break
      }
    }
  }
  public func recoverUncertainInsertions() throws {
    try locked { try execute("UPDATE insertions SET state='uncertain' WHERE state='attempting'") }
  }
  public func pendingInsertions(now: Date = Date()) throws -> [InsertionRequest] {
    try locked {
      let prune = try prepare("DELETE FROM insertions WHERE created<?")
      defer { sqlite3_finalize(prune) }
      sqlite3_bind_double(prune, 1, now.addingTimeInterval(-86_400).timeIntervalSince1970)
      try done(prune)
      let stmt = try prepare(
        "SELECT id FROM insertions WHERE state!='submitted' ORDER BY created DESC LIMIT 100")
      defer { sqlite3_finalize(stmt) }
      var ids: [UUID] = []
      while sqlite3_step(stmt) == SQLITE_ROW {
        if let id = UUID(uuidString: string(stmt, column: 0)) { ids.append(id) }
      }
      return try ids.compactMap { try insertion($0) }
    }
  }
  public func clearDeliveredInsertions() throws {
    try locked { try execute("DELETE FROM insertions WHERE state='submitted'") }
  }
  public func requestHostVoice(_ request: HostVoiceActivation, now: Date = Date(), replacingUnresponsiveHost: Bool = false) throws {
    guard request.phase == .requested, request.isValid(at: now) else { throw ContractError.invalidTransition }
    try transaction {
      if let current = try value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey),
        current.isValid(at: now), [.starting, .recording, .processing].contains(current.phase) {
        // A terminated host cannot finish its old request. Only a caller that
        // already tried the live IPC handshake may replace it, and a renewed
        // host snapshot wins this race inside the same SQLite transaction.
        let snapshot = try value(VoiceSnapshot.self, for: SharedContract.snapshotKey)
        guard replacingUnresponsiveHost, snapshot?.isFresh(at: now) != true else {
          throw ContractError.invalidTransition
        }
      }
      try set(request, for: HostVoiceActivation.mailboxKey)
    }
  }
  public func updateHostVoice(_ value: HostVoiceActivation) throws {
    try transaction {
      guard let current = try self.value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey),
        current.id == value.id, current.phase != .consumed else { throw ContractError.invalidTransition }
      try set(value, for: HostVoiceActivation.mailboxKey)
    }
  }
  /// Bind a completed result to the currently attached input. One result can
  /// create at most one durable delivery even if the extension restarts.
  public func prepareHostVoiceInsertion(_ id: UUID, target: InputTarget, now: Date = Date()) throws -> InsertionRequest {
    try transaction {
      guard var handoff = try value(HostVoiceActivation.self, for: HostVoiceActivation.mailboxKey),
        handoff.id == id, handoff.isValid(at: now) else { throw ContractError.invalidTransition }
      if let existing = handoff.insertionID, let record = try insertion(existing) {
        guard record.target == target else { throw ContractError.invalidTransition }
        return record
      }
      guard handoff.phase == .ready, let text = handoff.text,
        !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { throw ContractError.invalidTransition }
      let request = InsertionRequest(sessionID: id, target: target, text: text, now: now)
      try storeInsertion(request)
      handoff.phase = .consumed; handoff.insertionID = request.id
      try set(handoff, for: HostVoiceActivation.mailboxKey)
      return request
    }
  }
  public func discardInsertion(_ id: UUID) throws {
    try locked {
      let stmt = try prepare("DELETE FROM insertions WHERE id=?")
      defer { sqlite3_finalize(stmt) }
      bind(id.uuidString, to: stmt, at: 1)
      try done(stmt)
    }
  }

  private func setInsertionState(_ id: UUID, _ state: InsertionState) throws {
    let stmt = try prepare("UPDATE insertions SET state=? WHERE id=?")
    defer { sqlite3_finalize(stmt) }
    bind(state.rawValue, to: stmt, at: 1)
    bind(id.uuidString, to: stmt, at: 2)
    try done(stmt)
  }
  private func transaction<T>(_ body: () throws -> T) throws -> T {
    try locked {
      try execute("BEGIN IMMEDIATE")
      do {
        let value = try body()
        try execute("COMMIT")
        return value
      } catch {
        try? execute("ROLLBACK")
        throw error
      }
    }
  }
  private func locked<T>(_ body: () throws -> T) rethrows -> T {
    lock.lock()
    defer { lock.unlock() }
    return try body()
  }
  private func execute(_ sql: String) throws {
    guard sqlite3_exec(db, sql, nil, nil, nil) == SQLITE_OK else {
      throw ContractError.database("execute")
    }
  }
  private func prepare(_ sql: String) throws -> OpaquePointer {
    var stmt: OpaquePointer?
    guard sqlite3_prepare_v2(db, sql, -1, &stmt, nil) == SQLITE_OK, let stmt else {
      throw ContractError.database("prepare")
    }
    return stmt
  }
  private func done(_ stmt: OpaquePointer) throws {
    guard sqlite3_step(stmt) == SQLITE_DONE else { throw ContractError.database("write") }
  }
  private func scalar(_ sql: String) throws -> Int {
    let stmt = try prepare(sql)
    defer { sqlite3_finalize(stmt) }
    guard sqlite3_step(stmt) == SQLITE_ROW else { throw ContractError.database("count") }
    return Int(sqlite3_column_int64(stmt, 0))
  }
  private func bind(_ string: String, to stmt: OpaquePointer, at index: Int32) {
    _ = string.withCString { sqlite3_bind_text(stmt, index, $0, -1, transient) }
  }
  private func bind(_ data: Data, to stmt: OpaquePointer, at index: Int32) {
    _ = data.withUnsafeBytes {
      sqlite3_bind_blob(stmt, index, $0.baseAddress, Int32($0.count), transient)
    }
  }
  private func blob(_ stmt: OpaquePointer, column: Int32) -> Data {
    guard let pointer = sqlite3_column_blob(stmt, column) else { return Data() }
    return Data(bytes: pointer, count: Int(sqlite3_column_bytes(stmt, column)))
  }
  private func string(_ stmt: OpaquePointer, column: Int32) -> String {
    guard let pointer = sqlite3_column_text(stmt, column) else { return "" }
    return String(cString: pointer)
  }
}
