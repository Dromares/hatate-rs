# File formats: what the Python app reads and writes

Contract for the Rust rewrite (Phase 1 implements it, Phase 7 checks it on a real
profile). Plan: DAN-1961 rev 3. Task: DAN-1966.

**Status of this document.** `hatate-rs` is a work-in-progress Rust rewrite, not yet a
replacement for Hatate. This file describes the *Python* app's on-disk formats so the
rewrite can open them unchanged. Nothing here says the Rust code does any of it yet.

## Conventions

- **Source of truth:** `Dromares/Hatate-Linux-Redux` at `origin/main` `76c85c4`
  ("Merge pull request #58"). Every citation `path:line` below is a path in *that* tree
  at *that* commit, for example `core/paths.py:30`. Nothing in the Python repo was
  modified.
- **Observed** means I ran the Python code (system Python 3.14, PyQt6 and Pillow
  installed) in a throw-away profile (`HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME` all
  pointed inside a fresh `mktemp -d`) and recorded what happened. The raw run output is
  `corpus/formats/behavior-probes.txt`. Anything marked *from source* was read, not run.
- **Samples** live in `corpus/formats/`. They were produced by calling the app's own
  functions (`Settings.load`, `session_db.save_entries`, `session.save_session`,
  `search_cache.save_cached_result`, `export.to_csv/to_json`, `sidecar.plan`, …) on
  synthetic entries inside a fresh profile. The synthetic entries use `/data/hatate-sample/…`
  paths, `example.invalid` hosts and invented post ids. The board's real profile, config,
  session and Hydrus were never touched. See
  [`corpus/formats/README.md`](../corpus/formats/README.md) for the inventory.
- "Must" / "must not" in section 6 are requirements on the Rust app. Elsewhere the text is
  descriptive.

## 1. Paths

### 1.1 Directories

| Directory | Value | Cite |
|---|---|---|
| `CONFIG_DIR` | `$XDG_CONFIG_HOME/hatate-linux`, else `~/.config/hatate-linux` | `core/paths.py:30` |
| `CACHE_DIR` | `$XDG_CACHE_HOME/hatate-linux`, else `~/.cache/hatate-linux` | `core/paths.py:43` |
| `AUTO_IMPORT_STAGING_DIR` | `<CACHE_DIR>/auto_import_staging` | `core/paths.py:44` |
| `PROJECT_DIR` / `RESOURCES_DIR` | the source checkout (or `$APPDIR` copy) and its `resources/`; read-only app assets, not user data | `core/paths.py:15-16` |

Resolution rules (all **observed**; the Rust app must resolve every case to the same directory Python does):

- The variable is used **when set, even when empty or relative**: `XDG_CONFIG_HOME=""`
  gives `CONFIG_DIR = hatate-linux` (relative to the working directory), and
  `XDG_CONFIG_HOME=rel/dir` gives `rel/dir/hatate-linux`. A literal `~` is not expanded.
  The XDG spec says to ignore an empty or relative value; the Python app does not. Copying
  the quirk costs nothing, and diverging would make a profile "disappear" for the one user
  who has such a value set, so the Rust app must not validate or normalise the variable.
- `HOME` comes from `Path.home()` (`core/paths.py:30,43`), i.e. `$HOME` first, then the
  passwd entry.
- `CONFIG_DIR` holds both configuration **and** state/data (session, logs, caches,
  the Chromium profile). Nothing is placed under `XDG_DATA_HOME` or `XDG_STATE_HOME`. The
  only reference to `XDG_DATA_HOME` in the tree is a test-sandbox helper
  (`tools/render_states.py:69`).
- `CONFIG_DIR` is chmod-ed `0700` and `config.json` `0600` whenever the config is loaded
  or saved (`core/config.py:48-49,548-549,721,727,743`). Other files are created with the
  process umask (observed: `session.db` `0644`, logs `0644`, sidecars `0644`, named session
  `0600`); modes of the whole sample profile are in `corpus/formats/schemas/file-modes.txt`.

### 1.2 Files and sub-directories under `CONFIG_DIR`

| Name | What | Cite | Section |
|---|---|---|---|
| `config.json` | settings (JSON) | `core/paths.py:31` | 2 |
| `config.json.bak` | previous config, copied before every save | `core/config.py:19,726` | 2.4 |
| `config.json.tmp` | transient: the new config before the atomic rename | `core/config.py:737` | 2.4 |
| `config.json.unreadable` | a config that failed to parse, renamed aside | `core/config.py:21,34` | 2.3 |
| `session.db` | the automatic session (SQLite, WAL) | `core/paths.py:36` | 3 |
| `session.db-wal`, `session.db-shm` | SQLite WAL files while a connection is open | `core/session_db.py:52` | 3.1 |
| `session.json` | legacy automatic session (JSON), read once and migrated | `core/paths.py:34` | 3.6 |
| `search_cache/<sha256>.json` | one search result per file hash | `core/paths.py:33`, `core/search_cache.py:70-71` | 4 |
| `matched_urls.log` (+ `.1` `.2` `.3`) | one line per found image; the path is configurable (`log_file_path`) | `core/paths.py:32`, `core/logger.py:16-17,26-30` | 5.1 |
| `app.log` (+ `.1` `.2` `.3`) | application log, rotating | `core/applog.py:15,46-48` | 5.2 |
| `crash.log` | faulthandler output, append-only | `core/crashlog.py:39,227` | 5.3 |
| `crash.log.running` | "a run is in progress" marker | `core/crashlog.py:52,110` | 5.3 |
| `last_clean_exit` | time of the last clean shutdown | `core/crashlog.py:59,127` | 5.3 |
| `mcp_audit.jsonl` (+ `.1`) | MCP server audit log | `core/mcp_audit.py:27,106` | 5.4 |
| `saucenao_quota_pause.json` | batch paused by SauceNAO's daily quota | `core/saucenao.py:275,297` | 5.5 |
| `pawchive_index.db` | local index of pawchive artists (SQLite) | `core/pawchive_index.py:52,85-108` | 5.6 |
| `pawchive_creators.json` | cached pawchive creator list, 7-day TTL | `core/pawchive_index.py:53,60,357-373` | 5.6 |
| `lens_profile/` | Chromium persistent profile for the Google Lens engine | `core/lens_browser.py:50,425,439-440` | 5.7 |

### 1.3 Files under `CACHE_DIR`

- `auto_import_staging/hatate-linux-auto-<random>/` with an `owner.pid` file inside
  (`workers/search_worker.py:39-47,138-140,300-303`). Temporary download staging for the
  auto-import path; stale ones whose owner PID is dead are swept at the next worker start
  (`workers/search_worker.py:129-145`). Not data to migrate.

### 1.4 Temporary files (system temp dir, `$TMPDIR` or `/tmp`)

`hatate-linux-*` directories (`gui/hydrus_query_dialog.py:95`, `gui/main_window.py:3812,3985`
with the prefix `hatate-linux-dl-`, `gui/mcp_bridge.py:502,538` with `hatate-linux-mcp-`)
and `hatate-hydrus-relate-*` (`core/hydrus_import.py:627`). Scratch space for downloads,
deleted by the code that made it. Not data to migrate.

### 1.5 Environment variables the app reads

| Variable | Used for | Cite |
|---|---|---|
| `XDG_CONFIG_HOME` | `CONFIG_DIR` | `core/paths.py:30` |
| `XDG_CACHE_HOME` | `CACHE_DIR`; also the Playwright browser cache (`ms-playwright/`), where **empty counts as unset** (`or`), unlike `paths.py` | `core/paths.py:43`, `core/lens_browser.py:255` |
| `HOME` | via `Path.home()` | `core/paths.py:30,43`, `core/lens_browser.py:255` |
| `HATATE_APPIMAGE` | `"1"` inside the AppImage; switches off the "pip install playwright" advice | `core/paths.py:27` |
| `PLAYWRIGHT_BROWSERS_PATH` | where the Lens Chromium lives; `"0"` means "inside the package" | `core/lens_browser.py:249-253` |
| `XDG_CURRENT_DESKTOP` | KDE detection for the attention flash | `core/desktop_attention.py:38` |
| `HATATE_TAG_BAND_METRICS` | opt-in: path of a JSONL file that tag-band measurements are appended to | `core/tag_band_metrics.py:41,325` |
| `TMPDIR` | via `tempfile` (1.4) | — |

There is **no** `HATATE_CONFIG_DIR`-style override and no command-line flag for the data
directory: `main.py` takes none (`main.py:13`, `--version` only).

### 1.6 AppImage

The AppImage uses the **same** `CONFIG_DIR` and `CACHE_DIR` as a source checkout: a user
who switches from `run.sh` to the AppImage keeps their profile. The only differences are
environment variables set by `AppRun`:

- `HATATE_APPIMAGE=1` (`packaging/appimage/AppRun:14`).
- `PLAYWRIGHT_BROWSERS_PATH=$APPDIR/usr/lib/ms-playwright` (`AppRun:17`).
- A generated fontconfig file under `${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}/hatate-fonts-<uid>`
  (`AppRun:37`). Runtime scratch, not profile data.
- Nothing is written into `$APPDIR`; it is a read-only mount (`AppRun:6-9`).

Source install only: `install.sh` writes `~/.local/share/applications/hatate-linux.desktop`
(`install.sh:25-26`), and `run.sh` builds `<checkout>/venv` (`run.sh:12-16`). Neither is
user data and neither applies to a Rust binary.

### 1.7 Files the app writes outside the profile

User-chosen locations, written only on an explicit action:

| File | Default name offered | Cite | Section |
|---|---|---|---|
| Named session (JSON) | `~/hatate-session-%Y%m%d-%H%M.json` | `gui/main_window.py:1636-1646` | 3.5 |
| Exported results (CSV or JSON) | `~/hatate-results-%Y%m%d-%H%M.csv` | `gui/main_window.py:1673-1698` | 5.8 |
| Tag sidecar `.txt` next to each image | `<image>.txt` or `<stem>.txt` | `core/sidecar.py:80-86` | 5.9 |
| Matched-URL log | wherever `log_file_path` points | `core/logger.py:33-43` | 5.1 |

## 2. Config: `config.json`

### 2.1 Format

- A single JSON **object**, UTF-8, **no BOM**. It is written as
  `json.dumps(asdict(settings), indent=2)` (`core/config.py:722`), so:
  2-space indent, items separated by `,` at the end of the line, `": "` after keys, keys in
  **dataclass field order** (the order of the tables below), non-ASCII escaped as
  `\uXXXX` (Python's default `ensure_ascii`), floats in Python `repr` form (`45.0`, not
  `45`), empty containers as `[]` and `{}`, and **no trailing newline**.
