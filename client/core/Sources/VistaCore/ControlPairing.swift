#if !SKIP
import Crypto
import Foundation

// Client side of Vista control protocol v1 pairing. Mirrors host/src/protocol.rs:
// HMAC-SHA256 confirmations, HKDF-SHA256 session keys and the exact transcript
// bytes. Foundation + swift-crypto only, so it runs in Linux tests.

public struct PairingSecret: Equatable, Sendable {
    public static let byteCount = 32
    private let bytes: [UInt8]

    public init(bytes: [UInt8]) throws {
        guard bytes.count == Self.byteCount else { throw WireError.invalidNonce }
        self.bytes = bytes
    }

    public init(hex: String) throws {
        guard let decoded = [UInt8](hexString: hex), decoded.count == Self.byteCount else {
            throw WireError.invalidNonce
        }
        self.bytes = decoded
    }

    public static func generate() throws -> PairingSecret {
        var generator = SystemRandomNumberGenerator()
        let bytes = (0..<byteCount).map { _ in UInt8.random(in: 0...255, using: &generator) }
        return try PairingSecret(bytes: bytes)
    }

    public var hex: String { bytes.hexString }

    fileprivate var key: SymmetricKey { SymmetricKey(data: bytes) }
    fileprivate var raw: [UInt8] { bytes }
}

public struct SessionKeys: Equatable, Sendable {
    public let clientToHost: [UInt8]
    public let hostToClient: [UInt8]

    public var fingerprint: String { Array(clientToHost.prefix(8)).hexString }
}

public enum ClientState: Equatable, Sendable {
    case awaitingHello
    case awaitingServerHello
    case awaitingPairingAccepted
    case paired
    case active
    case closed
    case failed
}

public final class ControlPairingClient {
    public private(set) var state: ClientState = .awaitingHello
    public private(set) var sessionKeys: SessionKeys?
    public private(set) var sessionID: String?
    public private(set) var policy: PermissionSet?
    public private(set) var nextClientSeq: UInt64 = 0

    private let secret: PairingSecret
    private let clientID: String
    private let clientNonce: String
    private let versions: VersionRange
    private var expectedHostSeq: UInt64 = 0
    private var pairingID: String?
    private var transcript = Data()
    private var requested: PermissionSet?

    public init(
        secret: PairingSecret,
        clientID: String,
        versions: VersionRange = .current
    ) throws {
        try Self.validateIdentifier(clientID)
        guard versions.min <= versions.max else { throw WireError.unsupportedVersion }
        self.secret = secret
        self.clientID = clientID
        self.versions = versions
        self.clientNonce = try Self.randomHex(32)
    }

    public func hello() throws -> Envelope {
        guard state == .awaitingHello else {
            throw WireError.invalidTransition("a fresh client")
        }
        let envelope = Envelope(
            seq: nextClientSeq,
            body: .clientHello(versions: versions, clientID: clientID, clientNonce: clientNonce)
        )
        nextClientSeq &+= 1
        state = .awaitingServerHello
        return envelope
    }

    public func acceptServerHello(_ envelope: Envelope, nowSeconds: UInt64? = nil) throws -> Envelope {
        guard state == .awaitingServerHello else {
            throw WireError.invalidTransition("server_hello")
        }
        try checkHostSequence(envelope)
        guard case let .serverHello(
            selected, hostID, _, hostNonce, pairingID, expiresAt, policy
        ) = envelope.body else {
            state = .failed
            throw WireError.unexpectedMessage(envelope.body.typeName)
        }
        guard versions.supports(selected) else {
            state = .failed
            throw WireError.unsupportedVersion
        }
        try Self.validateIdentifier(hostID)
        try Self.validateHex(hostNonce, bytes: 32)
        try Self.validateHex(pairingID, bytes: 16)
        if let nowSeconds, nowSeconds > expiresAt {
            state = .failed
            throw WireError.expiredOffer
        }
        transcript = Self.transcript(
            selected: selected,
            clientID: clientID,
            clientNonce: clientNonce,
            hostID: hostID,
            hostNonce: hostNonce,
            pairingID: pairingID,
            policy: policy
        )
        let proof = Self.tag(secret: secret, label: Data("client-confirm\n".utf8), transcript: transcript)
        self.pairingID = pairingID
        self.policy = policy
        state = .awaitingPairingAccepted
        let envelope = Envelope(
            seq: nextClientSeq,
            body: .pairingRequest(pairingID: pairingID, tag: proof.hexString)
        )
        nextClientSeq &+= 1
        return envelope
    }

