# CSR enrollment and mTLS client identity

Status: **conformance target / implementation spike**. This is not an implemented transport or a production security guarantee. Existing Vista pairing remains PSK-based; do not infer mTLS from its authenticated control-record layer.

The machine-readable profile is [csr-enrollment-v1.json](../fixtures/csr-enrollment-v1.json). Rust fixture tests pin its security-critical values and mandatory negative cases. Host and clients must consume the same profile.

## Trust boundary and sequence

1. User explicitly opens a short-lived host pairing window. A QR carries endpoint hints, protocol version, host SPKI pin and one-use pairing secret.
2. Client validates the host pin before sending pairing material. It creates a P-256 signing key in platform key storage; the private key never leaves it.
3. Client sends a DER PKCS#10 CSR over a restricted enrollment channel. Host validates its signature and P-256 public key, invite expiry/one-use state, and explicit local approval.
4. Host ignores requested CSR extensions as authority. It issues a host-CA-signed leaf with CA:FALSE, digitalSignature and clientAuth, binding the issued public key to the verified CSR key.
5. Client associates the returned leaf/chain with the original private key. It reconnects using TLS 1.3 mTLS and proves possession in the ordinary certificate handshake.
6. Host validates chain, validity, EKU, explicit trust-database membership and revocation. No operational data is served before this succeeds. TLS early data is disabled.
7. Signaling is authorized to that principal. WebRTC's DTLS fingerprint must be bound to the authenticated signaling exchange; certificate identity does not replace DTLS peer verification.

Enrollment TLS and paired mTLS are separate trust states. Do not accept unauthenticated client certs as enrollment, or treat possession of a pairing secret as proof of possession of a future private key.

## Platform/library feasibility and unproven work

### Rust host
- Existing dependencies include ring; there is no rustls or rcgen dependency and no certificate trust store or TLS listener in this checkout.
- Evaluate rustls for TLS 1.3 and rcgen/a maintained X.509 implementation for PKCS#10 parsing, signature verification and constrained issuance. Confirm exact supported versions and CSR verification APIs before committing to an API.
- Issuance must construct extensions from the Vista profile, not copy CSR extensions. Host trust records need durable schema, atomic approval/invite consumption, revocation and identity lifecycle semantics.
- This slice adds no production listener, certificate issuer, verifier or mTLS integration.

### iOS
- Current Keychain storage covers the PSK, not an X.509 identity. SecKeyCreateRandomKey can create a P-256 Keychain key (Secure Enclave availability depends on device/configuration).
- Select and prove a maintained Swift PKCS#10/X.509 facility that signs a CSR using a non-exportable SecKey. Swift Crypto alone must not be assumed to provide a complete PKCS#10 builder/parser.
- Associate the issued certificate with its original key and obtain a usable SecIdentity for NWProtocolTLS. This association and handshake are not demonstrated in simulator/device tests.

### Android
- There is no AndroidKeyStore-backed certificate enrollment or mTLS key manager in the current app.
- Generate P-256 signing keys with AndroidKeyStore and suitable KeyGenParameterSpec purposes/digests; verify that CSR signing and certificate-chain association work without exporting private key material.
- Implement a JSSE X509KeyManager/SSLSocket or maintained TLS integration selecting the certificate matching the non-exportable key. Not demonstrated on emulator/device.

## Required conformance and security tests

The fixture enumerates mandatory fail-closed cases. Production proof must additionally demonstrate first enrollment after approval; reconnect after client/host restart; wrong host pin rejection; revocation on fresh connections and teardown of existing peers/leases; renewal/expiry and host-key-change behavior; and mutated SDP fingerprint rejection.

Record OS/API versions, TLS/library versions, certificate chain, key provenance and sanitized handshake result. Never log secrets, private keys, full tokens or session identifiers.

## Scope and decision

This change adds a shared profile fixture and executable Rust conformance tests. It does **not** prove a Rust↔iOS↔Android CSR/mTLS handshake and must not close the security gate. Native platform key/certificate association and cross-platform TLS interoperability remain explicit blockers requiring simulator/emulator/device evidence. No custom HMAC challenge or bespoke cryptographic primitive is an acceptable substitute.
