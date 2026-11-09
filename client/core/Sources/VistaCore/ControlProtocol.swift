#if !SKIP
import Foundation

// Wire-compatible implementation of Vista control protocol v1.
//
// This file is Foundation-only and testable on Linux. It deliberately contains
// no sockets: the iOS target supplies the byte transport. See docs/protocol.md
// and docs/fixtures/protocol-v1.json for the contract.

public struct ProtocolVersion: Codable, Equatable, Hashable, Sendable, Comparable, CustomStringConvertible {
    public let major: UInt16
    public let minor: UInt16

    public init(major: UInt16, minor: UInt16) {
        self.major = major
        self.minor = minor
    }

    public static let v1 = ProtocolVersion(major: 1, minor: 0)

    public static func < (lhs: ProtocolVersion, rhs: ProtocolVersion) -> Bool {
        (lhs.major, lhs.minor) < (rhs.major, rhs.minor)
    }

    public var description: String { "\(major).\(minor)" }
}

public struct VersionRange: Codable, Equatable, Hashable, Sendable {
    public let min: ProtocolVersion
    public let max: ProtocolVersion

    public init(min: ProtocolVersion, max: ProtocolVersion) {
        self.min = min
        self.max = max
    }

    public static let current = VersionRange(min: .v1, max: .v1)

    public func supports(_ version: ProtocolVersion) -> Bool {
        min <= version && version <= max
    }
}

public struct PermissionSet: Codable, Equatable, Hashable, Sendable {
    public var video: Bool
    public var audio: Bool
    public var input: Bool
    public var clipboard: Bool

    public init(video: Bool = false, audio: Bool = false, input: Bool = false, clipboard: Bool = false) {
        self.video = video
        self.audio = audio
        self.input = input
        self.clipboard = clipboard
    }

    public static let videoOnly = PermissionSet(video: true)

    public var bits: UInt8 {
        (video ? 1 << 0 : 0)
            | (audio ? 1 << 1 : 0)
            | (input ? 1 << 2 : 0)
            | (clipboard ? 1 << 3 : 0)
    }

    /// True when every permission in `self` is also present in `grant`.
    public func isSubset(of grant: PermissionSet) -> Bool {
        (!video || grant.video)
            && (!audio || grant.audio)
            && (!input || grant.input)
            && (!clipboard || grant.clipboard)
    }

    private enum CodingKeys: String, CodingKey {
        case video, audio, input, clipboard
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        video = try container.decodeIfPresent(Bool.self, forKey: .video) ?? false
        audio = try container.decodeIfPresent(Bool.self, forKey: .audio) ?? false
        input = try container.decodeIfPresent(Bool.self, forKey: .input) ?? false
        clipboard = try container.decodeIfPresent(Bool.self, forKey: .clipboard) ?? false
    }
}

public struct CatalogEntry: Codable, Equatable, Sendable, Identifiable {
    public let windowID: UInt64
    public let title: String?
    public let appID: String?
    public let workspaceID: UInt64?

    public var id: String { "w\(windowID)" }

    public init(windowID: UInt64, title: String? = nil, appID: String? = nil, workspaceID: UInt64? = nil) {
        self.windowID = windowID
        self.title = title
        self.appID = appID
        self.workspaceID = workspaceID
    }

    public var displayName: String {
        if let title, !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return title
        }
        if let appID, !appID.isEmpty {
            return appID
        }
        return "Window \(windowID)"
    }

    enum CodingKeys: String, CodingKey {
        case windowID = "window_id"
        case title
        case appID = "app_id"
        case workspaceID = "workspace_id"
    }
}

public struct CatalogSnapshot: Codable, Equatable, Sendable {
    public static let maxEntries = 256
    public static let maxTitleBytes = 256
    public static let maxAppIDBytes = 128

    public let v: UInt16
    public let messageType: String
    public let connectionEpoch: String
    public let sessionID: String?
    public let generation: String
    public let revision: UInt64
    public let entries: [CatalogEntry]

