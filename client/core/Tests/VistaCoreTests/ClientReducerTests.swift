import Foundation
import XCTest
@testable import VistaCore

/// PR #76's design cases intentionally omit prerequisite events. This runner
/// supplies a standard authorized host/target context where necessary, then
/// applies the relevant events and checks the original named expectation.
/// It never treats these design fixtures as canonical wire/protobuf vectors.
final class ClientReducerTests: XCTestCase {
    private let c1 = ClientConnectionID("c1")
    private let t1 = ClientTarget("t1", generation: 1)

    private struct Driver {
        var state = ClientAppState()
        var effects: [ClientEffect] = []
        mutating func send(_ event: ClientEvent) {
            let reduced = ClientReducer.reduce(state, event)
            state = reduced.state
            effects += reduced.effects
        }
        mutating func host(_ connection: ClientConnectionID, _ event: ClientHostEvent) {
            send(.remote(connection, event))
        }
    }

    private func ready(_ features: [String] = ["capture.picker", "input.pointer"],
                       _ actions: [ClientAction] = [], view: Bool = true) -> Driver {
        var d = Driver()
        d.send(.authenticated(c1))
        d.host(c1, .catalogSnapshot(ClientRevision(1), [t1]))
        d.host(c1, .capabilitySnapshot(ClientRevision(1), features, actions))
        d.host(c1, .selectTarget(t1))
        if view { d.host(c1, .permission("video.view", .granted, .unknown, t1)) }
        return d
    }

    private func live(_ d: inout Driver) {
        d.send(.intent(.openPicker("WINDOW")))
        d.host(c1, .captureApproved(t1, "WINDOW", nil))
        d.host(c1, .firstFrame(t1, 1, 1))
    }

    private func controlled() -> Driver {
        var d = ready()
        live(&d)
        d.host(c1, .permission("input.control", .granted, .sharedDesktop, t1))
        d.host(c1, .lease(ClientLease(id: "l1", target: t1, expiresAt: 100)))
        return d
    }

