import Combine
import Foundation

@MainActor
final class AccountSettings: ObservableObject {
  typealias Request = @MainActor ([String: Any]) async throws -> Data
  typealias StatusDecoder = @MainActor (Data) throws -> AccountStatus
  enum Operation { case loading, starting, polling, cancelling, signingOut }
  @Published private(set) var status: AccountStatus?
  @Published private(set) var operation: Operation?
  @Published private(set) var userCode: String?
  @Published private(set) var secondsRemaining = 0
  @Published private(set) var pollingInterval = 0
  @Published private(set) var activityBusy = false
  @Published private(set) var notice: String?
  @Published private(set) var error: String?
  @Published private(set) var cancelRequested = false
  @Published private(set) var signOutRequested = false
  private struct Flow {
    let id: String
    let code: String
    let expiresAt: TimeInterval
    var interval: Int
    var nextPollAt: TimeInterval
  }
  private var flow: Flow?
  private var request: Request?
  private var connectionID = UUID()
  private let now: @MainActor () -> TimeInterval
  private let decodeStatus: StatusDecoder
  private var visible = false
  private var foreground = false
  private var pollFailed = false
  var ready: Bool { request != nil }
  var blocked: Bool { !ready || activityBusy || operation != nil || cancelRequested || signOutRequested }
  var hasFlow: Bool { flow != nil || operation == .starting }
  var canCancel: Bool { ready && hasFlow && !cancelRequested && !signOutRequested }
  var needsPollRetry: Bool { flow != nil && pollFailed && !cancelRequested && !signOutRequested }
  var pollingPaused: Bool { hasFlow && (!visible || !foreground || activityBusy || pollFailed) }

