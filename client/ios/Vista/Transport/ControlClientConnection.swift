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
    case pong(UInt64)
    case rejected(String)
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

    public private(set) var hostName: String?
    public private(set) var pairingID: String?
    public private(set) var policy: PermissionSet?
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

    public func close() {
        queue.async {
            if let client = self.client, client.state == .active,
               let envelope = try? client.sessionClose(reason: "client closed") {
                try? self.send(envelope)
            }
            self.connection?.cancel()
            self.connection = nil
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
        case let .pairingRejected(reason):
            fail(WireError.authenticationFailed)
            onEvent?(.rejected(reason))
        case let .error(code, message):
            fail(WireError.invalidTransition("\(code): \(message)"))
            onEvent?(.message(message))
        case let .pong(nonce):
            try client.acceptPong(envelope, nonce: lastPingNonce ?? nonce)
            onEvent?(.pong(nonce))
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
