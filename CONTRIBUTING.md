# Contributing to Vista

Thanks for looking. Vista is an experimental application-streaming platform:
a Rust host for Linux (Niri first) and native iOS and Android clients. It is
maintained by one person, so the most useful contributions are the small,
verifiable ones: a bug with a reproduction, a test that closes a hole, a doc
that corrects a claim, or a fix scoped to one issue.

By participating you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Start here

1. Read the [README](README.md) for what works today and what does not.
2. Read [docs/plan.md](docs/plan.md) for the current phase order. Work that
   jumps the queue will be asked to wait.
3. Look for issues labelled [`good first issue`](https://github.com/goncalompontes/vista/labels/good%20first%20issue)
   or [`help wanted`](https://github.com/goncalompontes/vista/labels/help%20wanted).
4. Comment on the issue before you start so two people do not build the same
   thing.

For anything larger than a typo or a test fix, open an issue first. "Larger"
means: a new command, a protocol change, a dependency, a new client feature, or
anything that changes a documented behaviour. A pull request that arrives
without an agreed scope often cannot be merged, however good the code is.

## What is in scope right now

- Host capture, encoding, media transport and input on Linux.
- The v1 control protocol and its fixtures (`docs/fixtures/`, `docs/protocol.md`).
- The Rust host, the Swift `VistaCore` package, the iOS app, the Android app.
- Evidence: recorded verification runs, honest limitation notes, tests that
  encode a real failure.

Out of scope for now: macOS and Windows hosts, audio, clipboard, production
discovery, multi-client support, and App Store packaging. These are planned but
not current. See [docs/roadmap.md](docs/roadmap.md).

## This project's evidence rule

Vista's documentation makes many precise claims. Every claim must be backed by
a command you can run or a file you can inspect. When you contribute:

- Say what you ran and what it printed, not what you expected.
- If something is only partly verified, say which part.
- Do not mark a host, device, transport or codec as working because synthetic
  tests pass. Synthetic tests are valuable; they are not a hardware run.

This is the single most important convention in the repository. A pull request
that overstates a result will be sent back even if the code is correct.

## Set up a development environment

### Host (required for most work)

Requirements:

- Rust 1.88 or newer (`rust-version` in [`host/Cargo.toml`](host/Cargo.toml)).
  Stable is fine.
- `pkg-config` and the GStreamer development packages. On Debian/Ubuntu:

  ```sh
  sudo apt-get install -y --no-install-recommends \
    build-essential pkg-config \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad
  ```

- A running Niri session and the `niri` executable on `PATH` for the live host
  commands. Unit tests do not need a compositor, display server, GPU or portal.

Run the gates CI runs:

```sh
cd host
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

From the repository root, `cargo test --workspace` is equivalent to the last
line; the workspace has one member.

Quick manual check of the live environment:

```sh
cd host
cargo run -- doctor
cargo run -- windows --json
```

If your shell has a sanitized environment, export `NIRI_SOCKET` from the active
Niri session first.

### Swift core and iOS app

`client/core` is a portable Swift package; it builds and tests on Linux with
Swift 6.0:

```sh
swift test --package-path client/core
```

The iOS app needs macOS, Xcode, and [XcodeGen](https://github.com/yonaskolb/XcodeGen):

```sh
cd client/ios
xcodegen generate
xcodebuild -project Vista.xcodeproj -scheme Vista -configuration Debug \
  -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' \
  CODE_SIGNING_ALLOWED=NO build
```

### Android app

Requirements: JDK 17+ (21 recommended), Android SDK 36 with build-tools 36.0.0.

```sh
export ANDROID_HOME="$HOME/Android/Sdk"
client/android/build-android.sh
client/android/Android/gradlew -p client/android/Android :app:testDebugUnitTest
```

See [client/android/README.md](client/android/README.md) for emulator setup.

### Unsupported setups

Windows and macOS hosts are not supported. If a command fails on a platform not
listed above, that is expected, not a bug worth filing yet.

## Make a change

1. Branch from `main`.
2. Keep one logical change per pull request.
3. Match the surrounding style. Run the formatter; do not hand-format.
4. Add or update tests. A bug fix needs a test that fails before it.
5. Update the docs the change makes wrong. If you change a wire message, update
   the fixtures and all three implementations (Rust host, Swift `VistaCore`,
   Android `VistaWire.kt`) together.
6. Sign off every commit (see below).

### Commit messages

The history uses Conventional Commits, for example
`feat(host): add keyframe rate limit` or `docs(protocol): clarify replay
rejection`. Keep the subject under about 72 characters and use the body for the
evidence: what you ran, what it printed, and what remains unverified.

### Developer Certificate of Origin

Every commit must carry a `Signed-off-by` line matching its author:

```sh
git commit -s -m "fix(host): reject an empty pairing secret"
```

If you forgot, amend or rebase:

```sh
git rebase --signoff origin/main
```

The DCO is a statement that you wrote the change or have the right to submit
it. It is not a copyright assignment. See
[developercertificate.org](https://developercertificate.org/) for the exact
text.

## The review

- A first human response usually arrives within about a week. It is a
  best-effort target from a solo maintainer, not a contract.
- If a pull request goes quiet, comment on it. Do not assume it was rejected.
- Review order is: scope, correctness and tests, design, then style. Anything a
  formatter can fix should not appear as a review comment; if it does, ignore it.
- The maintainer may push to your branch to finish a small fix. You will be told
  when that happens.
- Merging uses a merge commit, so your commits and authorship are preserved.

## Where to ask

- Bugs and feature proposals: GitHub issues.
- Security reports: [SECURITY.md](SECURITY.md) only, never a public issue.
- Everything else: [SUPPORT.md](SUPPORT.md).
