#if !SKIP
import Foundation

/// One encoded H.264 access unit as carried by the media record transport.
public struct H264AccessUnit: Equatable, Sendable {
    public let ptsNanoseconds: UInt64
    public let isKeyframe: Bool
    public let discontinuity: Bool
    public let data: Data

    public init(ptsNanoseconds: UInt64, isKeyframe: Bool, discontinuity: Bool, data: Data) {
        self.ptsNanoseconds = ptsNanoseconds
        self.isKeyframe = isKeyframe
        self.discontinuity = discontinuity
        self.data = data
    }
}

public struct H264UnitDescription: Equatable, Sendable {
    public let nalUnits: [Data]
    public let sps: [Data]
    public let pps: [Data]
    public let isIDR: Bool

    public var hasParameterSets: Bool { !sps.isEmpty && !pps.isEmpty }
}

public enum H264ParseError: Error, Equatable, CustomStringConvertible {
    case empty
    case accessUnitTooLarge
    case noStartCode
    case emptyNALUnit

    public var description: String {
        switch self {
        case .empty: "H.264 access unit is empty"
        case .accessUnitTooLarge: "H.264 access unit exceeds the 16 MiB bound"
        case .noStartCode: "H.264 access unit has no Annex-B start code"
        case .emptyNALUnit: "H.264 access unit contains an empty NAL unit"
        }
    }
}

public enum H264AnnexB {
    public static let maxAccessUnitBytes = 16 * 1024 * 1024

    /// Split an Annex-B access unit into NAL units without copying parameter
    /// set semantics. Three- and four-byte start codes are both accepted.
    public static func split(_ data: Data) throws -> [Data] {
        guard !data.isEmpty else { throw H264ParseError.empty }
        guard data.count <= maxAccessUnitBytes else { throw H264ParseError.accessUnitTooLarge }
        let bytes = [UInt8](data)
        var starts: [(index: Int, length: Int)] = []
        var index = 0
        while index + 3 <= bytes.count {
            if bytes[index] == 0, bytes[index + 1] == 0, bytes[index + 2] == 1 {
                starts.append((index, 3))
                index += 3
            } else if index + 4 <= bytes.count,
                      bytes[index] == 0, bytes[index + 1] == 0,
                      bytes[index + 2] == 0, bytes[index + 3] == 1
            {
                starts.append((index, 4))
                index += 4
            } else {
                index += 1
            }
        }
        guard !starts.isEmpty else { throw H264ParseError.noStartCode }
        var units: [Data] = []
        for (position, start) in starts.enumerated() {
            let bodyStart = start.index + start.length
            let bodyEnd = position + 1 < starts.count ? starts[position + 1].index : bytes.count
            guard bodyEnd > bodyStart else { throw H264ParseError.emptyNALUnit }
            units.append(Data(bytes[bodyStart..<bodyEnd]))
        }
        guard !units.isEmpty else { throw H264ParseError.noStartCode }
        return units
    }

    public static func describe(_ unit: H264AccessUnit) throws -> H264UnitDescription {
        let nalUnits = try split(unit.data)
        var sps: [Data] = []
        var pps: [Data] = []
        var isIDR = false
        for nal in nalUnits {
            guard let first = nal.first else { throw H264ParseError.emptyNALUnit }
            switch first & 0x1F {
            case 7: sps.append(nal)
            case 8: pps.append(nal)
            case 5: isIDR = true
            default: break
            }
        }
        return H264UnitDescription(nalUnits: nalUnits, sps: sps, pps: pps, isIDR: isIDR)
    }
}

public enum H264GateResult: Equatable, Sendable {
    case deliver(H264UnitDescription)
    case suppress
}

/// Decoder gate: no delta unit is delivered until an SPS/PPS-prefixed IDR has
/// established decoder state, and any explicit discontinuity resets that rule.
/// The gate does not decode; it only decides delivery and reports counters.
public struct H264StreamGate: Sendable {
    public private(set) var waitingForKeyframe = true
    public private(set) var delivered = 0
    public private(set) var suppressed = 0
    public private(set) var sps: [Data] = []
    public private(set) var pps: [Data] = []

    public init() {}

    public mutating func accept(_ unit: H264AccessUnit) throws -> H264GateResult {
        let description = try H264AnnexB.describe(unit)
        if unit.discontinuity {
            waitingForKeyframe = true
        }
        if description.hasParameterSets {
            sps = description.sps
            pps = description.pps
        }
        if waitingForKeyframe {
            guard description.isIDR, !sps.isEmpty, !pps.isEmpty else {
                suppressed += 1
                return .suppress
            }
            waitingForKeyframe = false
            delivered += 1
            return .deliver(description)
        }
        delivered += 1
        return .deliver(description)
    }

    /// Force waiting for the next SPS/PPS-prefixed IDR, e.g. after a reconnect.
    public mutating func resetForDiscontinuity() {
        waitingForKeyframe = true
    }
}
#endif
