import Combine
import Foundation

@MainActor
final class StyleLibrary: ObservableObject {
  typealias Request = @MainActor ([String: Any]) async throws -> Data
  @Published private(set) var snapshot: StyleLibrarySnapshot?
  @Published private(set) var capabilities: StyleCapabilities?
  @Published private(set) var translation: StyleTranslationConfiguration?
  @Published private(set) var market: [MarketStyle] = []
  @Published private(set) var marketLoaded = false
  @Published private(set) var marketQuery = ""
  @Published private(set) var operation: String?
  @Published private(set) var activityBusy = false
  @Published private(set) var canCancelRead = false
  @Published private(set) var notice: String?
  /// Style whose activation is in flight. Feedback stays on that card only (no global busy UI).
  @Published private(set) var activatingStyleID: String?
  /// A refresh with no visible operation row (tab re-entry, pull to refresh, config change).
  @Published private(set) var silentReading = false
  @Published var error: String?
  @Published var pendingImport: URL?
  private var request: Request?
  private var connectionID = UUID()
  private var discardRead = false
  var ready: Bool { request != nil }
  /// Visible busy state (dims/disables controls). Card activation and silent refreshes deliberately
  /// do not enter it, so they cannot flash the page; `inFlight` still serialises them in code.
  var blocked: Bool { !ready || activityBusy || operation != nil }
  private var inFlight: Bool { activatingStyleID != nil || silentReading }