    func testAllDesignFixtureCasesHaveExplicitConformanceAssertions() throws {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "client-state-fixtures", withExtension: "json", subdirectory: "Fixtures"))
        let data = try Data(contentsOf: url)
        let root = try XCTUnwrap(JSONSerialization.jsonObject(with: data) as? [String: Any])
        XCTAssertEqual(root["fixture_version"] as? Int, 1)
        let cases = try XCTUnwrap(root["cases"] as? [[String: Any]])
        let ids = try cases.map { try XCTUnwrap($0["id"] as? String) }
        XCTAssertEqual(Set(ids).count, ids.count)
        XCTAssertEqual(ids.count, 23, "Keep the design fixture collection fully covered")
        for fixture in cases {
            let id = try XCTUnwrap(fixture["id"] as? String)
            XCTAssertNotNil(fixture["events"] as? [[String: Any]], id)
            XCTAssertNotNil(fixture["expected"] as? [String: Any], id)
            try verify(id, expected: fixture["expected"] as! [String: Any])
        }
    }

    private func verify(_ id: String, expected: [String: Any], file: StaticString = #filePath, line: UInt = #line) throws {
        // An unrecognized fixture must fail, never silently report coverage.
        switch id {
        case "unpaired_discovery":
            var d = Driver(); d.send(.discovered("Nearby device"))
            XCTAssertFalse(d.state.showCatalog); XCTAssertFalse(d.state.authenticated)
            XCTAssertEqual(d.state.trust, .untrusted); XCTAssertEqual(d.state.media, .none)
        case "wrong_host_identity":
            var d = Driver(); d.send(.pairingPending); d.send(.pairingRejected("HOST_IDENTITY_MISMATCH"))
            XCTAssertFalse(d.state.authenticated); XCTAssertFalse(d.state.showCatalog)
            XCTAssertEqual(d.state.warning, expected["warning"] as? String)
        case "portal_picker_only":
            var d = ready(["capture.picker"])
            live(&d)
            XCTAssertEqual(d.state.librarySections, expected["library_sections"] as? [String])
            XCTAssertEqual(d.state.sourceLabel, expected["source_label"] as? String)
            XCTAssertNil(d.state.verifiedCatalogTarget); XCTAssertTrue(d.state.isLive)
        case "view_only":
            var d = ready(); d.host(c1, .permission("input.control", .denied, .unknown, t1)); live(&d)
            XCTAssertEqual(d.state.streamStatus, expected["stream_status"] as? String)
            XCTAssertFalse(d.state.canSendInput); XCTAssertTrue(d.state.canRequestControl)
            XCTAssertFalse(d.state.canReadClipboard)
        case "input_desktop_scope":
            let d = controlled()
            XCTAssertEqual(d.state.inputScopeLabel, expected["input_scope_label"] as? String)
            XCTAssertTrue(d.state.canSendInput); XCTAssertNotEqual(d.state.inputScope, .privateSession)
        case "lease_revoked":
            var d = controlled(); d.effects = []
            d.host(c1, .leaseRevoked("l1"))
            XCTAssertFalse(d.state.canSendInput); XCTAssertTrue(d.effects.contains(.releaseAll))
        case "capability_removed":
            var d = ready([], [ClientAction("host.workspace.next")])
            d.host(c1, .capabilityDelta(ClientRevision(1), ClientRevision(2), ["host.workspace.next"]))
            d.send(.intent(.invoke("host.workspace.next", "req")))
            XCTAssertTrue(d.state.availableActions.isEmpty)
            XCTAssertFalse(d.effects.contains { if case .action = $0 { return true }; return false })
        case "revision_gap":
            var d = ready(); d.host(c1, .catalogSnapshot(ClientRevision(10), [t1])); d.effects = []
            d.host(c1, .catalogDelta(ClientRevision(12), ClientRevision(13), [ClientTarget("t2", generation: 1)], []))
            XCTAssertEqual(d.state.catalogRevision, ClientRevision(expected["catalog_revision"] as! UInt64Convertible))
            XCTAssertFalse(d.state.catalogFresh); XCTAssertFalse(d.state.catalog.contains { $0.id == "t2" })
            XCTAssertTrue(d.effects.contains(.requestCatalog))
        case "stale_target_response":
            var d = ready(); let old = ClientTarget("t1", generation: 3)
            d.host(c1, .selectTarget(old)); d.host(c1, .targetClosed(old)); d.effects = []
            d.host(c1, .actionResult("req", old, ClientRevision(1), true))
            XCTAssertFalse(d.state.targetIsCurrent); XCTAssertNil(d.state.lastConfirmedAction)
            XCTAssertTrue(d.effects.contains(.ignoredStaleEvent))
        case "window_capture_rejected_monitor":
            var d = ready(); d.send(.intent(.openPicker("WINDOW")))
            d.host(c1, .captureRejected("SCOPE_MISMATCH"))
            XCTAssertEqual(d.state.media, .none); XCTAssertNotNil(d.state.warning)
            XCTAssertFalse(d.effects.contains(.openPicker("MONITOR")))
        case "media_connected_no_frame":
            var d = ready(); d.host(c1, .rtcConnected)
            XCTAssertEqual(d.state.media, .waitingForFirstFrame); XCTAssertFalse(d.state.isLive)
        case "decoder_waiting_keyframe":
            var d = ready(); live(&d); d.effects = []
            d.host(c1, .decoderWaiting(t1, 2))
            XCTAssertEqual(d.state.media, .recovering); XCTAssertFalse(d.state.isLive)
            XCTAssertTrue(d.effects.contains(.requestKeyframe))
        case "disconnect_stops_input":
            var d = controlled(); d.send(.intent(.keyDown(4))); d.effects = []
            d.host(c1, .disconnected)
            XCTAssertFalse(d.state.canSendInput); XCTAssertEqual(d.state.media, .disconnected)
            XCTAssertTrue(d.state.heldKeys.isEmpty); XCTAssertTrue(d.effects.contains(.releaseAll))
        case "reconnect_clears_old_action":
            var d = ready([], [ClientAction("window.close")]); d.send(.intent(.invoke("window.close", "r1")))
            XCTAssertNotNil(d.state.pending["r1"])
            d.send(.authenticated(ClientConnectionID("c2"))); d.effects = []
            d.host(c1, .actionResult("r1", t1, ClientRevision(1), true))
            XCTAssertTrue(d.state.pending.isEmpty); XCTAssertNil(d.state.lastConfirmedAction)
            XCTAssertFalse(d.state.capabilitiesFresh); XCTAssertTrue(d.effects.contains(.ignoredStaleEvent))
        case "clipboard_grants_independent":
            var d = ready()
            d.host(c1, .permission("clipboard.read", .granted, .unknown, t1))
            d.host(c1, .permission("clipboard.write", .denied, .unknown, t1))
            XCTAssertTrue(d.state.canReadClipboard); XCTAssertFalse(d.state.canWriteClipboard)
            XCTAssertFalse(d.effects.contains(.clipboard(true)))
        case "managed_session_crash":
            var d = controlled(); d.host(c1, .sessionFailed(t1))
            XCTAssertEqual(d.state.media, .closed); XCTAssertFalse(d.state.canSendInput)
            XCTAssertNotNil(d.state.warning); XCTAssertTrue(d.state.managedSessionFailed)
        case "backgrounds_during_control":
            var d = controlled(); d.effects = []; d.send(.backgrounded)
            XCTAssertFalse(d.state.canSendInput); XCTAssertTrue(d.effects.contains(.releaseAll))
            d.send(.foregrounded); XCTAssertFalse(d.state.canSendInput)
        case "geometry_stale":
            var d = controlled(); d.host(c1, .geometry(t1, 6)); d.effects = []
            d.send(.intent(.absolutePointer(0.5, 0.5, 5, true)))
            XCTAssertFalse(d.effects.contains { if case .absolutePointer = $0 { return true }; return false })
            XCTAssertTrue(d.effects.contains(.requestGeometry))
        case "outside_letterboxed_video":
            var d = controlled(); d.effects = []
            d.send(.intent(.absolutePointer(0, 0.5, 1, false)))
            XCTAssertFalse(d.effects.contains { if case .absolutePointer = $0 { return true }; return false })
        case "unknown_host_action":
            var d = ready([], [ClientAction("vendor.new_action")]); d.effects = []
            d.send(.intent(.invoke("vendor.new_action", "r1")))
            XCTAssertTrue(d.state.availableActions.isEmpty); XCTAssertTrue(d.state.pending.isEmpty)
            XCTAssertFalse(d.effects.contains { if case .action = $0 { return true }; return false })
        case "user_cancelled_consent":
            var d = ready(); d.send(.intent(.openPicker("WINDOW"))); d.effects = []
            d.host(c1, .consentCancelled)
            XCTAssertEqual(d.state.media, .none); XCTAssertEqual(d.state.consent, .cancelled)
            XCTAssertFalse(d.effects.contains(.openPicker("MONITOR")))
        case "remote_touch_unsupported":
            var d = ready(["input.pointer"]); d.send(.intent(.inputMode("direct_touch")))
            XCTAssertEqual(d.state.inputMode, "pointer"); XCTAssertNotNil(d.state.warning)
        case "screen_resize_new_epoch":
            var d = controlled(); d.host(c1, .geometry(t1, 3)); d.effects = []
            d.send(.intent(.absolutePointer(0.5, 0.5, 1, true)))
            XCTAssertFalse(d.effects.contains { if case .absolutePointer = $0 { return true }; return false })
            XCTAssertTrue(d.effects.contains(.requestGeometry))
        default: XCTFail("Unmapped fixture: \(id)", file: file, line: line)
        }
    }

    func testOutOfOrderSnapshotsAndLeaseExpiry() {
        var d = controlled()
        d.host(c1, .capabilitySnapshot(ClientRevision(0), [], []))
        XCTAssertTrue(d.state.canSendInput)
        d.send(.tick(100)); XCTAssertFalse(d.state.canSendInput)
        d.send(.tick(1)); XCTAssertEqual(d.state.now, 100)
    }

    func testNewerSnapshotsReplaceStateAndOlderSnapshotsCannotRestoreIt() {
        var d = ready(["capture.picker"])
        let newerTarget = ClientTarget("new", generation: 2)
        d.host(c1, .catalogSnapshot(ClientRevision(2), [newerTarget]))
        d.host(c1, .capabilitySnapshot(ClientRevision(2), ["session.launch"], []))
        XCTAssertEqual(d.state.catalog, [newerTarget])
        XCTAssertEqual(d.state.features, ["session.launch"])
        d.host(c1, .catalogSnapshot(ClientRevision(1), [t1]))
        d.host(c1, .capabilitySnapshot(ClientRevision(1), ["input.pointer"], []))
        XCTAssertEqual(d.state.catalog, [newerTarget])
        XCTAssertEqual(d.state.features, ["session.launch"])
    }

    func testHostActionRequiresMatchingConnectionTargetAndRevision() {
        var d = ready([], [ClientAction("window.close")])
        d.send(.intent(.invoke("window.close", "r1")))
        d.host(c1, .actionResult("r1", t1, ClientRevision(99), true))
        XCTAssertNil(d.state.lastConfirmedAction)
        d.host(c1, .actionResult("r1", t1, ClientRevision(1), true))
        XCTAssertEqual(d.state.lastConfirmedAction, "window.close")
    }
}

private typealias UInt64Convertible = UInt64
