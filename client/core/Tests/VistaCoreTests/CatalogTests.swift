import Foundation
import XCTest
@testable import VistaCore

final class CatalogTests: XCTestCase {
    func testEntryDisplayNameFallsBackToAppIDThenWindowID() {
        XCTAssertEqual(CatalogEntry(windowID: 5, title: "Browser").displayName, "Browser")
        XCTAssertEqual(CatalogEntry(windowID: 5, title: "   ", appID: "firefox").displayName, "firefox")
        XCTAssertEqual(CatalogEntry(windowID: 5).displayName, "Window 5")
        XCTAssertEqual(CatalogEntry(windowID: 5).id, "w5")
    }

    func testSnapshotRoundTripsInsideAnEnvelope() throws {
        let snapshot = try CatalogSnapshot(
            connectionEpoch: "s1",
            sessionID: "s1",
            generation: "niri-test",
            revision: 3,
            entries: [
                CatalogEntry(windowID: 5, title: "Browser", appID: "firefox", workspaceID: 2),
                CatalogEntry(windowID: 9)
            ]
        )
        let envelope = Envelope(seq: 7, body: .catalogSnapshot(snapshot))
        let framed = try ControlFraming.encode(envelope)
        XCTAssertEqual(try ControlFraming.decode(framed), envelope)
    }

    func testValidationRejectsDuplicateIDsAndOversizedFields() throws {
        XCTAssertThrowsError(
            try CatalogSnapshot(
                connectionEpoch: "s1",
                sessionID: "s1",
                generation: "gen",
                revision: 1,
                entries: [CatalogEntry(windowID: 5), CatalogEntry(windowID: 5)]
            )
        )
        XCTAssertThrowsError(
            try CatalogSnapshot(
                connectionEpoch: "s1",
                sessionID: "s1",
                generation: "gen",
                revision: 1,
                entries: [CatalogEntry(windowID: 5, title: String(repeating: "x", count: 257))]
            )
        )
        XCTAssertThrowsError(
            try CatalogSnapshot(
                connectionEpoch: "s1",
                sessionID: "s1",
                generation: "gen",
                revision: 0,
                entries: []
            )
        )
        XCTAssertThrowsError(
            try CatalogSnapshot(
                connectionEpoch: "s1",
                sessionID: "s1",
                generation: "gen",
                revision: 1,
                entries: (0...256).map { CatalogEntry(windowID: UInt64($0)) }
            )
        )
    }

    func testEmptyCatalogIsValid() throws {
        let snapshot = try CatalogSnapshot(
            connectionEpoch: "s1",
            sessionID: nil,
            generation: "gen",
            revision: 1,
            entries: []
        )
        XCTAssertTrue(snapshot.entries.isEmpty)
    }

    func testClientConsumesCatalogInHostSequence() throws {
        let secret = try PairingSecret(bytes: [UInt8](repeating: 7, count: 32))
        let client = try ControlPairingClient(secret: secret, clientID: "test-client")
        _ = try client.hello()
        let snapshot = try CatalogSnapshot(
            connectionEpoch: "s1",
            sessionID: "s1",
            generation: "gen",
            revision: 1,
            entries: [CatalogEntry(windowID: 5)]
        )
        // Before pairing the client is awaiting server_hello; a catalog frame is
        // still sequence-checked and accepted for the state-machine test.
        let envelope = Envelope(seq: 0, body: .catalogSnapshot(snapshot))
        let accepted = try client.acceptCatalogSnapshot(envelope)
        XCTAssertEqual(accepted.entries.count, 1)
        XCTAssertThrowsError(
            try client.acceptCatalogSnapshot(
                Envelope(seq: 0, body: .catalogSnapshot(snapshot))
            )
        )
    }
}
