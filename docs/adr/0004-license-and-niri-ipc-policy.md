# ADR 0004 — Vista project license and Niri IPC dependency policy

Status: **accepted (2026-11-12)**. The maintainer chose Apache-2.0 for Vista's
original source and the protocol-only Niri integration described below. The root
[`LICENSE`](../../LICENSE), [`NOTICE`](../../NOTICE) and
[`THIRD_PARTY_NOTICES.md`](../../THIRD_PARTY_NOTICES.md) are in place and the
README states the license. Not legal advice.

## Context

Vista is publicly described as an open-source project, but the inspected `main` repository tree has no root `LICENSE`/`COPYING` file. Sill, used as a UX/source reference, is Apache-2.0. The official niri workspace declares GPL-3.0-or-later as its license; `niri-ipc` inherits that workspace package license and is not Rust API-stable. A production linked `niri-ipc` dependency may affect distribution obligations for an otherwise permissively licensed host.

The Apache Software Foundation describes compatibility as **one-directional**: Apache-2.0 code can be incorporated into a GPLv3 work subject to GPLv3 obligations, but a combined GPLv3-derived work cannot simply be distributed as Apache-2.0-only. Linking/derivative-work questions require a project-specific assessment, not assumptions about optional crates, plugins, dynamic libraries or separate processes.

## Recommendation (requires owner approval)

**License Vista's original source under Apache-2.0**, with appropriate SPDX headers and copyright attribution, and use independently written clients for publicly documented platform IPC protocols wherever GPL-licensed helper crates would add distribution complexity. The use of a protocol does not require embedding upstream implementation code. Do not copy GPL source, tests or proprietary schema implementation wholesale; write a fresh adapter from public protocol documentation, small observed fixtures and separately reviewed original code.

Why Apache-2.0:
- Matches the explicit license of the reference project Sill; copied/modified Sill code may remain under Apache-2.0 with preserved copyright and NOTICE obligations. **Do not claim all code can be relicensed as original Vista code.**
- Provides permissive reuse and explicit patent-license terms.
- Retains flexibility for eventual Windows/macOS hosts and alternative client implementations.
- Avoids involuntarily tying every host/SDK consumer to a GPL-linked distribution decision. GPL support can still be added deliberately as a **separate approved distribution policy** later.

Why not default GPL-3.0-only: valid alternative if the maintainer prefers copyleft and close coupling to upstream niri-ipc, but changes project redistributor obligations. It should be an explicit project decision, not an accidental consequence of importing a crate.

## Niri IPC design after this decision

1. In #57, implement `NiriCatalogProvider` and typed action handler over **Niri's publicly documented JSON-over-Unix-socket IPC**. Connect to `$NIRI_SOCKET` and send one JSON request per line/read one JSON reply. Use a separate socket for the ongoing event stream; do not shell out to `niri msg` for each operation.
2. Use `serde` and only narrowly declared Vista-owned request/response structs/variants actually needed by the product, with tolerant unknown fields and unknown enum handling; don't mirror all niri-ipc structures. Note the public JSON protocol promises additions rather than removal/renaming of existing fields, but not total semantic immutability.
3. Pin minimum/maximum supported compositor protocol features with fixture probes; re-establish event subscriptions and generation on restart. The Niri provider is optional: a portal-picker-only Vista host must work with no Niri socket.
4. Keep `niri-ipc` upstream docs as a **technical reference**, not a compiled library/copy-paste source. Add compatibility tests against documented JSON messages and a smoke test on actual Niri.
5. If a worker proposes linking official `niri-ipc`, open a new licensing ADR or amend this one after qualified review of exact binary distribution/copying/linking; do not sneak it in as optional feature.
6. No licensing theory should override security: an IPC adapter is still responsible for bounded input, protected local socket access, unexpected replies, reconnection and event stream cleanup.

## Third-party source and notice inventory

Before applying Apache-2.0:
- Inspect all tracked files and commit history for code copied from Sill or any other external project. Record source commit, original copyright notices, license, modifications, attribution requirements and whether a NOTICE file is needed.
- Generate a dependency/SBOM report for Rust Cargo.lock, Swift Package.resolved, Gradle, Skip/Android and bundled GStreamer/WebRTC distributions; include transitive native and codec plugin license concerns.
- Separate **source license** from binary dependency distribution, trademarks/branding, App Store/Play Store SDK compliance and patent/codec requirements.
- Verify current contributors' rights to license their contributions, including historical code with unclear third-party provenance.
- Create `LICENSE` with exact official Apache-2.0 text only after maintainer chooses it; add `NOTICE` (as necessary) and `THIRD_PARTY_NOTICES.md`, SPDX headers to original files, and update README with clear build/distribution obligations.
- Pin a license scanner and fail CI on disallowed shipped dependency licenses and missing notices; allow a documented exception review mechanism.

## Decision table for maintainer

| Choice | Preferred use | Advantages | Tradeoff |
| --- | --- | --- | --- |
| **Apache-2.0 (recommended)** + independently authored Niri JSON IPC | Permissive open-source Vista with Linux adapters | Matches Sill notices, straightforward BSD/MIT/Apache deps, avoids direct GPL niri-ipc linkage | Maintain a compact JSON IPC model and compatibility fixtures |
| GPL-3.0-or-later combined distribution | Owner explicitly wants strong copyleft and direct niri-ipc reuse | Reuse official typed crate and accept copyleft | GPL obligations for distributed combined work; still audit other deps |
| Dual MIT/Apache-2.0 original code | Owner prefers customary Rust dual license | Flexible reuse of *original* code | Any copied Sill Apache-2.0 portions are Apache-only; do not pretend they are MIT |
| Component-specific licenses | Complex third-party obligations justify it | Flexible per-component licensing | Higher overhead, needs professional review and clear artifact policy |

## Closure criteria for issue #73

Maintainer choice: **Apache-2.0**, with the Niri protocol-only integration. Done
on 2026-11-12: root `LICENSE` with unmodified Apache-2.0 text, `NOTICE`,
`THIRD_PARTY_NOTICES.md` covering Rust, Swift, Android and system GStreamer
dependencies, and a README license section.

Still open: a per-file provenance inventory for any source adapted from Sill, and
an automated dependency-license check in CI. Until those land, keep #73 open: the
license decision is final, the mechanical enforcement is not.

## Sources

- Sill Apache-2.0: https://github.com/Saffsanity/sill
- Niri workspace GPL-3.0-or-later: https://github.com/niri-wm/niri/blob/main/Cargo.toml
- Official Niri IPC JSON protocol and compatibility: https://niri-wm.github.io/niri/IPC.html
- Apache Software Foundation GPL compatibility analysis: https://apache.org/licenses/GPL-compatibility.html
- OSI Apache-2.0 license text: https://opensource.org/license/apache-2-0