- Nested objects: `saucenao`, `hydrus`, `match_conditions`, `mcp` (one level, no deeper).
- Counts: the file has **79** top-level keys, of which 4 are the nested objects holding
  **24** keys between them: **103** keys in total (section 2.5 lists every one).
  `corpus/formats/fresh-profile/xdg-config/hatate-linux/config.json` is the file a fresh
  profile gets.
- Defaults come from the dataclasses `MatchConditions`, `SauceNaoSettings`,
  `HydrusSettings`, `McpSettings`, `Settings` (`core/config.py:127,138,151,160,191`).
  Four defaults are not constants: `log_file_path` is `str(DEFAULT_LOG_FILE)`, an **absolute
  path computed from `CONFIG_DIR` at the moment the object is built** (`core/config.py:394`),
  so a profile copied to another machine keeps pointing at the old path;
  `enabled_sites` is `ALL_SITE_OPTIONS` (`core/config.py:432`, `core/sites.py:126-128`: the
  site names sorted case-insensitively, then `"Other"`; 26 entries in the sample);
  `tag_blacklist` is `DEFAULT_TAG_BLACKLIST` (`core/config.py:101-124`, 22 tags);
  `enabled_tag_sources` is `["User","Hydrus","Booru","Hatate-linux"]` (`core/config.py:395-397`).

### 2.2 Read path (`Settings.load`, `core/config.py:512-698`)

1. Config file absent → build defaults and **save immediately** (`:514-518`).
2. Try `config.json`, then `config.json.bak`, parsing each with
   `json.loads(read_text("utf-8"))` (`:525-537`). A parse or I/O error moves on to the next
   source. If the *backup* was the one that worked, the unreadable main file is renamed to
   `config.json.unreadable` (`:534-536`).
3. Neither readable → defaults, and the main file is renamed to `.unreadable` so the next
   save cannot overwrite it (`:539-542`).
4. `chmod 0600` the file and `0700` the directory (`:548-549`).
5. `stored_version = raw.get("schema_version", 1)` (`:551`).
6. Apply every key (`:554-570`): for the four nested keys **when the value is a dict**,
   `Cls(**{**defaults_of_Cls, **value})`; otherwise `setattr` if `hasattr(Settings, key)`;
   otherwise the key is remembered and dropped (debug-logged, `:568-570`).
7. Run the one-time migrations (2.3) for every version above `stored_version`, then, if
   `stored_version < 13`, set `schema_version = 13` and **save** (`:693-696`).

There is no value validation at all: types, ranges and enumerations are taken as found
(observed: `"search_timeout": "abc"` and `"delay_min_seconds": null` load as-is).

### 2.3 Migrations (`CURRENT_SCHEMA_VERSION = 13`, `core/config.py:92`)

| From < | Effect | Cite |
|---|---|---|
| 2 | append `"Hydrus"` to `enabled_tag_sources` if absent | `:572-580` |
| 3 | append `"e621"`, `"Safebooru"` to `enabled_sites` if absent | `:582-590` |
| 4 | read legacy key `use_saucenao_fallback`: `true` → `secondary_engine_mode="always"`, `false` → `"disabled"` | `:592-605` |
| 5 | read legacy key `use_filename_hashes`: `true` → `hash_source="hydrus"`, `false` → `"local"` | `:607-619` |
| 6 | append `"Rule34"`, `"Xbooru"`, `"Twitter"`, `"AniList"` to `enabled_sites` | `:621-631` |
| 7 | append `"Paheal"` | `:633-639` |
| 8 | append `"MangaUpdates"` | `:641-648` |
| 9 | append `"MyAnimeList"` | `:650-654` |
| 10 | append `"Rule34.us"` **only if** `"Other"` is in `enabled_sites` | `:656-663` |
| 11 | append `"Reddit"` only if `"Other"` is enabled | `:665-670` |
| 12 | append `"Toon34"` only if `"Other"` is enabled | `:672-677` |
| 13 | set `filter_sent_tags_by_source = false` (pinned off) | `:679-691` |

The two legacy keys `use_saucenao_fallback` and `use_filename_hashes` are **read but never
written** (they exist only in the raw dict, and are absent from the dataclass, so the next
save drops them). Observed: `corpus/formats/legacy-migration/` holds a v1 config as input
and the result after one `Settings.load()`: `schema_version` 13, `hash_source="local"`,
`secondary_engine_mode="disabled"`, the sites appended in order, a `config.json.bak`
holding the *input* bytes.

A config with a **newer** `schema_version` (observed, `99`) loads as-is, no downgrade, and
is saved back with `99`; unknown top-level keys are dropped on that save.

### 2.4 Write path (`Settings.save`, `core/config.py:700-745`)

1. `mkdir -p CONFIG_DIR`, `chmod 0700` (`:720-721`).
2. If `config.json` exists: `shutil.copy2` it to `config.json.bak`, `chmod 0600`
   (`:724-731`; failure is a warning, not an error).
3. Write the payload to `config.json.tmp` created with mode `0600` through `os.open`
   (`:737-738`), `flush` + `fsync` (`:741-742`), `chmod 0600`, then
   `os.replace(tmp, config.json)` (`:743-744`). The directory is **not** fsynced.

When the app saves: on first run (`:517`), after a migration (`:696`), when the Preferences
dialog is accepted (`gui/settings_dialog.py:2825`), when a site is toggled
(`gui/main_window.py:789,798`), on theme change (`:4091`), after editing match conditions
(`:4103`), on **every clean exit** (`gui/main_window.py:4179`), and when DeviantArt rotates a
session cookie in the middle of a search (`core/site_access.py:361-362`). Consequence: every
launch/exit cycle of the Python app **rewrites the whole file in its own format**, dropping
unknown keys and re-normalising everything.

### 2.5 Every key

Types are the dataclass annotations. "Default" is the value in a fresh
`config.json`. "Read by" lists up to three consumers outside the Preferences dialog, then
the line where the dialog loads the value into its widget. "Written by" lists the code that
assigns the value in a running app. Both lists are produced by a scripted search plus manual
review (method in 2.7). Keys that hold credentials are marked **[secret]**.

#### Top-level keys (79)

