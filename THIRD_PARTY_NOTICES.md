# Third-party notices

Vista's own source is licensed under Apache-2.0 (see [LICENSE](LICENSE)). This
file records the third-party components the tracked code builds against and
their licenses. It is a summary, not a replacement for the license texts that
ship with each dependency.

Snapshot: 2026-11-12, generated from the committed lockfiles.

## Rust host (`host/`)

Regenerate with:

```sh
cargo metadata --format-version 1 --offline \
  | jq -r '.packages[] | select(.source) | "\(.name) \(.version) \(.license)"' \
  | sort
```

### Direct dependencies

| Crate | Version | License |
| --- | --- | --- |
| `ashpd` | 0.13.13 | MIT |
| `enumflags2` | 0.7.12 | MIT OR Apache-2.0 |
| `futures-util` | 0.3.34 | MIT OR Apache-2.0 |
| `gstreamer` | 0.24.5 | MIT OR Apache-2.0 |
| `gstreamer-app` | 0.24.5 | MIT OR Apache-2.0 |
| `gstreamer-video` | 0.24.5 | MIT OR Apache-2.0 |
| `hex` | 0.4.3 | MIT OR Apache-2.0 |
| `ring` | 0.17.14 | Apache-2.0 AND ISC |
| `serde` | 1.0.229 | MIT OR Apache-2.0 |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 |
| `tokio` | 1.54.1 | MIT |
| `zeroize` | 1.9.1 | Apache-2.0 OR MIT |

### Transitive dependencies

All 139 packages resolved from `Cargo.lock` are permissively licensed: MIT,
Apache-2.0, ISC, Unicode-3.0, Unlicense, or a disjunction of those. No
copyleft-only crate is in the resolved graph. `niri-ipc` is deliberately not a
dependency; Vista talks to Niri over its documented JSON socket protocol
(see [ADR 0004](docs/adr/0004-license-and-niri-ipc-policy.md)).

### Runtime system libraries

The host links the system GStreamer libraries (`gstreamer-1.0`,
`gstreamer-app-1.0`, `gstreamer-video-1.0`) at load time. GStreamer itself is
LGPL-2.1-or-later. Vista does not redistribute GStreamer; it depends on the
distribution's copy, as most Linux desktop applications do. Distributors who
bundle GStreamer must apply its own license terms.

## Swift clients (`client/core`, `client/ios`)

Pinned in `client/core/Package.resolved` and `client/ios/Package.resolved`:

| Package | Version | License |
| --- | --- | --- |
| `swift-crypto` | 3.15.1 | Apache-2.0 |
| `swift-asn1` | 1.7.4 | Apache-2.0 |

## Android client (`client/android`)

Declared in `client/android/Android/gradle/libs.versions.toml`:

| Component | Version | License |
| --- | --- | --- |
| Android Gradle Plugin | 9.2.0 | Apache-2.0 |
| Kotlin | 2.4.10 | Apache-2.0 |
| AndroidX Activity Compose | 1.13.0 | Apache-2.0 |
| AndroidX Compose BOM / UI / Material 3 | 2026.05.01 | Apache-2.0 |
| `org.json:json` | 20250517 | Public domain |
| JUnit 4 | 4.13.2 | EPL-1.0 (test only) |

## Python MCP server (`integrations/vista-computer-use`)

See `integrations/vista-computer-use/pyproject.toml`. It is a development and
automation integration, not part of the host or client artifacts.

## Documentation and design references

`docs/sill-reference.md` maps Vista UI work to upstream Sill source. Sill is
Apache-2.0. No Sill source was vendored at the snapshot above; anything adapted
later must keep its copyright notice and be recorded here.
