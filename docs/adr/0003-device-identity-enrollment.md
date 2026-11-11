# ADR 0003 — Device identity enrollment and authenticated signaling

Status: **proposed after security design review (2026-11-09)**. Technical recommendation, **not yet verified with real iOS/Android TLS stacks**. Implementing #59 requires a successful cross-platform identity fixture/handshake spike and security review.

## Context

Vista requires: an out-of-band verified host identity, a one-time pairing invitation and local approval, durable device credentials, authenticated signaling for WebRTC and prompt server-side revocation. Standard TLS/WebRTC should provide transport security and forward secrecy. The earlier text proposed mutual TLS without specifying how mobile clients obtain usable X.509 identities.

A publicly presented client public key in `PairingEnroll` does **not** prove key possession. Making *every* TLS connection require a client certificate would also deadlock first-time enrollment: an unpaired device has no certificate yet.

## Proposed decision: two distinct TLS authorization paths and X.509 enrollment

Use a single long-lived pinned host TLS identity and a host-controlled, local client-certificate issuing authority (CA). The host CA key is distinct from the host TLS identity key and is stored only on the host. Use standard ECDSA P-256 + SHA-256, PKCS#10 CSR and X.509 client-auth certificates, then standard mutual TLS for subsequent signaling. No custom record cipher, PAKE or application-layer reusable secret is introduced.

**Transport TLS min TLS 1.3**, no application TLS 0-RTT. A QR pin is the host TLS *SPKI SHA-256* fingerprint. On ordinary connects, clients validate the saved pin and host certificate according to the selected pinning policy, even if DNS-SD/manual/Tailscale endpoint changes. For first pairing, the QR pin is validated **before** sending the one-time invitation secret. Host identity rotation must be explicit; a TLS leaf renewal with the same pinned SPKI may remain valid subject to validity/purpose checks and a documented policy.

### Path A — enrollment listener / enrollment ALPN or route

An explicitly enabled, rate-limited, pinned-TLS endpoint accepts **no client certificate initially** and exposes **only** version hello, invitation redemption, CSR submission and local-approval status. It never exposes catalog metadata, media, input, action, clipboard, session creation, WebRTC signaling or arbitrary RPC methods. Use one endpoint with strict pre-auth routing or separate listeners; pick one topology before implementing but **do not** share an unrestricted handler with paired-client endpoints.

1. User opens a pairing window locally; host creates 128-bit random invitation ID + 256-bit cryptographic invite secret, finite expiry (proposed two minutes), one-use attempt reservation; GUI displays QR with endpoint, host SPKI fingerprint, ID, secret and expiry.
2. Client verifies QR pin against the live TLS peer **before** sending secret. Client generates an **app-local non-exported private key** (P-256) and a standard PKCS#10 CSR; CSR signature proves possession of corresponding private key.
3. Client sends `PairingEnroll { invitation_id, one_time_secret, client_certificate_request_der, device_label }` over pinned TLS. Host validates expiry/attempt cap, atomically reserves the invitation, bounds/validates DER, verifies CSR signature/key algorithm, and associates CSR SHA-256 fingerprint with the pending approval. Do not trust requested CSR subject/SAN/EKU/CA attributes; host derives its own fixed certificate template.
4. Local host UI shows a sanitized client label and invitation fingerprint, asks approval explicitly. Approval is bound to the exact reserved CSR and client session; timeout/denial cancels and consumes the invitation. Concurrent enrollment cannot swap the key.
5. On approval, host issues a constrained X.509 end-entity certificate signed by its private client CA. Enforce `CA:FALSE`, `keyUsage=digitalSignature`, `extendedKeyUsage=clientAuth`, short lifetime (proposed **90 days**), randomly unique serial, host-issued opaque device ID. Return cert + CA chain via the pinned TLS enrollment channel. **Never return a private key**.
6. Client associates certificate with already generated private key, stores a retrievable TLS identity (iOS Security/Keychain, Android Keystore/X509KeyManager), discards invite secret and closes enrollment connection.
7. Client reconnects on **Path B** using standard mTLS with new certificate; only a successful TLS-authenticated principal may proceed to signaling.