| Key | Type | Default | Read by | Written by |
|---|---|---|---|---|
| `schema_version` | `int` | `13` | core/config.py:551 (stored_version) | core/config.py:695 (migration), every save via asdict (core/config.py:722) |
| `delay_min_seconds` | `float` | `45.0` | gui/activity_view.py:187, workers/search_worker.py:156, workers/search_worker.py:430, +1 more, dialog load gui/settings_dialog.py:319 | gui/settings_dialog.py:2669 |
| `search_timeout` | `float` | `30.0` | core/availability.py:123, core/engine_runner.py:371, core/engine_runner.py:373, +21 more, dialog load gui/settings_dialog.py:579 | gui/settings_dialog.py:2684 |
| `delay_max_seconds` | `float` | `75.0` | gui/activity_view.py:187, workers/search_worker.py:156, workers/search_worker.py:430, +1 more, dialog load gui/settings_dialog.py:322 | gui/settings_dialog.py:2670 |
| `engine_delays` | `Dict[str, List[float]]` | `{}` | workers/search_worker.py:454, dialog load gui/settings_dialog.py:339 | gui/settings_dialog.py:2683 |
| `hash_source` | `str` | `"hydrus"` | core/config.py:618, gui/main_window.py:1969, workers/file_hash_worker.py:73, +2 more, dialog load gui/settings_dialog.py:371 | core/config.py:615, gui/settings_dialog.py:2685 |
| `thumbnail_source` | `str` | `"hydrus"` | gui/main_window.py:2047, gui/main_window.py:2844, gui/main_window.py:2852, +1 more, dialog load gui/settings_dialog.py:391 | gui/settings_dialog.py:2686 |
| `lazy_thumbnails` | `bool` | `true` | gui/main_window.py:1454, gui/main_window.py:1903, gui/main_window.py:2024, +4 more, dialog load gui/settings_dialog.py:409 | gui/settings_dialog.py:2687 |
| `hash_workers` | `int` | `1` | gui/main_window.py:1968, dialog load gui/settings_dialog.py:422 | gui/settings_dialog.py:2688 |
| `retrieve_tags_from_booru` | `bool` | `true` | core/search_engine.py:148, dialog load gui/settings_dialog.py:870 | gui/settings_dialog.py:2689 |
| `drop_restricted_matches` | `bool` | `true` | core/search_engine.py:202, dialog load gui/settings_dialog.py:898 | gui/settings_dialog.py:2692 |
| `rank_matches_by_quality` | `bool` | `true` | core/search_engine.py:525, dialog load gui/settings_dialog.py:884 | gui/settings_dialog.py:2690 |
| `drop_dead_matches` | `bool` | `true` | core/search_engine.py:252, core/search_engine.py:364, core/search_engine.py:516, +2 more, dialog load gui/settings_dialog.py:887 | gui/settings_dialog.py:2691 |
| `borrow_tags_from_other_matches` | `bool` | `true` | core/tag_borrowing.py:103, dialog load gui/settings_dialog.py:994 | gui/settings_dialog.py:2693 |
| `borrow_tags_similarity_slack` | `float` | `5.0` | core/search_engine.py:105, core/tag_borrowing.py:111, tools/measure_tag_bands.py:321, +1 more, dialog load gui/settings_dialog.py:1001 | gui/settings_dialog.py:2694 |
| `continue_without_saucenao_on_quota` | `bool` | `false` | workers/search_worker.py:506, workers/search_worker.py:537, dialog load gui/settings_dialog.py:1565 | gui/settings_dialog.py:2798 |
| `retry_failed_searches` | `bool` | `true` | workers/search_worker.py:235, dialog load gui/settings_dialog.py:1015 | gui/settings_dialog.py:2695 |
| `remove_after_import` | `bool` | `false` | gui/main_window.py:2368, gui/main_window.py:3602, gui/main_window.py:3760, +10 more, dialog load gui/settings_dialog.py:1029 | gui/settings_dialog.py:2696 |
| `restore_session_on_start` | `bool` | `true` | gui/main_window.py:525, gui/main_window.py:4177, dialog load gui/settings_dialog.py:444 | gui/settings_dialog.py:2697 |
| `autosave_session` | `bool` | `true` | gui/session_autosave.py:65, dialog load gui/settings_dialog.py:447 | gui/settings_dialog.py:2698 |
| `autosave_interval_seconds` | `int` | `120` | gui/session_autosave.py:68, dialog load gui/settings_dialog.py:462 | gui/settings_dialog.py:2699 |
| `search_cache_ttl_days` | `float` | `30.0` | core/search_engine.py:356, gui/activity_view.py:192, dialog load gui/settings_dialog.py:859 | gui/settings_dialog.py:2700 |
| `use_search_cache` | `bool` | `true` | gui/activity_view.py:193, workers/search_worker.py:354, dialog load gui/settings_dialog.py:847 | gui/settings_dialog.py:2701 |
| `sankaku_cookies` | `str` | `""` | core/site_access.py:275, core/site_access.py:276, dialog load gui/settings_dialog.py:1167 | gui/settings_dialog.py:2710 |
| `animepictures_cookies` | `str` | `""` | core/site_access.py:279, dialog load gui/settings_dialog.py:1203 | gui/settings_dialog.py:2711 |
| `deviantart_cookies` | `str` | `""` | core/site_access.py:284, core/site_access.py:285, core/site_access.py:355, dialog load gui/settings_dialog.py:1247 | core/site_access.py:361, gui/settings_dialog.py:2723 |
| `e621_username` | `str` | `""` | core/site_access.py:84, dialog load gui/settings_dialog.py:1128 | gui/settings_dialog.py:2708 |
| `e621_api_key` | `str` | `""` | core/site_access.py:85, dialog load gui/settings_dialog.py:1139 | gui/settings_dialog.py:2709 |
| `pixiv_session_cookie` | `str` | `""` | core/site_access.py:273, core/site_access.py:274, dialog load gui/settings_dialog.py:1101 | gui/settings_dialog.py:2707 |
| `enable_tag_namespace_remap` | `bool` | `false` | core/tag_rules.py:103, dialog load gui/settings_dialog.py:1911 | gui/settings_dialog.py:2724 |
| `tag_namespace_remap` | `Dict[str, str]` | `{"artist": "creator"}` | core/tag_rules.py:99, core/tag_rules.py:103, core/tag_rules.py:106, dialog load gui/settings_dialog.py:1924 | gui/settings_dialog.py:2735 |
| `enable_tag_blacklist` | `bool` | `false` | core/tag_rules.py:171, dialog load gui/settings_dialog.py:1928 | gui/settings_dialog.py:2737 |
| `tag_blacklist` | `List[str]` | `["highres", "absurdres", "lowres", "incredibly_absurdres"…` | core/tag_rules.py:171, core/tag_rules.py:174, dialog load gui/settings_dialog.py:1950 | gui/settings_dialog.py:2741 |
| `add_rating_tag` | `bool` | `false` | core/tag_rules.py:134, dialog load gui/settings_dialog.py:1953 | gui/settings_dialog.py:2747 |
| `rating_tag_namespace` | `str` | `"rating"` | core/tag_rules.py:137, dialog load gui/settings_dialog.py:1968 | gui/settings_dialog.py:2748 |
| `auto_import_enabled` | `bool` | `false` | workers/search_worker.py:294, dialog load gui/settings_dialog.py:1037 | gui/settings_dialog.py:2702 |
| `auto_import_min_similarity` | `float` | `90.0` | workers/search_worker.py:314, workers/search_worker.py:552, workers/search_worker.py:675, dialog load gui/settings_dialog.py:1046 | gui/settings_dialog.py:2703 |
| `auto_import_method` | `str` | `"upload"` | workers/search_worker.py:296, workers/search_worker.py:314, workers/search_worker.py:568, dialog load gui/settings_dialog.py:1052 | gui/settings_dialog.py:2704 |
| `url_import_confirm_timeout` | `float` | `60.0` | gui/main_window.py:3889, workers/search_worker.py:612, dialog load gui/settings_dialog.py:1066 | gui/settings_dialog.py:2705 |
| `url_import_confirm_interval` | `float` | `2.0` | gui/main_window.py:3890, workers/search_worker.py:613, dialog load gui/settings_dialog.py:1078 | gui/settings_dialog.py:2706 |
| `set_hydrus_duplicate_relationships` | `bool` | `false` | core/hydrus_import.py:486, core/hydrus_import.py:666, workers/search_worker.py:584, dialog load gui/settings_dialog.py:1621 | gui/settings_dialog.py:2811 |
| `write_hydrus_provenance_note` | `bool` | `false` | core/hydrus_import.py:552, workers/search_worker.py:585, dialog load gui/settings_dialog.py:1644 | gui/settings_dialog.py:2812 |
| `hydrus_provenance_note_name` | `str` | `""` | core/hydrus_import.py:582, dialog load gui/settings_dialog.py:1668 | gui/settings_dialog.py:2813 |
| `enable_hydrus_tag_colors` | `bool` | `false` | core/tag_colors.py:47, dialog load gui/settings_dialog.py:1983 | gui/settings_dialog.py:2757 |
| `tag_namespace_colors` | `Dict[str, str]` | `{}` | core/tag_colors.py:50, dialog load gui/settings_dialog.py:1994 | gui/settings_dialog.py:2767 |
| `log_matched_urls` | `bool` | `true` | workers/search_worker.py:379, dialog load gui/settings_dialog.py:475 | gui/settings_dialog.py:2784 |
| `log_file_path` | `str` | `<CONFIG_DIR>/matched_urls.log`, absolute, frozen at first save | gui/main_window.py:2195, workers/search_worker.py:383, dialog load gui/settings_dialog.py:478 | gui/settings_dialog.py:2785 |
| `enabled_tag_sources` | `List[str]` | `["User", "Hydrus", "Booru", "Hatate-linux"]` | core/config.py:578, core/config.py:580, core/models.py:432, +2 more, dialog load gui/settings_dialog.py:494 | gui/settings_dialog.py:2786 |
| `filter_sent_tags_by_source` | `bool` | `false` | core/models.py:463, dialog load gui/settings_dialog.py:508 | core/config.py:689, gui/settings_dialog.py:2791 |
| `sidecar_filename_style` | `str` | `"append"` | core/sidecar.py:70 | gui/settings_dialog.py:2792 |
| `sidecar_overwrite` | `str` | `"ask"` | core/sidecar.py:76, dialog load gui/settings_dialog.py:543 | gui/settings_dialog.py:2793 |
| `tags_for_found` | `List[str]` | `[]` | core/tag_rules.py:319, dialog load gui/settings_dialog.py:2028 | gui/settings_dialog.py:2753 |
| `tags_for_not_found` | `List[str]` | `[]` | core/tag_rules.py:323, dialog load gui/settings_dialog.py:2038 | gui/settings_dialog.py:2754 |
| `tags_for_low_tag_count` | `List[str]` | `[]` | core/tag_rules.py:321, dialog load gui/settings_dialog.py:2051 | gui/settings_dialog.py:2755 |
| `enabled_sites` | `List[str]` | `["AniList", "Anime-Pictures", "Danbooru", "DeviantArt", "…` | core/config.py:587, core/config.py:590, core/config.py:627, +19 more | gui/main_window.py:787, gui/main_window.py:796 |
| `primary_engine` | `str` | `"iqdb"` | core/engine_runner.py:133, core/engines.py:129, gui/activity_view.py:132, +2 more, dialog load gui/settings_dialog.py:591 | gui/settings_dialog.py:2769 |
| `secondary_engine_mode` | `str` | `"always"` | core/config.py:604, core/engine_runner.py:135, core/engines.py:131, +2 more, dialog load gui/settings_dialog.py:598 | core/config.py:601, gui/settings_dialog.py:2770 |
| `fallback_below_similarity` | `float` | `0.0` | core/engine_runner.py:67, core/engines.py:169, core/engines.py:179, +1 more, dialog load gui/settings_dialog.py:610 | gui/settings_dialog.py:2771 |
| `extras_only_as_fallback` | `bool` | `false` | core/engine_runner.py:163, core/engine_runner.py:164, core/engines.py:150, +2 more, dialog load gui/settings_dialog.py:644 | gui/settings_dialog.py:2772 |
| `enable_ascii2d` | `bool` | `true` | core/engine_runner.py:140, core/engines.py:137, gui/activity_view.py:147, dialog load gui/settings_dialog.py:659 | gui/settings_dialog.py:2773 |
| `enable_tracemoe` | `bool` | `false` | core/engine_runner.py:142, core/engines.py:139, gui/activity_view.py:148, dialog load gui/settings_dialog.py:669 | gui/settings_dialog.py:2774 |
| `enable_iqdb3d` | `bool` | `false` | core/engine_runner.py:144, core/engines.py:141, gui/activity_view.py:149, dialog load gui/settings_dialog.py:691 | gui/settings_dialog.py:2775 |
| `enable_google_images` | `bool` | `false` | core/engine_runner.py:146, core/engines.py:143, gui/activity_view.py:150, dialog load gui/settings_dialog.py:699 | gui/settings_dialog.py:2776 |
| `enable_google_lens` | `bool` | `false` | core/engine_runner.py:148, core/engines.py:145, gui/activity_view.py:151, dialog load gui/settings_dialog.py:738 | gui/settings_dialog.py:2778 |
| `enable_yandex` | `bool` | `false` | core/engine_runner.py:150, core/engines.py:147, gui/activity_view.py:152, dialog load gui/settings_dialog.py:795 | gui/settings_dialog.py:2780 |
| `enable_pawchive` | `bool` | `false` | core/engine_runner.py:175, core/engines.py:156, gui/activity_view.py:142, dialog load gui/settings_dialog.py:814 | gui/settings_dialog.py:2781 |
| `enable_pawchive_index` | `bool` | `true` | core/engine_runner.py:173, core/engines.py:154, gui/activity_view.py:144, dialog load gui/settings_dialog.py:832 | gui/settings_dialog.py:2782 |
| `lens_search_whole_image` | `bool` | `true` | core/engine_runner.py:680, dialog load gui/settings_dialog.py:778 | gui/settings_dialog.py:2779 |
| `google_images_api_key` | `str` | `""` | core/engine_runner.py:521, dialog load gui/settings_dialog.py:723 | gui/settings_dialog.py:2777 |
| `tracemoe_min_similarity` | `float` | `85.0` | core/engine_runner.py:478, dialog load gui/settings_dialog.py:685 | gui/settings_dialog.py:2783 |
| `saucenao` | `SauceNaoSettings` | `{"api_key": "", "use_json_api": true, "min_similarity": 6…` | core/config.py:557-558 (nested load); leaves below | core/config.py:558; leaves below |
| `hydrus` | `HydrusSettings` | `{"api_url": "http://127.0.0.1:45869", "access_key": "", "…` | core/config.py:559-560 (nested load); leaves below | core/config.py:560; leaves below |
| `match_conditions` | `MatchConditions` | `{"min_width_gain_percent": 10.0, "min_height_gain_percent…` | core/config.py:561-562 (nested load); leaves below | core/config.py:562; leaves below |
| `mcp` | `McpSettings` | `{"enabled": false, "port": 8787, "token": "", "dry_run": …` | core/config.py:563-564 (nested load); leaves below | core/config.py:564; leaves below |
| `review_shortcuts` | `Dict[str, str]` | `{}` | gui/review_shortcuts.py:201, gui/review_view.py:581, dialog load gui/settings_dialog.py:2098 | gui/settings_dialog.py:2668 |
| `theme` | `str` | `"system"` | main.py:88, gui/main_window.py:688, 854, 928, 930 | gui/main_window.py:4090 |
| `window_geometry` | `str` | `""` | never read | never written (declared, always saved as its default) |
| `table_header_state` | `str` | `""` | gui/main_window.py:944, gui/main_window.py:2608 | gui/main_window.py:2617, gui/main_window.py:2624, gui/main_window.py:2628, gui/main_window.py:2659 |
| `table_header_columns` | `int` | `0` | gui/main_window.py:2611, gui/main_window.py:2614 | gui/main_window.py:2618, gui/main_window.py:2662 |

