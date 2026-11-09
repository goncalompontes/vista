# Vista security architecture and threat-model review

Status: **design candidate, not security-audited or implemented**. Companion [canonical protocol](canonical-protocol-spec.md), current [lifecycle-security draft](lifecycle-security.md), #59, #58, #60, and proposed portable-host ADR #67. This document supersedes *security design intent* in the prototype PSK/ChaCha proposal, not its historical test results. Do not delete old diagnostic tests until #64's migration gates.

## Security objectives and explicit non-goals

1. Unpaired clients cannot list private windows, receive media, inject input, read clipboard or launch apps.
2. A paired credential does **not** itself grant capture, input, audio or clipboard rights. Each effect checks Vista policy **and** real desktop/portal authorization where applicable.
3. Network confidentiality, peer authentication and forward secrecy come from maintained TLS 1.3 + WebRTC DTLS-SRTP/SCTP stacks, not Vista-invented AEAD framing.
4. A MITM cannot swap the chosen host identity, SDP DTLS fingerprint or permission scope during pairing/signaling.
5. Revocation, stale target generations, expired control leases and provider closure stop effects and retire held inputs/resources predictably.
6. The remote client must be honest about whether its control can affect the *whole shared desktop*, not just the streamed window.
7. No claim that Vista can secure the user's machine against a malicious program running with the host user's UID or a compromised host OS. A private Wayland socket is not a sandbox; host-user file/network/D-Bus access can remain.

## Assets and trust boundaries

| Asset | Owner / primary controls | Boundary and failure |
| --- | --- | --- |
| Host identity private key | Host identity service | User-restricted key storage; stolen host key enables impersonation, recovery is explicit |
| Paired client key/certificate | iOS Keychain / Android Keystore | Device compromise or revoked device needs server-side immediate revocation |
| Pairing invitation | Host UI/service | Short-lived single-use secret plus out-of-band pinned host identity |
| Discovery endpoint | LAN mDNS / manual / Tailscale | Address hint only; can be spoofed, never authorizes |
| Catalog/window titles | Host policy + catalogue provider | Private metadata; no pre-auth disclosure |
| Capture portal grant | Native desktop + Vista policy | User selection and source type, no catalog-target implication |
| Input lease | Host policy + real InputProvider | Scope can be whole desktop, input held-state must be released |
| Clipboard/audio | Independent provider/grant | Sensitive even when video/input authorized separately |
| Managed app process | SessionRuntime | Private Wayland socket/process control does not imply OS sandbox |
| Media/SDP/ICE | Authenticated signaling + WebRTC | DTLS fingerprint must be bound to verified signaling session, ICE leaks network topology |
| Diagnostics/snapshots | Host/client storage | Never log secrets, window titles, captured pixels or clipboard content |

Adversaries: passive LAN observer; active network MITM/rogue mDNS; unauthorized app/client; compromised previously paired device; malicious remote paired user attempting to escape grants; malicious or crashing portal/backend; malformed/fuzzed client data; lost device; stale client after restart; local same-UID process (outside strong isolation guarantee). Trust anchors: only verified out-of-band host pin, private keys, native permission/consent decisions and host policy; Tailscale or Bonjour membership alone is *not* a trust anchor.

## Preferred identity model (review before implementation)

**Host:** generate a long-lived asymmetric signing identity and TLS certificate on first launch. Pin the host **SPKI/public identity fingerprint**, not a hostname or mDNS label. Maintain a distinct, auditable key-rotation path. A TLS leaf may rotate if the same authorized pinned identity is cryptographically represented and the pin policy is explicit; do not silently accept a different host key. Use system-vetted signature/key APIs; no bespoke certificate or signature primitives.

**Client:** generate a device-bound asymmetric identity; preferred enrollment mechanism is a client certificate or proof of possession that can be used for **mTLS on subsequent sessions**. On iOS investigate Keychain/SecIdentity and P-256/Secure Enclave support; on Android use Keystore and a maintained TLS client path. Using an unknown client key string inside a JSON/protobuf field is **not proof of possession**. The host stores only client public identity, approved label/permissions, revocation state and timestamps.

