import Foundation
import Testing
@testable import OpenLess

@MainActor
struct AccountSettingsTests {
  @MainActor final class Fixture {
    var time: TimeInterval = 100
    var commands: [String] = []
    var requests: [[String: Any]] = []
    var auth = "signed_out"
    var fail: String?
    var url = AccountAuthorization.trustedVerificationURL
    var pollResult: [String: Any] = ["status": "pending", "slowDown": false]
    let session = "01234567-89ab-4cde-8fab-0123456789ab"
    func data(_ value: Any) throws -> Data { try JSONSerialization.data(withJSONObject: value) }
    func state(_ auth: String? = nil) -> [String: Any] {
      let auth = auth ?? self.auth
      return ["schemaVersion": 1, "authState": auth,
              "account": auth == "signed_in" ? ["githubID": NSNull(), "login": "verified-login"] : NSNull(),
              "identityState": auth == "signed_in" ? "verified_login_only" : "unresolved", "capabilities": NSNull(),
              "sync": ["available": false, "reason": "sync_scope_unavailable",
                       "allowedDocuments": ["confirmedVocabulary", "modeStyleReferences"]]]
    }
    func authorization() -> [String: Any] {
      ["authorizationSessionID": session, "userCode": "ABCD-EFGH", "verificationURL": url, "expiresIn": 30, "interval": 5]
    }
    func respond(_ action: [String: Any]) async throws -> Data {
      requests.append(action)
      #expect(action["action"] as? String == "accountSync")
      let payload = try #require(action["request"] as? [String: Any])
      let command = try #require(payload["command"] as? String)
      commands.append(command)
      #expect(Set(action.keys) == ["action", "request"])
      if command == "pollSignIn" || command == "cancelSignIn" {
        #expect(Set(payload.keys) == ["command", "authorizationSessionID"])
        #expect(payload["authorizationSessionID"] as? String == session)
      } else { #expect(Set(payload.keys) == ["command"]) }
      if command == fail { throw NSError(domain: "SECRET device_code token upstream body", code: 1) }
      switch command {
      case "status": return try data(state())
      case "beginSignIn": return try data(authorization())
      case "pollSignIn": return try data(pollResult)
      case "cancelSignIn": return try data(["cancelled": true])
      case "signOut": auth = "signed_out"; return try data(state())
      default: throw AccountResponseError.invalid
      }
    }
    func model() -> AccountSettings {
      AccountSettings(request: { try await self.respond($0) }, now: { self.time })
    }
    func activate(_ model: AccountSettings) { model.setLifecycle(visible: true, foreground: true) }
  }

  @Test func localStatusDistinguishesSavedTokenAndCurrentProcessVerifiedLogin() async throws {
    let fixture = Fixture(), model: AccountSettings
    model = fixture.model()
    await model.load()
    #expect(model.status?.authentication == .signedOut)
    fixture.auth = "unverified"; await model.load()
    #expect(model.status?.authentication == .unverified)
    fixture.auth = "signed_in"; await model.load()
    #expect(model.status?.authentication == .verifiedLogin("verified-login"))
    #expect(model.status?.account?.githubID == nil)
    #expect(model.status?.limitedScopeUnavailable == true)
    #expect(fixture.commands == ["status", "status", "status"])
    #expect(model.userCode == nil)
  }

  @Test func unknownFutureStatusCannotFabricateIdentityOrEnableSync() throws {
    let fixture = Fixture()
    var json = fixture.state("signed_in")
    json["identityState"] = "verified_numeric_identity"
    var status = try JSONDecoder().decode(AccountStatus.self, from: fixture.data(json))
    #expect(status.authentication == .unsupported)
    json = fixture.state("unverified"); json["account"] = ["githubID": NSNull(), "login": "pretend"]
    status = try JSONDecoder().decode(AccountStatus.self, from: fixture.data(json))
    #expect(status.authentication == .unsupported)
    json = fixture.state(); json["schemaVersion"] = 2
    #expect(try JSONDecoder().decode(AccountStatus.self, from: fixture.data(json)).authentication == .unsupported)
  }

  @Test func malformedAndMerelyHTTPSAuthorizationURLsFailClosed() async {
    let urls = ["https://github.com/login/device/", "https://github.com.evil.invalid/login/device",
                "https://github.com/login/device?secret=token", "http://github.com/login/device",
                "https://user@github.com/login/device", "HTTPS://github.com/login/device", "not a URL"]
    for url in urls {
      let fixture = Fixture(); fixture.url = url
      let model = fixture.model(); fixture.activate(model)
      await model.beginSignIn()
      #expect(model.userCode == nil)
      #expect(!model.hasFlow)
      #expect(model.error != nil)
      fixture.time = 200; await model.tick()
      #expect(fixture.commands == ["beginSignIn"])
    }
  }

