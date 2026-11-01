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
- **Never publish** a release, a crate or any package from this repo, except the
  one release the Phase 7 note below allows. No AUR, Flathub, crates.io or other
  registry, ever.
- **Never touch the board's live Hydrus, real browser profile, or real Hatate
  config/session files.** Never inject input into the board's live session; any
  GUI run uses a headless compositor.
- At most **two** Rust-rewrite author tickets in flight at once. Push a WIP
  commit about every 20 minutes; split runs that go past about 40 minutes.

### Phase 7 amendments (board card `d1c70a9e`, accepted 2026-10-31; added by DAN-2499)

These change the hard lines above only where they say so.

- **Publishing.** Exactly one publish is allowed: a GitHub Release on
  `Dromares/hatate-rs` carrying `Hatate-x86_64.AppImage` and `SHA256SUMS`, version
  `0.2.0`. It is created as a **draft** after step 7.d merges, and made public only
  by a separate publish ticket that is filed after the board's OK on its own card.
  Until that ticket exists, "never publish" stands as written. No tag, draft or
  release is created before the draft step. `RELEASING.md` is the procedure.
- **Status claim.** The work-in-progress claim and the `0.0.x` version stay until 7.d,
  which changes them under the status-claim sweep above. The AppImage's `--version`
  and About say work in progress.
- **The board's profile.** Only 7.b reads the board's real config, sessions and
  `lens_profile`, and only to copy them into a temporary `HOME`. Neither app opens or
  writes the originals. Every other ticket keeps "never touch".
- **One author lane.** Authors run one hatate-rs ticket at a time (7.a, 7.c, 7.b, 7.d
  in series), which replaces the "two tickets" line above.
- **Docs cite a function, not a line.** A `docs/*.md` cite is three code spans:
  the file, `fn name`, and a quoted fragment of the line. `cargo test -p hatate-gtk
  --test phase6_coverage` fails on a line-number cite and on a fragment that is not
  inside its named item. The presubmit tests (`hatate-doccite`, `presubmit_tabs`)
  need `HATATE_PY_ORACLE` (the Python repo at `76c85c4`) and a display; with `CI` set
  a missing input fails instead of skipping.

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
HATATE_GTK_SMOKE=1 GDK_BACKEND=x11 GSK_RENDERER=cairo xvfb-run -a cargo test --workspace
```

The window crate needs the GTK 4 and libadwaita development files and `xvfb`. Run the window
scenarios on Xvfb or a headless compositor with a temp `HOME` and temp XDG directories, never on
the board's `DISPLAY` or Wayland socket. With `CI` set, a missing `HATATE_GTK_SMOKE=1` or a missing
display fails the test.

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
