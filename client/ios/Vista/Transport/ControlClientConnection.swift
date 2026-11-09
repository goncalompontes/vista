#if canImport(Network)
import Foundation
import Network
import VistaCore

public enum ControlClientState: Equatable {
    case idle
    case connecting
    case handshaking
    case active
    case failed(String)
    case closed
}

public enum ControlClientEvent: Equatable {
    case state(ControlClientState)
    case serverHello(hostName: String, pairingID: String, expiresAt: UInt64)
    case sessionOpened(PermissionSet)
    case mediaOffer(MediaOffer)
    case mediaRecord(H264AccessUnit)
    case pong(UInt64)
    case rejected(String)
    case catalog(CatalogSnapshot)
    case capabilities(CapabilitySnapshot)
    case actionResult(ActionResultPayload)
    case message(String)
}

/// Network.framework transport for the versioned Vista control protocol.
///
/// Handshake frames are plaintext; once `PairingAccepted` is verified every
/// record is sealed with the session-derived directional key. Pairing secrets
/// are never logged.
public final class ControlClientConnection: @unchecked Sendable {
    private let queue = DispatchQueue(label: "dev.vista.client.control")
    private var connection: NWConnection?
    private var client: ControlPairingClient?
    private var secure: SecureControlChannel?
    private var buffer = Data()
    private var state: ControlClientState = .idle
    private var completion: ((Result<Void, Error>) -> Void)?
    private var timeoutWork: DispatchWorkItem?
    private var lastPingNonce: UInt64?
    private var mediaConnection: MediaClientConnection?
    private var pairingSecret: PairingSecret?
    private var hostAddress: String?

    public private(set) var hostName: String?
    public private(set) var pairingID: String?
    public private(set) var policy: PermissionSet?
    public private(set) var catalog: CatalogSnapshot?
    public private(set) var capabilitySnapshot: CapabilitySnapshot?
    private var capabilitySession: CapabilitySession?
    public var onEvent: ((ControlClientEvent) -> Void)?

    public init() {}

    public var currentState: ControlClientState {
        queue.sync { state }
    }

    public func connect(
        host: String,
        port: UInt16,
        secret: PairingSecret,
        clientID: String = "vista-ios",
        timeout: TimeInterval = 12,
        completion: @escaping (Result<Void, Error>) -> Void
    ) {
        queue.async {
            guard self.connection == nil else {
                return completion(.failure(WireError.invalidTransition("connection already in use")))
            }
            do {
                self.client = try ControlPairingClient(secret: secret, clientID: clientID)
            } catch {
                return completion(.failure(error))
            }
            guard let endpointPort = NWEndpoint.Port(rawValue: port) else {
                return completion(.failure(WireError.invalidField("port")))
            }
            self.completion = completion
            self.pairingSecret = secret
            self.hostAddress = host
            self.setState(.connecting)
            let connection = NWConnection(
                host: NWEndpoint.Host(host),
                port: endpointPort,
                using: .tcp
            )
            self.connection = connection
            connection.stateUpdateHandler = { [weak self] newState in
                self?.queue.async { self?.handleState(newState) }
            }
            let work = DispatchWorkItem { [weak self] in
                self?.fail(WireError.invalidTransition("control connection timed out"))
            }
            self.timeoutWork = work
            self.queue.asyncAfter(deadline: .now() + timeout, execute: work)
            connection.start(queue: self.queue)
        }
    }

    public func sendPing(nonce: UInt64) {
        queue.async {
            guard let client = self.client, client.state == .active else { return }
            do {
                self.lastPingNonce = nonce
                try self.send(client.ping(nonce: nonce))
            } catch {
                self.fail(error)
            }
        }
    }

    public func requestKeyframe() {
        queue.async {
            guard let client = self.client, client.state == .active else { return }
            do {
                try self.send(client.keyframeRequest())
            } catch {
                self.fail(error)
            }
        }
    }

    public func requestMediaKeyframe() {
        queue.async { self.mediaConnection?.requestKeyframe() }
    }

    public func invokeAction(
        id: String,
        parameters: [String: JSONValue] = [:]
    ) {
        queue.async {
            guard let client = self.client,
                  client.state == .active,
                  var session = self.capabilitySession,
                  let action = session.action(id: id)
            else { return }
            var parameters = parameters
            if id == "window.maximize.set", parameters.isEmpty {
                parameters["value"] = .bool(action.presentation.label == "Maximize")
            }
            guard let request = session.makeRequest(actionID: id, parameters: parameters) else { return }
            self.capabilitySession = session
            do {
                try self.send(client.actionInvoke(request))
            } catch {
                self.fail(error)
            }
        }
    }

    public func close() {
        queue.async {
            if let client = self.client, client.state == .active,
               let envelope = try? client.sessionClose(reason: "client closed") {
                try? self.send(envelope)
            }
            self.connection?.cancel()
            self.connection = nil
            self.mediaConnection?.close()
            self.mediaConnection = nil
            self.secure = nil
            self.buffer.removeAll()
            self.timeoutWork?.cancel()
            self.setState(.closed)
        }
    }

    // MARK: - Private

