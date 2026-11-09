#if canImport(Security)
import Foundation
import Security
import VistaCore

/// Keychain storage for a host's out-of-band pairing secret.
///
/// The secret is stored as generic-password data under `dev.vista.pairing`,
/// accessible after first unlock on this device only. It is never written to
/// defaults, logs or the app's documents.
public enum PairingSecretStore {
    private static let service = "dev.vista.pairing"

    private static func query(host: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: host
        ]
    }

    public static func save(_ secret: PairingSecret, for host: String) throws {
        var query = query(host: host)
        SecItemDelete(query as CFDictionary)
        query[kSecValueData as String] = Data(secret.hex.utf8)
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let status = SecItemAdd(query as CFDictionary, nil)
        guard status == errSecSuccess else {
            throw KeychainError(status: status)
        }
    }

    public static func load(host: String) -> PairingSecret? {
        var query = query(host: host)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess,
              let data = result as? Data,
              let hex = String(data: data, encoding: .utf8),
              let secret = try? PairingSecret(hex: hex)
        else {
            return nil
        }
        return secret
    }

    public static func delete(host: String) {
        SecItemDelete(query(host: host) as CFDictionary)
    }
}

public struct KeychainError: Error, CustomStringConvertible {
    public let status: OSStatus

    public var description: String {
        "Keychain operation failed with status \(status)"
    }
}
#endif
