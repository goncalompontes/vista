import Foundation
import XCTest
@testable import VistaCore

final class MediaReceiveSessionTests: XCTestCase {
    private let offer = MediaReceiveOffer(
        pairingID: "pair",
        sessionID: "session",
        generation: 7,
        authenticated: true
    )

    /// SPS + PPS + IDR slice in one access unit: the only unit that may start a
    /// decoder generation.
    private let idr = H264AccessUnit(
        ptsNanoseconds: 1,
        isKeyframe: true,
        discontinuity: false,
        data: Data([0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 4])
    )
    private let delta = H264AccessUnit(
        ptsNanoseconds: 2,
        isKeyframe: false,
        discontinuity: false,
        data: Data([0, 0, 0, 1, 0x41, 5])
    )

    private func make(
        decoder: FakeH264MediaDecoder = FakeH264MediaDecoder(),
        capacity: Int = 2
    ) throws -> (MediaReceiveSession, FakeH264MediaDecoder) {
        let session = try MediaReceiveSession(
            offer: offer,
            expectedPairingID: "pair",
            expectedSessionID: "session",
            generation: 7,
            capacity: capacity,
            decoder: decoder
        )
        return (session, decoder)
    }

    func testQueueStaysBoundedAndRequestsKeyframeAfterOverflow() throws {
        let (session, decoder) = try make(capacity: 2)
        XCTAssertEqual(try session.enqueue(idr, generation: 7), .queued)
        XCTAssertEqual(try session.enqueue(delta, generation: 7), .queued)
        XCTAssertEqual(try session.enqueue(delta, generation: 7), .requestKeyframe)
        XCTAssertLessThanOrEqual(session.queuedCount, 2)
        XCTAssertTrue(session.waitingForKeyframe)
        XCTAssertEqual(session.keyframeRequests, 1)
        XCTAssertEqual(decoder.resetCount, 1)
        XCTAssertEqual(try session.enqueue(idr, generation: 7), .queued)
        XCTAssertLessThanOrEqual(session.queuedCount, 2)
    }

    func testMalformedAndOversizedAccessUnitsCloseTheSession() throws {
        let (malformed, malformedDecoder) = try make()
        let unitWithoutStartCode = H264AccessUnit(
            ptsNanoseconds: 0,
            isKeyframe: false,
            discontinuity: false,
            data: Data([0x41, 5])
        )
        XCTAssertThrowsError(try malformed.enqueue(unitWithoutStartCode, generation: 7)) { error in
            XCTAssertEqual(error as? MediaReceiveError, .malformedAccessUnit)
        }
        XCTAssertTrue(malformed.isClosed)
        XCTAssertEqual(malformedDecoder.closeCount, 1)

        let (empty, _) = try make()
        let emptyUnit = H264AccessUnit(
            ptsNanoseconds: 0,
            isKeyframe: false,
            discontinuity: false,
            data: Data()
        )
        XCTAssertThrowsError(try empty.enqueue(emptyUnit, generation: 7))

        let (oversized, _) = try make()
        let huge = H264AccessUnit(
            ptsNanoseconds: 0,
            isKeyframe: false,
            discontinuity: false,
            data: Data(repeating: 0, count: SecureMediaFraming.maxAccessUnitBytes + 1)
        )
        XCTAssertThrowsError(try oversized.enqueue(huge, generation: 7)) { error in
            XCTAssertEqual(error as? MediaReceiveError, .payloadTooLarge)
        }
        XCTAssertTrue(oversized.isClosed)
    }

