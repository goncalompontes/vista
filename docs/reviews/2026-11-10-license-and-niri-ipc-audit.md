# Vista license and Niri IPC audit

**Date:** 2026-11-10  
**Scope:** repository state at `feat/end-to-end-mvp` (`15808e7`); issue [#73](https://github.com/goncalompontes/vista/issues/73).  
**Status:** evidence report and recommendation only. This is not legal advice or a maintainer license decision.

## Executive summary

1. The repository currently has no root `LICENSE`, `NOTICE`, `COPYING`, or other repository-wide license grant. `README.md` calls Vista open-source, but the repository does not state the license for Vista-authored code. Do not infer a default license from public visibility or from Sill's license.
2. The published `niri-ipc 26.4.0` crate explicitly declares `GPL-3.0-or-later`. It is not present in Vista's current `Cargo.lock` and is not a direct or transitive dependency in the resolved Rust graph audited here.
3. Vista's current Niri integration uses the `niri` CLI and JSON output (`niri msg ...`, `niri msg --json event-stream`) plus its own Serde models. It does not link `niri-ipc`. An independently authored client for Niri's published JSON IPC socket wire format is a plausible way to avoid adding that GPL-licensed crate, but protocol compatibility, versioning and conformance still need tests. Reimplementing a public protocol does not itself settle copyright questions about copying upstream source; do not copy `niri-ipc` implementation/types wholesale.
4. Current resolved Rust dependencies are overwhelmingly MIT/Apache dual-licensed or MIT/Apache/ISC. The audit found one Rust workspace package with unknown license metadata (`vista-host`, because the project has no chosen license metadata), and one target-specific `r-efi` metadata expression containing `LGPL-2.1-or-later` as an alternative. That expression is not evidence that Vista links or ships LGPL code in its Linux host artifact; it does need to remain visible to a future policy review.
5. Swift package metadata resolves `swift-crypto 3.15.1` and `swift-asn1 1.7.4`, both Apache-2.0. The Android Skip manifest declares Skip and SkipUI; their upstream repositories identify Apache-2.0, but this checkout does not include an Android SwiftPM resolved lockfile or a Gradle dependency lock/report, so the complete transitive Android build graph is not established here.
6. The project needs an explicit maintainer decision before adding `LICENSE`, SPDX headers, a deny-list/allow-list CI policy, or release notices. This change therefore records the evidence and gates rather than silently selecting a license.

## Repository-authored and copied source

- No root `LICENSE`, `NOTICE`, `COPYING`, or `LICENSE.*` file is tracked at the audited commit.
- Rust workspace: `Cargo.toml` and `host/Cargo.toml` contain no `license` / `license-file` package metadata. Cargo reports the workspace package `vista-host 0.1.0` as `license = null`.
- Swift packages (`client/core`, `client/ios`, `client/android`) declare no package-level license metadata in the manifests inspected.
- Python package `integrations/vista-computer-use/pyproject.toml` declares no project license metadata.
- Sill is referenced as inspiration and its design patterns/source are discussed in docs. The checked-in documentation says no Sill implementation source was copied in the inventory/design work and requires Apache-2.0 attribution/notices if source is ported. GitHub identifies Sill's `LICENSE` as Apache-2.0. This audit did not perform a line-by-line provenance comparison against Sill; any future copied/adapted source must be reviewed file-by-file and retain applicable notices and modification statements.

## Niri IPC evidence

### Published crate

`cargo info niri-ipc@26.4.0` downloaded and reported:

- version: `26.4.0`
- repository: <https://github.com/niri-wm/niri>
- license: `GPL-3.0-or-later`

The downloaded normalized crate manifest at `niri-ipc-26.4.0/Cargo.toml` independently contains `license = "GPL-3.0-or-later"`. The upstream Niri workspace manifest at <https://github.com/niri-wm/niri/blob/main/Cargo.toml> sets `workspace.package.license = "GPL-3.0-or-later"` and version `26.4.0`.

This is sufficient to reject treating `niri-ipc` as a permissively licensed dependency. It does not, by itself, decide what obligations attach to a particular use or distribution model. Ask qualified counsel/maintainer to assess any proposed linking or distribution arrangement; do not rely on optional Cargo features, process boundaries, or dynamic linking as an automatic exemption.

### Current Vista integration

At the audited commit:

- `host/src/lib.rs` shells out to `niri msg ...` and parses JSON output into Vista-owned Serde structures.
- `host/src/catalog.rs` consumes line-delimited JSON from `niri msg --json event-stream`.
- `host/Cargo.toml` does not declare `niri-ipc`, and the resolved `Cargo.lock` has no `niri-ipc` entry.

Thus the current implementation does not introduce the crate's GPL license into the Rust dependency graph. It does rely on the installed Niri CLI and its JSON output, with process invocation and output parsing as the current compatibility boundary.

### Recommendation for issue #57

Prefer an independently authored, narrowly scoped Niri JSON IPC adapter behind Vista's compositor-provider seam over adding `niri-ipc` as a normal dependency, **if** the adapter meets the provider's requirements and is validated against the supported Niri versions. The Niri IPC source documents a newline-delimited JSON request/reply protocol over the Unix socket named by `NIRI_SOCKET`; the `EventStream` request switches that connection to newline-delimited JSON events after a `Handled` reply. This is a concrete upstream interoperability surface, but the existence of the socket and crate documentation is not a promise that every schema detail is stable across Niri versions.

Before merging such an adapter, require:

- protocol/source documentation and the exact Niri versions tested;
- request/response and event-stream fixtures captured from supported upstream versions;
- malformed, unknown, and newly added fields to be handled deliberately (forward-compatible where safe, fail-closed where required);
- socket discovery, framing, timeout, reconnect, and compositor-restart tests;
- no copying of `niri-ipc` source or generated definitions without a separate provenance/license review;
- a documented upgrade/conformance process when Niri changes its IPC schema.

The alternative—using `niri-ipc`—should remain an explicit, opt-in decision that first receives a qualified license review and a distribution/notice plan. This report does not claim that the alternative is legally impossible.

## Dependency inventory snapshot

### Rust

Command: `cargo metadata --locked --format-version 1` at the audited commit. Result: **139 resolved packages**, including the workspace package.

- 84: `MIT OR Apache-2.0`
- 32: `MIT`
- 12: `Apache-2.0 OR MIT`
- 3: `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`
- 1: `Unlicense OR MIT`
- 1: `MIT/Apache-2.0` (legacy expression spelling; normalize/verify before policy matching)
- 1: `MIT OR Apache-2.0 OR LGPL-2.1-or-later` (`r-efi`, target-specific dependency metadata)
- 1: `Apache-2.0 AND ISC` (`ring`)
- 1: `Apache-2.0 WITH LLVM-exception`
- 1: `(MIT OR Apache-2.0) AND Unicode-3.0`
- 1: `ISC`
- 1: `UNKNOWN` (`vista-host`, workspace package has no declared license)

Direct Rust dependencies declared by `host/Cargo.toml`: `ashpd`, `enumflags2`, `futures-util`, `gstreamer`, `gstreamer-app`, `gstreamer-video`, `hex`, `ring`, `serde`, `serde_json`, `tokio`, and `zeroize`. No `niri-ipc` dependency is present.

Cargo metadata expresses package license declarations, not a complete source/binary distribution analysis. It does not establish which optional/target-specific code is built into a particular artifact, and it does not replace notice-file inspection or SBOM generation.

### Swift

`client/core/Package.swift` declares `apple/swift-crypto` from 3.15.1. `client/ios/Package.resolved` pins:

- `swift-crypto 3.15.1` (`95ba0316a9b733e92bb6b071255ff46263bbe7dc`), Apache-2.0 per upstream repository license metadata;
- `swift-asn1 1.7.4` (`917b472dbe53ba699cbb0495f9a95a084bc6364d`), Apache-2.0 per upstream repository license metadata.

The iOS app depends on the local `VistaCore` package. The Android Skip package manifest declares `skip` (from 1.9.20) and `skip-ui` (from 1.0.0); both upstream repositories identify Apache-2.0. No Android `Package.resolved` was present, so these are declared lower bounds, not an audited exact resolved graph. A build-time Skip plugin and its transitive dependencies also need to be included in a complete Android artifact inventory.

### Android / Python / native system libraries

- The checked-in Android Gradle project delegates plugin setup to the Skip tool. This checkout has no Gradle version catalog, dependency lock, or generated dependency report available for a reproducible transitive audit. Android SDK, NDK, Kotlin/Gradle plugins, Skip build tooling and native libraries need an artifact-specific inventory before publishing an APK/AAB.
- The Python integration package declares no runtime Python dependencies in `pyproject.toml`; build-system requirement is `setuptools>=68`. Python standard library use is not an external package dependency.
- Host builds dynamically link against system GStreamer/GLib and interact with Niri, Wayland portals, PipeWire and helper executables. Their licenses and the exact contents of downstream OS packages / release images are outside Cargo's dependency graph and need a distribution-specific SBOM/notice pass.

## CI and release controls

The existing Rust and iOS workflows run build/tests only. There is no license scanner, SBOM generation, third-party notice assembly, or dependency-license review gate in the inspected workflows.

Do **not** add a permissive-only `cargo-deny` allow-list yet: that would encode an unapproved project policy and could silently reject or approve dependencies on a false premise. Once the maintainer approves the project license and distribution policy, add ecosystem-specific lockfile scanning (Cargo, SwiftPM, Gradle/Skip, Python), generate an SBOM for each distributable artifact, retain upstream license/NOTICE texts, and fail CI on missing/unknown metadata or newly introduced licenses until reviewed. Make license and provenance evidence a required field in dependency-adding worker issues/PRs.

## Maintainer decisions still required

1. Select Vista's repository/source license and separately define the license policy for shipped host/client binaries and bundled assets. Apache-2.0 is a reasonable proposal consistent with Sill's license and the currently inspected first-party dependency ecosystem, but it is **not adopted by this report**.
2. Confirm copyright ownership/provenance for all existing Vista-authored code and review any copied/adapted source, generated code, logos, fonts, icons, screenshots and bundled assets.
3. Decide whether a single repository license is appropriate for all code, docs, assets and integrations, or whether some paths need explicit exceptions.
4. Obtain qualified review of GPL-3.0-or-later implications before any proposal to link/distribute `niri-ipc` or other copyleft components.
5. After decisions, add the approved license text, package SPDX metadata, third-party notices, and a CI policy that reflects the actual release artifacts.

## Sources

- [niri-ipc 26.4.0 on crates.io](https://crates.io/crates/niri-ipc/26.4.0)
- [Niri upstream workspace manifest](https://github.com/niri-wm/niri/blob/main/Cargo.toml)
- [Niri IPC source](https://github.com/niri-wm/niri/tree/main/niri-ipc), especially [`socket.rs`](https://github.com/niri-wm/niri/blob/main/niri-ipc/src/socket.rs) and [`lib.rs`](https://github.com/niri-wm/niri/blob/main/niri-ipc/src/lib.rs) for framing and request/event definitions
- [Sill LICENSE](https://github.com/Saffsanity/sill/blob/main/LICENSE)
- [swift-crypto LICENSE](https://github.com/apple/swift-crypto/blob/main/LICENSE.txt)
- [swift-asn1 LICENSE](https://github.com/apple/swift-asn1/blob/main/LICENSE.txt)
- [Skip LICENSE](https://github.com/skiptools/skip/blob/main/LICENSE.txt)
- [SkipUI LICENSE](https://github.com/skiptools/skip-ui/blob/main/LICENSE.txt)
- [Apache Software Foundation GPL compatibility guidance](https://www.apache.org/licenses/GPL-compatibility.html)

---

**Limitations:** snapshot audit, not a legal opinion, full source-provenance review, complete Android transitive dependency resolution, or release-artifact SBOM. Re-run against the exact commit and platform artifacts before release.
