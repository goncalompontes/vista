#if !SKIP
import Crypto
import Foundation

// ChaCha20-Poly1305 record layer matching host/src/transport.rs.
//
// Each direction has its own key, implicit nonce counter and AAD bound to the
// session id. Records are ciphertext + 16-byte tag with no nonce on the wire.

public enum RecordDirection: Sendable {
    case clientToHost
    case hostToClient

    var tag: UInt8 {
        switch self {
        case .clientToHost: 1
        case .hostToClient: 2
        }
    }
}

public struct SecureRecordCipher {
    public static let tagBytes = 16
    public static let maxFrameBytes = 64 * 1024
    public static let maxRecordBytes = maxFrameBytes + tagBytes

    private let key: SymmetricKey
    private let aad: Data
    private var counter: UInt64 = 0

    public init(key: [UInt8], sessionID: String, direction: RecordDirection) throws {
        guard key.count == 32 else { throw WireError.invalidNonce }
        self.key = SymmetricKey(data: key)
        var aad = Data("vista-record-v1".utf8)
        aad.append(Data(sessionID.utf8))
        aad.append(direction.tag)
        self.aad = aad
    }

    public var recordCount: UInt64 { counter }

    private func nonce() throws -> ChaChaPoly.Nonce {
        guard counter < UInt64.max else { throw WireError.invalidTransition("record counter exhausted") }
        var bytes = [UInt8](repeating: 0, count: 12)
        let value = counter.bigEndian
        withUnsafeBytes(of: value) { raw in
            bytes.replaceSubrange(4..<12, with: raw)
        }
        return try ChaChaPoly.Nonce(data: Data(bytes))
    }

    public mutating func seal(_ plaintext: Data) throws -> Data {
        guard plaintext.count <= Self.maxFrameBytes else { throw WireError.frameTooLarge }
        let sealed = try ChaChaPoly.seal(
            plaintext,
            using: key,
            nonce: try nonce(),
            authenticating: aad
        )
        counter &+= 1
        return Data(sealed.ciphertext) + Data(sealed.tag)
    }

    public mutating func open(_ ciphertext: Data) throws -> Data {
        guard ciphertext.count >= Self.tagBytes,
              ciphertext.count <= Self.maxRecordBytes
        else {
            throw WireError.frameTooLarge
        }
        let body = ciphertext.prefix(ciphertext.count - Self.tagBytes)
        let tag = ciphertext.suffix(Self.tagBytes)
        do {
            let box = try ChaChaPoly.SealedBox(
                nonce: try nonce(),
                ciphertext: body,
                tag: tag
            )
            let plaintext = try ChaChaPoly.open(box, using: key, authenticating: aad)
            counter &+= 1
            return plaintext
        } catch {
            throw WireError.authenticationFailed
        }
    }
}

public enum SecureControlFraming {
    /// Length-prefixed sealed record: u32 big-endian size, then ciphertext+tag.
    public static func encodeFrame(_ plaintext: Data, cipher: inout SecureRecordCipher) throws -> Data {
        let sealed = try cipher.seal(plaintext)
        guard sealed.count <= SecureRecordCipher.maxRecordBytes else { throw WireError.frameTooLarge }
        var data = Data()
        var length = UInt32(sealed.count).bigEndian
        withUnsafeBytes(of: &length) { data.append(contentsOf: $0) }
        data.append(sealed)
        return data
    }

    public static func decodeFrame(_ frame: Data, cipher: inout SecureRecordCipher) throws -> Data {
        guard frame.count >= 4 else { throw WireError.frameTruncated }
        let length = frame.prefix(4).reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
        guard length >= UInt32(SecureRecordCipher.tagBytes),
              length <= UInt32(SecureRecordCipher.maxRecordBytes)
        else {
            throw WireError.frameTooLarge
        }
        guard frame.count == 4 + Int(length) else { throw WireError.frameLengthMismatch }
        return try cipher.open(frame.suffix(from: 4))
    }

    public static func drain(_ buffer: inout Data, cipher: inout SecureRecordCipher) throws -> [Data] {
        var plaintexts: [Data] = []
        while buffer.count >= 4 {
            let length = buffer.prefix(4).reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
            guard length >= UInt32(SecureRecordCipher.tagBytes),
                  length <= UInt32(SecureRecordCipher.maxRecordBytes)
            else {
                throw WireError.frameTooLarge
            }
            let total = 4 + Int(length)
            guard buffer.count >= total else { break }
            plaintexts.append(try decodeFrame(Data(buffer.prefix(total)), cipher: &cipher))
            buffer.removeFirst(total)
        }
        return plaintexts
    }
}

public enum ChannelRole: Sendable {
    case client
    case host
}

/// Post-pairing control channel: JSON envelopes inside encrypted records.
public final class SecureControlChannel {
    private var readCipher: SecureRecordCipher
    private var writeCipher: SecureRecordCipher

    public init(keys: SessionKeys, sessionID: String, role: ChannelRole = .client) throws {
        switch role {
        case .client:
            readCipher = try SecureRecordCipher(
                key: keys.hostToClient,
                sessionID: sessionID,
                direction: .hostToClient
            )
            writeCipher = try SecureRecordCipher(
                key: keys.clientToHost,
                sessionID: sessionID,
                direction: .clientToHost
            )
        case .host:
            readCipher = try SecureRecordCipher(
                key: keys.clientToHost,
                sessionID: sessionID,
                direction: .clientToHost
            )
            writeCipher = try SecureRecordCipher(
                key: keys.hostToClient,
                sessionID: sessionID,
                direction: .hostToClient
            )
        }
    }

    public func seal(_ envelope: Envelope) throws -> Data {
        let json = try JSONEncoder().encode(envelope)
        guard json.count <= SecureRecordCipher.maxFrameBytes else { throw WireError.frameTooLarge }
        return try SecureControlFraming.encodeFrame(json, cipher: &writeCipher)
    }

    public func open(_ frame: Data) throws -> Envelope {
        let json = try SecureControlFraming.decodeFrame(frame, cipher: &readCipher)
        do {
            return try JSONDecoder().decode(Envelope.self, from: json)
        } catch let error as WireError {
            throw error
        } catch {
            throw WireError.malformedJSON
        }
    }

    public func drain(_ buffer: inout Data) throws -> [Envelope] {
        let plaintexts = try SecureControlFraming.drain(&buffer, cipher: &readCipher)
        return try plaintexts.map { data in
            do {
                return try JSONDecoder().decode(Envelope.self, from: data)
            } catch let error as WireError {
                throw error
            } catch {
                throw WireError.malformedJSON
            }
        }
    }
}

#endif