  @Test func malformedSessionCodeExpiryAndIntervalAreRejected() throws {
    let fixture = Fixture()
    for (key, value) in [("authorizationSessionID", "01234567-89ab-1cde-8fab-0123456789ab" as Any),
                         ("authorizationSessionID", fixture.session.uppercased()),
                         ("userCode", "code\nSECRET"), ("expiresIn", 0), ("expiresIn", 3601),
                         ("interval", 0), ("interval", 31)] {
      var json = fixture.authorization(); json[key] = value
      #expect(throws: (any Error).self) {
        try JSONDecoder().decode(AccountAuthorization.self, from: fixture.data(json)).validate()
      }
    }
  }

  @Test func explicitStartOnlyPollsAtIntervalAndSlowDownPersistsUntilExpiry() async {
    let fixture = Fixture(), model: AccountSettings
    model = fixture.model(); fixture.activate(model)
    await model.tick(); #expect(fixture.commands.isEmpty)
    await model.beginSignIn()
    #expect(model.userCode == "ABCD-EFGH")
    fixture.time = 104.99; await model.tick(); #expect(fixture.commands == ["beginSignIn"])
    fixture.time = 105; fixture.pollResult = ["status": "pending", "slowDown": true]; await model.tick()
    #expect(model.pollingInterval == 10)
    fixture.time = 114.99; await model.tick(); #expect(fixture.commands.count == 2)
    fixture.time = 115; fixture.pollResult = ["status": "pending", "slowDown": false]; await model.tick()
    #expect(model.pollingInterval == 10)
    fixture.time = 130; await model.tick()
    #expect(model.userCode == nil)
    #expect(!model.hasFlow)
    #expect(fixture.commands == ["beginSignIn", "pollSignIn", "pollSignIn"])
  }

  @Test func lifecycleBusyAndDisconnectionSuppressPollingWithoutAutomaticLogin() async {
    let fixture = Fixture(), model: AccountSettings
    model = fixture.model(); fixture.activate(model); await model.beginSignIn()
    fixture.time = 105
    model.setLifecycle(visible: true, foreground: false); await model.tick()
    model.setLifecycle(visible: false, foreground: true); await model.tick()
    fixture.activate(model); model.setActivityBusy(true); await model.tick(); await model.load()
    #expect(fixture.commands == ["beginSignIn"])
    model.setActivityBusy(false); await model.tick()
    #expect(fixture.commands.last == "pollSignIn")
    model.connect(nil); fixture.time = 110; await model.tick(); await model.beginSignIn()
    #expect(!model.ready && model.status == nil && model.userCode == nil)
    #expect(fixture.commands.count == 2)
  }

  @Test func pollErrorPausesAndExplicitRetryNeverShortensIntervalOrLeaksErrorText() async {
    let fixture = Fixture(), model: AccountSettings
    model = fixture.model(); fixture.activate(model); await model.beginSignIn()
    fixture.fail = "pollSignIn"; fixture.time = 105; await model.tick()
    #expect(model.needsPollRetry)
    #expect(model.error?.contains("SECRET") == false)
    #expect(model.error?.contains("保存尚未确认") == true)
    fixture.time = 110; await model.tick(); #expect(fixture.commands.count == 2)
    fixture.fail = nil; model.retryPolling()
    fixture.time = 114.99; await model.tick(); #expect(fixture.commands.count == 2)
    fixture.time = 115; await model.tick(); #expect(fixture.commands.count == 3)
  }

  @Test func signOutSaveFailurePreservesLastConfirmedStatusAndDoesNotClaimSuccess() async {
    let fixture = Fixture(); fixture.auth = "signed_in"
    let model = fixture.model(); await model.load()
    fixture.fail = "signOut"; await model.confirmSignOut()
    #expect(model.status?.authentication == .verifiedLogin("verified-login"))
    #expect(model.error?.contains("退出未确认") == true)
    #expect(model.error?.contains("SECRET") == false)
    #expect(model.notice == nil)
    fixture.fail = nil; await model.confirmSignOut()
    #expect(model.status?.authentication == .signedOut)
    #expect(model.notice == "已退出，本地数据已保留。")
    #expect(fixture.commands == ["status", "signOut", "signOut"])
  }