  init(core: CoreBridge?) { connect(core) }
  // Narrow transport seam for model tests; production always uses CoreBridge.
  init(request: @escaping Request) { self.request = request }
  func connect(_ core: CoreBridge?) {
    connectionID = UUID()
    if let core {
      request = { [weak core] action in
        guard let core else { throw DeveloperError.unavailable }
        return try await core.control(action)
      }
    } else { request = nil }
    snapshot = nil; capabilities = nil; translation = nil; market = []
    marketLoaded = false; marketQuery = ""; operation = nil; canCancelRead = false; activatingStyleID = nil; silentReading = false
    error = nil; notice = nil; pendingImport = nil
  }
  func setActivityBusy(_ value: Bool) { activityBusy = value }
  func cancelRead() {
    guard canCancelRead else { return }
    discardRead = true; canCancelRead = false
    notice = "已取消"
  }
  private func begin(_ title: String, read: Bool = false, silent: Bool = false) -> UUID? {
    guard !blocked, !inFlight else { return nil }
    if silent {
      silentReading = true; discardRead = false
    } else {
      operation = title; error = nil; notice = nil; discardRead = false; canCancelRead = read
    }
    return connectionID
  }
  private func finish(_ id: UUID) {
    if connectionID == id { operation = nil; canCancelRead = false; silentReading = false }
  }
  private func call<T: Decodable>(_ action: [String: Any], id: UUID) async throws -> T {
    guard let request, connectionID == id else { throw CancellationError() }
    let data = try await request(action)
    try Task.checkCancellation()
    guard connectionID == id, !discardRead else { throw CancellationError() }
    return try JSONDecoder().decode(T.self, from: data)
  }
  private func capture(_ failure: any Error, id: UUID) {
    if connectionID == id, !(failure is CancellationError) { error = failure.localizedDescription }
  }
  private func readLocal(_ id: UUID) async throws {
    let library: StyleLibrarySnapshot = try await call(["action": "styleLibrary"], id: id)
    let capability: StyleCapabilities = try await call(["action": "capabilitySnapshot"], id: id)
    let languages: StyleTranslationConfiguration = try await call(["action": "translationConfiguration"], id: id)
    snapshot = library; capabilities = capability; translation = languages
  }
  /// `silent` keeps the page visually still: no operation row, no dimmed cards.
  func load(silent: Bool = false) async {
    guard let id = begin("正在载入…", read: true, silent: silent) else { return }
    defer { finish(id) }
    do { try await readLocal(id); if silent { error = nil } } catch { capture(error, id: id) }
  }
  func detail(_ styleID: String) async throws -> StyleDetail {
    guard let id = begin("正在读取详情…", read: true) else { throw DeveloperError.unavailable }
    defer { finish(id) }
    return try await call(["action": "styleDetail", "id": styleID], id: id)
  }
  func searchMarket(query: String) async {
    struct Response: Decodable { let items: [MarketStyle] }
    guard let id = begin("正在载入…", read: true) else { return }
    defer { finish(id) }
    do {
      let result: Response = try await call(["action": "marketplaceList", "query": query, "limit": 50], id: id)
      market = result.items; marketLoaded = true; marketQuery = query
    } catch { capture(error, id: id) }
  }
  func marketDetail(_ styleID: String) async throws -> MarketStyleDetail {
    struct Response: Decodable { let detail: MarketStyleDetail }
    guard let id = begin("正在载入…", read: true) else { throw DeveloperError.unavailable }
    defer { finish(id) }
    let result: Response = try await call(["action": "marketplaceDetail", "id": styleID], id: id)
    return result.detail
  }
  func setEnabled(_ style: InstalledStyle, enabled: Bool) async {
    guard capabilities?.canChangeEnabled(style) == true else { return }
    await mutate(["action": "setStyleEnabled", "id": style.id, "enabled": enabled])
  }
  func activate(_ style: InstalledStyle) async {
    guard capabilities?.canActivate(style) == true else { return }
    await mutate(["action": "activateStyle", "id": style.id], localStyleID: style.id)
  }
  func remove(_ style: InstalledStyle) async {
    guard style.canRemove else { return }
    await mutate(["action": "removeStyle", "id": style.id])
  }
  func install(_ styleID: String) async { await mutate(["action": "marketplaceInstall", "id": styleID]) }
  private func mutate(_ action: [String: Any], localStyleID: String? = nil) async {
    let token: UUID
    if let localStyleID {
      // Local feedback: no `operation`/`notice`, so no page-level progress row is inserted.
      guard !blocked, !inFlight else { return }
      error = nil; discardRead = false
      activatingStyleID = localStyleID; token = connectionID
    } else {
      guard let id = begin("正在保存…") else { return }
      token = id
    }
    let id = token
    defer {
      if localStyleID != nil { if connectionID == id { activatingStyleID = nil } } else { finish(id) }
    }
    do {
      struct Response: Decodable {}
      let _: Response = try await call(action, id: id)
      if localStyleID == nil { notice = "已保存" }
      // Reconciliation (including Core's non-raw removal fallback) is authoritative.
      do { try await readLocal(id) }
      catch {
        capture(error, id: id)
        if let message = self.error { self.error = "Core 已保存，但刷新失败；请刷新后再操作。\n" + message }
      }
    } catch { capture(error, id: id) }
  }
  func importPending() async {
    guard let source = pendingImport, let policy = snapshot?.archive,
          let id = begin("正在导入风格包…") else { return }
    defer { finish(id) }
    var staged: StyleArchiveImport.Staged?
    do {
      let copy = try await Task.detached { try StyleArchiveImport.stage(source: source, policy: policy) }.value
      staged = copy
      try Task.checkCancellation()
      guard connectionID == id else { throw CancellationError() }
      guard !activityBusy else { throw DeveloperError.rejected("语音活动进行中，请稍后重试导入。") }
      struct Response: Decodable {}
      let _: Response = try await call(["action": "importStyleArchive", "stagedFileName": copy.leaf], id: id)
      pendingImport = nil; notice = "已导入，可在详情中选择使用"
      do { try await readLocal(id) }
      catch {
        capture(error, id: id)
        if let message = self.error { self.error = "已导入，但刷新失败；请勿重复导入，请先刷新。\n" + message }
      }
    } catch { capture(error, id: id) }
    if let staged {
      do { try staged.remove() } catch {
        if connectionID == id {
          self.error = [self.error, "临时 ZIP 清理失败：\(error.localizedDescription)"].compactMap { $0 }.joined(separator: "\n")
        }
      }
    }
  }
  func changeTranslationTargets(action: String, target: String? = nil, targets: [String]? = nil) async {
    guard ["addTranslationTarget", "deleteTranslationTarget", "reorderTranslationTargets", "selectTranslationTarget"].contains(action),
      let id = begin("正在保存翻译目标…") else { return }
    defer { finish(id) }
    var request: [String: Any] = ["action": action]
    if let targets { request["targets"] = targets }
    if let target { request["target"] = target }
    do {
      let result: StyleTranslationConfiguration = try await call(request, id: id)
      translation = result
      capabilities = try await call(["action": "capabilitySnapshot"], id: id)
    } catch { capture(error, id: id) }
  }

  func setTranslationTarget(_ target: String) async {
    guard let id = begin("正在保存翻译目标…") else { return }
    defer { finish(id) }
    do {
      let result: StyleTranslationConfiguration = try await call(["action": "setTranslationTarget", "targetLanguage": target], id: id)
      translation = result
      capabilities = try await call(["action": "capabilitySnapshot"], id: id)
    } catch { capture(error, id: id) }
  }
}