    public init(
        v: UInt16 = 1,
        messageType: String = "catalog.snapshot",
        connectionEpoch: String,
        sessionID: String?,
        generation: String,
        revision: UInt64,
        entries: [CatalogEntry]
    ) throws {
        self.v = v
        self.messageType = messageType
        self.connectionEpoch = connectionEpoch
        self.sessionID = sessionID
        self.generation = generation
        self.revision = revision
        self.entries = entries
        try validate()
    }

    public func validate() throws {
        guard v == 1, messageType == "catalog.snapshot" else {
            throw WireError.invalidField("catalog version")
        }
        guard !connectionEpoch.isEmpty, connectionEpoch.utf8.count <= 128 else {
            throw WireError.invalidField("connection_epoch")
        }
        if let sessionID, sessionID.isEmpty || sessionID.utf8.count > 128 {
            throw WireError.invalidField("session_id")
        }
        guard !generation.isEmpty, generation.utf8.count <= 256, revision > 0 else {
            throw WireError.invalidField("catalog header")
        }
        guard entries.count <= Self.maxEntries else {
            throw WireError.invalidField("catalog entries")
        }
        var seen = Set<UInt64>()
        for entry in entries {
            guard seen.insert(entry.windowID).inserted else {
                throw WireError.invalidField("duplicate window id")
            }
            if let title = entry.title,
               title.utf8.count > Self.maxTitleBytes
            {
                throw WireError.invalidField("title")
            }
            if let appID = entry.appID,
               appID.isEmpty || appID.utf8.count > Self.maxAppIDBytes
            {
                throw WireError.invalidField("app_id")
            }
        }
    }

    enum CodingKeys: String, CodingKey {
        case v
        case messageType = "type"
        case connectionEpoch = "connection_epoch"
        case sessionID = "session_id"
        case generation
        case revision
        case entries
    }

    init(from container: KeyedDecodingContainer<AnyCodingKey>) throws {
        func string(_ key: String) throws -> String {
            guard let value = try container.decodeIfPresent(String.self, forKey: AnyCodingKey(key)) else {
                throw WireError.invalidField(key)
            }
            return value
        }
        guard let version = try container.decodeIfPresent(UInt16.self, forKey: AnyCodingKey("v")),
              let revision = try container.decodeIfPresent(UInt64.self, forKey: AnyCodingKey("revision")),
              let entries = try container.decodeIfPresent([CatalogEntry].self, forKey: AnyCodingKey("entries"))
        else {
            throw WireError.invalidField("catalog header")
        }
        self.v = version
        self.messageType = "catalog.snapshot"
        self.connectionEpoch = try string("connection_epoch")
        self.sessionID = try container.decodeIfPresent(String.self, forKey: AnyCodingKey("session_id"))
        self.generation = try string("generation")
        self.revision = revision
        self.entries = entries
        try validate()
    }

    func encodePayload(to container: inout KeyedEncodingContainer<AnyCodingKey>) throws {
        try container.encode(v, forKey: AnyCodingKey("v"))
        try container.encode(connectionEpoch, forKey: AnyCodingKey("connection_epoch"))
        try container.encodeIfPresent(sessionID, forKey: AnyCodingKey("session_id"))
        try container.encode(generation, forKey: AnyCodingKey("generation"))
        try container.encode(revision, forKey: AnyCodingKey("revision"))
        try container.encode(entries, forKey: AnyCodingKey("entries"))
    }
}

