# Release policy

This is the contract Vista's users can rely on. It describes the version
scheme, what counts as a breaking change, and what gets supported. It is
deliberately modest: the project is pre-1.0 and maintained by one person.

## What ships

Vista ships as source, in one repository, with several artifacts:

| Artifact | Built with | Distributed as |
| --- | --- | --- |
| Rust host (`vista-host`) | `cargo build` in `host/` | Source; no crates.io publication yet |
| iOS / iPadOS app | Xcode + XcodeGen in `client/ios/` | Source; no App Store build |
| Android app | Gradle in `client/android/` | Source and a debug APK |
| Wire protocol v1 | Rust host + Swift core + Kotlin client | Specification and shared fixtures in `docs/` |

Nothing is published to a package registry, an app store, or a container
registry. A "release" today means a git tag on this repository.

## Version scheme

**Semantic Versioning 2.0.0, in the 0.x series.**

- `0.MINOR.0` — a coherent increment. **May contain breaking changes.** The
  changelog names them under `Changed` or `Removed`.
- `0.MINOR.PATCH` — compatible fixes and additions only. No removals, no
  changed defaults, no changed wire behaviour.

This is the rule the version number promises while the project is 0.x. It is
stricter than "0.x means anything goes": a patch release will not break you.

The first release will be **0.1.0**.

### Versioned surfaces

These are covered by the compatibility promise:

- **Host CLI**: command names, documented flags, exit codes, and the keys of
  `--json` output.
- **Wire protocol v1**: message types, field names and meanings, and the
  handshake and record-layer rules in [`docs/protocol.md`](docs/protocol.md).
- **Shared fixtures**: `docs/fixtures/*.json` are normative. A change to a
  fixture is a protocol change.
- **Supported runtimes**: Rust 1.88+ (`rust-version` in `host/Cargo.toml`),
  Android minSdk 28, iOS 17.0+.

The wire protocol carries its own version (`v1`). Additive fields are compatible;
renaming, removing or changing the meaning of a field requires a new protocol
version. See [ADR 0005](docs/adr/0005-canonical-schema-ownership.md).

### Not covered

- Human-readable terminal output, log text and progress messages.
- Diagnostic commands (`doctor`, `*-test`, `capture-*`, `desktop-probe*`,
  `pipewire-loopback`) — these exist to produce evidence and may change freely.
- Rust module, type and function names in `vista-host`. The crate is not
  published and has no external Rust consumers.
- Swift type names in `VistaCore`. The package is consumed only by the iOS app.
- Kotlin class names and the Android UI.
- The exact contents of screenshots, recorded verification runs, and docs that
  describe a past run.

### Deprecation

- A deprecation ships in a release that removes nothing, warns in the most
  visible channel available (CLI output or the changelog), and names the
  replacement.
- The earliest removal is the next `0.MINOR` after the deprecation release,
  and never less than 30 days later.
- Removals are listed under `Removed` with a pointer to what replaced them.

## Supporting older versions

**Latest line only.** Fixes, including security fixes, ship on `main` and on the
most recent `0.MINOR` line. Older lines are not patched.

The project has one maintainer and no release automation. Promising backports
for a previous minor would be a promise that cannot be kept. If the project
gains maintainers, this section changes first.

Security fixes ship out of band, as soon as a fix is ready, without waiting for
a scheduled release.

## Cadence

**Feature-based, no calendar.** A release is tagged when a coherent, verified
increment lands, not on a fixed date. There is no promise of a maximum gap yet;
until 0.1.0 ships, the tip of `main` is the only supported state.

When releases do begin, the project will publish a maximum interval and keep it.

## 1.0 criteria

Vista leaves 0.x when all of these hold:

1. Wire protocol v1 has no known incompatible change pending.
2. Host and at least one client have been verified on physical hardware, not
   only an emulator or a synthetic test.
3. The CLI surface has been stable across at least two minors.
4. The security model in [SECURITY.md](SECURITY.md) no longer depends on
   known-weak pieces like the pre-shared pairing secret.

Then `1.0.0` is tagged, and from that point a major version is required for any
breaking change on the surfaces listed above.

## Release checklist

1. The `CHANGELOG.md` section for the version is complete and matches the diff.
2. `host/Cargo.toml` version matches the tag.
3. `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
   warnings` and `cargo test --workspace` pass.
4. iOS and portable-client CI are green.
5. The tag is annotated: `git tag -a vX.Y.Z -m "vX.Y.Z"`.
6. The tag message links the changelog entry.

## Changing this policy

This policy changes by pull request, with the reason stated. A change that
narrows support takes effect no earlier than the next minor release, so nobody
is surprised mid-line.
