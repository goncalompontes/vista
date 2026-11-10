import Foundation
import XCTest
@testable import VistaCandidate

final class WireTests: XCTestCase {
    func fixtures() throws -> [(String, Data)] {
        let path = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
            .appendingPathComponent("../../../fixtures/golden.tsv").standardizedFileURL
        return try String(contentsOf: path, encoding: .utf8)
            .split(separator: "\n").map { line in
                let parts = line.split(separator: "\t")
                var data = Data()
                let hex = Array(parts[2])
                for i in stride(from: 0, to: hex.count, by: 2) {
                    data.append(UInt8(String(hex[i...i+1]), radix: 16)!)
                }
                return (String(parts[0]), data)
            }
    }
    func testGeneratedWireFixtures() throws {
        for (id, bytes) in try fixtures() {
            switch id {
            case "version", "unknown-field":
                XCTAssertEqual(try Vista_V1_ProtocolVersion(serializedBytes: bytes).major, 1)
            case "optional-zero":
                let v = try Vista_V1_VistaError(serializedBytes: bytes)
                XCTAssertTrue(v.hasRetryAfterMs)
                XCTAssertEqual(v.retryAfterMs, 0)
            case "grant-approved", "unknown-enum", "enum-zero":
                let v = try Vista_V1_PermissionGrant(serializedBytes: bytes)
                if id == "unknown-enum" { XCTAssertEqual(v.state.rawValue, 123) }
                if id == "enum-zero" { XCTAssertEqual(v.state.rawValue, 0) }
                if id == "grant-approved" { XCTAssertEqual(v.epoch, 9_007_199_254_740_993) }
            case "grant-revoked": _ = try Vista_V1_PermissionUpdate(serializedBytes: bytes)
            case "catalog", "action": _ = try Vista_V1_ControlEnvelope(serializedBytes: bytes)
            case "picker", "verified-binding": _ = try Vista_V1_CaptureSelection(serializedBytes: bytes)
            case "enroll": _ = try Vista_V1_BootstrapEnvelope(serializedBytes: bytes)
            case "certificate": _ = try Vista_V1_PairingResult(serializedBytes: bytes)
            case "lease": _ = try Vista_V1_LeaseResult(serializedBytes: bytes)
            case "input": _ = try Vista_V1_InputEnvelope(serializedBytes: bytes)
            case "required-feature", "additive-v1.1": _ = try Vista_V1_ControlHello(serializedBytes: bytes)
            case "missing-domain": _ = try Vista_V1_LeaseRequest(serializedBytes: bytes)
            default: XCTFail("Missing fixture: \(id)")
            }
        }
        XCTAssertThrowsError(try Vista_V1_ProtocolVersion(serializedBytes: Data([0x08])))
    }
}