public enum ControlMessage: Equatable, Sendable {
    case clientHello(versions: VersionRange, clientID: String, clientNonce: String)
    case serverHello(
        selected: ProtocolVersion,
        hostID: String,
        hostName: String,
        hostNonce: String,
        pairingID: String,
        expiresAt: UInt64,
        policy: PermissionSet
    )
    case pairingRequest(pairingID: String, tag: String)
    case pairingAccepted(tag: String, sessionID: String, permissions: PermissionSet)
    case pairingRejected(reason: String)
    case sessionOpen(sessionID: String, permissions: PermissionSet)
    case sessionOpened(sessionID: String, permissions: PermissionSet)
    case keyframeRequest(sessionID: String)
    case ping(nonce: UInt64)
    case pong(nonce: UInt64)
    case sessionClose(sessionID: String?, reason: String)
    case error(code: String, message: String)
    case catalogSnapshot(CatalogSnapshot)

    public var typeName: String {
        switch self {
        case .clientHello: "client_hello"
        case .serverHello: "server_hello"
        case .pairingRequest: "pairing_request"
        case .pairingAccepted: "pairing_accepted"
        case .pairingRejected: "pairing_rejected"
        case .sessionOpen: "session_open"
        case .sessionOpened: "session_opened"
        case .keyframeRequest: "keyframe_request"
        case .ping: "ping"
        case .pong: "pong"
        case .sessionClose: "session_close"
        case .error: "error"
        case .catalogSnapshot: "catalog.snapshot"
        }
    }
}

public struct Envelope: Equatable, Sendable {
    public var seq: UInt64
    public var body: ControlMessage

    public init(seq: UInt64, body: ControlMessage) {
        self.seq = seq
        self.body = body
    }
}

struct AnyCodingKey: CodingKey, Hashable {
    let stringValue: String
    var intValue: Int? { nil }

    init(_ string: String) {
        stringValue = string
    }

    init?(stringValue: String) {
        self.init(stringValue)
    }

    init?(intValue: Int) {
        nil
    }
}

extension Envelope: Codable {
    private enum Keys: String, CodingKey {
        case seq, type
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: Keys.self)
        seq = try container.decode(UInt64.self, forKey: .seq)
        let type = try container.decode(String.self, forKey: .type)
        body = try ControlMessage(from: decoder, type: type)
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.container(keyedBy: AnyCodingKey.self)
        try container.encode(seq, forKey: AnyCodingKey("seq"))
        try body.encodePayload(to: &container)
    }
}

extension ControlMessage {
    init(from decoder: Decoder, type: String) throws {
        let c = try decoder.container(keyedBy: AnyCodingKey.self)
        func string(_ key: String) throws -> String {
            guard let value = try c.decodeIfPresent(String.self, forKey: AnyCodingKey(key)) else {
                throw WireError.invalidField(key)
            }
            return value
        }
        func boolOrFalse(_ key: String) throws -> Bool {
            try c.decodeIfPresent(Bool.self, forKey: AnyCodingKey(key)) ?? false
        }
        func permissions() throws -> PermissionSet {
            PermissionSet(
                video: try boolOrFalse("video"),
                audio: try boolOrFalse("audio"),
                input: try boolOrFalse("input"),
                clipboard: try boolOrFalse("clipboard")
            )
        }

        switch type {
        case "client_hello":
            guard let versions = try c.decodeIfPresent(VersionRange.self, forKey: AnyCodingKey("versions")) else {
                throw WireError.invalidField("versions")
            }
            self = .clientHello(
                versions: versions,
                clientID: try string("client_id"),
                clientNonce: try string("client_nonce")
            )
        case "server_hello":
            guard let selected = try c.decodeIfPresent(ProtocolVersion.self, forKey: AnyCodingKey("selected")),
                  let expiresAt = try c.decodeIfPresent(UInt64.self, forKey: AnyCodingKey("expires_at"))
            else {
                throw WireError.invalidField("selected/expires_at")
            }
            self = .serverHello(
                selected: selected,
                hostID: try string("host_id"),
                hostName: try string("host_name"),
                hostNonce: try string("host_nonce"),
                pairingID: try string("pairing_id"),
                expiresAt: expiresAt,
                policy: try permissions()
            )
        case "pairing_request":
            self = .pairingRequest(pairingID: try string("pairing_id"), tag: try string("tag"))
        case "pairing_accepted":
            self = .pairingAccepted(
                tag: try string("tag"),
                sessionID: try string("session_id"),
                permissions: try permissions()
            )
        case "pairing_rejected":
            self = .pairingRejected(reason: try string("reason"))
        case "session_open":
            self = .sessionOpen(sessionID: try string("session_id"), permissions: try permissions())
        case "session_opened":
            self = .sessionOpened(sessionID: try string("session_id"), permissions: try permissions())
        case "keyframe_request":
            self = .keyframeRequest(sessionID: try string("session_id"))
        case "ping":
            guard let nonce = try c.decodeIfPresent(UInt64.self, forKey: AnyCodingKey("nonce")) else {
                throw WireError.invalidField("nonce")
            }
            self = .ping(nonce: nonce)
        case "pong":
            guard let nonce = try c.decodeIfPresent(UInt64.self, forKey: AnyCodingKey("nonce")) else {
                throw WireError.invalidField("nonce")
            }
            self = .pong(nonce: nonce)
        case "session_close":
            self = .sessionClose(
                sessionID: try c.decodeIfPresent(String.self, forKey: AnyCodingKey("session_id")),
                reason: try string("reason")
            )
        case "error":
            self = .error(code: try string("code"), message: try string("message"))
        case "catalog.snapshot":
            self = .catalogSnapshot(try CatalogSnapshot(from: c))
        default:
            throw WireError.unknownMessageType(type)
        }
    }

