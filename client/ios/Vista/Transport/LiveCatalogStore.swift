import Foundation
import VistaCore

/// Observable holder for the latest catalog snapshot received from a paired
/// host. It maps host metadata to portable `RemoteApplication` rows and never
/// invents entries when the host reports none.
@MainActor
final class LiveCatalogStore: ObservableObject {
    @Published private(set) var snapshot: CatalogSnapshot?
    @Published private(set) var hostName: String?
    @Published private(set) var pairingID: String?
    @Published private(set) var capabilities: CapabilitySnapshot?
    @Published private(set) var lastActionResult: ActionResultPayload?
    @Published private(set) var connectionState: ControlClientState = .idle
    @Published private(set) var mediaRecords: [H264AccessUnit] = []
    @Published private(set) var mediaRecordSequence: UInt64 = 0
    @Published private(set) var streamTarget: StreamTarget?
    @Published private(set) var streamWidth: UInt32 = 0
    @Published private(set) var streamHeight: UInt32 = 0
    @Published private(set) var geometryEpoch: UInt64 = 1
    @Published private(set) var streamMessage: String?
    @Published private(set) var inputEnabled = false

    private var connection: ControlClientConnection?

    var isLive: Bool { snapshot != nil }
    var revision: UInt64 { snapshot?.revision ?? 0 }
    var isStreaming: Bool { streamTarget != nil }
    var output: CatalogOutput? { snapshot?.output }

    var applications: [RemoteApplication] {
        guard let snapshot else { return [] }
        return snapshot.entries.map { entry in
            RemoteApplication(
                id: entry.id,
                name: entry.displayName,
                subtitle: entry.appID ?? "Window \(entry.windowID)",
                symbolName: "macwindow",
                source: .existingWindow
            )
        }
    }

    var desktops: [CatalogWorkspace] {
        snapshot?.workspaces ?? []
    }

    func setHost(_ name: String) {
        hostName = name
    }

    func apply(_ snapshot: CatalogSnapshot, hostName: String?) {
        if let hostName {
            self.hostName = hostName
        }
        self.snapshot = snapshot
    }

    func clear() {
        connection?.close()
        connection = nil
        snapshot = nil
        hostName = nil
        pairingID = nil
        capabilities = nil
        lastActionResult = nil
        mediaRecords.removeAll()
        streamTarget = nil
        streamWidth = 0
        streamHeight = 0
        streamMessage = nil
        inputEnabled = false
        connectionState = .closed
    }

    func connect(host: String, port: UInt16, secret: PairingSecret, completion: @escaping (Result<Void, Error>) -> Void) {
        connection?.close()
        let connection = ControlClientConnection()
        self.connection = connection
        connection.onEvent = { [weak self] event in
            Task { @MainActor in
                guard let self else { return }
                switch event {
                case let .state(state):
                    self.connectionState = state
                case let .serverHello(hostName, pairingID, _):
                    self.hostName = hostName
                    self.pairingID = pairingID
                case let .catalog(snapshot):
                    self.apply(snapshot, hostName: self.hostName)
                case let .sessionOpened(permissions):
                    self.inputEnabled = permissions.input
                case let .capabilities(snapshot):
                    self.capabilities = snapshot
                case let .actionResult(result):
                    self.lastActionResult = result
                case let .mediaRecord(record):
                    self.mediaRecords.append(record)
                    self.mediaRecordSequence &+= 1
                    if self.mediaRecords.count > 240 {
                        self.mediaRecords.removeFirst(self.mediaRecords.count - 240)
                    }
                case let .streamStarted(target, width, height, geometryEpoch):
                    self.streamTarget = target
                    self.streamWidth = width
                    self.streamHeight = height
                    self.geometryEpoch = geometryEpoch
                    self.streamMessage = nil
                case let .streamStopped(reason):
                    self.streamTarget = nil
                    self.streamMessage = reason
                case let .streamError(code, message):
                    self.streamTarget = nil
                    self.streamMessage = "\(code): \(message)"
                default:
                    break
                }
            }
        }
        connection.connect(host: host, port: port, secret: secret, completion: completion)
    }

    func invokeAction(id: String, parameters: [String: JSONValue] = [:]) {
        connection?.invokeAction(id: id, parameters: parameters)
    }

    func selectStream(_ target: StreamTarget) {
        mediaRecords.removeAll()
        streamMessage = nil
        connection?.selectStream(target)
    }

    func stopStream() {
        connection?.stopStream()
        streamTarget = nil
    }

    func sendInput(_ event: PortableInputEvent) {
        connection?.sendInput(event)
    }

    func requestCatalog() {
        connection?.requestCatalog()
    }

    func requestMediaKeyframe() {
        connection?.requestMediaKeyframe()
    }

    func closeConnection() {
        connection?.close()
        connection = nil
        connectionState = .closed
    }
}