    @discardableResult
    public func acceptPairing(_ envelope: Envelope) throws -> SessionKeys {
        guard state == .awaitingPairingAccepted else {
            throw WireError.invalidTransition("pairing_accepted")
        }
        try checkHostSequence(envelope)
        guard case let .pairingAccepted(tag, sessionID, permissions) = envelope.body else {
            state = .failed
            throw WireError.unexpectedMessage(envelope.body.typeName)
        }
        try Self.validateHex(sessionID, bytes: 16)
        guard let provided = [UInt8](hexString: tag), provided.count == 32 else {
            state = .failed
            throw WireError.authenticationFailed
        }
        guard Self.verifyHostTag(
            secret: secret,
            transcript: transcript,
            sessionID: sessionID,
            provided: provided
        ) else {
            state = .failed
            throw WireError.authenticationFailed
        }
        guard let pairingID else { throw WireError.unknownPairing }
        let keys = try Self.deriveKeys(
            secret: secret,
            pairingID: pairingID,
            transcript: transcript,
            sessionID: sessionID
        )
        self.policy = permissions
        self.sessionID = sessionID
        self.sessionKeys = keys
        state = .paired
        return keys
    }

    public func openSession(_ permissions: PermissionSet) throws -> Envelope {
        guard state == .paired else {
            throw WireError.invalidTransition("a paired client")
        }
        if let policy, !permissions.isSubset(of: policy) {
            throw WireError.permissionDenied
        }
        guard let sessionID else { throw WireError.sessionMismatch }
        requested = permissions
        let envelope = Envelope(
            seq: nextClientSeq,
            body: .sessionOpen(sessionID: sessionID, permissions: permissions)
        )
        nextClientSeq &+= 1
        return envelope
    }

    public func acceptSessionOpened(_ envelope: Envelope) throws {
        guard state == .paired else {
            throw WireError.invalidTransition("session_opened")
        }
        try checkHostSequence(envelope)
        guard case let .sessionOpened(sessionID, permissions) = envelope.body else {
            state = .failed
            throw WireError.unexpectedMessage(envelope.body.typeName)
        }
        guard self.sessionID == sessionID else {
            state = .failed
            throw WireError.sessionMismatch
        }
        guard let requested, permissions.isSubset(of: requested) else {
            state = .failed
            throw WireError.permissionDenied
        }
        state = .active
    }

    /// Consume an optional media endpoint announcement while preserving strict
    /// host sequencing.
    @discardableResult
    public func acceptMediaOffer(_ envelope: Envelope) throws -> MediaOffer {
        try checkHostSequence(envelope)
        guard case let .mediaOffer(offer) = envelope.body else {
            throw WireError.unexpectedMessage(envelope.body.typeName)
        }
        return offer
    }

    public func keyframeRequest() throws -> Envelope {
        guard state == .active else {
            throw WireError.invalidTransition("an active session")
        }
        guard let sessionID else { throw WireError.sessionMismatch }
        let envelope = Envelope(seq: nextClientSeq, body: .keyframeRequest(sessionID: sessionID))
        nextClientSeq &+= 1
        return envelope
    }

    public func actionInvoke(_ request: ActionRequestPayload) throws -> Envelope {
        guard state == .active else {
            throw WireError.invalidTransition("an active session")
        }
        let envelope = Envelope(seq: nextClientSeq, body: .actionInvoke(request))
        nextClientSeq &+= 1
        return envelope
    }

    public func acceptCapabilitySnapshot(_ envelope: Envelope) throws -> CapabilitySnapshot {
        try checkHostSequence(envelope)
        guard case let .capabilitySnapshot(snapshot) = envelope.body else {
            throw WireError.unexpectedMessage(envelope.body.typeName)
        }
        guard snapshot.sessionId == sessionID else {
            throw WireError.sessionMismatch
        }
        return snapshot
    }

    public func acceptActionResult(_ envelope: Envelope) throws -> ActionResultPayload {
        try checkHostSequence(envelope)
        guard case let .actionResult(result) = envelope.body else {
            throw WireError.unexpectedMessage(envelope.body.typeName)
        }
        guard result.sessionId == sessionID else {
            throw WireError.sessionMismatch
        }
        return result
    }

    public func ping(nonce: UInt64) throws -> Envelope {
        guard state == .active else {
            throw WireError.invalidTransition("an active session")
        }
        let envelope = Envelope(seq: nextClientSeq, body: .ping(nonce: nonce))
        nextClientSeq &+= 1
        return envelope
    }

    public func acceptPong(_ envelope: Envelope, nonce: UInt64) throws {
        try checkHostSequence(envelope)
        guard case let .pong(received) = envelope.body, received == nonce else {
            throw WireError.unexpectedMessage(envelope.body.typeName)
        }
    }

