#if !SKIP
import SwiftUI
import VistaCore

/// Minimal pairing sheet for a real Vista host. It performs the authenticated
/// control handshake and stops there: catalog and video arrive in later
/// increments, so success means "paired session opened", never "streaming".
struct PairingView: View {
    @Environment(\.dismiss) private var dismiss

    @State private var host = ""
    @State private var port = "7443"
    @State private var secretHex = ""
    @State private var status: String?
    @State private var isConnecting = false
    @State private var connection: ControlClientConnection?
    @State private var connectedHost: String?
    @State private var connectedPairingID: String?

    var body: some View {
        NavigationStack {
            Form {
                Section("Host") {
                    TextField("Hostname or LAN address", text: $host)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .accessibilityLabel("Host address")
                    TextField("Port", text: $port)
                        .keyboardType(.numberPad)
                        .accessibilityLabel("Host port")
                }

                Section("Pairing secret") {
                    SecureField("64-character hex secret", text: $secretHex)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .accessibilityLabel("Pairing secret")
                    Text("Shown by the host out of band. Stored in the Keychain only after a successful pairing.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Section {
                    Button(action: connect) {
                        HStack {
                            if isConnecting {
                                ProgressView()
                                    .controlSize(.small)
                            }
                            Text(isConnecting ? "Pairing…" : "Pair and open session")
                        }
                        .frame(maxWidth: .infinity)
                    }
                    .disabled(isConnecting || host.isEmpty || secretHex.isEmpty)

                    if let status {
                        Text(status)
                            .font(.footnote)
                            .foregroundStyle(status.hasPrefix("Paired") ? .green : .secondary)
                            .accessibilityLabel("Pairing status")
                    }
                }

                if let connectedHost, let connectedPairingID {
                    Section("Paired") {
                        LabeledContent("Host", value: connectedHost)
                        LabeledContent("Pairing id", value: connectedPairingID)
                        Text("Control session opened. Catalog and video are not implemented yet.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Connect")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        connection?.close()
                        dismiss()
                    }
                }
            }
            .onDisappear {
                connection?.close()
            }
        }
    }

    private func connect() {
        status = nil
        connectedHost = nil
        connectedPairingID = nil
        guard let portValue = UInt16(port), portValue > 0 else {
            status = "Port must be between 1 and 65535."
            return
        }
        let secret: PairingSecret
        do {
            secret = try PairingSecret(hex: secretHex.trimmingCharacters(in: .whitespacesAndNewlines))
        } catch {
            status = "The pairing secret must be 64 hex characters."
            return
        }

        let connection = ControlClientConnection()
        self.connection = connection
        isConnecting = true
        status = "Connecting…"
        connection.onEvent = { event in
            DispatchQueue.main.async {
                switch event {
                case .state(.handshaking):
                    status = "Pairing…"
                case let .state(.failed(message)):
                    isConnecting = false
                    status = "Failed: \(message)"
                case .state(.closed):
                    isConnecting = false
                case let .serverHello(hostName, pairingID, _):
                    connectedHost = hostName
                    connectedPairingID = pairingID
                case let .sessionOpened(permissions):
                    isConnecting = false
                    status = "Paired. Video=\(permissions.video) input=\(permissions.input)."
                    try? PairingSecretStore.save(secret, for: host)
                case let .rejected(reason):
                    isConnecting = false
                    status = "Rejected: \(reason)"
                case let .message(message):
                    status = message
                case .pong:
                    break
                case .state(.idle), .state(.connecting), .state(.active):
                    break
                }
            }
        }
        connection.connect(host: host, port: portValue, secret: secret) { result in
            DispatchQueue.main.async {
                isConnecting = false
                if case let .failure(error) = result {
                    status = "Failed: \(error.localizedDescription)"
                }
            }
        }
    }
}
#endif
