# CLAUDE.md: hatate-rs

Guidance for the agents who work in this repo. `hatate-rs` is a work-in-progress
Rust rewrite of Hatate. It is public from its first commit and is **not yet a
replacement for Hatate**. The plan is DAN-1961 (rev 3); the Python source of
truth is `Dromares/Hatate-Linux-Redux`.

## Hard lines

- **Public from the first commit.** Nothing private ever enters this history: no
  cookies, tokens, API keys, Hydrus access keys, real user session files or real
  user config, in fixtures, logs, screenshots or commits. A secret pushed once to
  a public repo is leaked even if it is force-pushed away.
- **Licence: MIT.**
- **Rust only** in the tree: no Python, no new `.sh`, no other scripting
  language, tooling and mockups included. CI is GitHub Actions YAML calling
  `cargo`.
- **Status claim.** The README says work-in-progress Rust rewrite, not yet a
  replacement for Hatate. No text anywhere may claim usable, parity,
  feature-complete, released, or a version beyond `0.0.x`. Any change to that
  claim carries the DAN-1729 status-claim sweep: run
  `git grep -n -i -E 'replace|parity|release|stable|ready|usable|install|version'`
  over the whole tree and list every hit with its disposition in the PR comment.
- **`ops/` is not ported.** The Python app is a black box: its fixtures are the
  inputs, its output is the oracle. No PR from this work modifies
  `Dromares/Hatate-Linux-Redux`.
- **Never publish** a release, a crate or any package from this repo. How the
  Rust build ships is a separate board decision before Phase 7.
- **Never touch the board's live Hydrus, real browser profile, or real Hatate
  config/session files.** Never inject input into the board's live session; any
  GUI run uses a headless compositor.
- At most **two** Rust-rewrite author tickets in flight at once. Push a WIP
  commit about every 20 minutes; split runs that go past about 40 minutes.

## No secrets in fixtures

Fixtures come only from the Python repo's own public test fixtures. Before a PR
that adds fixtures, scan them for cookies, tokens, API keys, Hydrus access keys
and session or config content, and say in the PR that you did. Secret scanning
and push protection are enabled on the repo; do not work around a push-protection
block, report it.

## Local gate

Run exactly the commands in `.github/workflows/ci.yml`:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Merge gate

A non-author run of the three commands above in a clean worktree at the PR head
SHA, log path cited, **and** green `get_check_runs` at the same head (newest
workflow run per gate). A red hosted gate is a fail; a billing-dead signature
(2 s or less, log download 404) means no evidence either way. The head is frozen
once the verify ticket opens; land fixups before it. The merge executor merges
only on the named verifier's PASS at the head it merges, re-reading
`git ls-remote origin refs/pull/N/head` right before the merge. An approver's
approval does not substitute for the verifier's PASS. Close check:
`git merge-base --is-ancestor <sha> origin/main` plus `git ls-remote origin main`.
`main` takes PRs only; the initial commit is the one direct commit.

## Network clients (rv-rs rule, adapted)

- Install the rustls `CryptoProvider` explicitly at startup. rv-rs shipped
  panicking on a missing provider (DAN-1909).
- Every PR that adds or changes a network client carries a test against a local
  `https://` server (a throw-away CA committed as a test fixture), not only a
  plain-`http://` mock. The verifier names that test and fails the PR if it is
  missing.
- Before any binary is checked, run its real network path against the real
  service, not only `--help`.

## Teardown

Build output (`target`, clones, rootfs) lives under
`/home/beeizebubs/work/worktrees/<TICKET>/`. The merge executor removes the
author and verify trees after the ancestor check and pastes `du -sh` before and
after. Keep `*.log`, `shots/` and `comment.md`.