#### Nested keys (24, inside `saucenao`, `hydrus`, `match_conditions`, `mcp`)

| Key | Type | Default | Read by | Written by |
|---|---|---|---|---|
| `match_conditions.min_width_gain_percent` | `float` | `10.0` | core/search_engine.py:675, gui/match_conditions_dialog.py:32 | gui/match_conditions_dialog.py:59 |
| `match_conditions.min_height_gain_percent` | `float` | `10.0` | core/search_engine.py:675, gui/match_conditions_dialog.py:38 | gui/match_conditions_dialog.py:60 |
| `match_conditions.min_tags_for_good` | `int` | `5` | core/search_engine.py:104, core/search_engine.py:128, core/search_engine.py:667, +3 more | gui/match_conditions_dialog.py:58 |
| `match_conditions.require_larger_than_local` | `bool` | `true` | core/search_engine.py:670, gui/match_conditions_dialog.py:42 | gui/match_conditions_dialog.py:61 |
| `saucenao.api_key` | `str` | `""` | core/saucenao.py:365, core/saucenao.py:378, dialog load gui/settings_dialog.py:1532 | gui/settings_dialog.py:2795 |
| `saucenao.use_json_api` | `bool` | `true` | core/saucenao.py:365, dialog load gui/settings_dialog.py:1537 | gui/settings_dialog.py:2796 |
| `saucenao.min_similarity` | `float` | `60.0` | core/engine_runner.py:410, dialog load gui/settings_dialog.py:1571 | gui/settings_dialog.py:2799 |
| `saucenao.tag_types` | `List[str]` | `["character", "material"]` | core/saucenao.py:533, dialog load gui/settings_dialog.py:1584 | gui/settings_dialog.py:2801 |
| `saucenao.namespace` | `str` | `""` | core/saucenao.py:542, gui/mcp_bridge.py:629, dialog load gui/settings_dialog.py:1574 | gui/settings_dialog.py:2800 |
| `saucenao.dbmask` | `int` | `999` | core/saucenao.py:366, 379, 550 | never written by the app (hand-edit only) |
| `saucenao.pause_search_on_quota_exhausted` | `bool` | `true` | workers/search_worker.py:531, dialog load gui/settings_dialog.py:1549 | gui/settings_dialog.py:2797 |
| `hydrus.api_url` | `str` | `"http://127.0.0.1:45869"` | core/hydrus_client.py:137, core/hydrus_client.py:145, core/hydrus_client.py:155, +4 more, dialog load gui/settings_dialog.py:1597 | gui/settings_dialog.py:2807 |
| `hydrus.access_key` | `str` | `""` | core/hydrus_client.py:129, core/hydrus_client.py:130, core/hydrus_tag_lookup.py:154, +12 more, dialog load gui/settings_dialog.py:1600 | gui/settings_dialog.py:2808 |
| `hydrus.verify_ssl` | `bool` | `true` | core/hydrus_client.py:141, core/hydrus_client.py:159, core/hydrus_client.py:168, +2 more, dialog load gui/settings_dialog.py:1618 | gui/settings_dialog.py:2810 |
| `hydrus.tag_service_key` | `str` | `""` | core/hydrus_client.py:713, core/hydrus_client.py:759, dialog load gui/settings_dialog.py:1607 | gui/settings_dialog.py:2809 |
| `hydrus.timeout` | `float` | `30.0` | core/hydrus_client.py:140, 158, 167, 590, 642; core/mcp_images.py:83 | never written by the app (hand-edit only) |
| `mcp.enabled` | `bool` | `false` | core/mcp_server.py:12, core/mcp_server.py:99, gui/main_window.py:541, +4 more, dialog load gui/settings_dialog.py:2228 | gui/settings_dialog.py:2815 |
| `mcp.port` | `int` | `8787` | core/mcp_server.py:135, core/mcp_server.py:170, core/mcp_server.py:179, +2 more, dialog load gui/settings_dialog.py:2241 | gui/settings_dialog.py:2816 |
| `mcp.token` | `str` | `""` | core/mcp_server.py:101, core/mcp_server.py:239, gui/main_window.py:4056, +1 more, dialog load gui/settings_dialog.py:2248 | gui/settings_dialog.py:2817 |
| `mcp.dry_run` | `bool` | `false` | gui/mcp_bridge.py:177, gui/mcp_bridge.py:180, gui/mcp_bridge.py:187, +1 more, dialog load gui/settings_dialog.py:2263 | gui/settings_dialog.py:2818 |
| `mcp.allow_research` | `bool` | `false` | core/mcp_tools.py:60, dialog load gui/settings_dialog.py:2287 | gui/settings_dialog.py:2819 |
| `mcp.allow_hydrus_writes` | `bool` | `true` | core/mcp_tools.py:62, core/mcp_tools.py:64, core/mcp_tools.py:66, dialog load gui/settings_dialog.py:2275 | gui/settings_dialog.py:2820 |
| `mcp.allow_destructive` | `bool` | `false` | core/mcp_tools.py:68, core/mcp_tools.py:70, dialog load gui/settings_dialog.py:2298 | gui/settings_dialog.py:2821 |
| `mcp.audit_log_max_bytes` | `int` | `5000000` | gui/mcp_bridge.py:173, gui/mcp_bridge.py:192, dialog load gui/settings_dialog.py:2563 | gui/settings_dialog.py:2822 |

**Secrets.** These keys hold credentials and must never be logged, printed, copied into
fixtures or sent anywhere except the service they belong to: `saucenao.api_key`,
`hydrus.access_key`, `mcp.token`, `google_images_api_key`, `e621_api_key`,
`e621_username`, `pixiv_session_cookie`, `sankaku_cookies`, `animepictures_cookies`,
`deviantart_cookies` (`core/config.py:42-47`, `core/mcp_audit.py:36` for the redaction list).

### 2.6 Keys with no reader, no writer, or only a legacy role

- **Declared, never read, never written:** `window_geometry`
  (`core/config.py:506`). `git grep -w window_geometry` finds only the declaration. It is
  always saved as `""`. Keep the key (see 6.1) but give it no behaviour.
- **Read, never written by the app:** `hydrus.timeout` (the Hydrus request timeout; the
  Preferences dialog has no field) and `saucenao.dbmask` (SauceNAO database mask, `999` =
  all). Both can only be changed by editing the file by hand, so a user who did must keep
  the value.
- **Read from the raw JSON, never written:** `use_saucenao_fallback`,
  `use_filename_hashes` (2.3).
- **Written, never read back as such:** `schema_version` is written on every save but only
  read as `stored_version` at load. `table_header_state`/`table_header_columns` are Qt
  (`QHeaderView.saveState`, base64) layout blobs; the Rust GUI cannot interpret them
  (see 6.1).
- **Written by something other than the Preferences dialog:** `theme`
  (`gui/main_window.py:4090`), `enabled_sites` (`:787,796`), `table_header_state`,
  `table_header_columns` (`:2617-2662`), `deviantart_cookies` (`core/site_access.py:361`),
  `match_conditions.*` (`gui/match_conditions_dialog.py:58-61`), and `schema_version`/
  `hash_source`/`secondary_engine_mode`/`filter_sent_tags_by_source`/`enabled_tag_sources`/
  `enabled_sites` by migrations (2.3).

### 2.7 Value domains (strings that behave like enums)

Nothing validates these on load (2.2). The code falls back as shown.

| Key | Values | Behaviour for anything else | Cite |
|---|---|---|---|
| `hash_source` | `"hydrus"`, `"local"` | falls back to reading every file | `workers/file_hash_worker.py:70-73` |
| `thumbnail_source` | `"hydrus"`, `"local"`, `"off"` | compared by equality with `"off"`/`"hydrus"` | `gui/main_window.py:2047` |
| `primary_engine` | `"iqdb"`, `"saucenao"` | falls back to `"iqdb"` | `core/engines.py:31,92-100` |
| `secondary_engine_mode` | `"always"`, `"fallback"`, `"disabled"` | treated as the default mode | `core/engines.py:131` |
| `auto_import_method` | `"upload"`, `"url_importer"`, `"download_send"` | anything but the last two takes the upload path | `workers/search_worker.py:296,568` |
| `theme` | `"system"`, `"dark"`, `"light"` | `set_theme` ignores a mode not in `MODES`; `resolve_mode` is used at startup | `gui/theme.py:128`, `gui/main_window.py:4088`, `main.py:88` |
| `sidecar_filename_style` | `"append"`, `"replace"` | `"append"` | `core/sidecar.py:62-71` |
| `sidecar_overwrite` | `"ask"`, `"skip"`, `"overwrite"` | `"ask"` | `core/sidecar.py:74-77` |
| `enabled_tag_sources` | `User`, `Search engine`, `Booru`, `Hatate-linux`, `Hydrus` (the `TagSource` values) | names that match no source are dropped | `core/models.py:12-18,431-439` |
| `engine_delays` keys | engine ids `iqdb saucenao ascii2d tracemoe iqdb3d googleimages googlelens yandex pawchive pawchiveindex`; value `[min_s, max_s]` | an absent engine uses the global delay | `core/engines.py:16-25`, `gui/settings_dialog.py:2675-2683` |
| `review_shortcuts` | `{action_id: key sequence}`; **only ids whose binding differs from the default** are stored; `""` means "deliberately cleared" | unknown ids are pruned on the next dialog save | `core/shortcuts.py:166-178`, `gui/settings_dialog.py:2668` |
| `tag_namespace_colors` | `{namespace: "#rrggbb"}`; key `""` is the unnamespaced colour | built-in defaults in `DEFAULT_NAMESPACE_COLORS` | `core/tag_colors.py:28-37,47-56` |
| `enabled_sites` | the site names of `ALL_SITE_OPTIONS` | unknown names are simply never matched | `core/sites.py:126-128` |

