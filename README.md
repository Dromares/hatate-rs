# hatate-rs

Hatate finds where a picture comes from. Point it at a folder of images, let it search IQDB,
SauceNAO, trace.moe, ascii2d, Yandex, Google Images and Google Lens, review the matches next to
your picture, and send the ones you accept (with their tags) to Hydrus. It also runs as a command
(`hatate batch`) and serves an MCP endpoint for assistants.

`hatate-rs` is the Rust rewrite of [Hatate](https://github.com/Dromares/Hatate-Linux-Redux). From
0.2.1 it replaces the Python app: the Python app takes no new release, and it stays available at
that repository. It opens the Python app's config and session files (checked on a copy of a real
profile before the release); the [Known gaps](#known-gaps) below say what is different or not
done.

## Install

From this repository's [Releases](https://github.com/Dromares/hatate-rs/releases) page, download
`Hatate-x86_64.AppImage` and `SHA256SUMS`, then:

```
sha256sum -c --ignore-missing SHA256SUMS
chmod +x Hatate-x86_64.AppImage
./Hatate-x86_64.AppImage              # the window
./Hatate-x86_64.AppImage --version    # hatate 0.2.1 (Rust rewrite of Hatate)
```

The first launch of a version copies the image (about 550 MB) to `~/.cache/hatate` (or
`$XDG_CACHE_HOME/hatate`) and runs from that copy, so the window does not depend on the AppImage's
mount staying up; later launches reuse it, and a newer version removes the older copy. If the copy
cannot be made, it runs from the mount and prints one `WARNING` line.

The AppImage is x86_64 Linux only. It carries GTK 4, libadwaita and a pinned Chromium (used for
Google Lens), so it needs no packages of its own. The glibc floor is that of the build host
(Ubuntu 24.04, glibc 2.39), and the release notes state it. Without FUSE, run it with
`APPIMAGE_EXTRACT_AND_RUN=1`. This is the only way it ships: there is no AUR, Flathub or crates.io
package, and there will not be one. [`RELEASING.md`](RELEASING.md) is how a release is built and
checked.

## Known gaps

What 0.2.1 does not do, or does differently from the Python app.

**Lens path**
- **AVIF and JPEG 2000 are refused on the Lens path** (DAN-2183). Python converts them to JPEG and
  searches; this build ends that image with a typed error and does not search. The other engines
  are not affected (`docs/google-lens.md`, F1).
- **The 5.c Lens difference.** In the one live Lens search made while porting (5.c), 7 of the 8
  matches were identical and the 8th was a different video, unexplained (`docs/lens-search.md`).
  The release check (7.b: one search per app, on a copy of a real profile) got the same 8 URLs from
  both apps, with the order differing because `hatate batch` re-ranks, and the difference did not
  recur. That is one run each. It does not explain the first one: Lens can answer differently from
  one search to the next, and no more than that is claimed.

**Crashes**
- **Native signal crashes are not written to `crash.log`** (7.c). A Rust panic is; a SIGSEGV or an
  abort from C code (GTK, SQLite) is not, where Python's `faulthandler` wrote the stacks. The next
  start still shows the Crashed banner, saying there is no new entry (`docs/phase7-7c.md`).

**Window, accepted after the design reads** (the design-read ledger of DAN-2488, a2; contrast
figures are in `docs/phase6-contrast.md`)
- Parser Health: the column titles scroll away with the 13 rows.
- Spacing is 10/10/12 where Python uses 16/16/18/28 (F-B4).
- The em dash in the Booru, Cache and Sent columns reads 3.09 and 3.02, against 4.33 to 4.75 for
  the text in the same row (a graphic, with a 0.02 margin in the light scheme).
- The popover and menu edge reads 2.93 against the light page (the token, kept).
- "Check every site" reads 3.07 and 3.09 while inactive.
- The Differences letterbox in the light scheme is the panel colour `#f2ede1`.
- There is no inactive-selection cue (DAN-2213).
- The MCP audit Tool column shows three characters at 560 px wide (`send_to_hydrus` and
  `select_candidate` differ at the third).
- The Hydrus Test pass and fail differ only in wording, with no glyph.
- The Shortcuts list has a 300 px gap between label and key and no row rule (as in Python).
- The focus box of the notebook scroll arrow is faint, and the selected row of the service popover
  was never judged.
- The string "Qt fires neither" sits in the copy of a GTK app (DAN-2433); it was not re-read and is
  unverified.
- DAN-2442: the underline is 1 px where one doc line says 2, and the Hydrus delete confirmation has
  an uneven row wash.

**Not judged from screenshots** (checked by the maintainer on real hardware before a release, not
by CI): window chrome, the GPU renderer, HiDPI, the hover wash (an overlay scrollbar widening under
the pointer takes the 8 to 10 px gutter next to the Queue and Review lists), and the
real-compositor backdrop.

**Phase 6 accepted gaps** (`docs/phase6-coverage.md`, section 5.1): the Parser Health items of 6.d
and 6.e, the D-A and D-B conditions, 6.g F6 to F8, 6.h F4 and F5, conflicting rows not marked in the
Shortcuts list, the 32-stop Shortcuts Tab order, and the recorder's Esc hint. Section 5.2 of that
file records that no menu action is a disabled placeholder, so it lists no gap.

`hatate batch` is experimental: its flags and output may change.

## Development

`CLAUDE.md` holds the rules for working in this repo, `docs/` the method and the coverage tables,
and `RELEASING.md` the release procedure.

## License

MIT. See [LICENSE](LICENSE). The AppImage also bundles Chromium, GTK 4, libadwaita and other
components under their own licences, listed in [NOTICE](NOTICE).
