import Foundation

/// Presentation projection only. Numeric identity, permissions and sync support
/// must come from a future Core contract, never from a GitHub display login.
struct AccountStatus: Decodable {
  enum Authentication: Equatable {
    case signedOut, unverified, verifiedLogin(String), unsupported
  }
  struct Account: Decodable { let githubID: UInt64?; let login: String }
  struct Sync: Decodable {
    let available: Bool
    let reason: String?
    let allowedDocuments: [String]
  }
  let schemaVersion: Int
  let authState: String
  let account: Account?
  let identityState: String
  let sync: Sync

  var authentication: Authentication {
    guard schemaVersion == 1 else { return .unsupported }
    switch (authState, identityState) {
    case ("signed_out", "unresolved") where account == nil: return .signedOut
    case ("unverified", "unresolved") where account == nil: return .unverified
    case ("signed_in", "verified_login_only"):
      guard let account, account.githubID == nil, Self.validLogin(account.login) else { return .unsupported }
      return .verifiedLogin(account.login)
    default: return .unsupported
    }
  }
  var limitedScopeUnavailable: Bool {
    !sync.available && sync.reason == "sync_scope_unavailable"
  }
  private static func validLogin(_ login: String) -> Bool {
    !login.isEmpty && login.utf8.count <= 39 && login.utf8.allSatisfy {
      (65...90).contains($0) || (97...122).contains($0) || (48...57).contains($0) || $0 == 45
    }
  }
}

enum AccountResponseError: Error { case invalid }

struct AccountAuthorization: Decodable {
  static let trustedVerificationURL = "https://github.com/login/device"
  let authorizationSessionID: String
  let userCode: String
  let verificationURL: String
  let expiresIn: Int
  let interval: Int

  func validate() throws {
    guard let id = UUID(uuidString: authorizationSessionID),
          id.uuidString.lowercased() == authorizationSessionID,
          authorizationSessionID[authorizationSessionID.index(authorizationSessionID.startIndex, offsetBy: 14)] == "4",
          ["8", "9", "a", "b"].contains(String(authorizationSessionID[authorizationSessionID.index(authorizationSessionID.startIndex, offsetBy: 19)])),
          verificationURL == Self.trustedVerificationURL,
          !userCode.isEmpty, userCode.utf8.count <= 64,
          userCode.utf8.allSatisfy({ (65...90).contains($0) || (97...122).contains($0) || (48...57).contains($0) || $0 == 45 }),
          (1...3600).contains(expiresIn), (1...expiresIn).contains(interval)
    else { throw AccountResponseError.invalid }
  }
}

struct AccountPollResponse: Decodable {
  let status: String
  let slowDown: Bool?
}

struct AccountCancelResponse: Decodable { let cancelled: Bool }
