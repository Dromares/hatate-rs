# 7.c: crash reports, one version source, the probe, the audit image dump (DAN-2501)

Python source of truth: `Dromares/Hatate-Linux-Redux` at `76c85c4`. The two layout items of 7.c (Settings at its
minimum, the Queue file name) are in sections 5 and 6, filled in with their pins.

## 1. Crash reports (`core/crashlog.py`, 6.j F-B2)

Every binary installs a panic hook at startup: `hatate-gtk`, `hatate` (all commands), `hatate-mcp` and `AppRun`.
The hook appends one entry to `crash.log` (the path `paths.rs` names, `CONFIG_DIR/crash.log`) and then runs the hook
that was installed before it, so stderr is as it was. The file is the same one Python writes and has the same shape:
a run's section opens with `--- session started, faulthandler armed ---` (the window and `hatate batch` write it at
start, the other commands at their first panic), and an entry starts with `Fatal Rust panic: <message>`, then
indented `time:`, `version:`, `binary:`, `thread:` and `location:` lines and `Backtrace:` with the frames. The
Crashed banner counts a `Fatal Rust panic:` line like a `Fatal Python error:` one (`startup::faults_in_previous_run`),
and the existing `crash.log.running` marker still turns the banner on, because a panic does not remove it.

`crash_report.rs` is `std` only and names nothing in `crate::`, so `AppRun` and the `hatate-mcp` placeholder, which
have no dependencies, include that one source file with `#[path]`.

**Known gap: native signal crashes.** A SIGSEGV, or an abort from C code (GTK, SQLite), does not unwind and runs no
hook, so it leaves nothing in `crash.log`. Python's `faulthandler` writes the stack of every thread for those. The
next launch still finds the running marker and shows the Crashed banner, with "no new entry". Closing the gap needs
a signal handler (`sigaction` with an alternate stack), which is a different, larger piece of code and is not done.

A panic inside a Tokio task that the runtime catches also writes an entry (the hook runs where the panic starts),
although the process goes on. That is more than Python's `faulthandler`, which only sees a process-ending fault.

| Python | Rust |
|---|---|
| `crashlog.install` (arm the handler, write the mark) | `crates/hatate-core/src/crash_marker.rs` `fn arm_crash_log` `crash_report::arm(&paths.crash_log());` |
| `crashlog.install` (the exception hooks) | `crates/hatate-core/src/crash_marker.rs` `fn install_panic_hook` `crash_report::install(paths.crash_log(), binary, version::VERSION);` |
| the hook writes an entry and the default hook still runs | `crates/hatate-core/tests/crash_report.rs` `fn a_panic_is_written_to_crash_log_and_the_next_start_shows_the_banner` `assert!(stderr.contains("main thread gives up"), "{stderr}");` |
| the next start shows the banner | `crates/hatate-core/tests/crash_report.rs` `fn a_panic_is_written_to_crash_log_and_the_next_start_shows_the_banner` `assert!(matches!(banner, LaunchBanner::Crashed(_)), "{banner:?}");` |
| the entry is counted | `crates/hatate-gtk/src/startup.rs` `fn a_rust_panic_entry_counts_as_one_fault` `assert_eq!(faults_in_previous_run(&format!("\n{m}\n{entry}\n{m}\n")), 1);` |
| every binary installs it | `crates/hatate-core/tests/crash_report.rs` `fn every_binary_installs_the_hook` `assert!(main.contains(call), "{krate} does not call {call}");` |
| a command that never panics makes no file | `crates/hatate-core/tests/crash_report.rs` `fn a_run_that_never_panics_creates_no_crash_log` `assert!(!paths.crash_log().exists());` |

## 2. One version source (`core/version.py`)

`hatate_core::version::VERSION` is `CARGO_PKG_VERSION` of `hatate-core`, which inherits `[workspace.package] version`
(`0.0.0` when 7.c landed; step 7.d bumped it to `0.2.0`). `hatate --version` and the MCP `serverInfo.version` read it, and so does the
`crash.log` entry. There is **no About dialog** in either app (Python has none: `main.py --version` prints
`Hatate 0.1.0` and `packaging/appimage/inner-build.sh` stamps the number into the AppImage), so the ticket's third
surface does not exist; the window title shows no number (7.c: it said work in progress; 7.d: it is `Hatate`).