**Cross-platform qualification:** Apple `SecIdentity` requires a certificate corresponding to the accessible private key; Android Keystore creates keypairs/certificates but TLS client identity selection needs a working JSSE KeyManager path. A successful CSR issuance alone is not enough: prove `SecIdentity` construction, host mTLS handshake and restart persistence on iOS/Android. iOS Secure Enclave keys may need a specific identity/CSR adapter and are optional until compatibility is proven; never export a hardware-backed private key to force PKCS#12 import.

### Path B — authenticated signaling

- The host requires a client certificate validated to its pinned local CA, with acceptable EKU/usage/expiry. After TLS validation, the host must look up device ID/certificate public key in **its own approval/revocation database** on every new signaling connection; a valid certificate signed by its CA is not automatically an authorized active device.
- The host authenticates the caller before parsing catalog/control/signaling requests. If revoked, close with a generic error; do not disclose target metadata.
- Normal pairing token no longer works here. Renewal uses either a fresh authenticated mTLS session with a new CSR signed by the current authorized client key (policy reviewed), or explicit local re-pair after identity loss; old cert is invalidated in the trust database.
- The host creates `authorized_session_id`/`connection_id` after mTLS. Bind SDP/ICE negotiation ID and offered DTLS fingerprints to that authenticated principal and current session; the WebRTC library checks peer DTLS certificate against authenticated SDP fingerprint. Revoking principal invalidates signaling, PeerConnection/media/data, portal grants, leases and outstanding session authorization.
- On session resumption, recheck the server-side revocation database. Disable TLS early application data and fail closed on identity/pin mismatch.

### Backend libraries to evaluate (not selected versions)

- Rust `rustls` client certificate verification and managed per-host CA trust; `rcgen` / standard X.509 parser for CSR signature verification and issuance. Host must **strip all client-supplied requested extensions**, set certificate extensions itself, bound CSR size, and reject unsupported signing algorithms.
- Apple Security `SecKeyCreateRandomKey`, `SecKeyCreateSignature`, `SecIdentityCreateWithCertificate`, `sec_protocol_options_set_local_identity` and a maintained PKCS#10/X.509 encoding library such as `apple/swift-certificates`/SwiftASN1 if compatible. Confirm it can sign CSR using the actual Keychain/Secure Enclave private-key handle.
- Android `AndroidKeyStore` P-256 keypair, certificate association and JSSE `X509KeyManager` on the chosen TLS/WebRTC signaling client; validate key usability when the app backgrounds or the screen locks.
- iOS and Android must be tested in native process, not inferred from Rust-only fixtures or pure Swift/Skip model builds.

## Key and certificate lifecycle

- Pin **host SPKI** separately from client certificate-issuing CA; never use mDNS name or IP as authentication.
- Host TLS certificate renewal with same SPKI requires current validity, key usage and host policy; new host SPKI requires explicit user action/re-pair, not automatic TOFU.
- Host CA root compromise implies unauthorized client cert issuance; revocation DB still controls admitted principals but must be protected. Consider optional offline CA hierarchy only if operational requirements justify it; initially host CA stored with restrictive filesystem permissions.
- Client certificates expire and must renew before expiry via authenticated flow; the host must not accept expired certificates even with valid DB entry.
- Host approval records store immutable credential identifier, fingerprint, permissions, issuer/serial, created/revoked status, expiry and audit correlation IDs; do not store client private keys.
- Revocation commits database transaction **before** closing current sessions; fresh new effects are denied immediately, in-flight effects cancelled, affected media/data channels closed, held keys/buttons released.
- When host CA identity or trust store is lost, all client trust is invalid; require explicit re-pair rather than silently rebuilding trust.
- Do not promise user-controlled device revocation from the iOS local “Forget” button; forgetting local keys and revoking host trust are different operations.

