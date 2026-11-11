# ADR 0005: Keep JSON canonical until protobuf migration gates pass

- Status: **Accepted — defer protobuf adoption**
- Date: 2026-11-11
- Decision owners: Vista maintainers

## Context

Vista currently has an implemented Rust JSON control envelope (`Envelope` / `ControlMessage`) with version-range negotiation, a 64 KiB frame bound, strict per-direction sequence numbers, pairing and session state machines. The documented Swift client contract is hand-written `Codable`; the Rust implementation and Swift models are not generated from a shared schema. The JSON fixtures are small and already exercise the current contract.

The protobuf proposal is materially more than a serialization replacement. It describes a future authenticated TLS/WebRTC architecture, a different bootstrap/enrollment flow, new target and grant identities, capability/permission snapshots, input leases, and multiple data channels. It is not a field-for-field encoding of the current JSON messages. Adopting it now would implicitly approve unresolved security (#59/#77), provider (#68/#71), and transport/migration (#64) decisions.

The #79 candidate evidence reports pinned `protoc`/Buf/prost/JavaLite/swift-protobuf generation, Rust and JavaLite tests, and macOS Swift CI. The JavaLite test is JVM wire parsing, not Android SDK/Gradle or Skip runtime interoperability. The candidate README explicitly excludes those mobile runtime gates. No end-to-end Rust/Swift/Android semantic parity is demonstrated. The candidate schema and tests live on the draft PR branch, not the current `main` checkout at the time of this decision. On this host, `protoc` is not installed (`protoc --version` returns command-not-found); therefore this decision does not claim a local schema compile.

## Decision

**Keep the implemented JSON envelope canonical for the existing v1 prototype. Keep protobuf as a non-canonical design candidate. Do not migrate production host or clients and do not close issue #78 as a completed cross-language conformance gate.**

This is an evidence-based deferral: the candidate has compiler and partial language-wire evidence, but it proposes a new domain contract and security/transport architecture, while the existing implementation is a functioning Rust JSON protocol with a hand-written Swift peer. The current evidence does not prove that protobuf-generated models can replace both client/server semantics safely, especially on Android/Skip. A schema that compiles is not sufficient proof of protocol equivalence or authorization correctness.

## Semantic comparison

| Concern | Current JSON protocol | Protobuf candidate | Assessment |
|---|---|---|---|
| Version negotiation | `ClientHello.versions` range; host selects highest intersection; mismatch fails | `BootstrapHello.supported_versions`, selected version and feature lists | Similar goal, different handshake and no direct adapter |
| Unknown fields | Serde's current typed decoding; documented additive-field tolerance is not a binary relay guarantee | Protobuf binary parsers can retain/skip unknown fields depending on runtime; unknown required features need explicit rejection | Different compatibility semantics; must be tested per runtime |
| Request idempotency | Action messages have IDs; host dispatcher caches 1,024 results for 60s and rejects conflicting ID reuse | `ControlEnvelope.request_id` plus proposed bounded cache and body-digest mismatch rejection | Candidate rule is not a wire-level guarantee and is not equivalent to existing action-only cache |
| Connection/lease epochs | `connection_epoch`, `grant_epoch`, session IDs, target generation, geometry epoch and revocable `InputLease` are present in action/capability/input contracts | `connection_id`, grant epochs, target generations, explicit `LeaseRequest`/`LeaseDescriptor`, input lease IDs | Stronger and broader proposed model; requires security/authorization implementation, not serialization mapping |
| Pairing/security | 256-bit out-of-band PSK, HMAC/HKDF, prototype encrypted record layer | QR-pinned TLS enrollment, CSR-issued client identity and mTLS signaling | Architectural replacement, not a message encoding change |
| Transport/framing | 4-byte length-prefixed JSON on reliable ordered TCP; 64 KiB max and sequence per direction | One protobuf message per WebRTC data-channel message; transport owns message boundary | Requires a separate transport migration |

The live JSON model includes `client_hello`, `server_hello`, pairing request/accepted/rejected, session open/opened/close, media offer, keyframe request, ping/pong/error, capability snapshot, action invoke/result, catalog snapshot/request, stream select/start/stop/error and input event. The candidate instead defines bootstrap/signaling, control, input and clipboard envelopes around a substantially expanded domain model. Similar concepts (version, session, catalog, action, input) do not make the fields or lifecycle interchangeable.

## Consequences

- `docs/protocol.md` remains the source of truth for the existing implemented JSON v1 prototype.
- `docs/design/canonical-protocol-spec.md`, `docs/design/wire/vista-v1.draft.proto`, and `docs/design/wire/golden-v1.json` are explicitly design-only and not canonical or production-ready.
- Keep the candidate compiler/conformance CI work and its generated artifacts isolated. CI success is evidence for the exact tested generator/runtime matrix, not permission to migrate.
- No generated Rust/Swift/Android model is claimed to be in use by production clients.
- Issue #78 remains open until the migration preconditions below are met or the issue is explicitly re-scoped to candidate-only conformance.

## Preconditions to reconsider adoption

1. Resolve and review security enrollment and channel identity decisions in #59/#77; demonstrate real Rust–iOS–Android TLS identity interoperability.
2. Resolve provider identity, capture provenance, permission and lease contracts in #68/#71; map every live JSON message and semantic invariant to a versioned protobuf contract, documenting intentional removals.
3. Prove Android SDK/Gradle and Skip runtime compatibility, in addition to JVM JavaLite parsing; run native Swift/macOS tests and Rust tests from clean CI.
4. Add independent cross-language semantic fixtures for negotiation failures, unknown fields/enums, idempotency/conflicting request IDs, grant/lease/connection epochs, stale target/geometry, malformed/oversized input and no-side-effect rejection.
5. Define compatibility policy, field reservation, Buf breaking checks, pinned generator/toolchain versions and reproducible zero-diff generation for every supported runtime.
6. Approve a staged migration under #64: generated model adapters behind a protocol-version/transport boundary, dual-read or explicit negotiated cutover, client rollout order, rollback criteria, and only then retirement of JSON.

Until every precondition is evidenced, protobuf remains a candidate and JSON remains canonical for the implemented protocol.