### 2.8 How the keys were enumerated, and the counts

The tables in 2.5 have one row per field of the five config dataclasses.

Count 1, straight from the source, one `awk` over `core/config.py` (a declaration is a
4-space-indented `name: Type` line inside one of the five classes):

```
$ awk '/^class /{inc=($2 ~ /^(MatchConditions|SauceNaoSettings|HydrusSettings|McpSettings|Settings)[(:]/); cls=$2} \
       inc&&/^    [a-z_][a-z0-9_]*: /{print cls}' core/config.py | sort | uniq -c
      5 HydrusSettings:
      4 MatchConditions:
      8 McpSettings:
      7 SauceNaoSettings:
     79 Settings:
```

→ 5 + 4 + 8 + 7 + 79 = **103** declarations; the 79 include the four nested objects, so
75 plain top-level keys + 4 objects + 24 nested leaves.

Count 2, from the file the app itself wrote for a fresh profile
(`corpus/formats/fresh-profile/xdg-config/hatate-linux/config.json`):

```
$ grep -c -E '^  "[a-z_0-9]+":' config.json      # indent 2: top-level keys
79
$ awk '/^  "(saucenao|hydrus|match_conditions|mcp)": \{/{n=$1; inb=1; next} inb&&/^  \}/{inb=0} \
       inb&&/^    "/{c[n]++} END{for(k in c) print k, c[k]}' config.json
"saucenao": 7
"hydrus": 5
"match_conditions": 4
"mcp": 8
```

→ 79 top-level + (7 + 5 + 4 + 8 = 24) nested = **103**. (A plain `grep -c` at indent 4 gives 25,
not 24: the extra one is `"artist"`, an entry *inside* the `tag_namespace_remap` map, which is
data, not a config key.)

Doc count: the two tables in 2.5 hold **79 + 24 = 103 rows**. All three numbers agree.

Readers and writers per key came from a scripted search over `core/`, `gui/`, `workers/`,
`tools/` and `main.py` for `.<key>` and `"<key>"` followed by a manual review of every row.
False positives removed: the redaction list at `core/mcp_audit.py:36`, the audit entry's own
`dry_run` argument at `core/mcp_audit.py:76`, the `schema_version` of *other* formats
(`core/export.py:192`, `core/session_db.py:116`), and the throw-away `Settings()` that the
Preferences dialog fills only to preview the engine pipeline
(`gui/settings_dialog.py:2830-2843`, never saved). The search script is scratch tooling and
is deliberately not in this repository (Rust only).

## 3. Session store: `session.db`

The automatic session is the working list (the Queue) so that a restart does not re-hash
tens of thousands of files (`core/session_db.py:1-22`).

### 3.1 The database

Opened with `sqlite3.connect(path, timeout=30.0)` for every operation and closed again
(`core/session_db.py:45-47,157-158`). On every open the app executes, in this order
(`core/session_db.py:52-61`):

```
PRAGMA journal_mode=WAL
PRAGMA synchronous=FULL
```

then the two `CREATE TABLE IF NOT EXISTS` statements below. `journal_mode=WAL` is a
property of the file: byte 18 and 19 of the header are both `2` in
`corpus/formats/fresh-profile/xdg-config/hatate-linux/session.db` (observed), page size
4096, UTF-8 text, `PRAGMA user_version` = 0. While a connection is open the files
`session.db-wal` and `session.db-shm` exist next to it; they are gone after the last
connection closes (observed: not present in the sample profile).

### 3.2 DDL, verbatim

Exactly as written in the source (`core/session_db.py:54-61`, the SQL strings passed to
`conn.execute`, including their indentation):

```sql
        CREATE TABLE IF NOT EXISTS entries (
            path    TEXT PRIMARY KEY,
            ordinal INTEGER NOT NULL,
            data    TEXT NOT NULL
        )
```

```sql
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)
```

SQLite stores the statement without `IF NOT EXISTS`, so `sqlite3 session.db .schema` prints
(identical to `corpus/formats/schemas/session.db.schema.txt`, produced by that command on
the sample):

```sql
CREATE TABLE entries (
            path    TEXT PRIMARY KEY,
            ordinal INTEGER NOT NULL,
            data    TEXT NOT NULL
        );
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
```

There are no indexes beyond the two implicit `sqlite_autoindex_*` ones for the primary
keys (observed), no triggers, no views, no foreign keys.

### 3.3 Tables and columns

**`entries`** (3 columns): one row per image in the list.

| Column | Meaning | Written | Read |
|---|---|---|---|
| `path` | The image's file path **as a string, exactly as the app was given it**; no normalisation (`ImageEntry.path`, `core/models.py:161-162`). Primary key, so one row per path; two spellings of the same file are two rows. | `core/session_db.py:127-131` | `:113` (to decide deletions), `:186` selects only `data` |
| `ordinal` | Position in the list, 0-based. Restores the order: `SELECT data FROM entries ORDER BY ordinal` (`:186`). After a full save they are contiguous `0..n-1` (`:139-143`); after incremental saves that remove rows there can be gaps; ties are not defined. | `:127-143` | `:186` |
| `data` | The entry as one JSON text value, section 3.4. | `:131` | `:186-198` |

**`meta`** (2 columns): `key`/`value` pairs, both text. Two keys, both rewritten on **every**
save (`INSERT … ON CONFLICT(key) DO UPDATE`, `:115-124`):

| `key` | `value` | Read by |
|---|---|---|
| `schema_version` | `str(SCHEMA_VERSION)` = `'1'` (`:37,118`) | **nothing**: no code in `core/`, `gui/`, `workers/`, `tools/` reads `meta` (`git grep -i 'from meta'` finds none) |
| `saved_at` | `str(time.time())`, e.g. `1792639849.160799` (`:123`) | **nothing**: the "how old is the session" banner uses the file's mtime (`core/session.py:290-303`) |

### 3.4 `entries.data`: the entry JSON (16 keys)

Written by `_entry_to_dict` (`core/session.py:64-94`) with `json.dumps(...)` defaults
(single line, `", "` and `": "` separators, non-ASCII escaped; `core/session_db.py:131`).
Read by `_entry_from_dict` (`core/session.py:97-148`) from `json.loads`.

| Key | JSON type | Meaning | Load behaviour (cite) |
|---|---|---|---|
| `path` | string | same as the column | missing/empty → the row yields no entry and is **silently dropped**, not counted as corrupt (`:98-100`) |
| `hydrus_hash` | string \| null | the file's lowercase SHA-256 hex (64 chars) once known; **also the search-cache key** (section 4) | `d.get` (`:102`) |
| `status` | string | `MatchStatus` value: `not_searched` `good` `poor` `not_found` `error` `searching` (`core/models.py:27-33`) | empty → `not_searched`; **unknown string → `not_searched`**, no error (`:118-121`) |
| `error_message` | string \| null | last error | `:103` |
| `result_source` | string \| null | `"cached"` or `"fresh"` | `:104` |
| `searched_without_saucenao` | bool | result is provisional (SauceNAO quota was spent) | `bool(...)` (`:105`) |
| `sent_to_hydrus` | bool | a send was attempted | `bool(...)` (`:106`) |
| `hydrus_import_confirmed` | bool | Hydrus acknowledged holding the file | `bool(...)` (`:107`) |
| `reviewed` | bool | the user decided against sending | `bool(...)`, absent = false (`:110`) |
| `upscale_verdict` | string \| null | `"flagged"` or `"clear"` | `:113` |
| `upscale_check_detail` | string \| null | the per-heuristic breakdown shown as a tooltip | `:114` |
| `local_width`, `local_height` | int \| null | pixel size of the local file | `:115-116` |
| `selected_candidate_index` | int | which candidate is active | out of range → 0 (`:129-132`); a non-number raises → row corrupt |
| `candidates` | array of candidate objects | section 3.4.1 | element error → row corrupt (`:127`) |
| `tags` | array of tag objects | section 3.4.2 | element error → row corrupt (`:138-146`) |

Any exception while turning a row into an entry makes it **corrupt**: it is skipped, counted,
and `load_entries` returns `(entries, skipped)` so the GUI can warn
(`core/session_db.py:194-211`). One bad row never discards the others.

**Restore order matters.** Candidates are restored first and the selected one is
"selected" (`select_candidate`), which repopulates `matched_url`, `matched_thumb_url`,
`booru_name`, `similarity`, `similarity_measured`, `match_width`, `match_height` and
**replaces** the search-engine and booru tags with that candidate's own
(`core/models.py:279-298`); only then are the saved `tags` array applied, and if it is
non-empty it **overwrites** the whole tag list (`core/session.py:123-146`). A Rust reader
must do the same or booru tags are lost.

`ImageEntry` has 29 fields (`core/models.py:161-250`). Not stored: `hydrus_file_id`,
`matched_*`, `booru_name`, `similarity`, `similarity_measured`, `match_width`,
`match_height` (all derived from the selected candidate on load), `last_searched` (comes
back `None`), `file_missing` and `interrupted_mid_search` (recomputed per launch,
`core/models.py:241-244`), `revision` (change counter) and `matched_thumb_bytes`
(thumbnails are never stored). An entry restored as `searching` (the previous run died
mid-search) is reset to `not_searched` after an unclean shutdown
(`gui/main_window.py:176-191,1427-1430`).

#### 3.4.1 Candidate object (20 keys)

Written by `_candidate_to_dict` (`core/search_cache.py:82-131`) and read by
`_candidate_from_dict` (`:178-223`). Shared by the session and the search cache.