    private func handleState(_ newState: NWConnection.State) {
        switch newState {
        case .ready:
            setState(.handshaking)
            guard let client else { return fail(WireError.invalidTransition("missing client")) }
            do {
                try send(client.hello())
            } catch {
                fail(error)
            }
            receiveLoop()
        case let .failed(error):
            fail(error)
        case .cancelled:
            setState(.closed)
        default:
            break
        }
    }

    private func receiveLoop() {
        connection?.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) {
            [weak self] data, _, isComplete, error in
            guard let self else { return }
            self.queue.async {
                if let error {
                    return self.fail(error)
                }
                if let data, !data.isEmpty {
                    do {
                        try self.consume(data)
                    } catch {
                        return self.fail(error)
                    }
                }
                if isComplete {
                    return self.fail(WireError.frameTruncated)
                }
                self.receiveLoop()
            }
        }
    }

    private func consume(_ data: Data) throws {
        buffer.append(data)
        if let secure {
            for envelope in try secure.drain(&buffer) {
                try handle(envelope)
            }
        } else {
            for envelope in try ControlFraming.drain(&buffer) {
                try handle(envelope)
            }
        }
    }

    private func handle(_ envelope: Envelope) throws {
        guard let client else { throw WireError.invalidTransition("missing client") }
        switch envelope.body {
        case .serverHello:
            let proof = try client.acceptServerHello(envelope)
            if case let .serverHello(_, _, hostName, _, pairingID, expiresAt, policy) = envelope.body {
                self.hostName = hostName
                self.pairingID = pairingID
                self.policy = policy
                onEvent?(.serverHello(hostName: hostName, pairingID: pairingID, expiresAt: expiresAt))
            }
            try send(proof)
        case .pairingAccepted:
            let keys = try client.acceptPairing(envelope)
            guard let sessionID = client.sessionID else { throw WireError.sessionMismatch }
            secure = try SecureControlChannel(keys: keys, sessionID: sessionID)
            let requested = policy ?? .videoOnly
            try send(client.openSession(requested))
        case .sessionOpened:
            try client.acceptSessionOpened(envelope)
            if case let .sessionOpened(_, permissions) = envelope.body {
                onEvent?(.sessionOpened(permissions))
            }
            complete(.success(()))
        case let .mediaOffer(offer):
            let acceptedOffer = try client.acceptMediaOffer(envelope)
            onEvent?(.mediaOffer(acceptedOffer))
            guard let hostAddress, let pairingSecret else { return }
            let media = MediaClientConnection()
            media.onRecord = { [weak self] record in
                self?.onEvent?(.mediaRecord(record))
            }
            media.onMessage = { [weak self] message in
                self?.onEvent?(.message(message))
            }
            mediaConnection?.close()
            mediaConnection = media
            media.connect(
                host: hostAddress,
                port: offer.port,
                secret: pairingSecret,
                expectedPairingID: offer.pairingID
            )
        case let .pairingRejected(reason):
            fail(WireError.authenticationFailed)
            onEvent?(.rejected(reason))
        case let .error(code, message):
            fail(WireError.invalidTransition("\(code): \(message)"))
            onEvent?(.message(message))
        case let .pong(nonce):
            try client.acceptPong(envelope, nonce: lastPingNonce ?? nonce)
            onEvent?(.pong(nonce))
        case .catalogSnapshot:
            let accepted = try client.acceptCatalogSnapshot(envelope)
            catalog = accepted
            onEvent?(.catalog(accepted))
        case .capabilitySnapshot:
            let accepted = try client.acceptCapabilitySnapshot(envelope)
            capabilitySnapshot = accepted
            if var session = capabilitySession {
                session.reconnect(with: accepted)
                capabilitySession = session
            } else {
                capabilitySession = CapabilitySession(snapshot: accepted)
            }
            onEvent?(.capabilities(accepted))
        case .actionResult:
            let result = try client.acceptActionResult(envelope)
            if var session = capabilitySession {
                _ = session.apply(result)
                capabilitySession = session
            }
            onEvent?(.actionResult(result))
        default:
            onEvent?(.message(envelope.body.typeName))
        }
    }

    private func send(_ envelope: Envelope) throws {
        let data: Data
        if let secure {
            data = try secure.seal(envelope)
        } else {
            data = try ControlFraming.encode(envelope)
        }
        connection?.send(content: data, completion: .contentProcessed { [weak self] error in
            if let error {
                self?.queue.async { self?.fail(error) }
            }
        })
    }

    private func complete(_ result: Result<Void, Error>) {
        timeoutWork?.cancel()
        timeoutWork = nil
        switch result {
        case .success:
            setState(.active)
        case let .failure(error):
            setState(.failed(error.localizedDescription))
        }
        completion?(result)
        completion = nil
    }

    private func fail(_ error: Error) {
        guard completion != nil || connection != nil else { return }
        connection?.cancel()
        connection = nil
        secure = nil
        buffer.removeAll()
        complete(.failure(error))
    }

    private func setState(_ newState: ControlClientState) {
        guard state != newState else { return }
        state = newState
        onEvent?(.state(newState))
    }
}
#endif