## Required wire changes from earlier candidate

- Replace `PairingEnroll.client_public_key` with `client_certificate_request_der`; raw public-key transfer is not sufficient proof.
- Pairing response must support `PAIRING_PENDING_LOCAL_APPROVAL`, `PAIRING_APPROVED`, `PAIRING_DENIED`, `PAIRING_EXPIRED`, and on approval return `client_certificate_der` plus constrained `issuing_ca_certificate_der`/chain and host-issued device ID. The client private key is never on wire.
- Explicitly separate `BootstrapHello` on enrollment route from already paired, mTLS-authenticated signaling route. A paired client must not be allowed to re-register another identity by a generic control message.
- Candidate schema is still uncompiled; generated Rust/Swift/Android fixtures and byte limits must be validated before adopting field numbers.

## Security properties and negative tests

- Wrong host QR pin -> **no invite secret ever transmitted**; TLS fails closed.
- Two clients race same invite -> exactly one immutable CSR reservation; denial/timeout consumes the invitation.
- CSR with invalid signature, unsupported algorithm/curve, oversized DER, malformed ASN.1 or attacker-requested CA/EKU privileges -> reject, no certificate.
- Host approves CSR A but network presents CSR B -> no issue; approval cannot swap keys.
- Cert from another host CA, expired or wrong-EKU cert, revoked DB principal -> no signaling/media.
- A copied client cert/public key with no private key -> mTLS fails.
- Remote endpoint spoof during reconnect -> pinned host fails; ICE/SDP fingerprint swap -> verified WebRTC DTLS rejects.
- Revoke while holding key/button and streaming -> input release-all, closure and no new side effect.
- Successful cert issuance but iOS cannot build `SecIdentity` -> **spike fails**, approach remains proposed; do not ship a software-key export workaround without explicit security review.

## Rejected/deferred alternatives

- Plain client-provided public key as “auth”: no proof of possession.
- Permanent shared PSK or homemade AEAD handshake: poor rotation/revocation and reintroduces a custom security protocol.
- QR secret alone on every reconnect: bearer credential; unsuitable long-term trust.
- Custom signed-challenge login: may be viable over pinned TLS but is an application auth protocol with new design/audit requirements; only revisit if native mTLS is unavailable.
- External CA / accounts / cloud pairing: undesirable for Vista local-first user experience.
- Requiring Secure Enclave for every device: excludes emulator/Android/older devices and does not establish library interoperability.

## Implementation gate / approval

Accept as the **preferred design for a dedicated proof-of-concept**. Do not label the identity system “fully resolved” until worker agent demonstrates: pinned TLS enrollment, DER CSR parsing/signature verification, local approval, constrained certificate issuance, iOS `SecIdentity` use, Android Keystore TLS identity, rustls mTLS/DB revocation and SDP fingerprint binding. Any divergence needs architecture/security review; no unreviewed fallback auth mechanism.

## References

- TLS 1.3 RFC 8446: https://www.rfc-editor.org/rfc/rfc8446
- PKCS#10 CSR RFC 2986: https://www.rfc-editor.org/rfc/rfc2986
- X.509 PKIX RFC 5280: https://www.rfc-editor.org/rfc/rfc5280
- Rustls client verifier: https://docs.rs/rustls/latest/rustls/server/struct.WebPkiClientVerifier.html
- Rcgen CSR signature verification and signing: https://docs.rs/rcgen/latest/rcgen/struct.CertificateSigningRequestParams.html
- Apple Security identity: https://developer.apple.com/documentation/security/identities
- Apple Swift X.509: https://github.com/apple/swift-certificates
- Android KeyGenParameterSpec: https://developer.android.com/reference/android/security/keystore/KeyGenParameterSpec