| Key | JSON type | Meaning | Load behaviour |
|---|---|---|---|
| `url` | string | the match's page URL | **required**: missing → `KeyError` → the whole entry row is corrupt (`:187`) |
| `source_name` | string \| null | site name (one of `enabled_sites`, e.g. `"Danbooru"`) | `.get` |
| `thumb_url` | string \| null | engine thumbnail URL | `.get` |
| `similarity` | number | 0–100 | default `0.0` |
| `similarity_measured` | bool | the number is a real comparison, not an engine ordinal | default false |
| `width`, `height` | int \| null | the match's pixel size | `.get` |
| `engine` | string | label such as `"IQDB"`, `"SauceNAO"`, `"ascii2d"` | default `"IQDB"` |
| `engine_tags`, `booru_tags` | array of tag objects (`name`, `namespace`) | tags from the engine / from the booru page | default `[]`; element without `name` → corrupt |
| `direct_file_url` | string \| null | the image file itself | `.get` |
| `preview_url` | string \| null | mid-size sample | `.get` |
| `remote_format` | string \| null | e.g. `"PNG"` | `.get` |
| `remote_size_bytes` | int \| null | from Content-Length | `.get` |
| `incomplete_reason` | string \| null | why tags/file are missing | `.get` |
| `tags_borrowed_from` | string \| null | URL of the match that lent the tags | `.get` |
| `remote_available` | bool \| null | source page alive? **tri-state** | accepted **only if a real bool** *and* `availability_checked_at` is a number not older than 30 days; otherwise unknown (`:152-175`, TTL `:149`) |
| `availability_checked_at` | number \| null | epoch seconds of that verdict | see above |
| `restricted` | string \| null | gated post (e.g. Danbooru Gold), with reason | `.get` |
| `tag_version` | int | the parser's `TAG_VERSION` when `booru_tags` were read | compared with the parser's current value (`:183`) |

Not stored: `thumb_bytes`, `page_index`, `booru_tags_fetched`, `remote_info_fetched`. In a
**session** load `_candidate_from_dict(c)` is called with `format_version = 0`
(`core/session.py:127`), so `booru_tags_fetched` and `remote_info_fetched` always come back
`false` and the page details are re-fetched on demand (`core/search_cache.py:184,217,222`).

#### 3.4.2 Tag object

`{"name": str, "namespace": str|null, "source": str}`. In a session `source` is one of
`User`, `Search engine`, `Booru`, `Hatate-linux`, `Hydrus` (`core/models.py:12-18`);
missing or unknown → `User` (`core/session.py:139-144`). `name` is required
(`core/search_cache.py:79`). The tag's display form is `namespace:name`, or `name` when the
namespace is null (`core/models.py:68-70`). Inside a candidate (`engine_tags`, `booru_tags`)
the tag object has **no** `source`: it is implied by the array.

### 3.5 Named sessions ("Save Session As", "Open Session")

A named session is a separate **JSON** file the user names, never the automatic one
(`core/session.py:176-194`, `gui/main_window.py:1627-1655,1842-1880`).

```
{"version": 1, "saved_at": <epoch float>, "entries": [ <entry JSON as in 3.4>, … ]}
```

- Written with `json.dumps(payload)` (single line), through a unique temp file
  `<name>.<random>.tmp` in the same directory, `flush` + `fsync`, `Path.replace`, then an
  `fsync` of the directory (`core/session.py:211-246`). Observed mode `0600` (the temp file
  is created by `NamedTemporaryFile`).
- Read: the file must parse to an **object** and `version == 1` exactly, otherwise the
  file is **ignored and the result is an empty list** (`core/session.py:369-376`). A
  malformed element is skipped (`:379-384`).
- The Open dialog filters on `*.json` and the Save dialog appends `.json`
  (`gui/main_window.py:1643-1646,1852`).

### 3.6 Legacy `session.json` and migration

`CONFIG_DIR/session.json` is the same document as 3.5, from before the SQLite store
(`core/paths.py:34-37`). `load_session()` (no path) works like this (`core/session.py:306-334`,
**observed** in `corpus/formats/legacy-migration/`):

1. `session.db` exists → read it. `session.json` is **not consulted**, even when the database
   holds no rows.
2. Only `session.json` exists → read it, write every entry into a new `session.db`
   (a full save), **leave `session.json` in place** as a backup (`:151-173`).
3. After a successful load from `session.db` with at least one entry, a leftover
   `session.json` is deleted if it is older than the database **and** at least 30 days old
   (`:337-357`, `LEGACY_BACKUP_DAYS = 30`, `:42`).

`clear_session()` empties `entries` (rows deleted, the file stays) and unlinks
`session.json` (`:406-429`, `core/session_db.py:231-248`).

### 3.7 When the app writes the session

- **Autosave** every `autosave_interval_seconds` (minimum 10, default 120) while
  `autosave_session` is on, from a worker thread, skipped when the list is empty
  (`gui/session_autosave.py:25,60-93`). Incremental: only rows whose in-memory `revision`
  moved are re-encoded, plus a rotating tenth of the list on every pass as a safety net;
  rows missing from the list are deleted; ordinals are rewritten only when the order changed
  (`core/session_db.py:42,84-143`).
- **Full save on a clean exit** when `restore_session_on_start` is on
  (`gui/main_window.py:4177-4178`), and on the actions that replace the list.
- **Restore on start** when `restore_session_on_start` is on (`gui/main_window.py:525-526`).
- One transaction per save (`with conn:`, `core/session_db.py:101`): the whole save lands or
  none of it. A failed save returns `False`, logs a warning and never raises
  (`:150-155`).
- There is **no lock file and no PID file**. A second process (the Rust app opened while
  the Python app runs) is not detected; SQLite's own locking and the 30-second busy
  timeout are the only protection, and each app's in-memory list will overwrite the
  other's rows.

### 3.8 Older, newer and foreign schemas (observed)

There is exactly **one** schema version (`SCHEMA_VERSION = 1`) and **no migration code**:
the `entries` and `meta` tables have never changed shape, and nothing reads
`meta.schema_version` or `PRAGMA user_version`. Results from
`corpus/formats/behavior-probes.txt`:

| Situation | Result |
|---|---|
| `meta.schema_version = '99'` plus a row that is not JSON | version ignored; the good row loads, the bad one is counted as skipped (`1 1 ['/a/b.png']`) |
| database without a `meta` table (an older/foreign file) | loads fine; the open **creates** `meta` (`CREATE TABLE IF NOT EXISTS`) |
| `PRAGMA user_version = 7` | ignored and left at 7 |
| extra **nullable** column on `entries` | read and save both work |
| extra **`NOT NULL` column without default** on `entries` | reads work; **every save fails** (`NOT NULL constraint failed: entries.extra`), logged as a warning, the session stops persisting |
| file that is not an SQLite database | `load_entries` logs `file is not a database` and returns `[]`; the file is **not** touched or replaced |
| `session.json` with `"version": 2` and no `session.db` | ignored, empty list |
| a row whose JSON has an unknown `status` and a candidate without `url` | the row is corrupt (the missing `url` raises), counted as skipped |

## 4. Search result cache: `search_cache/<sha256>.json`

One file per file hash, so one bad file cannot take the cache down
(`core/search_cache.py:1-17`). The key is the file's **lowercase SHA-256 hex**
(`entry.hydrus_hash`, `core/search_engine.py:353-356`; `hash_file` is
`hashlib.sha256(...).hexdigest()`, `core/hydrus_tag_lookup.py:94-108`; a Hydrus-style file
name `^[0-9a-f]{64}(\.ext)?$` supplies the hash without reading the file when
`hash_source` is `"hydrus"`, `core/hydrus_tag_lookup.py:30,52`, `workers/file_hash_worker.py:70-73`).

```
{"format_version": 2, "cached_at": <epoch float>, "status": "<MatchStatus value>",
 "error_message": str|null, "selected_candidate_index": int, "candidates": [ <3.4.1>, … ]}
```

6 keys (`core/search_cache.py:330-337`), written with `json.dumps(data)` and
`Path.write_text` (**not atomic**, no temp file, no fsync, `:341`). Samples:
`corpus/formats/fresh-profile/xdg-config/hatate-linux/search_cache/`.

Rules:

- `ERROR` results are never cached; neither are results found while SauceNAO was skipped
  (`searched_without_saucenao`) (`:323-328`).
- Only `status == "not_found"` expires, after `search_cache_ttl_days` (default 30, 0 = never)
  measured from `cached_at`; a missing/non-numeric `cached_at` on such an entry counts as
  expired. An expired file is **deleted** on the read that notices it (`:226-278`).
- A file that cannot be read or parsed is a cache miss, never an error (`:262-266`).
- `format_version < 2` (the current value, `:42`) keeps the candidates (the expensive part)
  but marks their page details as not fetched, so they are re-read (`:295-301,217-222`).
- `tag_version` compares with the parser's `TAG_VERSION`; **only MangaDex declares one
  (`1`, `core/boorus/mangadex.py:62`), every other parser is `0`** (`:45-67`). A cached
  candidate whose stored value differs has its booru tags re-read.
- Deleted on "Reset" of a row (`drop_cached_result`, `:361-381`) and by Help › Clear Search
  Cache (`clear_cache`, `:384-396`), which removes every `*.json` in the directory.

## 5. Other files the app writes

### 5.1 `matched_urls.log` (+ `.1` `.2` `.3`)

Text, UTF-8, one line per found image, appended: `[%Y-%m-%d %H:%M:%S] <filename> -> <url>`
in **local time** (`core/logger.py:38-40`). Written when `log_matched_urls` is true and the
status is `good` or `poor` and there is a matched URL (`workers/search_worker.py:378-383`).
Location is `log_file_path` (which defaults to the file in `CONFIG_DIR`; the parent folder is
created). Rotation: at `>= 10 MiB` (`MAX_BYTES = 10*1024*1024`) the file becomes `.1`, `.1`
becomes `.2`, `.2` becomes `.3` (`BACKUPS = 3`, `core/logger.py:16-30`). Sample:
`corpus/formats/fresh-profile/xdg-config/hatate-linux/matched_urls.log`.

### 5.2 `app.log` (+ `.1` `.2` `.3`)

UTF-8 text, `RotatingFileHandler(maxBytes=2_000_000, backupCount=3)`, level DEBUG, format
`%(asctime)s [%(levelname)s] %(name)s: %(message)s` with date format `%Y-%m-%d %H:%M:%S`
(`core/applog.py:42-48`). The first line of every run is
`Logging started, writing to <path>` (`core/applog.py:56`); the Python app uses that line to
find where the previous run's log ended when it explains an interrupted run
(`core/crashlog.py:61-69,181-213`). Sample: `…/hatate-linux/app.log` (shows the temp-profile
paths of the run that produced it).

