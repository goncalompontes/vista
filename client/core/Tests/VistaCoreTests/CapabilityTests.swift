import Foundation
import XCTest
@testable import VistaCore

final class CapabilityTests: XCTestCase {
    private let fixtureJSON = #"""
    {
      "capability_snapshot": {
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
            "presentation": {"label": "Focus window", "icon": "window.focus"},
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
            "id": "example.vendor.special",
            "schema_version": 1,
            "supported": true,
            "enabled": true,
            "authorization": "granted",
            "parameters": "vista.extension.v1",
            "presentation": {"label": "Vendor action"},
            "suggested_bindings": ["three_finger_swipe_left"]
          }
        ]
      },
      "action_request": {
        "v": 1,
        "type": "action.invoke",
        "id": "r17",
        "connection_epoch": "c1",
        "session_id": "s1",
        "grant_epoch": "g2",
        "target_id": "w5",
        "target_generation": "niri-boot1-w5",
        "capability_revision": 12,
        "action": "window.maximize.set",
        "schema_version": 1,
        "parameters": {"value": true}
      },
      "action_result": {
        "v": 1,
        "type": "action.result",
        "id": "r17",
        "connection_epoch": "c1",
        "session_id": "s1",
        "status": "completed",
        "state": {"maximized": true},
        "capability_revision": 13
      },
      "menu_snapshot": {
        "v": 1,
        "type": "menu.snapshot",
        "connection_epoch": "c1",
        "session_id": "s1",
        "target_id": "w5",
        "target_generation": "niri-boot1-w5",
        "menu_revision": 4,
        "items": [
          {
            "id": "file",
            "label": "File",
            "enabled": true,
            "children": [
              {"id": "file.save", "label": "Save", "enabled": true},
              {"id": "file.close", "label": "Close", "enabled": false}
            ]
          }
        ]
      },
      "input_events": [
        {"type": "pointer.absolute", "x": 0.5, "y": 0.5, "geometry_epoch": 3},
        {"type": "pointer.button", "button": "primary", "pressed": true},
        {"type": "key.physical", "usage": 4, "pressed": true, "repeat": false},
        {"type": "text.commit", "text": "hello"},
        {"type": "input.cancel"}
      ]
    }
    """#

    private func fixture() throws -> [String: JSONValue] {
        let value = try JSONDecoder().decode(JSONValue.self, from: Data(fixtureJSON.utf8))
        guard case .object(let object) = value else {
            throw NSError(domain: "CapabilityTests", code: 1)
        }
        return object
    }

    private func decode<T: Decodable>(_ type: T.Type, _ value: JSONValue) throws -> T {
        try JSONDecoder().decode(type, from: JSONEncoder().encode(value))
    }

    func testSnapshotDecodesAndOnlySelectableActionsAreOffered() throws {
        let object = try fixture()
        let snapshot = try decode(CapabilitySnapshot.self, try XCTUnwrap(object["capability_snapshot"]))
        XCTAssertEqual(snapshot.v, 1)
        XCTAssertEqual(snapshot.messageType, "capabilities.snapshot")
        XCTAssertEqual(snapshot.targetGeneration, "niri-boot1-w5")
        XCTAssertEqual(snapshot.actions.count, 5)
        XCTAssertEqual(snapshot.action(id: "window.focus")?.isSelectable, true)
        XCTAssertEqual(snapshot.action(id: "desktop.overview.show")?.supported, false)
        XCTAssertEqual(snapshot.action(id: "window.minimize")?.isSelectable, false)
        XCTAssertEqual(snapshot.action(id: "desktop.overview.show")?.authorization, .required)
        XCTAssertEqual(snapshot.action(id: "example.vendor.special")?.suggestedBindings,
                       ["three_finger_swipe_left"])
    }

    func testActionRequestAndResultRoundTrip() throws {
        let object = try fixture()
        let request = try decode(ActionRequestPayload.self, try XCTUnwrap(object["action_request"]))
        XCTAssertEqual(request.action, "window.maximize.set")
        XCTAssertEqual(request.parameters["value"], .bool(true))
        let requestData = try JSONEncoder().encode(request)
        XCTAssertEqual(try JSONDecoder().decode(ActionRequestPayload.self, from: requestData), request)

        let result = try decode(ActionResultPayload.self, try XCTUnwrap(object["action_result"]))
        XCTAssertEqual(result.status, .completed)
        XCTAssertNil(result.errorCode)
        XCTAssertEqual(result.state, .object(["maximized": .bool(true)]))
        XCTAssertEqual(result.capabilityRevision, 13)
    }

    func testMenuSnapshotDecodesWithChildren() throws {
        let object = try fixture()
        let menu = try decode(MenuSnapshot.self, try XCTUnwrap(object["menu_snapshot"]))
        XCTAssertEqual(menu.menuRevision, 4)
        XCTAssertEqual(menu.items.first?.children.count, 2)
        XCTAssertEqual(menu.items.first?.children.last?.enabled, false)
    }

    func testPortableInputEventsDecodeAndRoundTrip() throws {
        let object = try fixture()
        let events = try decode([InputEvent].self, try XCTUnwrap(object["input_events"]))
        XCTAssertEqual(events.count, 5)
        XCTAssertEqual(events[0], .pointerAbsolute(x: 0.5, y: 0.5, geometryEpoch: 3))
        XCTAssertEqual(events[1], .pointerButton(button: .primary, pressed: true))
        XCTAssertEqual(events[2], .keyPhysical(usage: 4, pressed: true, isRepeat: false))
        XCTAssertEqual(events[3], .textCommit(text: "hello"))
        XCTAssertEqual(events[4], .cancel)
        let encoded = try JSONEncoder().encode(events)
        XCTAssertEqual(try JSONDecoder().decode([InputEvent].self, from: encoded), events)
    }

    func testUnknownActionIsNotInventedLocally() throws {
        let object = try fixture()
        let snapshot = try decode(CapabilitySnapshot.self, try XCTUnwrap(object["capability_snapshot"]))
        XCTAssertNil(snapshot.action(id: "desktop.launcher.open"))
    }
}