    func encodePayload(to container: inout KeyedEncodingContainer<AnyCodingKey>) throws {
        func encodePermissions(_ permissions: PermissionSet) throws {
            try container.encode(permissions.video, forKey: AnyCodingKey("video"))
            try container.encode(permissions.audio, forKey: AnyCodingKey("audio"))
            try container.encode(permissions.input, forKey: AnyCodingKey("input"))
            try container.encode(permissions.clipboard, forKey: AnyCodingKey("clipboard"))
        }

        try container.encode(typeName, forKey: AnyCodingKey("type"))
        switch self {
        case let .clientHello(versions, clientID, clientNonce):
            try container.encode(versions, forKey: AnyCodingKey("versions"))
            try container.encode(clientID, forKey: AnyCodingKey("client_id"))
            try container.encode(clientNonce, forKey: AnyCodingKey("client_nonce"))
        case let .serverHello(selected, hostID, hostName, hostNonce, pairingID, expiresAt, policy):
            try container.encode(selected, forKey: AnyCodingKey("selected"))
            try container.encode(hostID, forKey: AnyCodingKey("host_id"))
            try container.encode(hostName, forKey: AnyCodingKey("host_name"))
            try container.encode(hostNonce, forKey: AnyCodingKey("host_nonce"))
            try container.encode(pairingID, forKey: AnyCodingKey("pairing_id"))
            try container.encode(expiresAt, forKey: AnyCodingKey("expires_at"))
            try encodePermissions(policy)
        case let .pairingRequest(pairingID, tag):
            try container.encode(pairingID, forKey: AnyCodingKey("pairing_id"))
            try container.encode(tag, forKey: AnyCodingKey("tag"))
        case let .pairingAccepted(tag, sessionID, permissions):
            try container.encode(tag, forKey: AnyCodingKey("tag"))
            try container.encode(sessionID, forKey: AnyCodingKey("session_id"))
            try encodePermissions(permissions)
        case let .pairingRejected(reason):
            try container.encode(reason, forKey: AnyCodingKey("reason"))
        case let .sessionOpen(sessionID, permissions), let .sessionOpened(sessionID, permissions):
            try container.encode(sessionID, forKey: AnyCodingKey("session_id"))
            try encodePermissions(permissions)
        case let .keyframeRequest(sessionID):
            try container.encode(sessionID, forKey: AnyCodingKey("session_id"))
        case let .ping(nonce), let .pong(nonce):
            try container.encode(nonce, forKey: AnyCodingKey("nonce"))
        case let .sessionClose(sessionID, reason):
            try container.encodeIfPresent(sessionID, forKey: AnyCodingKey("session_id"))
            try container.encode(reason, forKey: AnyCodingKey("reason"))
        case let .error(code, message):
            try container.encode(code, forKey: AnyCodingKey("code"))
            try container.encode(message, forKey: AnyCodingKey("message"))
        case let .catalogSnapshot(snapshot):
            try snapshot.encodePayload(to: &container)
        }
    }
}

