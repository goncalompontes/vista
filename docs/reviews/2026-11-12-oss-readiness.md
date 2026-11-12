# OSS readiness review — 2026-11-12

An open-source-project review of Vista's repository surface: landing page,
licensing, contribution path, triage, release policy, governance, changelog and
distribution. It records the baseline, what changed in this pass, and what should
not be called done.

Audience assumed: individual developers and technically curious Linux users
evaluating whether to try it, plus a smaller enterprise/security reader checking
license and disclosure before adopting anything.

## Baseline

Measured from the repository and GitHub on 2026-11-12:

| Signal | Value |
| --- | --- |
| Age of first commit | 4 days |
| Contributors | 1 (`goncalompontes`), author of all 46 issues and all pull requests |
| Stars / forks | 0 / 0 |
| Releases or tags | none |
| Repository description | empty |
| Repository topics | none |
| License file | none — repository was "all rights reserved" |
| Root `LICENSE` / `NOTICE` | absent |
| Community health files | `CONTRIBUTING`, `SECURITY`, `SUPPORT`, `GOVERNANCE`, `CODE_OF_CONDUCT`, `CODEOWNERS`, issue templates, PR template: all absent |
| Open issues | 27, all created in the previous 3 days |
| Untriaged open issues | 2 (7.4%) |
| Labels | 18, but 40 of 46 issues carried `enhancement` |
| CI workflows | 5; none use secrets, so all can run on fork pull requests |
| `cargo test --workspace` | passes |

This is a fast-moving single-maintainer project, not a stalled one. The gaps were
structural (no license, no contribution path, no release contract), not
maintenance neglect.

## README audit

Before this pass, against the five blocking checks:

| Check | Result |
| --- | --- |
| A1 Identity — what the project is in the first screen | Pass |
| A2 Install — the documented build path actually works | **Fail**: the "Develop" section was a 60-line diagnostic catalogue; the five commands a newcomer needs were buried among them |
| A3 Claims verifiable | **Fail**: "open-source" was stated while no license existed |
| A4 Honest status | Pass — unusually strong, with dated evidence and explicit non-claims |
| A5 Reachable license | **Fail**: no LICENSE file |

Scored checks: status 2/2, differentiation 1/2 (present but buried after 500
words), install 0/2, license 0/2, contribution 0/2, references 2/2.

Fixes in this pass:

- Added the license section and badge.
- Moved build-and-run above the limitations, so the first copyable command arrives
  at ~310 words instead of ~540.
- Split the full diagnostic catalogue into [docs/diagnostics.md](../diagnostics.md)
  and linked it, keeping the README a landing page.
- Added an explicit "What does not work yet" plus a "use this instead" sentence.
- Added a four-badge row of assessment signals only (three CI workflows and the
  license); no intention badges.

What was deliberately not done: no demo GIF or screenshot. A visual would help
this project, but it needs a recorded session and a maintained re-render path,
and a stale screenshot is worse than none.

## Contributor path

Baseline: the cold-run path was undocumented. `CONTRIBUTING.md` did not exist, so
there was no setup command to verify, no stated response target, no DCO or CLA
policy, and no merge-style statement. CI used no secrets, which is good: fork
pull requests can run every check.

Fixes in this pass:

- `CONTRIBUTING.md` with host, Swift and Android setup; every command was run in
  this environment (`cargo test --workspace` passes) or copied from the CI
  workflow that runs it.
- The project's evidence rule (state what you ran; never upgrade synthetic tests
  to a hardware claim) is written down as the first convention, because it is the
  one reviewers enforce.
- DCO sign-off required, with an automated check in
  [`.github/workflows/dco.yml`](../../.github/workflows/dco.yml).
- A published response target of about one week, phrased as best effort.
- Merge style (merge commit, authorship preserved) stated.

Still open:

- No `good first issue` stock. There is nothing labelled for a newcomer yet, and
  labelling old bugs as beginner-friendly without a location, a verification
  command and a named mentor would be worse than an empty queue.
- The two open pull requests predate the DCO check and will need
  `git rebase --signoff` before they can merge.

## Issue triage

Baseline: 27 open issues, median age 2 days, 7.4% untriaged, no stale items. All
issues were authored by the maintainer, so "time to first human response" is not
yet a meaningful number.

The queue is not the problem yet; inflow is small and every item is a planned
workstream. The design decision is therefore minimal:

- Issue forms for bug, proposal and question, with required fields for the exact
  command, output and environment. Blank issues are disabled so the forms are not
  optional.
- `needs reproduction` and `needs design` states, so an unverified report and an
  undecided design are visible as states rather than parked under `enhancement`.
- `priority:high` / `priority:low`, defined by phase-gate impact.
- No stale bot. A bot that closes a confirmed bug waiting on the maintainer tells
  the reporter their evidence was worthless. If staleness handling is added
  later, it should only sweep items waiting on the reporter, after a warning.

Deferred on purpose: no automated classifier, no auto-triage, no auto-close.

## License

The repository had no license, so despite the README calling the project open
source, nobody could legally use, modify or redistribute it.

Decision (2026-11-12, maintainer approved):

- **Outbound:** Apache-2.0, unmodified text in [`LICENSE`](../../LICENSE).
- **Why:** it is what [ADR 0004](../adr/0004-license-and-niri-ipc-policy.md)
  recommended; it matches Sill, the reference project; and it carries an explicit
  patent grant, which matters for a streaming protocol.
- **Inbound:** DCO sign-off, no CLA. Nothing in the current plan (no dual
  licensing, no foundation, no foreseeable relicense) needs the relicensing right
  a CLA would buy.
- **Dependencies:** all 139 resolved Rust packages are permissive; `niri-ipc` is
  deliberately not linked. Swift pins and Gradle dependencies are Apache-2.0.
  Recorded in [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md).
