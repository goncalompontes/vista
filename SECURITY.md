# Security policy

Vista is experimental. It streams a desktop over the network and injects input
into it, so treat every build as untrusted software until the verification
gates in [docs/plan.md](docs/plan.md) are met.

## Reporting a vulnerability

**Do not open a public issue for a vulnerability.** Use GitHub's private
vulnerability reporting for this repository:

1. Go to the **Security** tab.
2. Choose **Report a vulnerability**.
3. Describe the issue, the affected component, and the smallest reproduction you
   can manage.

If private reporting is unavailable, open a public issue that says only that you
have a security report and asks for a private channel. Do not include details.

This project has no bug bounty and no paid security team. Reports are handled on
a best-effort basis by the maintainer.

## What is in scope

- The host service: pairing, control records, media records, capability and
  action dispatch, control leases, input injection.
- The wire protocol and its documented invariants
  ([docs/protocol.md](docs/protocol.md)).
- The iOS and Android clients' pairing, transport and decoding paths.
- CI workflows and release artifacts, if any are added.

## Known limitations, not vulnerabilities

These are documented design states, not reports. A report that only restates one
of them will be closed with a link back here:

- `vista-host serve` is single-client, has no TLS, and is meant for loopback.
  Binding it to a non-loopback address is an explicit opt-in
  (`--allow-remote`) and is unsupported.
- Pairing uses a pre-shared 256-bit secret with no forward secrecy. A leaked
  secret compromises that session. See [ADR 0003](docs/adr/0003-device-identity-enrollment.md)
  and issue #77 for the enrollment work that replaces it.
- The custom AEAD record layer is not a general-purpose transport security
  protocol. It is scheduled for replacement (issue #59).
- There is no discovery, no multi-client support, no congestion control, and no
  reconnection guarantee.

## Supported versions

No release has been tagged yet. Only the tip of `main` is supported. See
[RELEASES.md](RELEASES.md) for the versioning policy once releases begin.

## Disclosure

Fixes ship as a normal commit on `main` and, once releases exist, as a patch on
the latest line. A GitHub security advisory is published after the fix is
available. The maintainer will credit reporters who want credit.
