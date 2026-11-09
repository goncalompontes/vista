import Foundation
import XCTest
@testable import VistaCore

final class SecureControlChannelTests: XCTestCase {
    private let sessionID = "s1"
    private let keys = SessionKeys(
        clientToHost: [UInt8](repeating: 0x11, count: 32),
        hostToClient: [UInt8](repeating: 0x22, count: 32)
    )

    private func channels() throws -> (SecureControlChannel, SecureControlChannel) {
        let client = try SecureControlChannel(keys: keys, sessionID: sessionID)
        let host = try SecureControlChannel(keys: keys, sessionID: sessionID, role: .host)
        return (client, host)
    }

    func testEnvelopeRoundTripsInBothDirections() throws {
        let (client, host) = try channels()
        let request = Envelope(seq: 0, body: .ping(nonce: 9))
        XCTAssertEqual(try host.open(client.seal(request)), request)

        let reply = Envelope(seq: 0, body: .pong(nonce: 9))
        XCTAssertEqual(try client.open(host.seal(reply)), reply)
    }

    func testTamperedRecordFailsAuthentication() throws {
        let (client, host) = try channels()
        var frame = try client.seal(Envelope(seq: 0, body: .ping(nonce: 1)))
        frame[frame.count - 1] ^= 0x01
        XCTAssertThrowsError(try host.open(frame)) { error in
            XCTAssertEqual(error as? WireError, .authenticationFailed)
        }
    }

    func testWrongDirectionOrKeyFails() throws {
        let (client, _) = try channels()
        let unrelated = try SecureControlChannel(
            keys: SessionKeys(
                clientToHost: [UInt8](repeating: 0x33, count: 32),
                hostToClient: [UInt8](repeating: 0x44, count: 32)
            ),
            sessionID: sessionID
        )
        let frame = try client.seal(Envelope(seq: 0, body: .keyframeRequest(sessionID: sessionID)))
        XCTAssertThrowsError(try unrelated.open(frame))
    }

    func testCounterAdvancesPerRecord() throws {
        let key = [UInt8](repeating: 0x55, count: 32)
        var cipher = try SecureRecordCipher(key: key, sessionID: sessionID, direction: .clientToHost)
        XCTAssertEqual(cipher.recordCount, 0)
        _ = try cipher.seal(Data([1]))
        _ = try cipher.seal(Data([2]))
        XCTAssertEqual(cipher.recordCount, 2)
        let secondKey = [UInt8](repeating: 0x55, count: 32)
        var reader = try SecureRecordCipher(key: secondKey, sessionID: sessionID, direction: .clientToHost)
        var writer = try SecureRecordCipher(key: secondKey, sessionID: sessionID, direction: .clientToHost)
        var buffer = try SecureControlFraming.encodeFrame(Data([1]), cipher: &writer)
        buffer.append(try SecureControlFraming.encodeFrame(Data([2]), cipher: &writer))
        let drained = try SecureControlFraming.drain(&buffer, cipher: &reader)
        XCTAssertEqual(drained, [Data([1]), Data([2])])
        XCTAssertEqual(reader.recordCount, 2)
    }

    func testEncryptedDrainWaitsForCompleteFrames() throws {
        let (client, host) = try channels()
        let first = Envelope(seq: 0, body: .ping(nonce: 1))
        let second = Envelope(seq: 1, body: .ping(nonce: 2))
        let firstFrame = try client.seal(first)
        let secondFrame = try client.seal(second)
        let split = secondFrame.count / 2

        var buffer = firstFrame
        buffer.append(secondFrame.prefix(split))
        XCTAssertEqual(try host.drain(&buffer), [first])
        XCTAssertFalse(buffer.isEmpty)

        buffer.append(secondFrame.suffix(from: split))
        XCTAssertEqual(try host.drain(&buffer), [second])
        XCTAssertTrue(buffer.isEmpty)
    }

    func testOversizedPlaintextIsRejectedBeforeSealing() throws {
        var cipher = try SecureRecordCipher(key: [UInt8](repeating: 1, count: 32), sessionID: sessionID, direction: .clientToHost)
        XCTAssertThrowsError(try cipher.seal(Data(repeating: 0, count: SecureRecordCipher.maxFrameBytes + 1))) { error in
            XCTAssertEqual(error as? WireError, .frameTooLarge)
        }
        XCTAssertEqual(cipher.recordCount, 0)
    }
}
