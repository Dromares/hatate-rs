# Phase 5 exit check: every module of the Python app, and where it went (DAN-2167)

Every `.py` file in `core/`, `core/boorus/`, `workers/` and `tools/` at `Hatate-Linux-Redux` `76c85c4`, with its Rust home or the phase that owns it and why. Cloud files Phase 6 from this table.

113 files: 86 ported, 12 partly ported, 13 not ported, 2 package markers. The table is generated from the listing of the tree at the pin by a script that fails if a file is missing or extra (script and listing are outside the repo; the check is `ls core core/boorus workers tools`).

**How to read the PR column.** A row that says *ported* names the merged PR that added the Rust home (`Dromares/hatate-rs`). *Partly* names the PR of what is ported and the phase that owns the rest. *Not ported* names the phase and says why. `gui/` is not in this table; it is Phase 6 as a whole, apart from `gui/mcp_bridge.py`, which is ported by this ticket (`docs/mcp-handlers.md`). `ops/` is not ported by decision. PR #55 is the number this PR is expected to take; it is corrected when the PR is opened.

**Phases.** *Phase 6* is the GTK application: the window, the list and review screens, the settings pages, the threads the window runs work on, and what is only presentation. *Phase 7* is packaging and the maintenance tools around the app (version stamping, the crash reporter, developer probes, the Plasma window hint).

