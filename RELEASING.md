# Releasing the Hatate AppImage

> **Status.** The workspace version is `0.2.1`. Until the publish step of section 6 has run, no
> release is public. **No tag, draft or release is created, and nothing is published, without the
> board's OK where this file says so** (plan DAN-1961, Phase 7): the draft is made by dispatching
> `release.yml` (section 3), and a separate publish ticket, filed only after the board's OK on its own
> card, makes it public.

A release is one file, `Hatate-x86_64.AppImage`, plus `SHA256SUMS`, attached to a GitHub Release on
`Dromares/hatate-rs`. The `release` workflow (`.github/workflows/release.yml`) builds them from a
`main` commit with the same recipe as the `appimage` job of `ci.yml`, and creates the release as a
**draft**. The draft is checked by someone other than the person who dispatched it, and only then
published. Never publish first and check afterwards. No AUR, Flathub, crates.io or other registry,
ever (DAN-1665).

Roles follow the repository's usual split: the **executor** dispatches the workflow and creates the
draft, a **non-author** checks it, and the **publish executor** publishes. The workflow itself cannot
publish: it has no publish switch, and the verifier of any PR that touches it greps for one.

## 1. Before building

1. `main` is green: the newest hosted `ci` run at the commit, with both jobs (`ci`, `appimage`)
   `completed` and `success`.
2. The version is changed in one place, `[workspace.package] version` in `Cargo.toml`.
   `hatate --version` must print `hatate <version> (Rust rewrite of Hatate)`, and the workflow refuses a
   dispatch whose `version` input differs from it.
3. Re-run the status-claim sweep over the **whole tree** and fix every stale hit before dispatching:

   ```bash
   git grep -n -i -E 'replace|parity|release|stable|ready|usable|install|version'
   ```

   List every hit with its disposition in the PR comment. The README's Known gaps list is checked
   against its sources in the same pass.

4. The pins in `packaging/appimage/pins.toml` are current. Each is a URL **and** a SHA-256, never
   `latest` or `continuous`; the build refuses a file whose hash differs. To change Chromium, edit
   `version`, `revision`, `url` and `sha256` together (the Lens lookup reads
   `usr/lib/ms-playwright/chromium-<revision>`, `BUNDLED_BROWSERS` in
   `crates/hatate-core/src/lens_browser/deps.rs`).

## 2. Build

The hosted job is the build of record: it runs on `ubuntu-latest`, so the image's glibc floor is
that runner's (printed by the build as `build host libc`; Ubuntu 24.04 is 2.39). To build the same
image by hand, on a machine with GTK 4 and libadwaita development files, `librsvg2-common`,
`fonts-dejavu-core`, `curl`, `unzip` and `sha256sum`:

```bash
git clone https://github.com/Dromares/hatate-rs && cd hatate-rs
git checkout <the commit that will be tagged>
cargo run -p xtask -- appimage --out /path/with/3GB/free 2>&1 | tee build.log
```

It writes `Hatate-x86_64.AppImage` to `--out`. The build fails if any bundled library, crate, font
or the browser has no licence entry, if a WOFF/WOFF2 face is in the image, or if a pinned input does
not hash to its pin. Keep `build.log`.

Smoke-test the result **on your own machine** before uploading:

```bash
./Hatate-x86_64.AppImage --version      # hatate <version> (Rust rewrite of Hatate), exit 0, no display
```

then launch it with throw-away `HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` and no Hydrus
configured. Never point it at a real profile.

## 3. Create the draft

Only after `main` is green (section 1) and the PR that last changed `release.yml` has merged: GitHub
dispatches a workflow only from the default branch, and answers 404 for one that exists only on a
branch. The executor, not the author of the change, runs:

```bash
gh workflow run release.yml --repo Dromares/hatate-rs --ref main -f version=0.2.1
gh run list --workflow release.yml --repo Dromares/hatate-rs --limit 1      # the run id
gh run watch <run id> --repo Dromares/hatate-rs --exit-status               # about 20 minutes
gh release view v0.2.1 --repo Dromares/hatate-rs --json isDraft,tagName,assets,targetCommitish
```