**Remaining security engineering gate:** choose a concrete enrollment proof-of-possession and certificate issuing/acceptance path that is actually interoperable between Rust TLS + Swift Network.framework + Android. Preferred: self-generated client certificate/private key, enrollment records the certificate/SPKI after the live TLS stack has verified possession; normal connections authenticate that exact enrolled identity with mTLS. If an anonymous initial pinned-TLS enrollment cannot obtain a valid client TLS certificate identity with the selected stack, explicitly document a standard challenge-signature alternative and obtain review before implementation. Do not silently switch to reusable shared PSK authentication.

**Transport binding:** signal SDP/ICE over the already pinned and authenticated TLS session. Bind each `negotiation_id`, `authorized_session_id` and SDP DTLS fingerprint to a single authenticated client/host principal and session; rely on the WebRTC library to verify the certificate matches fingerprint received over authenticated signaling (RFC 8842). No pixels/data channel effects before that binding is complete. Do **not** pin a permanent WebRTC DTLS certificate as if it were the host long-term identity; ephemeral DTLS certificates are allowed if authenticated by signaling.

## Pairing flow: PINNED TLS + one-time secret + local approval

QR content (conceptual, canonical encoding to be specified): scheme `vista-pair:`, protocol version/range, endpoint hints, host identity pin, random 128-bit invitation ID, random **256-bit invitation secret**, creation and expiry timestamps. Never include user/password, window list, private key or a reusable long-term client credential. Invite state is host-side, max lifetime **2 minutes** proposed, one use, maximum 3 concurrent invites and 5 failed validation attempts per invite (tunable policy with tests). QR may appear on host UI only after an explicit user gesture. A manual flow requires endpoint + full pin + long secret; no 6-digit authentication code without a reviewed PAKE.

```mermaid
sequenceDiagram
  participant HUI as Host UI
  participant HS as Host service
  participant C as Client
  participant TLS as TLS + identity verifier
  HUI->>HS: Open pairing window (explicit)
  HS-->>HUI: QR(invite_id, random_secret, pinned_host_id, expiry)
  C->>C: Scan + validate bounds + pin format
  C->>TLS: TLS 1.3 connect; check host SPKI pin before sending secret
  TLS-->>C: Verified host TLS peer
  C->>HS: BootstrapHello(version range)
  C->>HS: PairingEnroll(invite_id, secret, device credential)
  HS->>HS: Rate limit + atomic single-use claim + credential proof
  HS-->>HUI: Display untrusted device label; ask local approval
  HUI->>HS: Approve/deny
  HS-->>C: PairingResult via verified TLS channel
  C->>C: Discard invite secret; store own private key/host pin
  C->>HS: Reconnect with enrolled client authentication
  HS-->>C: Authorized principal (no portal grant yet)
```

**Important:** approval and credential enrollment must be atomic. A pending invitation may not be reused by another public key/client while local approval is showing. Approval expiry cancels the reservation. Network attackers must not be able to race the credential into an already-approved slot. Wrong pin: terminate before secret submission. Wrong/expired invite: generic denial, no secrets/identity in logs. Pairing success permits catalog only if the host's `catalog.view` policy explicitly allows it.

## Ordinary connection + media authorization

```mermaid
sequenceDiagram
  participant C as Paired client
  participant TLS as Authenticated TLS signaling
  participant P as Host policy
  participant Portal as Desktop permission provider
  participant W as WebRTC session
  C->>TLS: Client-authenticated TLS; verify pinned host
  TLS->>P: principal + protocol/feature negotiation
  P-->>C: Authorized bootstrap response
  C->>TLS: SDP offer / ICE tied to session ID
  TLS->>P: Check paired identity and admitted session
  P->>W: Bind remote DTLS fingerprint to authenticated SDP
  W-->>C: Verified DTLS/SRTP/data channel
  C->>P: Request single WINDOW capture
  P->>Portal: Ask desktop-native consent for WINDOW only
  Portal-->>P: Approved source kind/window/grant epoch
  P->>P: Check actual scope and source identity
  P->>W: Start media from approved source
  W-->>C: Decoded video (separate audio grant)
  C->>P: Request scoped input grant and control lease
  P->>Portal: Independent input consent (if available)
  P-->>C: Actual scope + lease, or view-only refusal
```