- **Boundary:** Apache-2.0 covers the repository's original source. The `NOTICE`
  records that client design is informed by Sill and that Sill does not endorse
  Vista.

Still open: a per-file provenance inventory for any Sill-adapted source, and a
dependency-license check in CI. ADR 0004 says both.

## Release policy

There were no tags and no policy, while `host/Cargo.toml` already said `0.1.0`.

- [`RELEASES.md`](../../RELEASES.md) defines SemVer 0.x with an explicit rule:
  `0.MINOR` may break, `0.MINOR.PATCH` may not.
- The covered surface is the host CLI (names, flags, exit codes, JSON keys), wire
  protocol v1, and the shared fixtures. Internal Rust/Swift names, human-readable
  output and diagnostics are explicitly outside the promise.
- Support is the latest line only. With one maintainer, backports would be a
  promise that cannot be kept.
- 1.0 criteria are written down, so "when is 1.0" is not a mood.
- The wire protocol versions independently from the product.

Still open: no tag has been cut. `0.1.0` is described as the first planned
release, and the changelog says so.

## Changelog

`CHANGELOG.md` now follows Keep a Changelog with a single `Unreleased` section
covering what exists, written impact-first, plus a "Known limitations" list. No
fabricated version numbers or dates: the file states plainly that nothing has
been tagged.

The project has 50 merged pull requests and no release notes, which is the
pattern this file exists to stop.

## Governance

As-is: written solo authority. One person holds merge rights, release rights and
repository settings; the contributor absence factor is 1.

- [`GOVERNANCE.md`](../../GOVERNANCE.md) states that honestly rather than
  pretending to be a council.
- It writes the two rules ad-hoc projects lack: how a decision is escalated when
  discussion stalls, and how a person becomes a maintainer.
- It splits merge rights from signing and publishing credentials, so access can be
  granted generously for review and slowly for releases.
- It defines inactivity (90 days + 30 days), emeritus status, and an explicit
  unmaintained end state instead of silent abandonment.

The honest limitation is stated in the file: this is not community governance. It
is a route out of solo authority that starts working the first time someone
reviews a pull request.

## Distribution

Audit findings, all fixed in this pass:

- Repository description was empty. Now one sentence that names the artifact, the
  platforms and the status.
- No topics. Now 12, covering language and domain.
- No release stream. The changelog and release policy are the groundwork; the
  first tag is still missing.
- No `SECURITY.md`, so GitHub's security tab showed the community profile as
  incomplete. Now present, and private vulnerability reporting is enabled on the
  repository.

Refused for now, with reasons:

- **Homebrew / AUR / distro packaging.** The install unit is "build a Rust host
  against a specific compositor". Packaging a pre-1.0, loopback-only daemon
  creates a channel that keeps serving an old version and cannot be unshipped.
  Revisit after a 1.0 with a stable CLI.
- **App stores.** No signed builds, no physical-device verification, and the
  protocol still changes. Store review cycles would gate the fastest-moving part
  of the project.
- **Curated "awesome" list submissions.** The project cannot yet survive the
  attention: no release, no demo media, no physical-device evidence. Submitting
  now spends the one-time first impression on an unfinished product.
- **GitHub Discussions.** `SUPPORT.md` says issues today; enable Discussions when
  question volume justifies a second queue, not before.

## Changes made in this pass

New files:

- `LICENSE`, `NOTICE`, `THIRD_PARTY_NOTICES.md`
- `CONTRIBUTING.md`, `SECURITY.md`, `SUPPORT.md`, `GOVERNANCE.md`,
  `RELEASES.md`, `CHANGELOG.md`, `CODE_OF_CONDUCT.md`
- `.github/CODEOWNERS`, `.github/PULL_REQUEST_TEMPLATE.md`
- `.github/ISSUE_TEMPLATE/{config,bug,proposal,question}.yml`
- `.github/workflows/dco.yml`
- `docs/diagnostics.md`
- this review

Edited:

- `README.md` restructured: status heading, build path moved up, diagnostics
  catalogue extracted, license and project sections added.
- `docs/adr/0004-license-and-niri-ipc-policy.md`: status moved from "request for
  maintainer decision" to accepted, with the remaining work named.
- `host/src/main.rs`: usage text advertised `media-stream --connect --key-hex
  --session-id`, which the parser rejects; it now says `--secret-hex`, and two
  duplicate usage lines were removed.

Repository settings changed:

- Description, topics, private vulnerability reporting.
- Four labels: `needs reproduction`, `needs design`, `priority:high`,
  `priority:low`.

## What is still not done

Ordered by what would break a stranger first:

1. **Push and tag 0.1.0.** Nothing here is visible to anyone until the files are
   on `main`, and the release stream stays empty until a tag exists.
2. **A three-command quickstart that works on a clean machine.** The commands are
   verified here, but this machine already had Rust, GStreamer and a Niri
   session. A container or fresh-VM run is the missing evidence.
3. **At least three `good first issue` items** with a location, a verification
   command and a named mentor. The label is a promise of help, not a difficulty
   rating.
4. **A recorded visual.** The product is video; the README has none. It needs a
   repeatable capture script, not a hand-made GIF.
5. **License and provenance automation.** A dependency-license allowlist in CI and
   a Sill-derived-file inventory.
6. **Second credential holder.** Everything ships from one account. `GOVERNANCE.md`
   names the target; the work is finding a second maintainer.
7. **Physical-device evidence** for iOS and Android. The docs are honest about
   this; it remains the largest gap between the claims and the product.

## Review date

Re-run this review at the first tagged release, or 2026-02-12, whichever comes
first. Re-check the README cold-reader test after the tag, because the install
path is the part most likely to drift.
