#if canImport(VideoToolbox) && canImport(AVFoundation) && !SKIP
import AVFoundation
import CoreMedia
import CoreVideo
import Foundation
import VideoToolbox
import VistaCore

/// VideoToolbox-backed presentation of H.264 access units.
///
/// `AVSampleBufferDisplayLayer` performs hardware-backed H.264 decode from the
/// CMSampleBuffers built here. The controller owns the discontinuity/keyframe
/// gate, so a delta unit is never enqueued before an SPS/PPS-prefixed IDR.
@MainActor
final class VideoStreamController: ObservableObject {
    let layer = AVSampleBufferDisplayLayer()

    @Published private(set) var framesPresented = 0
    @Published private(set) var suppressedUnits = 0
    @Published private(set) var status = "No video frames yet"
    @Published private(set) var lastError: String?

    var onKeyframeNeeded: (() -> Void)?

    private var gate = H264StreamGate()
    private var formatDescription: CMVideoFormatDescription?
    private var timebase: CMTimebase?
    private var failureObserver: NSObjectProtocol?

    func start() {
        layer.videoGravity = .resizeAspect
        var created: CMTimebase?
        CMTimebaseCreateWithSourceClock(
            allocator: kCFAllocatorDefault,
            sourceClock: CMClockGetHostTimeClock(),
            timebaseOut: &created
        )
        if let created {
            timebase = created
            layer.controlTimebase = created
            CMTimebaseSetTime(created, time: .zero)
            CMTimebaseSetRate(created, rate: 1.0)
        }
        failureObserver = NotificationCenter.default.addObserver(
            forName: .AVSampleBufferDisplayLayerFailedToDecode,
            object: layer,
            queue: .main
        ) { [weak self] notification in
            let error = notification.userInfo?[
                AVSampleBufferDisplayLayerFailedToDecodeNotificationErrorKey
            ] as? Error
            Task { @MainActor in
                self?.lastError = error?.localizedDescription ?? "decoder rejected a frame"
                self?.status = "Decoder error; waiting for a keyframe"
                self?.gate.resetForDiscontinuity()
                self?.layer.flush()
                self?.onKeyframeNeeded?()
            }
        }
    }

    func stop() {
        if let failureObserver {
            NotificationCenter.default.removeObserver(failureObserver)
            self.failureObserver = nil
        }
        layer.flushAndRemoveImage()
        gate.resetForDiscontinuity()
        formatDescription = nil
    }

    /// Feed one access unit. Suppressed units request a keyframe from the host.
    func receive(_ unit: H264AccessUnit) {
        do {
            let result = try gate.accept(unit)
            suppressedUnits = gate.suppressed
            switch result {
            case .suppress:
                status = "Waiting for a keyframed IDR"
                onKeyframeNeeded?()
            case let .deliver(description):
                try enqueue(description, unit: unit)
            }
        } catch {
            lastError = error.localizedDescription
            status = "Invalid H.264 access unit"
            gate.resetForDiscontinuity()
        }
    }

    func resetForReconnect() {
        gate.resetForDiscontinuity()
        formatDescription = nil
        layer.flush()
        status = "Reconnected; waiting for a keyframe"
        onKeyframeNeeded?()
    }

    private func enqueue(_ description: H264UnitDescription, unit: H264AccessUnit) throws {
        if description.hasParameterSets || formatDescription == nil {
            guard !gate.sps.isEmpty, !gate.pps.isEmpty else {
                throw H264ParseError.empty
            }
            formatDescription = try Self.makeFormatDescription(sps: gate.sps[0], pps: gate.pps[0])
        }
        guard let formatDescription else {
            throw H264ParseError.noStartCode
        }
        let sampleBuffer = try Self.makeSampleBuffer(
            description: description,
            unit: unit,
            formatDescription: formatDescription
        )
        if layer.status == .failed {
            layer.flush()
        }
        layer.enqueue(sampleBuffer)
        framesPresented += 1
        status = "\(framesPresented) frame(s) decoded"
    }

    // MARK: - Sample buffer construction

    static func makeFormatDescription(sps: Data, pps: Data) throws -> CMVideoFormatDescription {
        var description: CMVideoFormatDescription?
        let spsBytes = [UInt8](sps)
        let ppsBytes = [UInt8](pps)
        let status = spsBytes.withUnsafeBufferPointer { spsPointer in
            ppsBytes.withUnsafeBufferPointer { ppsPointer in
                var pointers: [UnsafePointer<UInt8>] = [
                    spsPointer.baseAddress!,
                    ppsPointer.baseAddress!
                ]
                var sizes = [spsBytes.count, ppsBytes.count]
                return CMVideoFormatDescriptionCreateFromH264ParameterSets(
                    allocator: kCFAllocatorDefault,
                    parameterSetCount: 2,
                    parameterSetPointers: &pointers,
                    parameterSetSizes: &sizes,
                    nalUnitHeaderLength: 4,
                    formatDescriptionOut: &description
                )
            }
        }
        guard status == noErr, let description else {
            throw WireError.invalidField("H.264 parameter sets")
        }
        return description
    }

