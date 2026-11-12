# Pull request

<!--
Thanks for the change. Keep this short, but keep the evidence specific.
"Tests pass" is not evidence; the command and its output are.
-->

## What this changes

<!-- One paragraph. What behaviour changes, for whom, and why. -->

## Evidence

<!-- What you ran, what it printed, and what it does NOT prove. -->

```
$ 
```

## Scope

- [ ] The linked issue is the agreed scope, or this is a typo/test/doc fix
- [ ] One logical change
- [ ] Every claim above is backed by a command or a file in this diff
- [ ] Wire-protocol changes update the fixtures and all three implementations
- [ ] Docs that this change makes wrong are updated in the same PR

## Checks

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `swift test --package-path client/core` (if `client/core` changed)
- [ ] `:app:testDebugUnitTest` (if `client/android` changed)
- [ ] Commits are signed off (DCO, `git commit -s`)

## Not verified

<!--
State the parts you could not test: no Niri session, no GPU, no physical
device, emulator only, synthetic input only. Leaving this empty is a claim.
-->
