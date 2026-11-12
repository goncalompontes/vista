# Governance

Status: **written solo authority with a published succession path**, reviewed
2026-11-12. Vista has one maintainer today. This document describes how
decisions are actually made, not an aspiration.

## Who decides

| Decision | Decided by | How |
| --- | --- | --- |
| Routine change, docs, tests, small fixes | Any contributor with a merged pull request | Pull request review by the maintainer |
| New feature, new command, new dependency | Maintainer | An accepted issue, after discussion |
| Wire protocol change | Maintainer | Issue + updated fixtures + all three implementations |
| Architecture change | Maintainer | An ADR in [`docs/adr/`](docs/adr/), committed with the change |
| Release, version number, support window | Maintainer | [RELEASES.md](RELEASES.md) |
| Licensing, trademark, funding | Maintainer | Written in this repository, with counsel when needed |
| Security response | Maintainer | [SECURITY.md](SECURITY.md) |
| Change to this document | Maintainer, in public | Pull request |

Maintainer of record: [@goncalompontes](https://github.com/goncalompontes).
They hold merge rights, release rights, and the GitHub organization's
repository settings. No company owns or funds the project.

## How a decision is made

1. **Routine work** uses lazy consensus: a pull request is open for comment and
   merges when it is approved and CI is green. Silence is not approval.
2. **Substantial work** starts as a GitHub issue. The issue is the record. A
   decision that exists only in a chat or a call is not a decision.
3. **Architecture** is recorded as an ADR. Accepted ADRs are binding until
   superseded by a later ADR.
4. **Escalation**: if a discussion stalls, the maintainer decides and writes the
   reasoning in the issue. Any participant may ask for that decision to be
   revisited by opening a new issue with new evidence. There is no vote.

Timers exist so that nothing stalls indefinitely:

- A pull request with no maintainer response for 14 days may be pinged.
- A design issue with no decision for 30 days is either scheduled or explicitly
  declined in the issue.
- A security report is acknowledged as soon as practical and never left without
  a status.

## Adding a maintainer

The project needs more than one person. The path is deliberately gradual, and
the bar rises with the privilege:

1. **Contributor** — one or more merged pull requests.
2. **Triager** — label, close duplicates, reproduce bugs. Granted after a track
   record of accurate reports; does not require commit access.
3. **Reviewer** — may approve others' pull requests. Granted after sustained,
   high-quality contributions and demonstrated judgement on scope.
4. **Maintainer** — merge rights on `main`. Granted after several months of
   review work and on agreement between existing maintainers.
5. **Publisher** — release signing and package credentials. Granted only after
   maintainer status and only to a second person, so no single account can ship
   a release alone.

The split between steps matters: merge rights are cheap to grant and cheap to
revoke, while signing keys and registry tokens are neither. Being slow with keys
is policy, not distrust.

Any new maintainer is announced in a pinned issue and added to this document.

## Inactivity and emeritus

- A maintainer with no review, commit or triage activity for **90 days** is
  asked whether they want to continue.
- With no response for a further **30 days**, they move to **emeritus**:
  credited, listed here, but without merge or publish rights.
- Emeritus status is reversible on request.

## Succession

If the maintainer becomes unable to continue:

1. The repository stays public and the license stays Apache-2.0. Nothing is
   revoked.
2. A fork is a legitimate continuation; the [NOTICE](NOTICE) and license travel
   with it.
3. If a successor exists, they are named in a pinned issue and in this file.
4. If no successor exists, the project is marked **unmaintained** in a pinned
   issue and in the README rather than left to look alive.

There is no legal entity, no foundation, and no fiscal host. If the project ever
handles money or needs to prove vendor neutrality to adopters, that becomes a
separate public decision.

## What this document does not promise

- It does not promise a response time. See [SUPPORT.md](SUPPORT.md).
- It does not promise that every proposal gets built.
- It does not make the project community-governed. It is honest about being
  single-maintainer, with a written route out of that state.

## Review

This document is reviewed every 12 months, and sooner if a maintainer joins or
leaves, or if a decision is contested and no rule covers it.
