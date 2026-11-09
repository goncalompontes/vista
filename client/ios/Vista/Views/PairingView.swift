#if !SKIP
import SwiftUI
import VistaCore

/// Pairing sheet for a real Vista host. The session owner keeps the encrypted
/// connection alive after this sheet closes so the library can consume the
/// host catalog and capability snapshot.
struct PairingView: View {
    @ObservedObject var store: LiveCatalogStore
    @Environment(\.dismiss) private var dismiss

    @State private var host = ""
    @State private var port = "7443"
    @State private var secretHex = ""
    @State private var status: String?
    @State private var isConnecting = false

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

                if let connectedHost = store.hostName, let connectedPairingID = store.pairingID {
                    Section("Paired") {
                        LabeledContent("Host", value: connectedHost)
                        LabeledContent("Pairing id", value: connectedPairingID)
                        Text("Control session stays connected while you browse the live catalog.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Connect")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        dismiss()
                    }
                }
            }
        }
    }

    private func connect() {
        status = nil
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

        isConnecting = true
        status = "Connecting…"
        store.connect(host: host, port: portValue, secret: secret) { result in
            DispatchQueue.main.async {
                isConnecting = false
                if case let .failure(error) = result {
                    status = "Failed: \(error.localizedDescription)"
                } else {
                    status = "Paired. Waiting for the host catalog…"
                    try? PairingSecretStore.save(secret, for: host)
                }
            }
        }
    }
}
#endif