  init(core: CoreBridge?) {
    now = { ProcessInfo.processInfo.systemUptime }
    decodeStatus = { try JSONDecoder().decode(AccountStatus.self, from: $0) }
    connect(core)
  }
  // Pure transport/clock/decoder seams: tests never open Core, Keychain or HTTP.
  init(request: @escaping Request, now: @escaping @MainActor () -> TimeInterval = { ProcessInfo.processInfo.systemUptime },
       decodeStatus: @escaping StatusDecoder = { try JSONDecoder().decode(AccountStatus.self, from: $0) }) {
    self.request = request; self.now = now; self.decodeStatus = decodeStatus
  }
  func connect(_ core: CoreBridge?) {
    if let core {
      let transport: Request = { [weak core] action in
        guard let core else { throw AccountResponseError.invalid }
        return try await core.control(action)
      }
      replaceConnection(transport)
    } else { replaceConnection(nil) }
  }
  func replaceConnection(_ request: Request?) {
    connectionID = UUID(); self.request = request
    status = nil; operation = nil; flow = nil; userCode = nil
    secondsRemaining = 0; pollingInterval = 0; pollFailed = false
    cancelRequested = false; signOutRequested = false; notice = nil; error = nil
  }
  func setActivityBusy(_ value: Bool) { activityBusy = value }
  func setLifecycle(visible: Bool, foreground: Bool) {
    self.visible = visible; self.foreground = foreground
    updateExpiry()
  }
  private func updateExpiry() {
    guard let flow else { return }
    secondsRemaining = max(0, Int(ceil(flow.expiresAt - now())))
    if secondsRemaining == 0 { userCode = nil }
    if secondsRemaining == 0 && operation == nil && !cancelRequested && !signOutRequested {
      clearFlow(); notice = "登录代码已过期，请重新登录。"
    }
  }
  private func clearFlow() {
    flow = nil; userCode = nil; secondsRemaining = 0; pollingInterval = 0; pollFailed = false
  }
  private func call(_ command: String, id: UUID, session: String? = nil) async throws -> Data {
    guard connectionID == id, let request else { throw CancellationError() }
    var payload: [String: Any] = ["command": command]
    if let session { payload["authorizationSessionID"] = session }
    let data = try await request(["action": "accountSync", "request": payload])
    // Never claim Task cancellation interrupts Core's in-flight HTTP/persistence.
    // Its result still reconciles this connection; an old Core can never publish.
    guard connectionID == id else { throw CancellationError() }
    return data
  }
  private func failure(_ message: String, id: UUID) {
    if connectionID == id { error = message } // Never reflect upstream text/codes/secrets.
  }
  private func finish(_ id: UUID) async {
    guard connectionID == id else { return }
    operation = nil
    await drainIntent()
  }
  func load() async {
    guard !blocked else { return }
    let id = connectionID; operation = .loading; error = nil
    do { status = try decodeStatus(await call("status", id: id)) }
    catch { failure("无法读取账户状态。请稍后刷新；未验证任何云服务。", id: id) }
    await finish(id)
  }
  func beginSignIn() async {
    guard !blocked, !hasFlow, visible, foreground else { return }
    let id = connectionID; operation = .starting; error = nil; notice = nil
    do {
      let response = try JSONDecoder().decode(AccountAuthorization.self, from: await call("beginSignIn", id: id))
      try response.validate()
      let time = now()
      flow = Flow(id: response.authorizationSessionID, code: response.userCode,
                  expiresAt: time + Double(response.expiresIn), interval: response.interval,
                  nextPollAt: time + Double(response.interval))
      pollingInterval = response.interval; secondsRemaining = response.expiresIn
      userCode = cancelRequested || signOutRequested ? nil : response.userCode
      pollFailed = false
    } catch { failure("无法开始登录或 Core 返回了不受信任的登录信息。请刷新后重试。", id: id) }
    await finish(id)
  }
  /// Only schedules new polls while the owning view is visible and foreground.
  func tick() async {
    updateExpiry()
    await drainIntent()
    guard !blocked, visible, foreground, !pollFailed, let flow, now() >= flow.nextPollAt else { return }
    let id = connectionID; operation = .polling
    do {
      let data = try await call("pollSignIn", id: id, session: flow.id)
      let response = try JSONDecoder().decode(AccountPollResponse.self, from: data)
      switch response.status {
      case "pending":
        guard let slowDown = response.slowDown else { throw AccountResponseError.invalid }
        // RFC 8628 slow_down increases this and every subsequent interval by 5s.
        var next = flow
        if slowDown { next.interval = min(3600, next.interval + 5) }
        next.nextPollAt = now() + Double(next.interval)
        self.flow = next; pollingInterval = next.interval
      case "signed_in":
        let root = try JSONSerialization.jsonObject(with: data) as? [String: Any]
        guard let object = root?["state"] as? [String: Any] else { throw AccountResponseError.invalid }
        let state = try decodeStatus(JSONSerialization.data(withJSONObject: object))
        guard case .verifiedLogin = state.authentication else { throw AccountResponseError.invalid }
        status = state; clearFlow()
        notice = "已登录 GitHub"
      case "expired": clearFlow(); notice = "登录代码已过期，请重新登录。"
      default: throw AccountResponseError.invalid
      }
    } catch {
      if connectionID == id {
        pollFailed = true
        failure("登录检查失败，已暂停轮询。凭据是否保存尚未确认；请刷新状态后重试或取消。", id: id)
      }
    }
    await finish(id)
  }
  func retryPolling() {
    guard !blocked, let flow else { return }
    pollFailed = false; error = nil; userCode = flow.code
    // Never shorten the server interval, even after an error or lifecycle pause.
    var next = flow; next.nextPollAt = max(next.nextPollAt, now() + Double(next.interval)); self.flow = next
  }
  func cancelSignIn() async {
    guard canCancel else { return }
    cancelRequested = true; userCode = nil; error = nil
    notice = "正在取消登录…"
    await drainIntent()
  }
  /// Only the view's explicit destructive confirmation may call this method.
  func confirmSignOut() async {
    guard ready, !signOutRequested else { return }
    signOutRequested = true; userCode = nil; error = nil
    notice = "正在退出账户…"
    await drainIntent()
  }
  private func drainIntent() async {
    guard ready, !activityBusy, operation == nil else { return }
    let id = connectionID
    if signOutRequested {
      operation = .signingOut
      do {
        let state = try decodeStatus(await call("signOut", id: id))
        guard state.authentication == .signedOut else { throw AccountResponseError.invalid }
        status = state; clearFlow(); cancelRequested = false
        notice = "已退出，本地数据已保留。"
      } catch {
        if connectionID == id { pollFailed = true; notice = nil }
        failure("退出未确认，可能未能保存凭据变更。请刷新状态后重试；本地数据未要求删除。", id: id)
      }
      guard connectionID == id else { return }
      signOutRequested = false; operation = nil
    } else if cancelRequested {
      guard let flow else { cancelRequested = false; return }
      operation = .cancelling
      do {
        let result = try JSONDecoder().decode(AccountCancelResponse.self,
          from: await call("cancelSignIn", id: id, session: flow.id))
        guard result.cancelled else { throw AccountResponseError.invalid }
        clearFlow(); notice = "已取消登录检查"
      } catch {
        if connectionID == id { pollFailed = true; notice = nil }
        failure("取消未确认，轮询保持暂停。请重试取消或刷新账户状态。", id: id)
      }
      guard connectionID == id else { return }
      cancelRequested = false; operation = nil
      // A confirmed logout requested while cancellation was settling must not
      // depend on another foreground timer tick to reach Core.
      if signOutRequested { await drainIntent() }
    }
  }
}