| Surface | Pin |
|---|---|
| the constant is the workspace version | `crates/hatate-core/src/version.rs` `fn the_version_is_the_workspace_package_version` `assert_eq!(line.trim(), format!("version = \"{VERSION}\""));` |
| every crate inherits it | `crates/hatate-core/src/version.rs` `fn every_crate_inherits_it` `"{} sets its own version",` |
| `hatate --version` | `crates/hatate-cli/tests/version.rs` `fn version_flag_prints_the_workspace_version` `hatate_core::version::VERSION` |
| MCP `serverInfo.version` | `crates/hatate-core/tests/mcp_server.rs` `fn server_info_reports_the_one_version_source` `json!(hatate_core::version::VERSION)` |

The version string differs from Python's by design (`0.0.0` against `0.1.0` when 7.c landed, `0.2.1` against `0.1.0` since DAN-2601 (7.d set `0.2.0`, which was published on 2026-11-03),
and `hatate` against `Hatate`, with a suffix naming the rewrite); there is nothing to differential-test on the number itself.

## 3. `core/parser_probe.py` (ported) and `tools/measure_tag_bands.py` (dropped)

**`parser_probe` is ported**, because the shipped Python app calls it at runtime. The call sites at `76c85c4`:

```
gui/parser_health_dialog.py:27   from core import parser_health, parser_probe
gui/parser_health_dialog.py:53       results = parser_probe.run(            (in _ProbeWorker.run)
gui/parser_health_dialog.py:123  | {site for site, _ in parser_probe.PROBES})
gui/parser_health_dialog.py:160  self.progress.setMaximum(len(parser_probe.PROBES))
gui/parser_health_dialog.py:108  self.check_button.clicked.connect(self._start_probe)   ("Check every site now")
gui/main_window.py:2202          self._run_dialog(ParserHealthDialog(self.settings, self))   (Help > Parser Health)
```

