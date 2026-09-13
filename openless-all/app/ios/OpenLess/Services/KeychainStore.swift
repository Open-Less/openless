import Foundation
import Security

enum CredentialKind: String, CaseIterable, Identifiable {
    case transcription, polishing
    var id: String { rawValue }
    var title: String { self == .transcription ? "语音转写 API Key" : "文字润色 API Key" }
}

enum KeychainStore {
    private static func query(_ kind: CredentialKind) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: (Bundle.main.bundleIdentifier ?? "com.openless.ios") + ".providers",
         kSecAttrAccount as String: kind.rawValue]
    }

    static func read(_ kind: CredentialKind) throws -> String {
        var attributes = query(kind)
        attributes[kSecReturnData as String] = true
        attributes[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(attributes as CFDictionary, &result)
        if status == errSecItemNotFound { return "" }
        guard status == errSecSuccess, let data = result as? Data,
              let value = String(data: data, encoding: .utf8) else { throw failure(status) }
        return value
    }

    static func save(_ value: String, for kind: CredentialKind) throws {
        let value = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else { try delete(kind); return }
        let attributes: [String: Any] = [kSecValueData as String: Data(value.utf8),
                                       kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly]
        let status = SecItemUpdate(query(kind) as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound {
            let addition = query(kind).merging(attributes) { _, new in new }
            let added = SecItemAdd(addition as CFDictionary, nil)
            guard added == errSecSuccess else { throw failure(added) }
        } else if status != errSecSuccess { throw failure(status) }
    }

    static func delete(_ kind: CredentialKind) throws {
        let status = SecItemDelete(query(kind) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw failure(status) }
    }

    private static func failure(_ status: OSStatus) -> OpenLessError {
        .message("无法访问系统钥匙串（\(status)）。请解锁设备后重试。")
    }
}