    static func makeSampleBuffer(
        description: H264UnitDescription,
        unit: H264AccessUnit,
        formatDescription: CMVideoFormatDescription
    ) throws -> CMSampleBuffer {
        var avcc = Data()
        for nal in description.nalUnits {
            guard nal.count < 16 * 1024 * 1024 else {
                throw H264ParseError.accessUnitTooLarge
            }
            var length = UInt32(nal.count).bigEndian
            withUnsafeBytes(of: &length) { avcc.append(contentsOf: $0) }
            avcc.append(nal)
        }

        var blockBuffer: CMBlockBuffer?
        guard CMBlockBufferCreateWithMemoryBlock(
            allocator: kCFAllocatorDefault,
            memoryBlock: nil,
            blockLength: avcc.count,
            blockAllocator: kCFAllocatorDefault,
            customBlockSource: nil,
            offsetToData: 0,
            dataLength: avcc.count,
            flags: 0,
            blockBufferOut: &blockBuffer
        ) == kCMBlockBufferNoErr, let blockBuffer else {
            throw WireError.invalidField("CMBlockBuffer")
        }
        let copyStatus = avcc.withUnsafeBytes { raw in
            CMBlockBufferReplaceDataBytes(
                with: raw.baseAddress!,
                blockBuffer: blockBuffer,
                offsetIntoDestination: 0,
                dataLength: avcc.count
            )
        }
        guard copyStatus == kCMBlockBufferNoErr else {
            throw WireError.invalidField("CMBlockBuffer copy")
        }

        var timing = CMSampleTimingInfo(
            duration: .invalid,
            presentationTimeStamp: CMTime(value: Int64(unit.ptsNanoseconds), timescale: 1_000_000_000),
            decodeTimeStamp: .invalid
        )
        var sampleSize = avcc.count
        var sampleBuffer: CMSampleBuffer?
        let status = CMSampleBufferCreateReady(
            allocator: kCFAllocatorDefault,
            dataBuffer: blockBuffer,
            formatDescription: formatDescription,
            sampleCount: 1,
            sampleTimingEntryCount: 1,
            sampleTimingArray: &timing,
            sampleSizeEntryCount: 1,
            sampleSizeArray: &sampleSize,
            sampleBufferOut: &sampleBuffer
        )
        guard status == noErr, let sampleBuffer else {
            throw WireError.invalidField("CMSampleBuffer")
        }
        return sampleBuffer
    }

    // MARK: - Synthetic self-test

    /// Encode four synthetic frames with VideoToolbox and feed them through the
    /// same gate/decoder path. This proves the local decode path without a host.
    func runSyntheticSelfTest() {
        status = "Encoding synthetic H.264…"
        lastError = nil
        gate = H264StreamGate()
        framesPresented = 0
        suppressedUnits = 0
        SyntheticH264Encoder.encode(frames: 4, width: 160, height: 90) { result in
            Task { @MainActor in
                switch result {
                case let .success(units):
                    self.resetForReconnect()
                    for unit in units {
                        self.receive(unit)
                    }
                    self.status = self.framesPresented > 0
                        ? "Self-test decoded \(self.framesPresented) frame(s)"
                        : "Self-test produced no decoded frames"
                case let .failure(error):
                    self.lastError = error.localizedDescription
                    self.status = "Self-test failed: \(error.localizedDescription)"
                }
            }
        }
    }
}

enum SyntheticH264Encoder {
    static func encode(
        frames: Int,
        width: Int,
        height: Int,
        completion: @escaping (Result<[H264AccessUnit], Error>) -> Void
    ) {
        var session: VTCompressionSession?
        let status = VTCompressionSessionCreate(
            allocator: kCFAllocatorDefault,
            width: Int32(width),
            height: Int32(height),
            codecType: kCMVideoCodecType_H264,
            encoderSpecification: nil,
            imageBufferAttributes: nil,
            compressedDataAllocator: nil,
            outputCallback: nil,
            refcon: nil,
            compressionSessionOut: &session
        )
        guard status == noErr, let session else {
            completion(.failure(WireError.invalidField("VTCompressionSession")))
            return
        }
        VTCompressionSessionPrepareToEncodeFrames(session)

        let lock = NSLock()
        var units: [H264AccessUnit] = []
        var failure: Error?
        let group = DispatchGroup()

        for index in 0..<frames {
            guard let pixelBuffer = makePixelBuffer(width: width, height: height, frame: index) else {
                completion(.failure(WireError.invalidField("CVPixelBuffer")))
                return
            }
            group.enter()
            let status = VTCompressionSessionEncodeFrame(
                session,
                imageBuffer: pixelBuffer,
                presentationTimeStamp: CMTime(value: Int64(index), timescale: 30),
                duration: .invalid,
                frameProperties: nil,
                infoFlagsOut: nil
            ) { status, _, sampleBuffer in
                defer { group.leave() }
                guard status == noErr, let sampleBuffer, let unit = annexBUnit(from: sampleBuffer) else {
                    lock.lock()
                    failure = failure ?? WireError.invalidField("synthetic encode")
                    lock.unlock()
                    return
                }
                lock.lock()
                units.append(unit)
                lock.unlock()
            }
            if status != noErr {
                group.leave()
                completion(.failure(WireError.invalidField("VTCompressionSessionEncodeFrame")))
                return
            }
        }

        group.notify(queue: .global()) {
            VTCompressionSessionCompleteFrames(session, untilPresentationTimeStamp: .invalid)
            lock.lock()
            let result = failure ?? (units.isEmpty ? WireError.invalidField("no encoded frames") : nil)
            let output = units
            lock.unlock()
            if let result {
                completion(.failure(result))
            } else {
                completion(.success(output))
            }
        }
    }

