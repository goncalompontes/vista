import XCTest
@testable import VistaCore

final class CapabilitySessionTests: XCTestCase {
    private let snapshotJSON = #"""
    {
      "v": 1,
      "type": "capabilities.snapshot",
      "connection_epoch": "c1",
      "session_id": "s1",
      "target_id": "w5",
      "target_generation": "niri-boot1-w5",
      "capability_revision": 12,
      "grant_epoch": "g2",
      "actions": [
        {
          "id": "window.focus",
          "schema_version": 1,
          "supported": true,
          "enabled": true,
          "authorization": "granted",
          "parameters": "vista.empty.v1",
          "presentation": {"label": "Focus window"},
          "suggested_bindings": []
        },
        {
          "id": "window.maximize.set",
          "schema_version": 1,
          "supported": true,
          "enabled": true,
          "authorization": "granted",
          "parameters": "vista.window.maximize.set.v1",
          "presentation": {"label": "Maximize"},
          "suggested_bindings": []
        },
        {
          "id": "window.minimize",
          "schema_version": 1,
          "supported": true,
          "enabled": false,
          "reason": "No managed session is selected",
          "authorization": "granted",
          "parameters": "vista.empty.v1",
          "presentation": {"label": "Minimize"},
          "suggested_bindings": []
        },
        {
          "id": "desktop.overview.show",
          "schema_version": 1,
          "supported": false,
          "enabled": false,
          "reason": "Backend does not implement overview",
          "authorization": "required",
          "parameters": "vista.empty.v1",
          "presentation": {"label": "Overview"},
          "suggested_bindings": []
        },
        {
          "id": "edit.copy",
          "schema_version": 1,
          "supported": true,
          "enabled": true,
          "authorization": "denied",
          "parameters": "vista.empty.v1",
          "presentation": {"label": "Copy"},
          "suggested_bindings": []
        }
      ]
    }
    """#

    private func snapshot() throws -> CapabilitySnapshot {
        try JSONDecoder().decode(CapabilitySnapshot.self, from: Data(snapshotJSON.utf8))
    }

    func testSelectableActionsExcludeUnsupportedDisabledAndDenied() throws {
        let session = CapabilitySession(snapshot: try snapshot())
        XCTAssertEqual(session.selectableActions.map(\.id), ["window.focus", "window.maximize.set"])
        XCTAssertNil(session.action(id: "window.nonexistent"))
        XCTAssertEqual(session.action(id: "window.minimize")?.enabled, false)
        XCTAssertEqual(session.action(id: "desktop.overview.show")?.supported, false)
        XCTAssertEqual(session.action(id: "edit.copy")?.authorization, .denied)
    }

    func testRequestsUseSnapshotEpochsAndUniqueIDs() throws {
        var session = CapabilitySession(snapshot: try snapshot())
        let first = try XCTUnwrap(session.makeRequest(actionID: "window.focus"))
        XCTAssertEqual(first.id, "r1")
        XCTAssertEqual(first.connectionEpoch, "c1")
        XCTAssertEqual(first.sessionId, "s1")
        XCTAssertEqual(first.grantEpoch, "g2")
        XCTAssertEqual(first.targetId, "w5")
        XCTAssertEqual(first.targetGeneration, "niri-boot1-w5")
        XCTAssertEqual(first.capabilityRevision, 12)
        XCTAssertEqual(first.action, "window.focus")
        XCTAssertEqual(first.schemaVersion, 1)
        XCTAssertEqual(session.pendingRequestIDs, ["r1"])

        let second = try XCTUnwrap(session.makeRequest(actionID: "window.maximize.set", parameters: ["value": .bool(true)]))
        XCTAssertEqual(second.id, "r2")
        XCTAssertEqual(second.parameters["value"], .bool(true))
        XCTAssertEqual(session.pendingRequestIDs, ["r1", "r2"])
    }

    func testUnavailableActionsCannotProduceRequests() throws {
        var session = CapabilitySession(snapshot: try snapshot())
        for id in ["window.minimize", "desktop.overview.show", "edit.copy", "window.nonexistent"] {
            XCTAssertNil(session.makeRequest(actionID: id), id)
        }
        XCTAssertTrue(session.pendingRequestIDs.isEmpty)
    }

    func testCompletedResultUpdatesRevisionAndPresentation() throws {
        var session = CapabilitySession(snapshot: try snapshot())
        let request = try XCTUnwrap(session.makeRequest(actionID: "window.maximize.set"))
        let result = ActionResultPayload(
            v: 1,
            messageType: "action.result",
            id: request.id,
            connectionEpoch: "c1",
            sessionId: "s1",
            status: .completed,
            errorCode: nil,
            state: .object(["maximized": .bool(true)]),
            capabilityRevision: 13
        )
        XCTAssertTrue(session.apply(result))
        XCTAssertEqual(session.revision, 13)
        XCTAssertEqual(session.action(id: "window.maximize.set")?.presentation.label, "Restore")
        XCTAssertEqual(session.pendingRequestIDs, [])
        XCTAssertEqual(session.lastResult?.status, .completed)
    }

    func testStaleUnknownAndLowerRevisionResultsAreIgnored() throws {
        var session = CapabilitySession(snapshot: try snapshot())
        let request = try XCTUnwrap(session.makeRequest(actionID: "window.focus"))

        let staleConnection = ActionResultPayload(
            v: 1, messageType: "action.result", id: request.id,
            connectionEpoch: "old", sessionId: "s1", status: .completed,
            errorCode: nil, state: nil, capabilityRevision: 13
        )
        XCTAssertFalse(session.apply(staleConnection))
        XCTAssertEqual(session.revision, 12)
        XCTAssertEqual(session.pendingRequestIDs, [request.id])

        let unknown = ActionResultPayload(
            v: 1, messageType: "action.result", id: "nope",
            connectionEpoch: "c1", sessionId: "s1", status: .completed,
            errorCode: nil, state: nil, capabilityRevision: 13
        )
        XCTAssertFalse(session.apply(unknown))

        let oldRevision = ActionResultPayload(
            v: 1, messageType: "action.result", id: request.id,
            connectionEpoch: "c1", sessionId: "s1", status: .completed,
            errorCode: nil, state: nil, capabilityRevision: 11
        )
        XCTAssertFalse(session.apply(oldRevision))
    }

    func testRejectionIsSurfacedWithoutChangingRevision() throws {
        var session = CapabilitySession(snapshot: try snapshot())
        let request = try XCTUnwrap(session.makeRequest(actionID: "window.focus"))
        let rejected = ActionResultPayload(
            v: 1, messageType: "action.result", id: request.id,
            connectionEpoch: "c1", sessionId: "s1", status: .rejected,
            errorCode: .staleContext, state: nil, capabilityRevision: nil
        )
        XCTAssertTrue(session.apply(rejected))
        XCTAssertEqual(session.revision, 12)
        XCTAssertEqual(session.lastResult?.errorCode, .staleContext)
        XCTAssertEqual(session.pendingRequestIDs, [])
    }

    func testReconnectDiscardsQueuedRequests() throws {
        var session = CapabilitySession(snapshot: try snapshot())
        _ = session.makeRequest(actionID: "window.focus")
        XCTAssertFalse(session.pendingRequestIDs.isEmpty)

        var nextJSON = snapshotJSON
        nextJSON = nextJSON.replacingOccurrences(of: "\"connection_epoch\": \"c1\"", with: "\"connection_epoch\": \"c2\"")
        let next = try JSONDecoder().decode(CapabilitySnapshot.self, from: Data(nextJSON.utf8))
        session.reconnect(with: next)
        XCTAssertTrue(session.pendingRequestIDs.isEmpty)
        XCTAssertNil(session.lastResult)
        XCTAssertEqual(session.snapshot.connectionEpoch, "c2")
    }
}

extension CapabilitySessionTests {
    func testDemoSnapshotOffersNoSelectableActions() {
        let session = CapabilitySession(snapshot: DemoCatalog.capabilitySnapshot)
        XCTAssertTrue(session.selectableActions.isEmpty)
        XCTAssertEqual(session.snapshot.actions.count, 4)
    }
}