### 5.3 Crash-detection files

- `crash.log.running`: contains `str(time.time())` (`core/crashlog.py:110`). Written at
  startup **after** `check_unclean_shutdown()` has looked for the previous one
  (`main.py:47-48`); removed at the very end of a clean shutdown (`:121`,
  `gui/main_window.py:4183`). Finding it at startup means the previous run crashed or was
  killed, which turns on the "interrupted run" banner and the reset of a `searching` row
  (3.4).
- `last_clean_exit`: `str(time.time())`, written by the same clean shutdown (`:127`).
- `crash.log`: opened in append mode at startup, `--- session started, faulthandler armed ---`
  written first (`:78,227-228`), then Python's `faulthandler` dumps on a hard crash.

### 5.4 `mcp_audit.jsonl` (+ `.1`)

JSON Lines, one object per MCP tool call (`core/mcp_audit.py:70-96`):
`timestamp` (UTC, `%Y-%m-%dT%H:%M:%SZ`), `tool`, `tier`, `allowed`, `reason`, `dry_run`,
`row`, `candidate_index`, `arguments`, `outcome`; `json.dumps(entry, default=str)` + `\n`,
opened in append mode. Keys named `token`, `bearer`, `access_key`, `accesskey`, `api_key`,
`apikey`, `authorization` (case-insensitive, any depth) have their value replaced by the string
`"<redacted>"` (`:35-50`). At `>= mcp.audit_log_max_bytes` (default 5,000,000) the file is
renamed to `.1` replacing any earlier `.1` (`:99-108`; the limit is passed from
`gui/mcp_bridge.py:173,192`).

### 5.5 `saucenao_quota_pause.json`

`{"paused_at": <epoch float>, "searched": int, "remaining": int, "reset_at": "<ISO-8601 with +00:00>"}`
written with `json.dumps(state.__dict__)` when a batch stops on SauceNAO's daily quota
(`core/saucenao.py:278-299`). `reset_at` is the next **00:00 UTC** (`:248-259`). Read
leniently: missing/corrupt → no pause (`:314-327`). Deleted when the user starts a new
search, or at startup when `reset_at` is already past or unparsable
(`:302-311`, `gui/main_window.py:544-560`).

### 5.6 Pawchive artist index

`pawchive_index.db` (SQLite, rollback journal, **no** `PRAGMA`s, no version table). Opened for
each operation with `timeout=30`, running the schema script every time
(`core/pawchive_index.py:204-215`). DDL verbatim, `core/pawchive_index.py:85-108`:

```sql
CREATE TABLE IF NOT EXISTS creators (
    service     TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    name        TEXT,
    added_at    REAL,
    indexed_at  REAL,
    post_count  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (service, user_id)
);
CREATE TABLE IF NOT EXISTS images (
    service     TEXT NOT NULL,
    user_id     TEXT NOT NULL,
    post_id     TEXT NOT NULL,
    page_index  INTEGER NOT NULL,
    sha256      TEXT NOT NULL,
    path        TEXT NOT NULL,
    title       TEXT,
    dhash8      TEXT,               -- hex; NULL when pawchive had no thumbnail
    dhash16     TEXT,
    PRIMARY KEY (service, user_id, post_id, page_index)
);
CREATE INDEX IF NOT EXISTS images_by_sha256 ON images (sha256);
```

`sqlite3 pawchive_index.db .schema` on the sample is identical apart from the dropped
`IF NOT EXISTS` (`corpus/formats/schemas/pawchive_index.db.schema.txt`). Columns:

| Table.column | Meaning | Cite |
|---|---|---|
| `creators.service`, `creators.user_id` | the artist's pawchive service (`patreon`, `fanbox`, …) and id; both text, together the key | `:231-236,178-181` |
| `creators.name` | display name, set when known | `:238-239,267-272` |
| `creators.added_at` | `time.time()` when added | `:234-236` |
| `creators.indexed_at` | `time.time()` when the last crawl finished | `:270` |
| `creators.post_count` | posts seen by that crawl | `:270` |
| `images.service`, `images.user_id`, `images.post_id`, `images.page_index` | the image's post and its page within the post (key) | `:499-503` |
| `images.sha256` | lowercase SHA-256 taken from the pawchive file path; `''` when the path has none | `:499-501`, regex `:82` |
| `images.path` | the file's path **on pawchive** (not a local path) | `:479,501` |
| `images.title` | the post title | `:501` |
| `images.dhash8` | 64-bit difference hash, `f"{h8:016x}"`; NULL when pawchive had no thumbnail | `:502,160-175` |
| `images.dhash16` | 256-bit difference hash, `f"{h16:064x}"`; NULL likewise; the search ranks by Hamming distance on it | `:503`, `:313` |

`pawchive_creators.json` is the downloaded creator list (`json.dumps` of the API reply,
~15 MB), reused for 7 days by file mtime (`:357-373`). It is a cache of a public API reply
and can be deleted.

### 5.7 `lens_profile/`

A **Chromium persistent profile** (`launch_persistent_context(str(PROFILE_DIR), …)`,
`core/lens_browser.py:438-440`) used by the Google Lens engine so Google sees a browser with
history (`:52-53`). The format is Chromium's, not Hatate's, and it holds Google cookies:
nothing from it may be copied into fixtures or logs. Only one Chromium can use it at a time
(`:429-435`).

### 5.8 Exported results (CSV / JSON)

User-initiated, written by `results_export.render` (`core/export.py`, `gui/main_window.py:1657-1710`).
Both walk the same `FIELDS` list (22 columns, `core/export.py:84-112`):
`filename path status matched_url site engine similarity similarity_measured local_width
local_height match_width match_height tag_count tags candidate_count sent reviewed
upscale_verdict cache error file_missing hydrus_hash`; the exact order is the header row of
`corpus/formats/exports/export.csv`.

- **CSV:** UTF-8, `csv.writer` with `lineterminator="\r\n"`, header row first, booleans as
  `TRUE`/`FALSE`, tags joined with `", "` into one cell (`core/export.py:117-165`).
- **JSON:** `{"schema_version": 1, "generated_at": "%Y-%m-%dT%H:%M:%S" (local time), "count": n,
  "images": [ … ]}`, `indent=2`, `ensure_ascii=False`; in JSON `tags` is an array of
  `{"name","namespace","source"}` (`core/export.py:168-197`).
- Tags exported are the same list a Hydrus send carries: all tags, or only the enabled
  sources when `filter_sent_tags_by_source` is on (`core/models.py:442-466`).
- The format is chosen by the file dialog's filter, else by extension (`.json` → JSON,
  anything else → CSV) (`core/export.py:200-207`, `gui/main_window.py:1684-1691`).
- Written with `open(path, "w", encoding="utf-8", newline="")`, not atomically.

### 5.9 Tag sidecars (next to the images)

The only thing Hatate writes into the user's picture folders, and only on Files › Write Tag
Files (`core/sidecar.py:30-35`, `gui/main_window.py:1712-1756`).

- **Name:** `append` style (default) `cat.jpg` → `cat.jpg.txt`; `replace` style `cat.jpg` →
  `cat.txt` (`Path.with_suffix`) (`core/sidecar.py:80-86`). Extension `.txt`
  (`DEFAULT_EXTENSION`, `:50`).
- **Content:** one tag per line, `namespace:name` or `name`, each line ending in `\n`
  (also the last), UTF-8, no BOM; duplicates dropped keeping the first; the order is the
  entry's own tag order (`core/sidecar.py:89-105`). Which tags: `tags_to_send`
  (`core/models.py:442-466`). Samples: `corpus/formats/sidecars/`.
- **When not written:** an entry with no tags to send (no empty files), an entry whose file
  is missing (`:141-168`).
- **Existing target:** policy `sidecar_overwrite`: `ask` (the GUI shows the plan first),
  `skip`, `overwrite`. New files are created with `O_CREAT|O_EXCL`, mode `0644`
  (`:179-194`); overwriting uses a temp file `.<target name>.<random>.tmp` in the same
  folder, `chmod 0644`, `os.replace` (`:197-220`). One failing file does not stop the batch
  (`:223-249`).

### 5.10 What the app writes into Hydrus

Not files, but state the user keeps. All through the Client API with the header
`Hydrus-Client-API-Access-Key: <hydrus.access_key>` (`core/hydrus_client.py:129-130`),
base URL `hydrus.api_url`, TLS verification `hydrus.verify_ssl`. Which of these happen depends
on the send method (`upload`, `url_importer`, `download_send`):

| What | Endpoint | Payload | When | Cite |
|---|---|---|---|---|
| the file | `/add_files/add_file` | file path/bytes | upload and download-and-send | `core/hydrus_client.py:658-683`, `core/hydrus_import.py:123-137` |
| the match URL | `/add_urls/associate_url` | `{"url_to_add": url, "hashes": [hash]}` | after an upload | `core/hydrus_client.py:687-696`, `core/hydrus_import.py:149-154` |
| URL import | `/add_urls/add_url` | `{"url", "show_destination_page", "service_keys_to_additional_tags"}` | `url_importer` | `core/hydrus_client.py:698-726` |
| tags | `/add_tags/add_tags` | `{"hash", "service_keys_to_tags"` or `"service_names_to_tags": {service: [tag…]}}`; service = `hydrus.tag_service_key`, else `"my tags"`; tries key then name | every send with tags | `core/hydrus_client.py:758-786`, `core/hydrus_import.py:155-162` |
| duplicate relationship | `/manage_file_relationships/set_file_relationships` (and `set_kings`) | relationship `4` (better) or `3` (alternates) | only if `set_hydrus_duplicate_relationships` | `core/hydrus_client.py:93-100,407,433` |
| provenance note | `/add_notes/set_notes` | `{"hash", "notes": {name: text}}` | only if `write_hydrus_provenance_note`, after Hydrus confirms holding the file | `core/hydrus_client.py:479`, `core/hydrus_import.py:533-590` |
| delete / undelete | `/add_files/delete_files`, `/add_files/undelete_files` | hashes (+ reason) | user actions | `core/hydrus_client.py:533,541` |
| result page | `/manage_pages/new_page` | hashes + name | user action | `core/hydrus_client.py:321` |

