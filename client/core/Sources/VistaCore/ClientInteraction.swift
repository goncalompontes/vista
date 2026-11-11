import Foundation

/// Gesture ownership is resolved locally before any event is sent to the host.
/// Higher-priority owners may pre-empt lower-priority owners; cancellation never
/// synthesizes a host action.
public enum GestureOwner: Int, CaseIterable, Comparable, Sendable {
    case remoteInput = 0
    case hostBinding = 1
    case localPresentation = 2
    case nativeInterface = 3
    case accessibility = 4
    case operatingSystem = 5

    public static func < (lhs: GestureOwner, rhs: GestureOwner) -> Bool {
        lhs.rawValue < rhs.rawValue
    }
}

public struct GestureOwnership: Equatable, Sendable {
    public private(set) var owner: GestureOwner?
    public private(set) var isCancelled = false

    public init() {}

    /// Claims a gesture. The current owner is stable unless a higher-priority
    /// owner takes over, preventing recognizer changes from leaking stray input.
    @discardableResult
    public mutating func claim(_ candidate: GestureOwner) -> Bool {
        guard !isCancelled else { return false }
        guard let current = owner else {
            owner = candidate
            return true
        }
        guard candidate >= current else { return candidate == current }
        owner = candidate
        return true
    }

    public mutating func cancel() {
        owner = nil
        isCancelled = true
    }

    public mutating func reset() {
        owner = nil
        isCancelled = false
    }

    public var mayEmitRemoteInput: Bool {
        !isCancelled && owner == .remoteInput
    }
}

/// A finite, positive rectangle in view coordinates occupied by the actual
/// displayed video content (not its containing view or letterbox bars).
public struct VideoContentRect: Equatable, Sendable {
    public let x: Double
    public let y: Double
    public let width: Double
    public let height: Double

    public init?(x: Double, y: Double, width: Double, height: Double) {
        guard x.isFinite, y.isFinite, width.isFinite, height.isFinite,
              width > 0, height > 0 else { return nil }
        self.x = x
        self.y = y
        self.width = width
        self.height = height
    }

    /// Converts a point in the containing view into normalized source content
    /// coordinates. Points in safe-area insets, bars, or outside the picture are
    /// rejected rather than clamped to an unintended edge.
    public func sourcePoint(x: Double, y: Double) -> (x: Double, y: Double)? {
        guard x.isFinite, y.isFinite else { return nil }
        let localX = (x - self.x) / width
        let localY = (y - self.y) / height
        guard (0...1).contains(localX), (0...1).contains(localY) else { return nil }
        return (localX, localY)
    }
}

/// Inverse mapping from a view-space touch to the unzoomed video source.
/// The content rect is expressed after the current layout/orientation transform;
/// zoom and pan are inverted around the displayed content centre.
public struct VideoInputGeometry: Equatable, Sendable {
    public let contentRect: VideoContentRect
    public let zoom: Double
    public let panX: Double
    public let panY: Double
    public let epoch: UInt64

    public init?(contentRect: VideoContentRect, zoom: Double = 1, panX: Double = 0, panY: Double = 0, epoch: UInt64) {
        guard zoom.isFinite, zoom >= 1, zoom <= 5,
              panX.isFinite, panY.isFinite else { return nil }
        self.contentRect = contentRect
        self.zoom = zoom
        self.panX = panX
        self.panY = panY
        self.epoch = epoch
    }

    public func sourcePoint(viewX: Double, viewY: Double, currentEpoch: UInt64) -> (x: Double, y: Double)? {
        guard currentEpoch == epoch,
              let displayed = contentRect.sourcePoint(x: viewX, y: viewY) else { return nil }
        let x = (displayed.x - 0.5 - panX) / zoom + 0.5
        let y = (displayed.y - 0.5 - panY) / zoom + 0.5
        guard (0...1).contains(x), (0...1).contains(y) else { return nil }
        return (x, y)
    }
}

public enum ClipboardDirection: Sendable {
    case deviceToHost
    case hostToDevice
}

/// Clipboard access is never implied by a connected session. A transfer is
/// permitted only for the granted direction and an explicit foreground user
/// action; writing to the device additionally requires an explicit confirmation.
public struct ClipboardAccessPolicy: Equatable, Sendable {
    public let deviceToHostGranted: Bool
    public let hostToDeviceGranted: Bool
    public let inputGranted: Bool

    public init(deviceToHostGranted: Bool = false, hostToDeviceGranted: Bool = false, inputGranted: Bool = false) {
        self.deviceToHostGranted = deviceToHostGranted
        self.hostToDeviceGranted = hostToDeviceGranted
        self.inputGranted = inputGranted
    }

    public func mayReadDeviceClipboard(userInitiated: Bool, appIsForeground: Bool) -> Bool {
        deviceToHostGranted && userInitiated && appIsForeground
    }

    public func maySendToHostClipboard(userInitiated: Bool, appIsForeground: Bool) -> Bool {
        mayReadDeviceClipboard(userInitiated: userInitiated, appIsForeground: appIsForeground)
    }

    public func mayOfferHostClipboard(userInitiated: Bool, appIsForeground: Bool) -> Bool {
        hostToDeviceGranted && userInitiated && appIsForeground
    }

    public func mayWriteDeviceClipboard(userInitiated: Bool, appIsForeground: Bool, explicitlyConfirmed: Bool) -> Bool {
        mayOfferHostClipboard(userInitiated: userInitiated, appIsForeground: appIsForeground) && explicitlyConfirmed
    }

    public func mayPasteNow(userInitiated: Bool, appIsForeground: Bool, explicitlyConfirmed: Bool) -> Bool {
        mayReadDeviceClipboard(userInitiated: userInitiated, appIsForeground: appIsForeground)
            && inputGranted && explicitlyConfirmed
    }
}
