import Foundation
import XCTest
@testable import VistaCore

final class ControlProtocolTests: XCTestCase {
    private struct Fixtures: Decodable {
        let protocolVersion: String
        let clientFrames: [Envelope]
        let hostFrames: [Envelope]

        enum CodingKeys: String, CodingKey {
            case protocolVersion = "protocol"
            case clientFrames = "client_frames"
            case hostFrames = "host_frames"
        }
    }

    private func repositoryRoot() -> URL {
        URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent() // VistaCoreTests
            .deletingLastPathComponent() // Tests
            .deletingLastPathComponent() // core
            .deletingLastPathComponent() // client
            .deletingLastPathComponent() // repository root
    }

    func testSharedProtocolFixturesDecodeAndRoundTrip() throws {
        let url = repositoryRoot().appendingPathComponent("docs/fixtures/protocol-v1.json")
        let fixtures = try JSONDecoder().decode(Fixtures.self, from: Data(contentsOf: url))
        XCTAssertEqual(fixtures.protocolVersion, "1.0")
        XCTAssertFalse(fixtures.clientFrames.isEmpty)

        guard case let .clientHello(versions, clientID, _) = fixtures.clientFrames[0].body else {
            return XCTFail("first client frame is not client_hello")
        }
        XCTAssertEqual(versions, .current)
        XCTAssertEqual(clientID, "iphone-15")

        for envelope in fixtures.clientFrames + fixtures.hostFrames {
            let framed = try ControlFraming.encode(envelope)
            XCTAssertEqual(try ControlFraming.decode(framed), envelope)
        }

        guard case let .catalogSnapshot(catalog) = fixtures.hostFrames.last?.body else {
            return XCTFail("last host frame is not catalog.snapshot")
        }
        XCTAssertEqual(catalog.entries.count, 2)
        XCTAssertEqual(catalog.entries[0].displayName, "Browser")
        XCTAssertEqual(catalog.revision, 1)
    }

    func testEveryMessageRoundTrips() throws {
        let catalog = try CatalogSnapshot(
            connectionEpoch: "s1",
            sessionID: "s1",
            generation: "niri-test",
            revision: 1,
            entries: [
                CatalogEntry(windowID: 5, title: "Browser", appID: "firefox", workspaceID: 2)
            ]
        )
        let messages: [ControlMessage] = [
            .clientHello(versions: .current, clientID: "iphone", clientNonce: String(repeating: "00", count: 32)),
            .serverHello(
                selected: .v1,
                hostID: "vista-host",
                hostName: "Vista host",
                hostNonce: String(repeating: "11", count: 32),
                pairingID: String(repeating: "22", count: 16),
                expiresAt: 1_700_000_000,
                policy: .videoOnly
            ),
            .pairingRequest(pairingID: String(repeating: "22", count: 16), tag: String(repeating: "33", count: 32)),
            .pairingAccepted(tag: String(repeating: "44", count: 32), sessionID: String(repeating: "55", count: 16), permissions: .videoOnly),
            .pairingRejected(reason: "denied"),
            .sessionOpen(sessionID: String(repeating: "55", count: 16), permissions: PermissionSet(video: true, input: true)),
            .sessionOpened(sessionID: String(repeating: "55", count: 16), permissions: .videoOnly),
            .keyframeRequest(sessionID: String(repeating: "55", count: 16)),
            .ping(nonce: 42),
            .pong(nonce: 42),
            .sessionClose(sessionID: String(repeating: "55", count: 16), reason: "done"),
            .sessionClose(sessionID: nil, reason: "anonymous close"),
            .error(code: "denied", message: "no"),
            .catalogSnapshot(catalog)
        ]
        for (index, message) in messages.enumerated() {
            let envelope = Envelope(seq: UInt64(index), body: message)
            let framed = try ControlFraming.encode(envelope)
            XCTAssertEqual(try ControlFraming.decode(framed), envelope)
        }
    }

    func testPermissionSubsetMatchesHostRules() {
        let grant = PermissionSet(video: true, input: true)
        XCTAssertTrue(PermissionSet(video: true).isSubset(of: grant))
        XCTAssertFalse(PermissionSet(video: true, clipboard: true).isSubset(of: grant))
        XCTAssertEqual(grant.bits, 0b101)
    }

    func testFramingDrainsPartialFrames() throws {
        let first = Envelope(seq: 0, body: .ping(nonce: 1))
        let second = Envelope(seq: 1, body: .pong(nonce: 1))
        var buffer = try ControlFraming.encode(first)
        buffer.append(try ControlFraming.encode(second))
        let split = try ControlFraming.encode(second).count / 2
        let third = try ControlFraming.encode(Envelope(seq: 2, body: .ping(nonce: 2)))
        buffer.append(third.prefix(split))

        let complete = try ControlFraming.drain(&buffer)
        XCTAssertEqual(complete.count, 2)
        XCTAssertEqual(complete[0], first)
        XCTAssertEqual(complete[1], second)
        XCTAssertFalse(buffer.isEmpty, "partial frame must remain buffered")

        buffer.append(contentsOf: third.suffix(from: split))
        let rest = try ControlFraming.drain(&buffer)
        XCTAssertEqual(rest, [Envelope(seq: 2, body: .ping(nonce: 2))])
        XCTAssertTrue(buffer.isEmpty)
    }

    func testFramingRejectsTruncatedAndOversizedInput() {
        XCTAssertThrowsError(try ControlFraming.decode(Data([0, 0, 0])))
        var oversized = Data([0, 1, 0, 1]) // 65537 bytes claimed
        oversized.append(Data(repeating: 0, count: 4))
        XCTAssertThrowsError(try ControlFraming.decode(oversized)) { error in
            XCTAssertEqual(error as? WireError, .frameTooLarge)
        }
        XCTAssertThrowsError(try ControlFraming.decode(Data([0, 0, 0, 2, 0x7b, 0x7d])))
    }

    func testUnknownMessageTypeIsRejected() throws {
        let json = Data(#"{"seq":0,"type":"future_message"}"#.utf8)
        var frame = Data()
        var length = UInt32(json.count).bigEndian
        withUnsafeBytes(of: &length) { frame.append(contentsOf: $0) }
        frame.append(json)
        XCTAssertThrowsError(try ControlFraming.decode(frame)) { error in
            XCTAssertEqual(error as? WireError, .unknownMessageType("future_message"))
        }
    }
}