    /// Consume a catalog snapshot and advance the host sequence. The catalog is
    /// metadata, not a grant; callers must not infer permissions from it.
    @discardableResult
    public func acceptCatalogSnapshot(_ envelope: Envelope) throws -> CatalogSnapshot {
        try checkHostSequence(envelope)
        guard case let .catalogSnapshot(snapshot) = envelope.body else {
            throw WireError.unexpectedMessage(envelope.body.typeName)
        }
        return snapshot
    }

    public func sessionClose(reason: String) throws -> Envelope {
        guard state == .active else {
            throw WireError.invalidTransition("an active session")
        }
        let envelope = Envelope(seq: nextClientSeq, body: .sessionClose(sessionID: sessionID, reason: reason))
        nextClientSeq &+= 1
        state = .closed
        return envelope
    }

    private func checkHostSequence(_ envelope: Envelope) throws {
        guard envelope.seq == expectedHostSeq else {
            state = .failed
            throw WireError.invalidTransition(
                "host sequence expected \(expectedHostSeq), received \(envelope.seq)"
            )
        }
        expectedHostSeq &+= 1
    }

    // MARK: - Wire crypto

    static func transcript(
        selected: ProtocolVersion,
        clientID: String,
        clientNonce: String,
        hostID: String,
        hostNonce: String,
        pairingID: String,
        policy: PermissionSet
    ) -> Data {
        Data(
            "vista-pairing-v1\nversion=\(selected)\nclient_id=\(clientID)\nclient_nonce=\(clientNonce)\nhost_id=\(hostID)\nhost_nonce=\(hostNonce)\npairing_id=\(pairingID)\npolicy=\(policy.bits)\n".utf8
        )
    }

    static func tag(secret: PairingSecret, label: Data, transcript: Data) -> [UInt8] {
        let code = HMAC<SHA256>.authenticationCode(
            for: label + transcript,
            using: secret.key
        )
        return Array(code)
    }

    static func verifyHostTag(
        secret: PairingSecret,
        transcript: Data,
        sessionID: String,
        provided: [UInt8]
    ) -> Bool {
        let expected = hostTag(secret: secret, transcript: transcript, sessionID: sessionID)
        guard expected.count == provided.count else { return false }
        var difference: UInt8 = 0
        for (lhs, rhs) in zip(expected, provided) {
            difference |= lhs ^ rhs
        }
        return difference == 0
    }

    static func hostTag(secret: PairingSecret, transcript: Data, sessionID: String) -> [UInt8] {
        let data = Data("host-confirm\n".utf8)
            + transcript
            + Data("\nsession=".utf8)
            + Data(sessionID.utf8)
        return Array(HMAC<SHA256>.authenticationCode(for: data, using: secret.key))
    }

    static func deriveKeys(
        secret: PairingSecret,
        pairingID: String,
        transcript: Data,
        sessionID: String
    ) throws -> SessionKeys {
        var info = Data("vista-session-v1\n".utf8)
        info.append(Data(pairingID.utf8))
        info.append(0x0A)
        info.append(Data(sessionID.utf8))
        info.append(0x0A)
        info.append(transcript)
        let derived = HKDF<SHA256>.deriveKey(
            inputKeyMaterial: secret.key,
            salt: Data(pairingID.utf8),
            info: info,
            outputByteCount: 64
        )
        let bytes = derived.withUnsafeBytes { Array($0) }
        guard bytes.count == 64 else { throw WireError.entropyUnavailable }
        return SessionKeys(
            clientToHost: Array(bytes[0..<32]),
            hostToClient: Array(bytes[32..<64])
        )
    }

    // MARK: - Validation

    static func validateIdentifier(_ value: String) throws {
        guard !value.isEmpty,
              value.utf8.count <= 128,
              value.utf8.allSatisfy({ (0x20...0x7E).contains($0) })
        else {
            throw WireError.invalidIdentifier
        }
    }

    static func validateHex(_ value: String, bytes: Int) throws {
        guard let decoded = [UInt8](hexString: value), decoded.count == bytes else {
            throw WireError.invalidNonce
        }
    }

    static func randomHex(_ byteCount: Int) throws -> String {
        var generator = SystemRandomNumberGenerator()
        return (0..<byteCount)
            .map { _ in UInt8.random(in: 0...255, using: &generator) }
            .hexString
    }
}

extension Array where Element == UInt8 {
    init?(hexString: String) {
        guard hexString.count.isMultiple(of: 2) else { return nil }
        var bytes: [UInt8] = []
        bytes.reserveCapacity(hexString.count / 2)
        var index = hexString.startIndex
        while index < hexString.endIndex {
            let next = hexString.index(index, offsetBy: 2)
            guard let byte = UInt8(hexString[index..<next], radix: 16) else { return nil }
            bytes.append(byte)
            index = next
        }
        self = bytes
    }

    var hexString: String {
        map { String(format: "%02x", $0) }.joined()
    }
}

#endif