A previously paired user **cannot bypass the portal**. Persisted portal permission/restore tokens are native-sensitive state scoped to a specific principal/desktop/source and are never treated as global Vista rights. No automatic escalation from WINDOW to MONITOR, or from window-scoped expectation to shared-seat input.

## Permission matrix

| Operation | Vista policy | Native grant / scope | Default |
| --- | --- | --- | --- |
| Catalog view | Paired + host-approved | Provider availability; metadata may leak titles | Restricted |
| Video capture | Target/source + epoch | ScreenCast user approval, exact selected kind | New capture consent |
| Audio | Separate media grant | Actual system/app/session audio source | Off |
| Input | Lease + scope + epoch | RemoteDesktop/EIS shared seat or managed-session input | View-only |
| Desktop actions | Explicit action grant | Typed allowlisted provider | Off |
| Clipboard host -> client | Separate read grant | Compatible portal session/ownership | Off |
| Clipboard client -> host | Separate write grant | Compatible portal session/ownership | Off |
| Managed app launch | Allowed app ID + policy | Supervisor's process/cgroup ownership | Off |
| Terminate managed session | Owner/authorized control | Only owned process tree | Confirm |
| Window close / destructive action | Typed action + revision | Shared-desktop side effect possible | Confirm as appropriate |

Permission state is an intersection: **paired AND Vista policy AND currently approved provider grant AND fresh target/grant generation AND (where input) control lease**. An unauthorized result must not issue side effects or reveal sensitive target metadata.

## Revocation semantics and cleanup

Events: user revoke/forget, host revokes device, portal closure, lease expiry, provider crash, desktop restart, app exit, client disconnect/background transition, detection of lost authenticated binding.

1. Mark permission epoch revoked and atomically prevent **new** effects before sending UI updates.
2. Cancel pending native capture/input/clipboard operations; stop WebRTC affected tracks, detach PipeWire sources and invalidate grant/capture handles.
3. Release all held buttons/keys/touch contacts **even when rate limits are exceeded**; if the backend cannot release, invalidate lease and report cleanup failure, never claim it succeeded.
4. Revoke session and close authenticated connections on paired device revocation; new connections reject that identity. In-flight network buffers must be dropped and a new connection needs fresh snapshots.
5. Managed sessions: policy decides stop immediately versus short grace period after disconnect, but never permit orphaned externally controllable sessions; record owning process metadata to avoid killing unrelated user processes on recovery.
6. Client: immediately disable input/clipboard UI on permission update or transport disconnect; suppress speculative action resend and wipe local sensitive previews according to retention policy.

Use cancellation tokens, task ownership, deadlines and idempotent cleanup. **Security invariant:** after revocation is committed, no fresh native side effect may occur. A side effect already executing at the boundary may need to be cancelled/rolled back; document unavoidable in-flight races and measured propagation latency rather than promising mathematical instant cessation.

## Threat cases and acceptance tests

| Threat | Required prevention/detection | Required test fixture |
| --- | --- | --- |
| Rogue mDNS host / MITM | Validate QR/saved host SPKI pin in TLS before sending invite | Swap endpoint/cert; assert no secret or media transferred |
| QR secret replay or racing enrollment | Single-use atomic reservation, expiry, rate limits and local approval | Two clients use one invite concurrently; at most one approved |
| Stolen but unapproved QR | User explicitly approves key-bound client identity | Reject remote request without host approval |
| Spoofed enrolled client | Prove private-key possession via chosen standard TLS identity | Copy client public key only; no connection |
| Device revoked while streaming | Host-side principal revoke closes sessions/tracks | Input, media and clipboard terminate; new request denied |
| TLS 0-RTT replay | Disable early application data for enrollment/actions | Replayed early data cannot execute action |
| SDP/DTLS fingerprint swap | Authenticated signaling + DTLS cert check | Mutate offer fingerprint; media never starts |
| Unauthorized screen mode escalation | Approved source kind must equal WINDOW request | Portal returns MONITOR; reject and close |
| Catalog/source confusion | Verified binding only, else picker-only target | Same title/PID two windows; no fabricated binding |
| Shared-desktop input exposed as window | Actual input scope advertised and consented | Refuse private-window guarantee for seat-wide input |
| Expired lease, stale generation | Revalidate before native effect | Reuse ID after compositor restart; reject |
| Input held on disconnect | Release-all on every teardown path | Key-down then disconnect; native release verified |
| Clipboard transfer after revoke | Separate grant, byte limits, cancellation | Revoke mid-chunk; stop and clear |
| Poisoned metadata / oversized message | Bound, parse, sanitize, localize by keys | Fuzz titles, SDP, nested proto, unknown enum, unicode |
| Same-UID managed app access | Don't claim a sandbox; document residual risk | Explicit isolation tier and permission audit |
| Log leaks | Central redaction and structured correlation | Inject recognizable secret/title/text; ensure absent in logs |