Dispatch it **once**. The `build` job checks that the input is the workspace version and the ref is
`main`, builds the AppImage, answers `--version`, writes `SHA256SUMS` and the release notes, and
uploads them as the `release-assets` artifact. The `draft` job then creates `v0.2.1` as a draft
(the tag does not exist until the release is published) and stops if a release of that name is
already there. Expect `isDraft=true` and two assets. The notes carry the AppImage's size, the glibc
floor, the SHA-256 and the licence line ("Hatate's own code is MIT; the AppImage also bundles
Chromium, GTK 4 and libadwaita and other components under their own licences, listed in `NOTICE`").

A dry run of the same build (no draft, no write permission) runs on every pull request that changes
`release.yml`, `Cargo.toml`, `xtask/`, `packaging/` or `crates/hatate-apprun/`. Read the commit and
the checksum from its job summary.

## 4. Non-author draft check

Done by someone who did not dispatch the workflow, on the assets **as uploaded to the draft**, not on a
local copy. Paste each output into the check.

1. From the draft's own asset, in an empty directory, download both assets with an authenticated
   `gh release download v<version> --repo Dromares/hatate-rs`, then
   `sha256sum -c --ignore-missing SHA256SUMS`.
2. `env HOME=$(mktemp -d) ./Hatate-x86_64.AppImage --version` prints `hatate 0.2.1 (Rust rewrite of
   Hatate)` (the version must be `0.2.1`), exit 0.
3. **Run the shipped binary's network paths, not just `--version`** (DAN-1909). rv-rs went public
   twice with an update check that panicked, because two draft checks had run only `--version` and
   `--help`. From the draft's own downloaded asset, in an empty directory:

   ```bash
   env HOME=$(mktemp -d) ./Hatate-x86_64.AppImage batch <dir with one synthetic image>; echo exit=$?
   ```

   with a throw-away `config.json` and **every engine the default config enables, once each** (one
   try, no retry), plus **one Lens search** with a fresh temporary profile, driven by the bundled
   Chromium. Pass: each engine ends in a result or a typed error on its row, `exit=0`, and no
   `panicked` and no `CryptoProvider` on stderr. A Cloudflare or Google challenge is recorded, never
   worked around. The anonymous API cannot see a draft, so this exercises the real TLS path of the
   binary against the real services. Hydrus and the MCP server are checked against a scripted local
   Hydrus and the app's own MCP port, never the board's.
4. **The bundled browser starts from inside the image** under Xvfb and answers one CDP
   `Browser.getVersion` on `about:blank`; `ps` shows Chromium running under the AppImage's mount, not
   the host's. No Lens request is needed for this step.
5. **A clean-system run**: a minimal Arch root filesystem (`bwrap`) with only the base system and
   Xvfb, binding nothing else. `--version` works there, the window starts under Xvfb, and `ldd` over
   `usr/bin/hatate-gtk` and the bundled `chrome` prints no `not found`.
6. Nothing is written under the mount; list what landed in the temporary XDG directories.
7. Every bundled component has a licence entry: `usr/share/licenses/INDEX.txt` lists them, and
   `NOTICE` is in the image root.

## 5. The DAN-1702 probes: is the draft really private?

While the release is a draft, anonymously (no token) each asset URL
`https://github.com/Dromares/hatate-rs/releases/download/v0.2.1/<asset>` returns 404, **and**
`https://api.github.com/repos/Dromares/hatate-rs/releases/tags/v0.2.1` returns 404. Never use the HTML
tag page: it can answer 200 for a tag even when the release is not public, so it proves nothing. The
probes, in full:

| Probe | Expect while draft | Expect after publish |
|-------|--------------------|----------------------|
| `curl -s -o /dev/null -w '%{http_code}' https://api.github.com/repos/Dromares/hatate-rs/releases/tags/v<version>` | `404` | `200` |
| each asset URL `https://github.com/Dromares/hatate-rs/releases/download/v<version>/<asset>` (`-L`, final status) | `404` | `200` |
| control: `.../releases/tags/v0.0.0-nope` | `404` | `404` |
| control: `.../releases/tags/<an already published tag>` | `200` | `200` |

If the first two return `200` before you published, the release is not a draft: stop and tell the
release owner. Use `v0.2.0` (published 2026-11-03) as the already published tag for the last control.

## 6. Publish

Only after the non-author check passes at the exact assets in the draft, **and** the board has said
yes on the release card. **Do not publish unless the draft-check verdict for these assets pastes the
section 4 output.** The publish ticket is filed only after the board accepts the card (the desktop
launch, the hardware check and the OK to publish):

```bash
gh release edit v<version> --draft=false --latest
```

Then re-run the probes in step 5 against the public assets; both asset URLs must answer `200` and
the downloaded file must match `SHA256SUMS`.

## 7. Clean up

Delete the build directory (the AppDir, the image and the downloaded Chromium are about a gigabyte).
Keep `build.log` and the check evidence with the release ticket.
