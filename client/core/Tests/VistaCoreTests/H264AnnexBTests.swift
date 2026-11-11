import Foundation
import XCTest
@testable import VistaCore

final class H264AnnexBTests: XCTestCase {
    private struct Fixture: Decodable {
        let width: Int
        let height: Int
        let frames: [Frame]

        struct Frame: Decodable {
            let index: Int
            let ptsNS: UInt64
            let keyframe: Bool
            let nalTypes: [Int]
            let dataBase64: String

            enum CodingKeys: String, CodingKey {
                case index
                case ptsNS = "pts_ns"
                case keyframe
                case nalTypes = "nal_types"
                case dataBase64 = "data_base64"
            }
        }
    }

    private func fixture() throws -> (Fixture, [H264AccessUnit]) {
        let url = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .appendingPathComponent("docs/fixtures/h264-synthetic.json")
        let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: url))
        let units = try fixture.frames.map { frame in
            H264AccessUnit(
                ptsNanoseconds: frame.ptsNS,
                isKeyframe: frame.keyframe,
                discontinuity: false,
                data: Data(try XCTUnwrap(Data(base64Encoded: frame.dataBase64)))
            )
        }
        return (fixture, units)
    }

    func testSyntheticFixtureParsesIntoSpsPpsAndIdr() throws {
        let (fixture, units) = try fixture()
        XCTAssertEqual(fixture.width, 160)
        XCTAssertEqual(fixture.height, 90)
        XCTAssertEqual(units.count, 4)
        let first = try H264AnnexB.describe(units[0])
        XCTAssertTrue(first.isIDR)
        XCTAssertEqual(first.sps.count, 1)
        XCTAssertEqual(first.pps.count, 1)
        XCTAssertTrue(first.nalUnits.contains { ($0[0] & 0x1F) == 7 })
        let second = try H264AnnexB.describe(units[1])
        XCTAssertFalse(second.isIDR)
        XCTAssertFalse(second.hasParameterSets)
    }

    func testGateDeliversIdrThenDeltas() throws {
        let (_, units) = try fixture()
        var gate = H264StreamGate()
        XCTAssertTrue(gate.waitingForKeyframe)
        guard case .deliver(let first) = try gate.accept(units[0]) else {
            return XCTFail("IDR should be delivered")
        }
        XCTAssertTrue(first.isIDR)
        XCTAssertFalse(gate.waitingForKeyframe)
        for unit in units.dropFirst() {
            guard case .deliver = try gate.accept(unit) else {
                return XCTFail("delta after IDR should be delivered")
            }
        }
        XCTAssertEqual(gate.delivered, 4)
        XCTAssertEqual(gate.suppressed, 0)
        XCTAssertEqual(gate.sps.count, 1)
        XCTAssertEqual(gate.pps.count, 1)
    }

    func testDeltaBeforeIdrIsSuppressed() throws {
        let (_, units) = try fixture()
        var gate = H264StreamGate()
        XCTAssertEqual(try gate.accept(units[1]), .suppress)
        XCTAssertTrue(gate.waitingForKeyframe)
        XCTAssertEqual(gate.suppressed, 1)
        guard case .deliver = try gate.accept(units[0]) else {
            return XCTFail("IDR should recover")
        }
        XCTAssertFalse(gate.waitingForKeyframe)
    }

    func testDiscontinuitySuppressesDeltasUntilFreshIdr() throws {
        let (_, units) = try fixture()
        var gate = H264StreamGate()
        _ = try gate.accept(units[0])
        let deltaAfterLoss = H264AccessUnit(
            ptsNanoseconds: 100,
            isKeyframe: false,
            discontinuity: true,
            data: units[1].data
        )
        XCTAssertEqual(try gate.accept(deltaAfterLoss), .suppress)
        XCTAssertTrue(gate.waitingForKeyframe)
        guard case .deliver = try gate.accept(units[0]) else {
            return XCTFail("fresh IDR should recover after a discontinuity")
        }
    }

    func testDiscontinuityRequiresFreshParameterSetsAndIdr() throws {
        let (_, units) = try fixture()
        var gate = H264StreamGate()
        _ = try gate.accept(units[0])

        let idrNALs = try H264AnnexB.split(units[0].data)
            .filter { ($0[0] & 0x1F) == 5 }
        var idrOnlyData = Data()
        for nal in idrNALs {
            idrOnlyData.append(contentsOf: [0, 0, 0, 1])
            idrOnlyData.append(nal)
        }
        let idrWithoutParameterSets = H264AccessUnit(
            ptsNanoseconds: 100,
            isKeyframe: true,
            discontinuity: true,
            data: idrOnlyData
        )
        XCTAssertEqual(try gate.accept(idrWithoutParameterSets), .suppress)
        XCTAssertTrue(gate.waitingForKeyframe)
        XCTAssertTrue(gate.sps.isEmpty)
        XCTAssertTrue(gate.pps.isEmpty)

        let fresh = H264AccessUnit(
            ptsNanoseconds: units[0].ptsNanoseconds,
            isKeyframe: true,
            discontinuity: false,
            data: units[0].data
        )
        guard case .deliver = try gate.accept(fresh) else {
            return XCTFail("fresh SPS/PPS-prefixed IDR should recover")
        }
    }

    func testExplicitResetRequiresFreshParameterSetsAndIdr() throws {
        let (_, units) = try fixture()
        var gate = H264StreamGate()
        _ = try gate.accept(units[0])
        XCTAssertFalse(gate.sps.isEmpty)
        XCTAssertFalse(gate.pps.isEmpty)

        gate.resetForDiscontinuity()
        XCTAssertTrue(gate.waitingForKeyframe)
        XCTAssertTrue(gate.sps.isEmpty)
        XCTAssertTrue(gate.pps.isEmpty)

        let idrNALs = try H264AnnexB.split(units[0].data)
            .filter { ($0[0] & 0x1F) == 5 }
        var idrOnlyData = Data()
        for nal in idrNALs {
            idrOnlyData.append(contentsOf: [0, 0, 0, 1])
            idrOnlyData.append(nal)
        }
        let staleConfigurationIDR = H264AccessUnit(
            ptsNanoseconds: 100,
            isKeyframe: true,
            discontinuity: false,
            data: idrOnlyData
        )
        XCTAssertEqual(try gate.accept(staleConfigurationIDR), .suppress)
        XCTAssertTrue(gate.sps.isEmpty)
        XCTAssertTrue(gate.pps.isEmpty)

        guard case .deliver = try gate.accept(units[0]) else {
            return XCTFail("fresh SPS/PPS-prefixed IDR should recover after reset")
        }
    }

    func testParserRejectsMalformedUnits() {
        XCTAssertThrowsError(try H264AnnexB.split(Data()))
        XCTAssertThrowsError(try H264AnnexB.split(Data([1, 2, 3, 4])))
        XCTAssertThrowsError(try H264AnnexB.split(Data([0, 0, 1])))
        XCTAssertThrowsError(
            try H264AnnexB.split(Data(repeating: 0, count: H264AnnexB.maxAccessUnitBytes + 1))
        )
    }

    func testGateRejectsIdrWithoutParameterSets() throws {
        let (_, units) = try fixture()
        let idrOnly = try XCTUnwrap(
            try H264AnnexB.split(units[0].data).last { ($0[0] & 0x1F) == 5 }
        )
        var data = Data()
        data.append(contentsOf: [0, 0, 0, 1])
        data.append(idrOnly)
        let unit = H264AccessUnit(ptsNanoseconds: 0, isKeyframe: true, discontinuity: false, data: data)
        var gate = H264StreamGate()
        XCTAssertEqual(try gate.accept(unit), .suppress)
        XCTAssertTrue(gate.waitingForKeyframe)
    }
}
