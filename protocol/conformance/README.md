# Vista protobuf conformance candidate (issue #78)

This is **experimental validation**, not a production transport migration or security approval. It is stacked on PR #76 (`docs/protocol-security-adapters-client-specs`). The schema is copied from its design draft; generated types are isolated from host/client code.

## Reproduce

From repository root:

```sh
python3 protocol/conformance/scripts/tools.py
python3 protocol/conformance/scripts/generate.py
protocol/conformance/.cache/buf-1.50.0 lint
protocol/conformance/.cache/buf-1.50.0 format -d --exit-code
PROTOC="$PWD/protocol/conformance/.cache/protoc-29.3/bin/protoc" cargo test --locked --manifest-path protocol/conformance/rust/Cargo.toml
python3 protocol/conformance/scripts/test_jvm.py
```

Pinned artifacts/sha256: `toolchain.lock.json`; Rust dependency resolution: `rust/Cargo.lock`. `fixtures/golden.tsv` comes from `protoc --encode`, not manually-encoded wire bytes. `scripts/generate.py` also verifies the original design fixture set decodes, exercises additive v1.1 compilation, and compares schema tokens against the design draft. Generated `rust` and `java` sources are compiler outputs. Generated schema descriptors are not production artifacts.

Wire transport: one binary protobuf envelope per reliable ordered WebRTC data-channel message, no custom length prefix/framing; message size bounds and valid state/authorization remain application responsibilities. Protobuf decode success does **not** imply permission to perform a native side effect. Reject unknown required features, unspecified/unknown privileged enums, missing required domain identities, stale epochs, invalid signatures, oversized/ill-formed messages at the authenticated policy boundary.

## Coverage and exclusions

The Linux host can run Rust/prost and Java/Javalite tests. JVM tests exercise Android-compatible JavaLite **wire parsing**, not Android SDK/Gradle/Skip integration. A Swift test target is supplied; native Swift execution requires an installed compatible Swift toolchain and swift-protobuf generator. An iOS Simulator/macOS test and Android app build are separate required acceptance gates. No native mobile-platform success is implied by passing Rust/JVM. Security #59/#77, provider #68/#71 and schema freeze need separate review before issue #64 migration.