    private static func makePixelBuffer(width: Int, height: Int, frame: Int) -> CVPixelBuffer? {
        var buffer: CVPixelBuffer?
        let attributes: [CFString: Any] = [
            kCVPixelBufferCGImageCompatibilityKey: true,
            kCVPixelBufferCGBitmapContextCompatibilityKey: true
        ]
        guard CVPixelBufferCreate(
            kCFAllocatorDefault,
            width,
            height,
            kCVPixelFormatType_32BGRA,
            attributes as CFDictionary,
            &buffer
        ) == kCVReturnSuccess, let buffer else {
            return nil
        }
        CVPixelBufferLockBaseAddress(buffer, [])
        defer { CVPixelBufferUnlockBaseAddress(buffer, []) }
        guard let base = CVPixelBufferGetBaseAddress(buffer) else { return nil }
        let bytesPerRow = CVPixelBufferGetBytesPerRow(buffer)
        let pointer = base.assumingMemoryBound(to: UInt8.self)
        for row in 0..<height {
            for column in 0..<width {
                let offset = row * bytesPerRow + column * 4
                pointer[offset] = UInt8((column + frame * 17) & 0xFF)
                pointer[offset + 1] = UInt8((row + frame * 11) & 0xFF)
                pointer[offset + 2] = UInt8((column + row + frame) & 0xFF)
                pointer[offset + 3] = 0xFF
            }
        }
        return buffer
    }

    /// Convert AVCC CMSampleBuffer data into Annex-B access units.
    private static func annexBUnit(from sampleBuffer: CMSampleBuffer) -> H264AccessUnit? {
        guard let dataBuffer = CMSampleBufferGetDataBuffer(sampleBuffer) else { return nil }
        var length = 0
        var totalLength = 0
        var dataPointer: UnsafeMutablePointer<Int8>?
        guard CMBlockBufferGetDataPointer(
            dataBuffer,
            atOffset: 0,
            lengthAtOffsetOut: &length,
            totalLengthOut: &totalLength,
            dataPointerOut: &dataPointer
        ) == kCMBlockBufferNoErr, let dataPointer else {
            return nil
        }
        let bytes = UnsafeRawPointer(dataPointer).assumingMemoryBound(to: UInt8.self)
        var annexB = Data()
        var offset = 0
        while offset + 4 <= totalLength {
            var nalLength: UInt32 = 0
            for index in 0..<4 {
                nalLength = (nalLength << 8) | UInt32(bytes[offset + index])
            }
            offset += 4
            let end = offset + Int(nalLength)
            guard end <= totalLength else { return nil }
            annexB.append(contentsOf: [0, 0, 0, 1])
            annexB.append(contentsOf: UnsafeBufferPointer(start: bytes + offset, count: Int(nalLength)))
            offset = end
        }
        let attachments = CMSampleBufferGetSampleAttachmentsArray(sampleBuffer, createIfNecessary: false)
        var keyframe = true
        if let attachments, CFArrayGetCount(attachments) > 0 {
            let dictionary = unsafeBitCast(
                CFArrayGetValueAtIndex(attachments, 0),
                to: CFDictionary.self
            )
            let value = CFDictionaryGetValue(dictionary, Unmanaged.passUnretained(kCMSampleAttachmentKey_NotSync).toOpaque())
            keyframe = value == nil
        }
        let pts = CMSampleBufferGetPresentationTimeStamp(sampleBuffer)
        let nanoseconds = pts.isValid
            ? UInt64(max(0, pts.seconds * 1_000_000_000))
            : 0
        return H264AccessUnit(
            ptsNanoseconds: nanoseconds,
            isKeyframe: keyframe,
            discontinuity: false,
            data: annexB
        )
    }
}
#endif