No penetration-testing proof is claimed by this design. Implementers must add Rust/Swift/Android protocol fixtures, a local simulated identity/trust store, hostile-peer tests, and opt-in physical desktop/device verification. CI must prevent nonproduction insecure bypasses entering release configuration.

## Host/user lifecycle edge cases

- Lost host identity key: surface "host identity changed", invalidate all trust and require re-pair; no silent TOFU.
- Pairing approval shown while app backgrounds: invitation times out; no future surprise approval.
- Client offline during revocation: host trust DB denies on reconnect; client stale local UI cannot grant access.
- TLS resumption: standard library may resume authenticated session, but **never use TLS 0-RTT application commands**. Re-check current server-side revocation and policy on each new connection.
- ICE reconnect/Tailscale IP change: fresh authenticated signaling identity required; transport migration does not change principal or restore revoked grants.
- Client forget host: delete local host pin and cached credentials/previews; host-side revocation is separate and should be offered clearly (forget alone is not proof host removed trust).
- Conflicting simultaneous controllers: one input lease owner per scope, explicit busy/revoke/transfer policy; view-only observers remain separately authorized.
- Unknown/older clients: reject unknown critical security features; never downgrade to legacy PSK/TCP in production.

## Engineering decisions already fixed by this proposal

- TLS 1.3 preferred with host identity pin and standard client-auth; WebRTC DTLS-SRTP/data-channel security, no custom AEAD.
- High-entropy one-time invite, local approval and fresh paired client credentials; no unverified six-digit PIN flow.
- Discovery/mDNS/Tailscale address is not identity or authorization.
- Native portal grants remain separate from pairing and per-session Vista policy.
- Default view-only and no silent monitor or whole-desktop input escalation.
- No action input replay after reconnect; every native effect checked at point of execution.

**Review blockers (not delegated to worker discretion):** exact Rust/iOS/Android interoperable mTLS enrollment/proof mechanism; key rotation/recovery policy; certificate validity/pinning semantics; decision on input-lease expiry/revocation timeout and managed-session disconnect grace. Record an approved security ADR before implementation #59 is closed.

## Worker milestones (#59)

- S1: threat model review, formal permission/state invariants and identity library feasibility spike.
- S2: pinned TLS host identity + QR invite lifecycle + no-TLS-0-RTT policy; headless tests.
- S3: client key enrolment with verified possession + local approval + persisted trust; Swift/Android parity.
- S4: authenticated signaling and SDP/DTLS fingerprint binding with negative MITM fixtures.
- S5: independent portal grants + native security scoping + control leases, revocation and cleanup.
- S6: audit hostile inputs, logging, storage/rotation/recovery, hardware evidence and explicit documented residual risks.

## References

- RFC 8446, TLS 1.3 and early-data replay: https://www.rfc-editor.org/rfc/rfc8446
- RFC 8827, WebRTC security architecture: https://www.rfc-editor.org/rfc/rfc8827
- RFC 8842, SDP/DTLS certificate fingerprint binding: https://www.rfc-editor.org/rfc/rfc8842
- Apple Secure Enclave/key constraints: https://developer.apple.com/documentation/Security/protecting-keys-with-the-secure-enclave
- Apple Network.framework TLS identity: https://developer.apple.com/documentation/security/sec_protocol_options_set_local_identity(_:_:)
