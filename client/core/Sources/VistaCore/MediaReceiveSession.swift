#if !SKIP
import Foundation

public struct MediaReceiveOffer: Equatable, Sendable {
    public let pairingID: String
    public let sessionID: String
    public let generation: UInt64
    public let authenticated: Bool

    public init(pairingID: String, sessionID: String, generation: UInt64, authenticated: Bool) {
        self.pairingID = pairingID
        self.sessionID = sessionID
        self.generation = generation
        self.authenticated = authenticated
    }
}

public enum MediaReceiveError: Error, Equatable {
    case unauthenticatedOffer
    case wrongPairingIdentity
    case wrongSession
    case staleGeneration
    case invalidGeneration
    case closed
    case decoderFailure
}

public enum MediaQueuePolicy: Sendable { case dropOldest, dropNewest }
public enum MediaEnqueueResult: Equatable { case queued, dropped, requestKeyframe }

/// Platform decoder boundary. Implementations own codec resources; the receive
/// state machine never depends on MediaCodec, VideoToolbox, or an emulator.
public protocol H264MediaDecoder: AnyObject {
    func decode(_ accessUnit: H264AccessUnit) throws
    func reset()
    func close()
}

/// Bounded single-consumer queue and lifecycle guard for an authenticated media
/// session. Callers serialize access (e.g. on their transport queue/actor).
public final class MediaReceiveSession {
    public let offer: MediaReceiveOffer
    public let capacity: Int
    public let policy: MediaQueuePolicy
    public private(set) var isClosed = false
    public private(set) var waitingForKeyframe = true
    public private(set) var keyframeRequests = 0
    public private(set) var droppedAccessUnits = 0
    private var queue: [H264AccessUnit] = []
    private let decoder: H264MediaDecoder

    public init(offer: MediaReceiveOffer, expectedPairingID: String, expectedSessionID: String,
                generation: UInt64, capacity: Int = 8, policy: MediaQueuePolicy = .dropOldest,
                decoder: H264MediaDecoder) throws {
        guard offer.authenticated else { throw MediaReceiveError.unauthenticatedOffer }
        guard offer.pairingID == expectedPairingID else { throw MediaReceiveError.wrongPairingIdentity }
        guard offer.sessionID == expectedSessionID else { throw MediaReceiveError.wrongSession }
        guard generation > 0, offer.generation > 0 else { throw MediaReceiveError.invalidGeneration }
        guard offer.generation == generation else { throw MediaReceiveError.staleGeneration }
        self.offer = offer
        self.capacity = max(1, capacity)
        self.policy = policy
        self.decoder = decoder
    }

    public var queuedCount: Int { queue.count }

    @discardableResult
    public func enqueue(_ unit: H264AccessUnit, generation: UInt64) throws -> MediaEnqueueResult {
        guard !isClosed else { throw MediaReceiveError.closed }
        guard generation == offer.generation else { throw MediaReceiveError.staleGeneration }
        guard !unit.data.isEmpty, unit.data.count <= SecureMediaFraming.maxAccessUnitBytes else {
            close()
            throw MediaRecordError.payloadTooLarge
        }
        if unit.discontinuity {
            queue.removeAll(keepingCapacity: true)
            waitingForKeyframe = true
            decoder.reset()
        }
        if waitingForKeyframe {
            let description = try H264AnnexB.describe(unit)
            guard description.isIDR, description.hasParameterSets else {
                requestKeyframe()
                return .requestKeyframe
            }
            waitingForKeyframe = false
        }
        if queue.count == capacity {
            droppedAccessUnits += 1
            waitingForKeyframe = true
            queue.removeAll(keepingCapacity: true)
            decoder.reset()
            requestKeyframe()
            // Never enqueue a delta after loss. An IDR can seed the new queue.
            guard unit.isKeyframe else { return .requestKeyframe }
        }
        if queue.count == capacity {
            if policy == .dropNewest { return .dropped }
            queue.removeFirst()
            droppedAccessUnits += 1
        }
        queue.append(unit)
        return .queued
    }

    /// Decode at most one item, allowing the caller to apply backpressure by
    /// controlling how frequently it drains the queue.
    @discardableResult
    public func decodeNext() throws -> Bool {
        guard !isClosed else { throw MediaReceiveError.closed }
        guard !queue.isEmpty else { return false }
        let unit = queue.removeFirst()
        do { try decoder.decode(unit); return true }
        catch {
            queue.removeAll(keepingCapacity: true)
            waitingForKeyframe = true
            decoder.reset()
            requestKeyframe()
            throw MediaReceiveError.decoderFailure
        }
    }

    public func markDiscontinuity() throws {
        guard !isClosed else { throw MediaReceiveError.closed }
        queue.removeAll(keepingCapacity: true)
        waitingForKeyframe = true
        decoder.reset()
        requestKeyframe()
    }

    public func stop() { close() }
    public func disconnect() { close() }
    public func fail() { close() }

    private func requestKeyframe() { keyframeRequests += 1 }

    private func close() {
        guard !isClosed else { return }
        isClosed = true
        queue.removeAll(keepingCapacity: false)
        decoder.close()
    }
}
#endif