| Python module | Rust home | Status | PR / owner | Note |
|---|---|---|---|---|
| `core/__init__.py` | - | n/a | - | Package marker. |
| `core/applog.py` | `rotating_log.rs` | partly | #8; Phase 6 | The rotating file is ported. The in-memory ring buffer for the log viewer dialog belongs to the GUI that shows it. |
| `core/ascii2d.py` | `engines/ascii2d.rs` | ported | #18, #27 | Parser (#18), client with block latch (#27). |
| `core/availability.py` | `availability.rs` | ported | #26 |  |
| `core/config.py` | `config/*` | ported | #5 |  |
| `core/crashlog.py` | `crash_marker.rs` | partly | #8; Phase 7 | The `crash.log.running` and `last_clean_exit` markers are ported. Python's faulthandler output is not written (`docs/cache-and-startup-files.md`); a Rust crash reporter is a packaging decision. |
| `core/desktop_attention.py` | `lens_browser.rs` (`WindowAttention` trait, no-op) | not ported | Phase 7 | KDE Plasma taskbar and attention script for the Lens window. Behind a trait with a no-op shipped implementation (`docs/lens-browser.md` section 1); needs an owner before Lens is wired for Plasma users. |
| `core/engine_alerts.py` | `engine_alerts.rs` | ported | #48 |  |
| `core/engine_runner.py` | `engine_runner.rs` | ported | #31, #48, #53 | Runner (#31); Lens wiring (#48, #53). |
| `core/engines.py` | `engine_ids.rs`, `net/pacing.rs` | ported | #23, #6 |  |
| `core/entry_filter.py` | - | not ported | Phase 6 | The filter bar's predicate over entries. It decides what the GUI shows, so it is built with the list view. `_matches_filter` of the MCP tools is a separate, smaller language and is ported (`mcp/tool_handlers.rs`). |
| `core/eta.py` | - | not ported | Phase 6 | The status bar's time-remaining estimate for a search run. |
| `core/export.py` | - | not ported | Phase 6 | CSV/JSON export of the list (File > Export). Pure functions over entries; ported with the menu that calls them. |
| `core/file_intake.py` | `batch/intake.rs` | ported | #35 |  |
| `core/formats.py` | `batch/intake.rs` | ported | #35 | The extension list the scan uses. |
| `core/google_images.py` | `engines/google_images.rs` | ported | #20, #29 | Parser (#20), request half (#29). |
| `core/google_lens.py` | `engines/google_lens.rs`, `engines/lens_prep.rs`, `engines/lens_rank.rs`, `engines/lens_search.rs`, `engines/lens_sites.rs` | ported | #21, #48, #53 | Parsers (#21), engine half (#48), per-search sites (#53). |
| `core/hard_timeout.py` | `hard_timeout.rs` | ported | #23 |  |
| `core/hydrus_client.py` | `hydrus_client.rs`, `hydrus_client/write.rs` | ported | #36, #41 | Read half (#36), write half (#41). |
| `core/hydrus_import.py` | `hydrus_import.rs`, `hydrus_import/url.rs`, `hydrus_url.rs` | ported | #43, #45, #26 | File upload (#43), URL import (#45). |
| `core/hydrus_import_poll.py` | `hydrus_import_poll.rs` | ported | #45 |  |
| `core/hydrus_reconcile.py` | `hydrus_reconcile.rs` | ported | #45 |  |
| `core/hydrus_tag_lookup.py` | `hydrus_tag_lookup.rs` | ported | #33, #36 |  |
| `core/image_compare.py` | `image_compare.rs` | ported | #28 |  |
| `core/image_diff.py` | `image_diff.rs` | ported | #37 |  |
| `core/image_prep.py` | `image_prep.rs` | ported | #23 |  |
| `core/iqdb.py` | `engines/iqdb.rs` | ported | #18 |  |
| `core/lens_browser.py` | `lens_browser.rs`, `lens_browser/*` | ported | #51, #53 | Lifecycle (#51), per-search page work (#53). |
| `core/logger.py` | `batch/matched_urls.rs` | ported | #35 | The matched-URL log. |
| `core/lru_cache.py` | `lru.rs` | ported | #24 |  |
| `core/mcp_audit.py` | `mcp/audit.rs` | ported | #50 |  |
| `core/mcp_content.py` | `mcp/content.rs` | ported | #50 |  |
| `core/mcp_images.py` | `mcp_images.rs` | ported | #55 | This ticket (DAN-2167); the PR number is filled in when it is opened. |
| `core/mcp_server.py` | `mcp/protocol.rs`, `mcp/server.rs`, `mcp/schema.rs`, `mcp/envelope.rs`, `mcp/pyvalidate.rs` | ported | #50 | FastMCP's wire, ported on hyper (`docs/mcp-server.md`). |
| `core/mcp_tools.py` | `mcp/tools.rs` | ported | #50 |  |
| `core/missing_files.py` | - | not ported | Phase 6 | Finds entries whose file is gone. Run at session restore by the GUI (with `workers/missing_file_worker.py`). |
| `core/models.py` | `model.rs`, `cache/model.rs` | ported | #7, #8 |  |
| `core/multipage.py` | `multipage.rs` | ported | #28 |  |
| `core/multipage_resolve.py` | `multipage_resolve.rs` | ported | #28 |  |
| `core/net.py` | `net/*`, `pyresponse.rs` | ported | #6, #24, #26 |  |
| `core/parser_health.py` | `parser_health.rs` | ported | #24 |  |
| `core/parser_probe.py` | - | not ported | Phase 7 | A developer tool that checks each parser against a live post on the real site. The Rust parsers are checked against the corpus; a live probe cannot run in CI (no live network) and is a maintenance tool, not part of the app. Phase 7 decides whether to keep one. The Parser Health button ships disabled (6.e, DAN-2290); if Phase 7 drops the probe, the button goes with it. |
| `core/paths.py` | `paths.rs` | ported | #5 |  |
| `core/pawchive_index.py` | `pawchive_index/*` | ported | #38, #44 | Index (#38), wiring into the engines (#44). |
| `core/pawchive_lookup.py` | `engines/pawchive_lookup.rs` | ported | #18 |  |
| `core/progress_ticker.py` | `progress.rs` | ported | #24 |  |
| `core/provenance_note.py` | `provenance_note.rs` | ported | #43 |  |
| `core/ranking.py` | `ranking.rs` | ported | #31 |  |
| `core/rate_limit.py` | `net/pacing.rs` | ported | #6 |  |
| `core/reddit.py` | `reddit.rs` | ported | #16, #24 | Parser (#16), feed GET (#24). |
| `core/remote.py` | `remote.rs` | ported | #24 |  |
| `core/saucenao.py` | `engines/saucenao.rs`, `quota_pause.rs` | ported | #18, #27, #8 |  |
| `core/search_cache.py` | `cache/*` | ported | #8 |  |
| `core/search_engine.py` | `search_engine.rs` | ported | #33 |  |
| `core/session.py` | `session/*` | ported | #7 |  |
| `core/session_db.py` | `session/db.rs` | ported | #7 |  |
| `core/shortcuts.py` | `mcp/tools.rs` (`REVIEW_ACTION_IDS` only) | partly | #50; Phase 6 | The review action ids the MCP registry maps to are ported. The key bindings and the user's overrides are the review screen's. |
| `core/sidecar.py` | - | not ported | Phase 6 | Tag sidecar `.txt` files next to the images. Pure planning functions; ported with the action that calls them. |
| `core/similarity_check.py` | `similarity_check.rs` | ported | #28 |  |
| `core/similarity_display.py` | `similarity_display.rs` | ported | #43 |  |
| `core/site_access.py` | `net/site.rs`, `net/secret.rs` | ported | #6 |  |
| `core/sites.py` | `sites.rs` | ported | #18 |  |
| `core/tag_band_metrics.py` | `tag_band_metrics.rs` | ported | #33 |  |
| `core/tag_borrowing.py` | `tag_borrowing.rs` | ported | #33 |  |
| `core/tag_colors.py` | - | not ported | Phase 6 | Hydrus namespace colours for the tag panel; presentation only. |
| `core/tag_rules.py` | `tag_rules.rs` | ported | #31 |  |
| `core/tracemoe.py` | `engines/tracemoe.rs` | ported | #18 |  |
| `core/upscale_detect.py` | `upscale_detect.rs` | ported | #37 |  |
| `core/version.py` | `Cargo.toml` workspace version | not ported | Phase 7 | The version number lives in one place. Rust's is the workspace version, `0.0.0`; the number beyond `0.0.x` is a release decision. |
| `core/viewport.py` | - | not ported | Phase 6 | Viewport arithmetic for lazy thumbnail loading in the list view. |
| `core/yandex.py` | `engines/yandex.rs` | ported | #20, #22 | Parser (#20), search half (#22). |
| `core/boorus/__init__.py` | `boorus/mod.rs` | ported | #10 |  |
| `core/boorus/_host.py` | `boorus/host.rs` | ported | #10 |  |
| `core/boorus/_parsed.py` | `boorus/parsed.rs` | ported | #10 |  |
| `core/boorus/_rating.py` | `boorus/rating.rs` | ported | #10 |  |
| `core/boorus/_remote_size.py` | `boorus/remote_size.rs` | ported | #24 |  |
| `core/boorus/_sizes.py` | `boorus/sizes.rs` | ported | #10 |  |
| `core/boorus/animepictures.py` | `boorus/animepictures.rs` | ported | #14, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/danbooru.py` | `boorus/danbooru.rs` | ported | #10, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/deviantart.py` | `boorus/deviantart.rs` | ported | #17, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/e621.py` | `boorus/e621.rs` | ported | #13, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/eshuushuu.py` | `boorus/eshuushuu.rs` | ported | #15, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/gelbooru.py` | `boorus/gelbooru.rs` | ported | #12, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/joyreactor.py` | `boorus/joyreactor.rs` | ported | #14, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/mangadex.py` | `boorus/mangadex.rs` | ported | #15, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/moebooru.py` | `boorus/moebooru.rs` | ported | #13, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/paheal.py` | `boorus/paheal.rs` | ported | #13, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/pawchive.py` | `boorus/pawchive.rs` | ported | #17, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/pixiv.py` | `boorus/pixiv.rs` | ported | #16, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/redditpost.py` | `boorus/redditpost.rs` | ported | #16, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/rule34.py` | `boorus/rule34.rs` | ported | #12, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/rule34us.py` | `boorus/rule34us.rs` | ported | #13, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/safebooru.py` | `boorus/safebooru.rs` | ported | #12, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/sankaku.py` | `boorus/sankaku.rs` | ported | #14, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/twitter.py` | `boorus/twitter.rs` | ported | #16, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/xbooru.py` | `boorus/xbooru.rs` | ported | #12, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `core/boorus/zerochan.py` | `boorus/zerochan.rs` | ported | #15, #24 | Parser; the fetch half is `boorus/fetch.rs` (#24). |
| `workers/__init__.py` | - | n/a | - | Package marker. |
| `workers/availability_worker.py` | `availability.rs` | partly | #26; Phase 6 | The check is ported; the thread that runs it and reports to the table is the GUI's. |
| `workers/candidate_worker.py` | `SearchEngine::fetch_candidate_details` | partly | #33; Phase 6 | Fetches one candidate's picture and tags when the user picks it. The call is ported (and used by `mcp_images`); the thread shell is the GUI's. |
| `workers/file_hash_worker.py` | `batch/hashing.rs` | ported | #35 |  |
| `workers/hydrus_import_poll_worker.py` | `hydrus_import_poll.rs` | partly | #45; Phase 6 | The poll is ported; the thread that drives it from the window is the GUI's. |
| `workers/hydrus_lookup_worker.py` | `hydrus_tag_lookup.rs` | partly | #33, #36; Phase 6 |  |
| `workers/hydrus_reconcile_worker.py` | `hydrus_reconcile.rs` | partly | #45; Phase 6 |  |
| `workers/missing_file_worker.py` | - | not ported | Phase 6 | Runs `core/missing_files.py` at session restore. |
| `workers/pawchive_index_worker.py` | `pawchive_index/crawl.rs` | partly | #38; Phase 6 | The crawl is ported; the Pawchive Index window's thread shell is the GUI's. |
| `workers/search_worker.py` | `batch/run.rs`, `batch/auto_import.rs`, `batch/staging.rs`, `batch/research.rs` | ported | #35, #47, #55 | The loop (#35), auto-import and staging (#47), the queue's re-search launcher (this ticket). |
| `workers/session_autosave_worker.py` | `batch/run.rs` (`Autosave`), `session/*` | partly | #35, #7; Phase 6 | The interval and the write are ported; the off-GUI-thread shell is the GUI's. |
| `workers/thumbnail_worker.py` | - | not ported | Phase 6 | Decodes row thumbnails for the list view. |
| `workers/upscale_check_worker.py` | `upscale_detect.rs` | partly | #37; Phase 6 | The check is ported; the thread shell is the GUI's. |
| `tools/mcp_stdio_bridge.py` | `mcp/bridge.rs`, `hatate-cli` `mcp-stdio-bridge` | ported | #50 |  |
| `tools/measure_tag_bands.py` | `tag_band_metrics.rs` (the recorder) | partly | #33; Phase 7 | The `HATATE_TAG_BAND_METRICS` recorder is ported. The offline report script is a measurement tool, not part of the app. |
| `tools/render_states.py` | - | not ported | Phase 6 | Renders the real main window offscreen for comparison with the mockup (DAN-1294). Redone against the GTK window. |

## What stays with Phase 6, in one list

Not ported or only partly ported, owner Phase 6: `core/applog.py`, `core/entry_filter.py`, `core/eta.py`, `core/export.py`, `core/missing_files.py`, `core/shortcuts.py`, `core/sidecar.py`, `core/tag_colors.py`, `core/viewport.py`, `workers/availability_worker.py`, `workers/candidate_worker.py`, `workers/hydrus_import_poll_worker.py`, `workers/hydrus_lookup_worker.py`, `workers/hydrus_reconcile_worker.py`, `workers/missing_file_worker.py`, `workers/pawchive_index_worker.py`, `workers/session_autosave_worker.py`, `workers/thumbnail_worker.py`, `workers/upscale_check_worker.py`, `tools/render_states.py`.

The queue model the MCP tools drive (`hatate_core::queue::QueueModel`) is the contract between this table and Phase 6: the GUI owns it, renders its `QueueEvent`s, and starts the MCP server on Settings > MCP (`McpServerController` plus `McpToolHandlers`). No CLI subcommand starts the server in Phase 5.