  @Test func cancelWaitsForInFlightPollAndDoesNotPretendToCancelHTTP() async throws {
    let fixture = Fixture()
    var continuation: CheckedContinuation<Data, any Error>?
    let model = AccountSettings(request: { action in
      if (action["request"] as? [String: Any])?["command"] as? String == "pollSignIn" {
        return try await withCheckedThrowingContinuation { continuation = $0 }
      }
      return try await fixture.respond(action)
    }, now: { fixture.time })
    fixture.activate(model); await model.beginSignIn(); fixture.time = 105
    let poll = Task { await model.tick() }
    while continuation == nil { await Task.yield() }
    await model.cancelSignIn()
    #expect(model.cancelRequested && model.userCode == nil)
    #expect(model.notice == "正在取消登录…")
    #expect(fixture.commands == ["beginSignIn"])
    continuation?.resume(returning: try fixture.data(["status": "pending", "slowDown": false]))
    await poll.value
    #expect(fixture.commands == ["beginSignIn", "cancelSignIn"])
    #expect(!model.hasFlow && !model.cancelRequested)
    fixture.time = 110; await model.tick(); #expect(fixture.commands.count == 2)
  }

  @Test func confirmedSignOutQueuesBehindPollAndCompletedLoginCannotUndoLogout() async throws {
    let fixture = Fixture()
    var continuation: CheckedContinuation<Data, any Error>?
    let model = AccountSettings(request: { action in
      if (action["request"] as? [String: Any])?["command"] as? String == "pollSignIn" {
        return try await withCheckedThrowingContinuation { continuation = $0 }
      }
      return try await fixture.respond(action)
    }, now: { fixture.time })
    fixture.activate(model); await model.beginSignIn(); fixture.time = 105
    let poll = Task { await model.tick() }
    while continuation == nil { await Task.yield() }
    await model.confirmSignOut()
    #expect(model.signOutRequested)
    #expect(!fixture.commands.contains("signOut"))
    continuation?.resume(returning: try fixture.data(["status": "signed_in", "state": fixture.state("signed_in")]))
    await poll.value
    #expect(fixture.commands == ["beginSignIn", "signOut"])
    #expect(model.status?.authentication == .signedOut)
    #expect(!model.hasFlow)
  }

  @Test func cancelDuringStartWaitsAndCancelFailureKeepsPollingPausedForRetry() async throws {
    let fixture = Fixture()
    var continuation: CheckedContinuation<Data, any Error>?
    let model = AccountSettings(request: { action in
      if (action["request"] as? [String: Any])?["command"] as? String == "beginSignIn" {
        return try await withCheckedThrowingContinuation { continuation = $0 }
      }
      return try await fixture.respond(action)
    }, now: { fixture.time })
    fixture.activate(model)
    let start = Task { await model.beginSignIn() }
    while continuation == nil { await Task.yield() }
    fixture.fail = "cancelSignIn"; await model.cancelSignIn()
    continuation?.resume(returning: try fixture.data(fixture.authorization()))
    await start.value
    #expect(model.userCode == nil && model.hasFlow && model.needsPollRetry)
    #expect(model.error?.contains("取消未确认") == true)
    fixture.time = 105; await model.tick()
    #expect(fixture.commands == ["cancelSignIn"])
    fixture.fail = nil; await model.cancelSignIn()
    #expect(!model.hasFlow)
  }

  @Test func newCoreEpochDropsStaleResultWithoutUnlockingNewOperation() async throws {
    let fixture = Fixture()
    var old: CheckedContinuation<Data, any Error>?
    var new: CheckedContinuation<Data, any Error>?
    let model = AccountSettings(request: { _ in try await withCheckedThrowingContinuation { old = $0 } })
    let oldTask = Task { await model.load() }
    while old == nil { await Task.yield() }
    model.replaceConnection { _ in try await withCheckedThrowingContinuation { new = $0 } }
    let newTask = Task { await model.load() }
    while new == nil { await Task.yield() }
    old?.resume(returning: try fixture.data(fixture.state("signed_in")))
    await oldTask.value
    #expect(model.status == nil && model.operation == .loading)
    new?.resume(returning: try fixture.data(fixture.state("unverified")))
    await newTask.value
    #expect(model.status?.authentication == .unverified && model.operation == nil)
  }

  @Test func staleBeginAndPollCannotPublishCodesOrCredentialsOnReplacementCore() async throws {
    for command in ["beginSignIn", "pollSignIn"] {
      let fixture = Fixture()
      var continuation: CheckedContinuation<Data, any Error>?
      let model = AccountSettings(request: { action in
        if (action["request"] as? [String: Any])?["command"] as? String == command {
          return try await withCheckedThrowingContinuation { continuation = $0 }
        }
        return try await fixture.respond(action)
      }, now: { fixture.time })
      fixture.activate(model)
      if command == "pollSignIn" { await model.beginSignIn(); fixture.time = 105 }
      let task = Task { if command == "beginSignIn" { await model.beginSignIn() } else { await model.tick() } }
      while continuation == nil { await Task.yield() }
      model.replaceConnection { try await fixture.respond($0) }
      await model.load()
      continuation?.resume(returning: try fixture.data(command == "beginSignIn" ? fixture.authorization() : ["status": "signed_in", "state": fixture.state("signed_in")]))
      await task.value
      #expect(model.status?.authentication == .signedOut)
      #expect(model.userCode == nil && !model.hasFlow && model.error == nil)
    }
  }

