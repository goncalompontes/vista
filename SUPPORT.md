# Support

Vista is an experimental project maintained by one person in their own time.
Support is best-effort: there is no SLA, no support contract, and no promise that
a question gets answered.

## Where to ask

- **Bug reports and feature requests**: open a GitHub issue. Use the templates;
  they ask for the commands and output that make a report actionable.
- **Usage questions**: open an issue with the `question` label. If the volume
  grows, this moves to GitHub Discussions.
- **Security reports**: use [SECURITY.md](SECURITY.md). Never a public issue.
- **Contributor questions**: [CONTRIBUTING.md](CONTRIBUTING.md).

## Before you ask

1. Run `cd host && cargo run -- doctor` and include the output.
2. Say which compositor and version you run, which GPU, and whether the failing
   command needs a desktop portal or a Niri session.
3. Include the exact command line and the exit code.

## What is not supported

- Any host other than Linux with Niri.
- Running `vista-host serve` on a non-loopback address.
- Production or security-critical use. The host has no TLS, no discovery, and no
  forward secrecy yet.
- macOS or Windows hosts, audio, clipboard, and isolated application sessions.

## Response expectations

- Issue triage: usually within a week.
- Pull requests: a first human response usually within about a week.
- Security reports: acknowledged as soon as practical; see
  [SECURITY.md](SECURITY.md).

If an issue has no reply after two weeks, comment on it. A ping is welcome and
does not count as rude.
