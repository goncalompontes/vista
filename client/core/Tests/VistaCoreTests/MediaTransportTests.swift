import Foundation
import XCTest
@testable import VistaCore

final class MediaTransportTests: XCTestCase {
    private let key = [UInt8](repeating: 0x11, count: 32)
    private let sessionID = "media-session"

    private func frame(
        pts: UInt64,
        flags: UInt8,
        bytes: [UInt8],
        cipher: inout SecureRecordCipher
    ) throws -> Data {
        var payload = Data()
        var bigEndianPTS = pts.bigEndian
        withUnsafeBytes(of: &bigEndianPTS) { payload.append(contentsOf: $0) }
        payload.append(flags)
        payload.append(contentsOf: bytes)
        let sealed = try cipher.seal(payload)
        var length = UInt32(sealed.count).bigEndian
        var framed = Data()
        withUnsafeBytes(of: &length) { framed.append(contentsOf: $0) }
        framed.append(sealed)
        return framed
    }

    func testEncryptedMediaRecordDrainsWithMetadata() throws {
        var writer = try SecureRecordCipher(
            key: key,
            sessionID: sessionID,
            direction: .hostToClient,
            maxPlaintextBytes: SecureMediaFraming.maxPlaintextBytes
        )
        var reader = try SecureRecordCipher(
            key: key,
            sessionID: sessionID,
            direction: .hostToClient,
            maxPlaintextBytes: SecureMediaFraming.maxPlaintextBytes
        )
        var buffer = try frame(
            pts: 42,
            flags: 0b11,
            bytes: [0, 0, 0, 1, 0x67, 0, 0, 0, 1, 0x68, 0, 0, 0, 1, 0x65],
            cipher: &writer
        )
        let records = try SecureMediaFraming.drain(&buffer, cipher: &reader)
        XCTAssertEqual(records.count, 1)
        XCTAssertEqual(records[0].ptsNanoseconds, 42)
        XCTAssertTrue(records[0].isKeyframe)
        XCTAssertTrue(records[0].discontinuity)
        XCTAssertTrue(buffer.isEmpty)
    }

    func testMediaDrainLeavesPartialFrameBuffered() throws {
        var writer = try SecureRecordCipher(
            key: key,
            sessionID: sessionID,
            direction: .hostToClient,
            maxPlaintextBytes: SecureMediaFraming.maxPlaintextBytes
        )
        var reader = try SecureRecordCipher(
            key: key,
            sessionID: sessionID,
            direction: .hostToClient,
            maxPlaintextBytes: SecureMediaFraming.maxPlaintextBytes
        )
        let complete = try frame(pts: 1, flags: 1, bytes: [0, 0, 0, 1, 0x65], cipher: &writer)
        var buffer = Data(complete.prefix(complete.count / 2))
        XCTAssertTrue(try SecureMediaFraming.drain(&buffer, cipher: &reader).isEmpty)
        buffer.append(contentsOf: complete.suffix(from: complete.count / 2))
        XCTAssertEqual(try SecureMediaFraming.drain(&buffer, cipher: &reader).count, 1)
    }
}