  @Test func busySignOutIsQueuedAndNoUnsupportedSyncCommandsAreIssued() async {
    let fixture = Fixture(), model: AccountSettings
    model = fixture.model(); fixture.activate(model); await model.beginSignIn()
    model.setActivityBusy(true); await model.confirmSignOut(); await model.tick()
    #expect(fixture.commands == ["beginSignIn"])
    model.setActivityBusy(false); await model.tick()
    #expect(fixture.commands == ["beginSignIn", "signOut"])
    #expect(model.status?.limitedScopeUnavailable == true)
  }

  @Test func taskCancellationDoesNotDiscardCompletedCredentialSaveOrPretendLogout() async throws {
    let fixture = Fixture()
    var continuation: CheckedContinuation<Data, any Error>?
    var statusDecodes = 0
    let model = AccountSettings(request: { action in
      if (action["request"] as? [String: Any])?["command"] as? String == "pollSignIn" {
        return try await withCheckedThrowingContinuation { continuation = $0 }
      }
      return try await fixture.respond(action)
    }, now: { fixture.time }, decodeStatus: { data in
      statusDecodes += 1
      return try JSONDecoder().decode(AccountStatus.self, from: data)
    })
    fixture.activate(model); await model.beginSignIn(); fixture.time = 105
    let poll = Task { await model.tick() }
    while continuation == nil { await Task.yield() }
    poll.cancel()
    model.setLifecycle(visible: false, foreground: false)
    continuation?.resume(returning: try fixture.data(["status": "signed_in", "state": fixture.state("signed_in")]))
    await poll.value
    #expect(model.status?.authentication == .verifiedLogin("verified-login"))
    #expect(statusDecodes == 1) // Nested poll status uses the replaceable decoder too.
    #expect(model.userCode == nil && model.operation == nil)
    #expect(fixture.commands == ["beginSignIn"])
  }

  @Test func expiredCodeIsHiddenWhilePollSettlesAndCancelCannotUndoCompletedLogin() async throws {
    let fixture = Fixture()
    var continuation: CheckedContinuation<Data, any Error>?
    let model = AccountSettings(request: { action in
      if (action["request"] as? [String: Any])?["command"] as? String == "pollSignIn" {
        return try await withCheckedThrowingContinuation { continuation = $0 }
      }
      return try await fixture.respond(action)
    }, now: { fixture.time })
    fixture.activate(model); await model.beginSignIn(); fixture.time = 105
    let poll = Task { await model.tick() }
    while continuation == nil { await Task.yield() }
    fixture.time = 130; await model.tick()
    #expect(model.secondsRemaining == 0 && model.userCode == nil)
    await model.cancelSignIn()
    continuation?.resume(returning: try fixture.data(["status": "signed_in", "state": fixture.state("signed_in")]))
    await poll.value
    #expect(model.status?.authentication == .verifiedLogin("verified-login"))
    #expect(fixture.commands == ["beginSignIn"])
    #expect(!model.cancelRequested && !model.hasFlow)
  }

  @Test func confirmedSignOutDuringCancellationDrainsWithoutAnotherViewTick() async throws {
    let fixture = Fixture()
    var continuation: CheckedContinuation<Data, any Error>?
    let model = AccountSettings(request: { action in
      if (action["request"] as? [String: Any])?["command"] as? String == "cancelSignIn" {
        return try await withCheckedThrowingContinuation { continuation = $0 }
      }
      return try await fixture.respond(action)
    }, now: { fixture.time })
    fixture.activate(model); await model.beginSignIn()
    let cancel = Task { await model.cancelSignIn() }
    while continuation == nil { await Task.yield() }
    await model.confirmSignOut(); model.setLifecycle(visible: false, foreground: false)
    continuation?.resume(returning: try fixture.data(["cancelled": true]))
    await cancel.value
    #expect(fixture.commands == ["beginSignIn", "signOut"])
    #expect(model.status?.authentication == .signedOut)
  }

  @Test func statusDecoderCanBeReplacedWithoutChangingTransport() async throws {
    let fixture = Fixture()
    var decoded = false
    let model = AccountSettings(request: { try await fixture.respond($0) }, decodeStatus: { data in
      decoded = true
      return try JSONDecoder().decode(AccountStatus.self, from: data)
    })
    await model.load()
    #expect(decoded)
    #expect(fixture.commands == ["status"])
  }
}
