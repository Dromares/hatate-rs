# Releasing the Hatate AppImage

> **Status.** Nothing has been released. `hatate-rs` is still a work-in-progress Rust rewrite and
> its version is `0.0.0`. This file is the procedure that a release will follow once the board has
> approved one. **No tag, draft or release is created, and nothing is published, without the
> board's OK on its own card** (plan DAN-1961, Phase 7). The first release is planned as `0.2.0`,
> and it is made only by a separate publish ticket.

A release is one file, `Hatate-x86_64.AppImage`, plus `SHA256SUMS`, attached to a GitHub Release on
`Dromares/hatate-rs`. It is built by the hosted `appimage` job of `ci.yml` from a `main` commit,
uploaded as a **draft**, checked by someone other than the person who built it, and only then
published. Never publish first and check afterwards. No AUR, Flathub, crates.io or other registry,
ever (DAN-1665).

Roles follow the repository's usual split: the **author** builds and uploads the draft, a
**non-author** checks it, and the **publish executor** publishes.

## 1. Before building

1. `main` is green: the newest hosted `ci` run at the commit, with both jobs (`ci`, `appimage`)
   `completed` and `success`.
2. The version is changed in one place, `[workspace.package] version` in `Cargo.toml`.
   `hatate --version` must print it. While the claim is work in progress it also says so.
3. Re-run the status-claim sweep over the **whole tree** and fix every stale hit before tagging:

   ```bash
   git grep -n -i -E 'appimage|released|replacement|parity|usable|stable|0\.0\.|<old version>'
   ```

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
./Hatate-x86_64.AppImage --version      # hatate <version> (work-in-progress ...), exit 0, no display
```

then launch it with throw-away `HOME`, `XDG_CONFIG_HOME` and `XDG_CACHE_HOME` and no Hydrus
configured. Never point it at a real profile.

## 3. Upload a draft

Only with the board's approval on the release card:

```bash
sha256sum Hatate-x86_64.AppImage > SHA256SUMS
git tag v<version> <commit> && git push origin v<version>
gh release create v<version> --draft --title "Hatate <version>" \
    --notes-file notes.md Hatate-x86_64.AppImage SHA256SUMS
```

`--draft` is not optional. The notes carry the AppImage's size, the glibc floor, the SHA-256 and the
licence line ("Hatate's own code is MIT; the AppImage also bundles Chromium, GTK 4 and libadwaita
and other components under their own licences, listed in `NOTICE`").

## 4. Non-author draft check

Done by someone who did not build it, on the assets **as uploaded to the draft**, not on a local
copy. Paste each output into the check.

1. Download both assets with an authenticated `gh release download`, then
   `sha256sum -c --ignore-missing SHA256SUMS`.
2. `./Hatate-x86_64.AppImage --version` prints `hatate <version> ...`, exit 0.
3. **Run the shipped binary's network path, not just `--version`** (DAN-1909). rv-rs went public
   twice with an update check that panicked, because two draft checks had run only `--version` and
   `--help`. From the draft's own downloaded asset, in an empty directory:

   ```bash
   env HOME=$(mktemp -d) ./Hatate-x86_64.AppImage batch <dir with one generated noise PNG>; echo exit=$?
   ```

   with a throw-away `config.json` that turns on only IQDB and trace.moe (the engines that answer an
   anonymous upload). Pass: `exit=0` or a typed engine error on the row, and no `panicked` and no
   `CryptoProvider` on stderr. A Cloudflare challenge is recorded, never worked around. The anonymous
   API cannot see a draft, so this exercises the real TLS path of the binary against the real services.
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

While the release is a draft, the tag page on github.com can still return 200 (the git tag is public
even when the release is not), so the HTML tag page proves nothing. Use these instead, anonymously
(no token):

| Probe | Expect while draft | Expect after publish |
|-------|--------------------|----------------------|
| `curl -s -o /dev/null -w '%{http_code}' https://api.github.com/repos/Dromares/hatate-rs/releases/tags/v<version>` | `404` | `200` |
| each asset URL `https://github.com/Dromares/hatate-rs/releases/download/v<version>/<asset>` (`-L`, final status) | `404` | `200` |
| control: `.../releases/tags/v0.0.0-nope` | `404` | `404` |
| control: `.../releases/tags/<an already published tag>` | `200` | `200` |

If the first two return `200` before you published, the release is not a draft: stop and tell the
release owner. Until the first release exists there is no published tag for the last control; say so
in the check and skip only that row.

## 6. Publish

Only after the non-author check passes at the exact assets in the draft, **and** the board has said
yes on the release card:

```bash
gh release edit v<version> --draft=false --latest
```

Then re-run the probes in step 5 against the public assets; both asset URLs must answer `200` and
the downloaded file must match `SHA256SUMS`.

## 7. Clean up

Delete the build directory (the AppDir, the image and the downloaded Chromium are about a gigabyte).
Keep `build.log` and the check evidence with the release ticket.