`hatate_core::parser_probe` is the module: `PROBES`, `NO_TAGS_EXPECTED`, `NO_DIMENSIONS_EXPECTED`, `ProbeResult`
with `summary`, and `run(timeout, on_progress, only)` over a `PageFetch`, the seam that `patch("core.boorus.fetch_page_info")`
is in Python's tests. `LiveFetch` is the shipped fetch: `fetch_page_info` over the engine's net layer and booru session,
so the check also fills the dialog's "This session" column, as Python's module-level health does. The shipped window
gives the dialog `hatate_gtk::parser_health::LiveProbe` (`Env::live_probe`), so the "Check every site now" button is on
and the "Live check" column is there (this closes DAN-2290's option 1, which shipped the check off until Phase 7 decided).
A window with no check (a test's) keeps the disabled button and its caption.

No test reaches a real site. The unit tests run over a scripted fetch; `tests/parser_probe_https.rs` runs `LiveFetch`
through `NetClient::new` against a local https server (the `CLAUDE.md` network-client rule). The probe's own difference
from Python is the text of a transport failure in the "Last problem" column, which is `reqwest`'s and not `requests`'s
(the same class as every other network error text, `docs/network-halves.md`); it carries the fixed reference URL and no
query, key or profile path, because the probe sends no credentials.

The Python tests of `TestParserProbe` (`tests/test_core_utils.py`):

| Python test | Rust assertion |
|---|---|
| `TestParserProbe.test_a_healthy_site_passes` | `crates/hatate-core/src/parser_probe.rs` `fn a_healthy_site_passes` `assert!(run_one(Ok(info(\|_\| {})), "danbooru").ok);` |
| `TestParserProbe.test_no_tags_is_a_fault_for_a_booru` | `crates/hatate-core/src/parser_probe.rs` `fn no_tags_is_a_fault_for_a_booru` `assert!(r.problem.unwrap().contains("no tags"));` |
| `TestParserProbe.test_no_tags_is_expected_for_sites_without_them` | `crates/hatate-core/src/parser_probe.rs` `fn no_tags_is_expected_for_sites_without_them` `assert!(run_one(Ok(info(\|p\| p.tags.clear())), "twitter").ok);` |
| `TestParserProbe.test_missing_dimensions_are_a_fault` | `crates/hatate-core/src/parser_probe.rs` `fn missing_dimensions_are_a_fault` `assert!(r.problem.unwrap().contains("dimensions"));` |
| `TestParserProbe.test_a_post_the_site_withholds_is_not_a_parser_fault` | `crates/hatate-core/src/parser_probe.rs` `fn a_post_the_site_withholds_is_not_a_parser_fault` `assert!(r.summary().contains("held back"));` |
| `TestParserProbe.test_a_fetch_failure_is_reported_not_raised` | `crates/hatate-core/src/parser_probe.rs` `fn a_fetch_failure_is_reported_not_raised` `assert!(r.problem.unwrap().contains("site is down"));` |
| `TestParserProbe.test_progress_is_reported_for_each_site` | `crates/hatate-core/src/parser_probe.rs` `fn progress_is_reported_for_each_site` `assert_eq!(last.0, last.1, "the last call should report completion");` |
| `TestParserProbe.test_every_probe_url_matches_the_parser_it_claims` | `crates/hatate-core/src/parser_probe.rs` `fn every_probe_url_matches_the_parser_it_claims` `assert_eq!(name, site, "{url}");` |

The dialog's own tests (`TestParserHealthDialog`, rows 1996 to 2020 of `docs/gtk-6e.md`) are unchanged; the one new
assertion is that the shipped window's dialog has the button on and the column:
`crates/hatate-gtk/tests/smoke.rs` `fn parser_health_dialog_and_warning` `assert!(dialog.check_button.is_sensitive());`.

**`tools/measure_tag_bands.py` is dropped.** It is a developer measurement tool: `git grep -n -l measure_tag_bands`
at `76c85c4` finds one file, the script itself; the AppImage build copies only `main.py core gui workers resources`.
The recorder it reads (`tag_band_metrics.rs`, the `HATATE_TAG_BAND_METRICS` file) stays.

## 4. The audit log's image dump (class A1, `docs/mcp-server.md`)

`core/mcp_audit.record` writes `outcome=result` through `json.dumps(default=str)`, so a `get_images` call put the
whole picture into `mcp_audit.jsonl` as `str(bytes)`. This build writes `<image: N bytes, sha256 HEX>` in that place
(`content::audit_image_text`), and nothing else about the line changes. Pins:

| What | Pin |
|---|---|
| both pictures of a call are recorded by size and hash, and no `b'` literal is in the file | `crates/hatate-core/tests/mcp_tool_handlers.rs` `fn the_audit_log_records_an_image_by_size_and_hash_not_by_its_bytes` `want(image_bytes(&blocks[2]))` |
| a call with no image writes the line Python writes (the key order, `json.dumps` text) | `crates/hatate-core/src/mcp/audit.rs` `fn line_format_is_pythons_json_dumps_with_the_fixed_key_order` (unchanged) |

## 5. Settings at the 560x440 minimum (M2, DAN-2488 a2)

At the stated minimum (`MIN_WIDTH` x `MIN_HEIGHT` in `settings_dialog.rs`, 560x440, **not raised**: that changes
DAN-2025 line 2.5) five pages scrolled sideways: General 769, Engine 666, Import 581, Hydrus 659 and Site Logins 510 px
in a 504 px view, so `max (seconds)`, the Tag service placeholder and `Fetch from Hydrus…` ran off the view. The layout
now gives way instead: a `Node::Line` wraps (a caption stays with the control after it), form captions, check-box
captions, group titles and drop-down rows wrap, and entries shrink to 8 characters (the Tag service entry is sized to its
placeholder, so `empty = "my tags" local service` shows whole and the button drops to the next line). The code is in
`settings_form.rs` (`fn wrap_caption`, `fn wrapping_drop`).

| What | Pin |
|---|---|
| no Settings page has more width than its view at the minimum (red at `12ed04c`: the five pages above) | `crates/hatate-gtk/tests/smoke.rs` `fn settings_pages_fit_their_view_at_the_minimum_size` `too_wide.is_empty(),` |

The fit test above measures the window's child, which a sideways-scrolling page never widens; this pin reads each
page's horizontal adjustment. Shots, contrast and the Tab table are on the ticket and the PR (hosted-CI shot path).

## 6. Queue: the whole file name (M3, DAN-2488 a2)

The File column cuts a long name in the middle. The pointer gets the whole name in the cell's tooltip. The keyboard's
focus is on the row, not on a cell, and a tooltip on the row widget crashed every Queue scenario (SIGSEGV, reverted), so
the keyboard gets the same name from a **Details line** under the table (Cloud's ruling on DAN-2511, option 2): one
selected row shows that row's whole File name; no row, several rows, or a filter that hides the selected row shows
nothing. The line wraps at the character and has a width of one character, so it never widens the page, and it is one
line tall when empty (`min-height: 18px`) so the table does not jump. It is not a Tab stop (a label).

| What | Pin |
|---|---|
| the File cell's tooltip is the whole name (the hover half) | `crates/hatate-gtk/tests/smoke.rs` `fn queue_file_cell_carries_the_whole_file_name` `assert_eq!(cell.tooltip.as_deref(), Some(long));` |
| one row selected shows its whole name; the line follows the selection | `crates/hatate-gtk/tests/smoke.rs` `fn queue_details_line_shows_the_selected_rows_whole_file_name` `assert_eq!(details(), long, "one row selected");` |
| several rows, no row, and a filter that hid the row show nothing | `crates/hatate-gtk/tests/smoke.rs` `fn queue_details_line_shows_the_selected_rows_whole_file_name` `assert_eq!(details(), "", "a filter hid the selected row");` |
| a real `Down` key (no pointer) lands on the long name | `crates/hatate-gtk/tests/smoke.rs` `fn queue_details_line_shows_the_selected_rows_whole_file_name` `assert_eq!(details(), long, "Down selected the long-named row");` |

Revert-and-red (`worktrees/DAN-2501-mutation.log`): dropping the `notify_selection` call fails at "one row selected";
dropping the `rebuild` call fails at "a filter hid the selected row".

## 7. The Python tests of `crashlog` and `version` (`TestCrashDiagnostics`, `TestVersionFlag`)

Nine tests at `76c85c4` exercise the two modules sections 1 and 2 port, and no earlier doc names them
(`docs/batch.md` deferred the class as "Phase 1's"; `docs/phase5-coverage.md` handed `crashlog.py` to Phase 7). The
tenth, `TestCrashDiagnostics.test_retiring_a_worker_does_not_warn`, is dispositioned at `docs/phase6-coverage.md` row 1453.
The rest of `tests/test_appimage_lens.py` (`TestLensBuild` and the other classes) stays with 7.d, which owns the AppImage
build; `TestVersionFlag` is here because it is the version source's test.

Python's `crashlog` has three hooks (`sys.excepthook`, `threading.excepthook`, and a Qt message handler) and
`faulthandler`. Rust has one hook (`std::panic`), which sees every thread, and no Qt: there is **no glib log handler**
in this tree (`git grep -nE 'log_set_|log_default|set_print_handler'` finds nothing), so a GTK warning or critical
goes to stderr as glib prints it and never reaches `crash.log`. That is a decision, not an oversight: a GTK warning is
not a crash, and `g_error` aborts like any native signal (the **Known gap** of section 1).

| Python test | Rust |
|---|---|
| `TestCrashDiagnostics.test_install_is_idempotent` | **Different, half pinned.** The section half: a hook installed and `arm` called gives one `--- session started` mark however often either runs (`ARMED`). `install` itself is not guarded (a second call chains a second hook), so what holds it to one is that each binary calls it once: `crates/hatate-core/tests/crash_report.rs` `fn a_panic_is_written_to_crash_log_and_the_next_start_shows_the_banner` `assert_eq!(marks, 1, "one section for the one run:\n{log}");` and `crates/hatate-core/tests/crash_report.rs` `fn every_binary_installs_the_hook` `assert!(main.contains(call), "{krate} does not call {call}");` |
| `TestCrashDiagnostics.test_python_exceptions_are_logged` | `crates/hatate-core/tests/crash_report.rs` `fn a_panic_is_written_to_crash_log_and_the_next_start_shows_the_banner` `"Fatal Rust panic: main thread gives up"` |
| `TestCrashDiagnostics.test_keyboard_interrupt_is_left_alone` | **n/a**: Rust has no `KeyboardInterrupt`. Ctrl-C is SIGINT, whose default action ends the process without unwinding, so no panic and no hook run; nothing is written, which is what the Python test asserts |
| `TestCrashDiagnostics.test_worker_thread_exceptions_are_logged` | `crates/hatate-core/tests/crash_report.rs` `fn a_panic_is_written_to_crash_log_and_the_next_start_shows_the_banner` `assert!(log.contains("  thread: worker-7\n"), "{log}");` |
| `TestCrashDiagnostics.test_a_qt_fatal_message_is_logged_before_it_aborts` | **n/a**: there is no Qt message handler and no glib log handler (above). The nearest case, a native abort, leaves no entry: the **Known gap** of section 1, which the running marker covers with the Crashed banner ("no new entry") |
| `TestCrashDiagnostics.test_qt_warnings_do_not_masquerade_as_crashes` | **n/a**: it tests the Qt handler's routing of `QtWarningMsg` to `log.warning` instead of `critical`. There is no handler to route; only a panic writes to `crash.log`, so a warning cannot appear there as a crash. Not asserted by a Rust test (nothing to assert against) |
| `TestCrashDiagnostics.test_a_real_disconnect_problem_still_warns` | **n/a**: it pins that the Qt handler quietens only the one benign `QObject::disconnect: wildcard call` message (`test_retiring_a_worker_does_not_warn`, `docs/phase6-coverage.md` row 1453) and still warns on any other disconnect text. No Qt, no handler, no allow-list |
| `TestVersionFlag.test_version_is_0_1_0` | **Design difference, not an equal assertion.** Python pins `0.1.0`; this build was `0.0.0` until 7.d bumped it to `0.2.0`. What is pinned is the single source: `crates/hatate-core/src/version.rs` `fn the_version_is_the_workspace_package_version` `assert_eq!(line.trim(), format!("version = \"{VERSION}\""));` |
| `TestVersionFlag.test_main_prints_version_and_exits_without_qt` | `crates/hatate-cli/tests/version.rs` `fn version_flag_prints_the_workspace_version` `assert!(out.status.success());`. The text differs by design (`hatate 0.2.1 (Rust rewrite of Hatate)` against `Hatate 0.1.0`, section 2; 7.c printed `hatate 0.0.0 (work-in-progress ...)`). "Without Qt" has no analogue: the `hatate` binary links no GTK |

The presubmit walk for 7.c's modules (`parser_probe`, `crashlog`, `version`) also found nine tests that touch `crashlog` or a
version and that no doc named, besides the ones above. They are the crash marker and the crash-log tail (ported earlier, in
`crash_marker.rs` and `logview.rs`) and one packaging test:

| Python test | Rust |
|---|---|
| `TestUncleanShutdownMarker.test_first_ever_launch_is_not_an_unclean_shutdown` | `crates/hatate-core/tests/startup_files.rs` `fn a_clean_run_leaves_no_marker_and_no_banner_for_the_next_launch` `assert!(!started.previous_run_unclean, "fresh profile");` |
| `TestUncleanShutdownMarker.test_marker_left_behind_means_the_last_run_crashed` | `crates/hatate-core/tests/startup_files.rs` `fn a_run_that_never_finishes_cleanly_is_seen_by_the_next_launch` `assert!(second.previous_run_unclean);` |
| `TestUncleanShutdownMarker.test_clean_shutdown_clears_the_marker` | `crates/hatate-core/tests/startup_files.rs` `fn a_clean_run_leaves_no_marker_and_no_banner_for_the_next_launch` `"marker removed on clean exit"` |
| `TestUncleanShutdownMarker.test_clearing_a_marker_that_was_never_written_does_not_raise` | `crates/hatate-core/tests/startup_files.rs` `fn closing_cleanly_a_run_that_never_wrote_a_marker_is_not_an_error` `.mark_clean_shutdown(1.0)` (new in this fixup; the first Rust test of the missing-marker case) |
| `TestUncleanShutdownMarker.test_full_cycle_two_launches` | the two launches and the clean third: `crates/hatate-core/tests/startup_files.rs` `fn a_run_that_never_finishes_cleanly_is_seen_by_the_next_launch` `assert!(second.previous_run_unclean);`, then `crates/hatate-core/tests/startup_files.rs` `fn a_clean_run_leaves_no_marker_and_no_banner_for_the_next_launch` `assert!(!next.previous_run_unclean);` (in two tests, not one sequence) |
| `TestCrashLogTail.test_missing_crash_log_reads_as_empty_not_an_error` | `crates/hatate-gtk/src/logview.rs` `fn the_ring_source_reads_the_ring_and_the_crash_tail` `assert_eq!(src.recent_crash_log(), "");` |
| `TestCrashLogTail.test_reads_back_what_was_written` | `crates/hatate-gtk/src/logview.rs` `fn the_ring_source_reads_the_ring_and_the_crash_tail` `assert!(src.recent_crash_log().contains("QThread: boom"));` (new in this fixup) |
| `TestCrashLogTail.test_only_the_tail_is_returned_for_a_log_grown_over_time` | `crates/hatate-gtk/src/logview.rs` `fn the_ring_source_reads_the_ring_and_the_crash_tail` `assert_eq!(src.recent_crash_log().chars().count(), CRASH_LOG_TAIL_CHARS);` |
| `TestWheelLock.test_same_versions_as_requirements_lock` | **n/a here, handed to 7.d**: it compares the AppImage's pinned wheels with `requirements-lock.txt`, which is the AppImage build's input. It matched the walk only through a local named `version`. The Rust build pins its tools in `packaging/appimage/pins.toml`, which 7.d owns |
