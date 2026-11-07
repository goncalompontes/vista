import Foundation

/// Source of a remotely displayed application. These cases describe planned
/// capabilities; the iOS preview does not currently open either kind of session.
public enum ApplicationSource: String, Codable, CaseIterable, Hashable, Sendable {
    case existingWindow
    case independentSession

    public var description: String {
        switch self {
        case .existingWindow: "Existing window"
        case .independentSession: "Independent app"
        }
    }
}

/// Portable catalog metadata that can eventually be populated by Vista's
/// authenticated host API. No compositor or media details leak into this type.
public struct RemoteApplication: Identifiable, Codable, Hashable, Sendable {
    public let id: String
    public let name: String
    public let subtitle: String
    public let symbolName: String
    public let source: ApplicationSource

    public init(
        id: String,
        name: String,
        subtitle: String,
        symbolName: String,
        source: ApplicationSource
    ) {
        self.id = id
        self.name = name
        self.subtitle = subtitle
        self.symbolName = symbolName
        self.source = source
    }
}

/// A host address isn't a pairing credential or proof of identity.
/// Authentication and discovery will be introduced with the host transport.
public struct HostEndpoint: Codable, Hashable, Sendable {
    public let hostname: String
    public let port: UInt16

    public init?(hostname: String, port: Int = 7443) {
        let trimmed = hostname.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty,
              (1...65535).contains(port),
              !trimmed.contains(where: { $0.isWhitespace || "/\\@?#".contains($0) }),
              !trimmed.contains("["),
              !trimmed.contains("]")
        else {
            return nil
        }
        self.hostname = trimmed
        self.port = UInt16(port)
    }

    public var displayAddress: String {
        let host = hostname.contains(":") ? "[\(hostname)]" : hostname
        return "\(host):\(port)"
    }
}
