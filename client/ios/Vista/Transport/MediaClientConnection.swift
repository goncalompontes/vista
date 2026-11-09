#if canImport(Network)
import Foundation
import Network
import VistaCore

/// Client for the opt-in H.264 endpoint announced by the authenticated
/// control session. The media handshake is deliberately separate from the
/// control handshake so each direction gets fresh record counters and keys.
final class MediaClientConnection: @unchecked Sendable {
    private let queue = DispatchQueue(label: "dev.vista.client.media")
    private var connection: NWConnection?
    private var client: ControlPairingClient?
    private var mediaCipher: SecureRecordCipher?
    private var controlCipher: SecureRecordCipher?
    private var buffer = Data()
    private var active = false

    var onRecord: ((H264AccessUnit) -> Void)?
    var onMessage: ((String) -> Void)?

    func connect(
        host: String,
        port: UInt16,
        secret: PairingSecret,
        expectedPairingID: String
    ) {
        queue.async {
            do {
                let client = try ControlPairingClient(
                    secret: secret,
                    clientID: "vista-ios-media"
                )
                guard let endpointPort = NWEndpoint.Port(rawValue: port) else {
                    throw WireError.invalidField("media port")
                }
                self.client = client
                let connection = NWConnection(
                    host: NWEndpoint.Host(host),
                    port: endpointPort,
                    using: .tcp
                )
                self.connection = connection
                connection.stateUpdateHandler = { [weak self] state in
                    guard let self else { return }
                    self.queue.async {
                        if case let .failed(error) = state {
                            self.onMessage?("Media connection failed: \(error.localizedDescription)")
                        }
                    }
                }
                connection.start(queue: self.queue)
                connection.send(
                    content: try ControlFraming.encode(client.hello()),
                    completion: .contentProcessed { [weak self] error in
                        if let error {
                            self?.onMessage?("Media handshake failed: \(error.localizedDescription)")
                        }
                    }
                )
                self.receiveLoop(expectedPairingID: expectedPairingID)
            } catch {
                self.onMessage?("Media connection failed: \(error.localizedDescription)")
            }
        }
    }

    func close() {
        queue.async {
            self.active = false
            self.connection?.cancel()
            self.connection = nil
            self.mediaCipher = nil
            self.controlCipher = nil
            self.buffer.removeAll()
        }
    }

    private func receiveLoop(expectedPairingID: String) {
        connection?.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) {
            [weak self] data, _, isComplete, error in
            guard let self else { return }
            self.queue.async {
                if let error {
                    self.onMessage?("Media receive failed: \(error.localizedDescription)")
                    return
                }
                do {
                    if let data, !data.isEmpty {
                        self.buffer.append(data)
                        if self.active {
                            guard var cipher = self.mediaCipher else { throw WireError.invalidTransition("media cipher") }
                            let records = try SecureMediaFraming.drain(&self.buffer, cipher: &cipher)
                            self.mediaCipher = cipher
                            records.forEach { self.onRecord?($0) }
                        } else {
                            for envelope in try ControlFraming.drain(&self.buffer) {
                                try self.handleHandshake(envelope, expectedPairingID: expectedPairingID)
                            }
                        }
                    }
                    if isComplete {
                        self.onMessage?("Media host closed the connection")
                        return
                    }
                    self.receiveLoop(expectedPairingID: expectedPairingID)
                } catch {
                    self.onMessage?("Media protocol failed: \(error.localizedDescription)")
                }
            }
        }
    }

    private func handleHandshake(_ envelope: Envelope, expectedPairingID: String) throws {
        guard let client else { throw WireError.invalidTransition("media client") }
        switch envelope.body {
        case .serverHello:
            let proof = try client.acceptServerHello(envelope)
            if case let .serverHello(_, _, _, _, pairingID, _, _) = envelope.body,
               pairingID != expectedPairingID {
                throw WireError.unknownPairing
            }
            try sendPlain(proof)
        case .pairingAccepted:
            let keys = try client.acceptPairing(envelope)
            try sendPlain(client.openSession(.videoOnly))
            self.mediaCipher = try SecureRecordCipher(
                key: keys.hostToClient,
                sessionID: client.sessionID ?? "",
                direction: .hostToClient,
                maxPlaintextBytes: SecureMediaFraming.maxPlaintextBytes
            )
            self.controlCipher = try SecureRecordCipher(
                key: keys.clientToHost,
                sessionID: client.sessionID ?? "",
                direction: .clientToHost
            )
        case .sessionOpened:
            try client.acceptSessionOpened(envelope)
            active = true
        default:
            throw WireError.unexpectedMessage(envelope.body.typeName)
        }
    }

    private func sendPlain(_ envelope: Envelope) throws {
        let data = try ControlFraming.encode(envelope)
        connection?.send(content: data, completion: .contentProcessed { [weak self] error in
            if let error {
                self?.onMessage?("Media send failed: \(error.localizedDescription)")
            }
        })
    }

    func requestKeyframe() {
        guard active,
              let client,
              var cipher = controlCipher,
              let envelope = try? client.keyframeRequest(),
              let json = try? JSONEncoder().encode(envelope),
              let data = try? SecureControlFraming.encodeFrame(json, cipher: &cipher)
        else { return }
        controlCipher = cipher
        connection?.send(content: data, completion: .contentProcessed { [weak self] error in
            if let error {
                self?.onMessage?("Media keyframe request failed: \(error.localizedDescription)")
            }
        })
    }
}
#endif