public enum WireError: Error, Equatable, CustomStringConvertible {
    case frameTooLarge
    case frameTruncated
    case frameLengthMismatch
    case malformedJSON
    case unknownMessageType(String)
    case invalidField(String)
    case unexpectedMessage(String)
    case invalidTransition(String)
    case invalidIdentifier
    case invalidNonce
    case unsupportedVersion
    case expiredOffer
    case unknownPairing
    case authenticationFailed
    case sessionMismatch
    case permissionDenied
    case entropyUnavailable

    public var description: String {
        switch self {
        case .frameTooLarge: "control frame exceeds the size limit"
        case .frameTruncated: "control frame is truncated"
        case .frameLengthMismatch: "control frame length does not match"
        case .malformedJSON: "control frame is not valid JSON"
        case let .unknownMessageType(type): "unknown control message type \(type)"
        case let .invalidField(field): "missing or invalid field \(field)"
        case let .unexpectedMessage(type): "unexpected message \(type)"
        case let .invalidTransition(detail): "invalid transition: \(detail)"
        case .invalidIdentifier: "invalid identifier"
        case .invalidNonce: "invalid nonce or pairing identifier"
        case .unsupportedVersion: "no supported protocol version overlap"
        case .expiredOffer: "pairing offer expired"
        case .unknownPairing: "unknown pairing identifier"
        case .authenticationFailed: "pairing authentication failed"
        case .sessionMismatch: "session identifier does not match"
        case .permissionDenied: "permission request exceeds the grant"
        case .entropyUnavailable: "secure randomness is unavailable"
        }
    }
}

public enum ControlFraming {
    public static let maxFrameBytes = 64 * 1024

    public static func encode(_ envelope: Envelope) throws -> Data {
        let json = try JSONEncoder().encode(envelope)
        guard json.count <= maxFrameBytes else { throw WireError.frameTooLarge }
        var data = Data()
        var length = UInt32(json.count).bigEndian
        withUnsafeBytes(of: &length) { data.append(contentsOf: $0) }
        data.append(json)
        return data
    }

    public static func decode(_ data: Data) throws -> Envelope {
        guard data.count >= 4 else { throw WireError.frameTruncated }
        let length = data.prefix(4).reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
        guard length <= maxFrameBytes else { throw WireError.frameTooLarge }
        guard data.count == 4 + Int(length) else { throw WireError.frameLengthMismatch }
        let json = data.suffix(from: 4)
        do {
            return try JSONDecoder().decode(Envelope.self, from: json)
        } catch let error as WireError {
            throw error
        } catch {
            throw WireError.malformedJSON
        }
    }

    /// Remove and return every complete frame in `buffer`. Leaves a partial
    /// frame for the next read.
    public static func drain(_ buffer: inout Data) throws -> [Envelope] {
        var envelopes: [Envelope] = []
        while buffer.count >= 4 {
            let length = buffer.prefix(4).reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
            guard length <= maxFrameBytes else { throw WireError.frameTooLarge }
            let total = 4 + Int(length)
            guard buffer.count >= total else { break }
            let frame = buffer.prefix(total)
            envelopes.append(try decode(Data(frame)))
            buffer.removeFirst(total)
        }
        return envelopes
    }
}

#endif
