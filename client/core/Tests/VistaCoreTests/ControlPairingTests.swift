import Foundation
import XCTest
@testable import VistaCore

final class ControlPairingTests: XCTestCase {
    private let secret = try! PairingSecret(bytes: [UInt8](repeating: 0x07, count: 32))
    private let otherSecret = try! PairingSecret(bytes: [UInt8](repeating: 0x08, count: 32))
    private let hostNonceBytes = Array(UInt8(0x20)...UInt8(0x3F))
    private let pairingIDBytes = Array(UInt8(0x40)...UInt8(0x4F))
    private let sessionIDBytes = Array(UInt8(0x50)...UInt8(0x5F))

    private func repositoryRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
    }

    private struct PairingVector: Decodable {
        let secretHex: String
        let clientID: String
        let clientNonce: String
        let hostID: String
        let hostNonce: String
        let pairingID: String
        let selected: ProtocolVersion
        let policy: PermissionSet
        let transcriptHex: String
        let clientTag: String
        let sessionID: String
        let hostTag: String
        let clientToHost: String
        let hostToClient: String

        enum CodingKeys: String, CodingKey {
            case secretHex = "secret_hex"
            case clientID = "client_id"
            case clientNonce = "client_nonce"
            case hostID = "host_id"
            case hostNonce = "host_nonce"
            case pairingID = "pairing_id"
            case selected
            case policy
            case transcriptHex = "transcript_hex"
            case clientTag = "client_tag"
            case sessionID = "session_id"
            case hostTag = "host_tag"
            case clientToHost = "client_to_host"
            case hostToClient = "host_to_client"
        }
    }

    func testSharedPairingVectorMatchesRust() throws {
        let url = repositoryRoot().appendingPathComponent("docs/fixtures/pairing-v1.json")
        let vector = try JSONDecoder().decode(PairingVector.self, from: Data(contentsOf: url))
        let vectorSecret = try PairingSecret(hex: vector.secretHex)
        let transcript = ControlPairingClient.transcript(
            selected: vector.selected,
            clientID: vector.clientID,
            clientNonce: vector.clientNonce,
            hostID: vector.hostID,
            hostNonce: vector.hostNonce,
            pairingID: vector.pairingID,
            policy: vector.policy
        )
        XCTAssertEqual(transcript.map { String(format: "%02x", $0) }.joined(), vector.transcriptHex)
        XCTAssertEqual(
            ControlPairingClient.tag(
                secret: vectorSecret,
                label: Data("client-confirm\n".utf8),
                transcript: transcript
            ).hexString,
            vector.clientTag
        )
        XCTAssertEqual(
            ControlPairingClient.hostTag(
                secret: vectorSecret,
                transcript: transcript,
                sessionID: vector.sessionID
            ).hexString,
            vector.hostTag
        )
        let keys = try ControlPairingClient.deriveKeys(
            secret: vectorSecret,
            pairingID: vector.pairingID,
            transcript: transcript,
            sessionID: vector.sessionID
        )
        XCTAssertEqual(keys.clientToHost.hexString, vector.clientToHost)
        XCTAssertEqual(keys.hostToClient.hexString, vector.hostToClient)
    }

    private struct RecordVector: Decodable {
        let keyHex: String
        let sessionID: String
        let direction: String
        let counter: UInt64
        let plaintextHex: String
        let ciphertextHex: String

        enum CodingKeys: String, CodingKey {
            case keyHex = "key_hex"
            case sessionID = "session_id"
            case direction
            case counter
            case plaintextHex = "plaintext_hex"
            case ciphertextHex = "ciphertext_hex"
        }
    }

    private struct PairingFixture: Decodable {
        let record: RecordVector
    }

    func testSharedRecordVectorMatchesRust() throws {
        let url = repositoryRoot().appendingPathComponent("docs/fixtures/pairing-v1.json")
        let fixture = try JSONDecoder().decode(PairingFixture.self, from: Data(contentsOf: url))
        let record = fixture.record
        XCTAssertEqual(record.direction, "client_to_host")
        XCTAssertEqual(record.counter, 0)
        var cipher = try SecureRecordCipher(
            key: try XCTUnwrap([UInt8](hexString: record.keyHex)),
            sessionID: record.sessionID,
            direction: .clientToHost
        )
        let sealed = try cipher.seal(Data(try XCTUnwrap([UInt8](hexString: record.plaintextHex))))
        XCTAssertEqual(sealed.map { String(format: "%02x", $0) }.joined(), record.ciphertextHex)
        XCTAssertEqual(cipher.recordCount, 1)
    }

    func testFullPairingAndSession() throws {
        let client = try ControlPairingClient(secret: secret, clientID: "test-client")
        let hello = try client.hello()
        guard case let .clientHello(versions, _, clientNonce) = hello.body else {
            return XCTFail("expected client_hello")
        }
        XCTAssertEqual(versions, .current)

        let serverHello = Envelope(
            seq: 0,
            body: .serverHello(
                selected: .v1,
                hostID: "test-host",
                hostName: "Test host",
                hostNonce: hostNonceBytes.hexString,
                pairingID: pairingIDBytes.hexString,
                expiresAt: 2_000,
                policy: .videoOnly
            )
        )
        let proof = try client.acceptServerHello(serverHello, nowSeconds: 1_000)
        let transcript = ControlPairingClient.transcript(
            selected: .v1,
            clientID: "test-client",
            clientNonce: clientNonce,
            hostID: "test-host",
            hostNonce: hostNonceBytes.hexString,
            pairingID: pairingIDBytes.hexString,
            policy: .videoOnly
        )
        guard case let .pairingRequest(pairingID, tag) = proof.body else {
            return XCTFail("expected pairing_request")
        }
        XCTAssertEqual(pairingID, pairingIDBytes.hexString)
        XCTAssertEqual(
            tag,
            ControlPairingClient.tag(
                secret: secret,
                label: Data("client-confirm\n".utf8),
                transcript: transcript
            ).hexString
        )

        let accepted = Envelope(
            seq: 1,
            body: .pairingAccepted(
                tag: ControlPairingClient.hostTag(
                    secret: secret,
                    transcript: transcript,
                    sessionID: sessionIDBytes.hexString
                ).hexString,
                sessionID: sessionIDBytes.hexString,
                permissions: .videoOnly
            )
        )
        let keys = try client.acceptPairing(accepted)
        XCTAssertEqual(client.state, .paired)
        XCTAssertEqual(client.sessionID, sessionIDBytes.hexString)
        XCTAssertEqual(
            keys,
            try ControlPairingClient.deriveKeys(
                secret: secret,
                pairingID: pairingIDBytes.hexString,
                transcript: transcript,
                sessionID: sessionIDBytes.hexString
            )
        )

        let open = try client.openSession(.videoOnly)
        XCTAssertEqual(open.seq, 2)
        try client.acceptSessionOpened(
            Envelope(seq: 2, body: .sessionOpened(sessionID: sessionIDBytes.hexString, permissions: .videoOnly))
        )
        XCTAssertEqual(client.state, .active)

        try client.acceptStreamEvent(Envelope(seq: 3, body: .streamStarted(
            sessionID: sessionIDBytes.hexString,
            target: .desktop,
            width: 1920,
            height: 1080,
            geometryEpoch: 2,
            encoder: "test"
        )))
        try client.acceptStreamEvent(Envelope(seq: 4, body: .streamStopped(
            sessionID: sessionIDBytes.hexString, reason: "stopped"
        )))
        try client.acceptStreamEvent(Envelope(seq: 5, body: .streamError(
            sessionID: sessionIDBytes.hexString, code: "test", message: "failed"
        )))

        let ping = try client.ping(nonce: 42)
        XCTAssertEqual(ping.seq, 3)
        try client.acceptPong(Envelope(seq: 6, body: .pong(nonce: 42)), nonce: 42)
        XCTAssertEqual(try client.sessionClose(reason: "done").seq, 4)
        XCTAssertEqual(client.state, .closed)
    }

    private func clientAndTranscript() throws -> (ControlPairingClient, Data) {
        let client = try ControlPairingClient(secret: secret, clientID: "test-client")
        let hello = try client.hello()
        guard case let .clientHello(_, _, clientNonce) = hello.body else {
            throw WireError.unexpectedMessage(hello.body.typeName)
        }
        let transcript = ControlPairingClient.transcript(
            selected: .v1,
            clientID: "test-client",
            clientNonce: clientNonce,
            hostID: "test-host",
            hostNonce: hostNonceBytes.hexString,
            pairingID: pairingIDBytes.hexString,
            policy: .videoOnly
        )
        return (client, transcript)
    }

    private func serverHello(expiresAt: UInt64 = 2_000, selected: ProtocolVersion = .v1) -> Envelope {
        Envelope(
            seq: 0,
            body: .serverHello(
                selected: selected,
                hostID: "test-host",
                hostName: "Test host",
                hostNonce: hostNonceBytes.hexString,
                pairingID: pairingIDBytes.hexString,
                expiresAt: expiresAt,
                policy: .videoOnly
            )
        )
    }

    func testWrongHostSecretIsRejected() throws {
        let (client, transcript) = try clientAndTranscript()
        _ = try client.acceptServerHello(serverHello())
        let accepted = Envelope(
            seq: 1,
            body: .pairingAccepted(
                tag: ControlPairingClient.hostTag(
                    secret: otherSecret,
                    transcript: transcript,
                    sessionID: sessionIDBytes.hexString
                ).hexString,
                sessionID: sessionIDBytes.hexString,
                permissions: .videoOnly
            )
        )
        XCTAssertThrowsError(try client.acceptPairing(accepted)) { error in
            XCTAssertEqual(error as? WireError, .authenticationFailed)
        }
        XCTAssertEqual(client.state, .failed)
    }

    func testExpiredOfferAndUnsupportedVersionAreRejected() throws {
        let client = try ControlPairingClient(secret: secret, clientID: "test-client")
        _ = try client.hello()
        XCTAssertThrowsError(try client.acceptServerHello(serverHello(expiresAt: 1_000), nowSeconds: 1_001)) { error in
            XCTAssertEqual(error as? WireError, .expiredOffer)
        }

        let versionClient = try ControlPairingClient(secret: secret, clientID: "test-client")
        _ = try versionClient.hello()
        let future = serverHello(selected: ProtocolVersion(major: 2, minor: 0))
        XCTAssertThrowsError(try versionClient.acceptServerHello(future)) { error in
            XCTAssertEqual(error as? WireError, .unsupportedVersion)
        }
    }

    func testHostSequenceGapIsRejected() throws {
        let client = try ControlPairingClient(secret: secret, clientID: "test-client")
        _ = try client.hello()
        var gap = serverHello()
        gap.seq = 4
        XCTAssertThrowsError(try client.acceptServerHello(gap)) { error in
            guard case .invalidTransition = error as? WireError else {
                return XCTFail("expected invalidTransition, got \(error)")
            }
        }
        XCTAssertEqual(client.state, .failed)
    }

    func testPermissionEscalationIsRejected() throws {
        let (client, transcript) = try clientAndTranscript()
        _ = try client.acceptServerHello(serverHello())
        _ = try client.acceptPairing(
            Envelope(
                seq: 1,
                body: .pairingAccepted(
                    tag: ControlPairingClient.hostTag(
                        secret: secret,
                        transcript: transcript,
                        sessionID: sessionIDBytes.hexString
                    ).hexString,
                    sessionID: sessionIDBytes.hexString,
                    permissions: .videoOnly
                )
            )
        )
        XCTAssertThrowsError(try client.openSession(PermissionSet(video: true, input: true))) { error in
            XCTAssertEqual(error as? WireError, .permissionDenied)
        }
    }
}
