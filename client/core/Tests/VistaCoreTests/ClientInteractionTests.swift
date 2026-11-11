import XCTest
@testable import VistaCore

final class ClientInteractionTests: XCTestCase {
    func testGestureOwnershipHonorsPrecedenceAndCancellation() {
        var gesture = GestureOwnership()
        XCTAssertTrue(gesture.claim(.remoteInput))
        XCTAssertTrue(gesture.mayEmitRemoteInput)
        XCTAssertTrue(gesture.claim(.hostBinding))
        XCTAssertFalse(gesture.mayEmitRemoteInput)
        XCTAssertTrue(gesture.claim(.accessibility))
        XCTAssertFalse(gesture.mayEmitRemoteInput)
        gesture.cancel()
        XCTAssertFalse(gesture.claim(.operatingSystem))
        XCTAssertFalse(gesture.mayEmitRemoteInput)
        gesture.reset()
        XCTAssertTrue(gesture.claim(.remoteInput))
    }

    func testGeometryMapsContentAndRejectsLetterboxAndOutsidePoints() throws {
        let rect = try XCTUnwrap(VideoContentRect(x: 100, y: 50, width: 800, height: 600))
        let geometry = try XCTUnwrap(VideoInputGeometry(contentRect: rect, epoch: 7))
        let center = try XCTUnwrap(geometry.sourcePoint(viewX: 500, viewY: 350, currentEpoch: 7))
        XCTAssertEqual(center.x, 0.5, accuracy: 0.0001)
        XCTAssertEqual(center.y, 0.5, accuracy: 0.0001)
        XCTAssertNil(geometry.sourcePoint(viewX: 99, viewY: 350, currentEpoch: 7))
        XCTAssertNil(geometry.sourcePoint(viewX: 901, viewY: 350, currentEpoch: 7))
        XCTAssertNil(geometry.sourcePoint(viewX: 500, viewY: 350, currentEpoch: 8))
    }

    func testGeometryInvertsZoomAndPanWithoutClamping() throws {
        let rect = try XCTUnwrap(VideoContentRect(x: 0, y: 0, width: 1000, height: 500))
        let geometry = try XCTUnwrap(VideoInputGeometry(contentRect: rect, zoom: 2, panX: 0.1, panY: -0.1, epoch: 3))
        let source = try XCTUnwrap(geometry.sourcePoint(viewX: 700, viewY: 200, currentEpoch: 3))
        XCTAssertEqual(source.x, 0.55, accuracy: 0.0001)
        XCTAssertEqual(source.y, 0.5, accuracy: 0.0001)
        let panned = try XCTUnwrap(VideoInputGeometry(contentRect: rect, zoom: 2, panX: 0.6, epoch: 3))
        XCTAssertNil(panned.sourcePoint(viewX: 0, viewY: 250, currentEpoch: 3))
    }

    func testGeometryRejectsInvalidDimensionsAndNonFiniteInput() {
        XCTAssertNil(VideoContentRect(x: 0, y: 0, width: 0, height: 100))
        XCTAssertNil(VideoContentRect(x: .nan, y: 0, width: 100, height: 100))
        let rect = VideoContentRect(x: 0, y: 0, width: 100, height: 100)!
        XCTAssertNil(VideoInputGeometry(contentRect: rect, zoom: .infinity, epoch: 1))
        let geometry = VideoInputGeometry(contentRect: rect, epoch: 1)!
        XCTAssertNil(geometry.sourcePoint(viewX: .infinity, viewY: 2, currentEpoch: 1))
    }

    func testClipboardRequiresDirectionGrantForegroundAndUserIntent() {
        let denied = ClipboardAccessPolicy()
        XCTAssertFalse(denied.mayReadDeviceClipboard(userInitiated: true, appIsForeground: true))
        XCTAssertFalse(denied.mayOfferHostClipboard(userInitiated: true, appIsForeground: true))

        let policy = ClipboardAccessPolicy(deviceToHostGranted: true, hostToDeviceGranted: true, inputGranted: false)
        XCTAssertTrue(policy.maySendToHostClipboard(userInitiated: true, appIsForeground: true))
        XCTAssertFalse(policy.mayReadDeviceClipboard(userInitiated: false, appIsForeground: true))
        XCTAssertFalse(policy.mayReadDeviceClipboard(userInitiated: true, appIsForeground: false))
        XCTAssertTrue(policy.mayOfferHostClipboard(userInitiated: true, appIsForeground: true))
        XCTAssertFalse(policy.mayWriteDeviceClipboard(userInitiated: true, appIsForeground: true, explicitlyConfirmed: false))
        XCTAssertTrue(policy.mayWriteDeviceClipboard(userInitiated: true, appIsForeground: true, explicitlyConfirmed: true))
        XCTAssertFalse(policy.mayPasteNow(userInitiated: true, appIsForeground: true, explicitlyConfirmed: true))

        let pasteAllowed = ClipboardAccessPolicy(deviceToHostGranted: true, inputGranted: true)
        XCTAssertTrue(pasteAllowed.mayPasteNow(userInitiated: true, appIsForeground: true, explicitlyConfirmed: true))
    }
}