    func testIdentityAndGenerationMismatchesAreRejected() throws {
        XCTAssertThrowsError(
            try MediaReceiveSession(
                offer: offer,
                expectedPairingID: "other",
                expectedSessionID: "session",
                generation: 7,
                decoder: FakeH264MediaDecoder()
            )
        )
        XCTAssertThrowsError(
            try MediaReceiveSession(
                offer: offer,
                expectedPairingID: "pair",
                expectedSessionID: "other",
                generation: 7,
                decoder: FakeH264MediaDecoder()
            )
        )
        XCTAssertThrowsError(
            try MediaReceiveSession(
                offer: offer,
                expectedPairingID: "pair",
                expectedSessionID: "session",
                generation: 8,
                decoder: FakeH264MediaDecoder()
            )
        )
        XCTAssertThrowsError(
            try MediaReceiveSession(
                offer: MediaReceiveOffer(
                    pairingID: "pair",
                    sessionID: "session",
                    generation: 7,
                    authenticated: false
                ),
                expectedPairingID: "pair",
                expectedSessionID: "session",
                generation: 7,
                decoder: FakeH264MediaDecoder()
            )
        ) { error in
            XCTAssertEqual(error as? MediaReceiveError, .unauthenticatedOffer)
        }

        let (session, _) = try make()
        XCTAssertThrowsError(try session.enqueue(idr, generation: 8)) { error in
            XCTAssertEqual(error as? MediaReceiveError, .staleGeneration)
        }
        XCTAssertFalse(session.isClosed)
    }

    func testDecoderFailureDropsQueuedStateAndRecoversOnNewIDR() throws {
        let (session, decoder) = try make()
        try session.enqueue(idr, generation: 7)
        try session.enqueue(delta, generation: 7)
        decoder.shouldFail = true
        XCTAssertThrowsError(try session.decodeNext()) { error in
            XCTAssertEqual(error as? MediaReceiveError, .decoderFailure)
        }
        XCTAssertTrue(session.waitingForKeyframe)
        XCTAssertEqual(session.queuedCount, 0)
        XCTAssertEqual(session.keyframeRequests, 1)

        decoder.shouldFail = false
        XCTAssertEqual(try session.enqueue(delta, generation: 7), .requestKeyframe)
        XCTAssertEqual(try session.enqueue(idr, generation: 7), .queued)
        XCTAssertTrue(try session.decodeNext())
        XCTAssertEqual(decoder.decoded.count, 1)
        XCTAssertEqual(session.decodedAccessUnits, 1)
    }

    func testCloseIsDeterministicAndIdempotent() throws {
        let (session, decoder) = try make()
        try session.enqueue(idr, generation: 7)
        session.stop()
        session.disconnect()
        session.fail()
        XCTAssertTrue(session.isClosed)
        XCTAssertEqual(session.queuedCount, 0)
        XCTAssertEqual(decoder.closeCount, 1)
        XCTAssertThrowsError(try session.decodeNext())
        XCTAssertThrowsError(try session.enqueue(idr, generation: 7))
    }

    func testDiscontinuityDropsQueuedFramesAndRequestsAKeyframe() throws {
        let (session, decoder) = try make()
        try session.enqueue(idr, generation: 7)
        let discontinuousDelta = H264AccessUnit(
            ptsNanoseconds: 3,
            isKeyframe: false,
            discontinuity: true,
            data: delta.data
        )
        XCTAssertEqual(try session.enqueue(discontinuousDelta, generation: 7), .requestKeyframe)
        XCTAssertTrue(session.waitingForKeyframe)
        XCTAssertEqual(session.queuedCount, 0)
        XCTAssertEqual(session.keyframeRequests, 1)
        XCTAssertEqual(decoder.resetCount, 1)
        XCTAssertEqual(try session.enqueue(idr, generation: 7), .queued)
    }

    func testDeltaBeforeFirstKeyframeIsNeverDelivered() throws {
        let (session, decoder) = try make()
        XCTAssertEqual(try session.enqueue(delta, generation: 7), .requestKeyframe)
        XCTAssertEqual(session.queuedCount, 0)
        XCTAssertFalse(try session.decodeNext())
        XCTAssertTrue(decoder.decoded.isEmpty)
    }

    func testReconnectDiscontinuityResetsAnOpenSession() throws {
        let (session, decoder) = try make()
        try session.enqueue(idr, generation: 7)
        try session.markDiscontinuity()
        XCTAssertTrue(session.waitingForKeyframe)
        XCTAssertEqual(session.queuedCount, 0)
        XCTAssertEqual(session.keyframeRequests, 1)
        XCTAssertEqual(decoder.resetCount, 1)
        XCTAssertFalse(session.isClosed)
    }
}