The provenance note is the one piece of Hatate-authored text stored in Hydrus
(`core/provenance_note.py:38-98`): default name `hatate: match source` (or
`hydrus_provenance_note_name`), body lines in this order, omitting any that are missing, no
note at all when there is neither a matched URL nor an engine:
`Match found by Hatate-Linux-Redux.`, `Engine: <engine>`, `Site: <site>`,
`Similarity: <label> - <measured|ranking note>` (`~NN%` when unmeasured,
`core/similarity_display.py:27-43`), `Match: <url>`, `Sent to Hydrus: %Y-%m-%d %H:%M` (local
time). The app **replaces** the note of that name on every send, so a re-send from either app
must use the same name to replace rather than duplicate.

The Hydrus tag source **`Hatate-linux`** (`TagSource.HATATE`) marks tags this app adds by rule
(`tags_for_found` etc., `core/tag_rules.py`); it is a label inside Hatate's own lists, not
something Hydrus stores.

## 6. Compatibility rules for the Rust app

"Must" = a requirement for the promise "launch the new binary and your existing files just
work"; "should" = strongly preferred, the Python app tolerates a deviation.

### 6.1 Config

1. **Must** find `config.json` at the paths of section 1.1, with the same env handling.
2. **Must** open any `config.json` the Python app can produce: `schema_version` 1–13 (apply
   the migrations of 2.3, including the two legacy keys), missing keys (use the default),
   unknown top-level keys (ignore), a newer `schema_version` (load, keep the number).
3. **Must not crash on bad input.** The Python app fails to start on some (observed): a
   top-level JSON array (`AttributeError`), and a nested object with an unknown key such as
   `"hydrus": {"bogus": 1}` (`TypeError`, because of `Cls(**{…})`, `core/config.py:557-564`).
   The Rust reader should be per-field tolerant (a wrong-typed value falls back to the
   default and is reported, where Python would carry the wrong value forward). A file that
   cannot be parsed must be **moved aside to `config.json.unreadable`, never overwritten**,
   and `config.json.bak` is the fallback (2.2). A UTF-8 BOM makes Python treat the file as
   unreadable (observed), so the Rust writer **must not** emit one.
4. **Must write all 103 keys** with the names and JSON types of 2.5, including the dead key
   `window_geometry`, in all four nested objects **with exactly their 7/5/4/8 keys and no
   others**. A Rust-only setting added to a nested object makes the Python app fail to start.
   A Rust-only setting at the top level is accepted by the Python reader but **dropped at the
   Python app's next save** (2.4), so Rust-only settings belong in a separate file whose name
   is a Phase 1 decision and must not collide with section 1.2.
5. **Must keep `schema_version` at 13** (never lower, never raise); a value read above 13 is
   preserved.
6. **Must preserve `table_header_state` and `table_header_columns` verbatim** (Qt blobs the
   Rust GUI cannot use) and must not repurpose them; they are what the Python app uses to
   restore its Queue column layout.
7. **Must** write atomically and restrictively: `config.json.tmp` created `0600`, fsync,
   rename; copy the previous file to `config.json.bak` first; directory `0700`
   (`core/config.py:720-745`).
8. **Should** emit the same bytes as Python so profiles diff cleanly: UTF-8, `indent=2`,
   field order of 2.5, no trailing newline, floats printed with a `.0` when integral, empty
   containers as `[]`/`{}`. Differences the Python reader does not mind: key order, extra
   whitespace, `\uXXXX` versus raw UTF-8 for non-ASCII. Test idea for Phase 1: load
   `fresh-profile/…/config.json`, save, compare (only `log_file_path` is machine-specific).
9. **Must not** invent a new `log_file_path` for an existing config: it is an absolute path
   frozen at first save (2.1).
10. **Secrets** (2.5) must never reach a log line, an error message or a fixture; the Python
    app redacts the same names in its audit log (`core/mcp_audit.py:35-50`).
11. The Python app **rewrites the whole file at every exit** (2.4). If both apps are used in
    turn, Rust-written values for known keys survive, anything else does not.

### 6.2 Session database

1. **Must** open `session.db` unchanged at `CONFIG_DIR/session.db`, read rows
   `ORDER BY ordinal`, tolerate extra columns and extra tables, and ignore
   `meta`/`user_version` (as Python does).
2. **Must** keep the DDL of 3.2: same two tables, same columns and constraints, no renamed or
   dropped objects, **no added `NOT NULL` column without a default** (Python's saves would
   fail, 3.8), `journal_mode=WAL`.
3. **Must** write `data` as a JSON object with the 16 keys and the candidate/tag shapes of
   3.4 and the invariants that keep Python's loader from raising: `path` non-empty string;
   every candidate has a string `url`; every tag has a string `name`; `selected_candidate_index`
   an integer; `status` one of the six values; `remote_available` a real boolean or null
   with a numeric `availability_checked_at`.
4. **Must** write `meta` rows `schema_version = '1'` and `saved_at = <epoch seconds as
   text>` on every save (nothing reads them, but the file should stay indistinguishable).
5. **Must** apply the restore order of 3.4 (candidates, select, then saved tags overwrite).
6. **Must** give the sticky states the same meaning: `searching` → `not_searched` after an
   unclean shutdown (3.4); unknown status → `not_searched`; unknown tag source → `User`.
7. **Should** write contiguous ordinals on a full save, one transaction per save, and
   `synchronous=FULL` (the file exists to be correct after a crash,
   `core/session_db.py:48-53`).
8. **Should** also implement the legacy `session.json` migration and the 30-day retirement
   (3.6) so a very old profile still opens.
9. Named sessions: **must** read and write the `{"version":1,"saved_at","entries"}` document
   of 3.5 (strict `version == 1`), because a user can open a Rust-saved file in Python and
   the reverse.
10. Single-writer assumption: no lock exists (3.7). The Rust app should refuse to start a
    second writer when it can detect the Python app (for example `crash.log.running` present
    **and** a live process), and say so.

### 6.3 Search cache

Same JSON (section 4). **Must** key by lowercase SHA-256, set `format_version: 2`,
`tag_version` = `1` for MangaDex URLs and `0` otherwise, never cache `error` results or
provisional results, and treat unreadable/truncated files as a miss. **Should** write
atomically (Python does not, so a Python-written file can be truncated after a crash).

### 6.4 Files that only need to be tolerated or reproduced

- `crash.log.running` / `last_clean_exit` (5.3): if the Rust app takes part in the protocol it
  **must** remove the marker on a clean exit, otherwise the next Python launch shows the
  "interrupted run" banner. If it does not take part, it must not create the marker.
- `app.log` format (5.2) **should** be kept if the Rust app writes the same file; otherwise
  it should use another name. The Python app only reads the tail to find a timestamp.
- `matched_urls.log`, `mcp_audit.jsonl`, `saucenao_quota_pause.json`: formats of 5.1, 5.4,
  5.5. The quota file is read by whichever app starts next, so the Rust app **must** write
  the exact keys if it writes it and **must** honour an existing one.
- `pawchive_index.db`: **must** keep the DDL of 5.6 if the Rust app uses the pawchive index
  feature; the rows are a crawl of a public site and can be rebuilt, so a mismatch is
  recoverable but costs the user a long re-crawl.
- `lens_profile/`: Chromium's format; a different Chromium major version may upgrade it so
  that the older one cannot read it. The Rust app **should** use a separate directory unless
  it ships the same Chromium. The profile is disposable (Lens is challenged more often
  without history, `core/lens_browser.py:52-53`).
- Sidecars (5.9): **must** be byte-identical to Python's for the same tags, and keep the
  never-clobber guarantee (`O_EXCL`) and the atomic overwrite.
- Exports (5.8): **should** be byte-identical (CRLF CSV, JSON as specified); scripts
  consume them.
- Hydrus writes (5.10): **must** use the same note name and wording, tag-service
  resolution and relationship codes, because both apps replace and compare against what the
  other wrote.

### 6.5 Things the Python app would fail on

Each is observed or read from the cited code:

| If the Rust app writes… | Python's result |
|---|---|
| `config.json` as a JSON array, or not UTF-8, or with a BOM | crash (array) / config treated unreadable and moved aside (BOM) |
| an extra key inside `saucenao`/`hydrus`/`match_conditions`/`mcp` | **Python fails to start** (`TypeError`) |
| `session.db` `entries` with an extra `NOT NULL` column without default | loading works; **no save ever succeeds again** |
| `entries.data` that is not JSON, or lacks a candidate `url` / tag `name` | that row is dropped and counted as corrupt |
| `entries.data` with `status` outside the six values | silently becomes `not_searched` |
| a named session with `"version"` other than the integer 1 | the file is ignored, empty list |
| `session.db` in a non-SQLite format | empty list, a warning; the file is left alone |
| search-cache JSON for MangaDex with `tag_version: 0` | tags re-read from the page (harmless but wasteful) |
| `crash.log.running` left behind | "interrupted run" banner on the next Python launch |

## 7. Samples (`corpus/formats/`)

Produced by the Python app's own functions from a **fresh empty profile**
(`HOME=$(mktemp -d)`, `XDG_CONFIG_HOME`/`XDG_CACHE_HOME` inside it; the code asserts that
`CONFIG_DIR` is inside the temp dir before anything is written). Inputs are synthetic.
No sample was produced through the GUI, and none needed to be built by hand from the DDL
(`corpus/formats/README.md` lists every file and how it was made; two are hand-written
*inputs* to a migration and are labelled as such). Highlights:

- `fresh-profile/xdg-config/hatate-linux/config.json`: what `Settings.load()` writes for an
  empty profile.
- `fresh-profile/xdg-config/hatate-linux/session.db`: six entries (good with two candidates
  and tags of every source, not found, error with non-ASCII path, not searched, provisional
  `searching`, poor with a measured ascii2d candidate). `schemas/session.db.schema.txt` is the
  `sqlite3 … .schema` output.
- `named-session/named-session.json`, `exports/export.csv`, `exports/export.json`,
  `sidecars/append/`, `sidecars/replace/`, search-cache entries, the small state files, an
  empty `pawchive_index.db`.
- `legacy-migration/`: a v1 `config.json` and a legacy `session.json` as inputs, and the
  files the Python app produced from them (`config.json`, `config.json.bak`, `session.db`).
- `behavior-probes.txt`: the raw output of the failure-mode probes quoted in 2.3, 3.8, 6.1
  and 6.5.

## 8. Verification recorded for this document

See the PR description for the commands and outputs: the config-key count (103 = 103 = 103),
the `sqlite3 .schema` diff against the DDL in 3.2 and 5.6, the secret scan of `corpus/` and
this file with every hit explained, and the ten spot-checked citations.
