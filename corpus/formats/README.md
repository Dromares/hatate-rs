# corpus/formats

Synthetic samples of every file format the Python Hatate app reads and writes, for
`docs/file-formats.md` (DAN-1966). Python source: `Dromares/Hatate-Linux-Redux` at `76c85c4`.

**Nothing here comes from a real profile.** Every sample was produced by calling the Python app's
own functions inside a fresh, empty profile (`HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME` pointed
inside a `mktemp -d`; the generator asserted that `CONFIG_DIR` was inside it before writing).
Entries are invented: `/data/hatate-sample/images/…` paths, `example.invalid` hosts, post ids
`1000001…`, hashes `aaaa…`/`bbbb…`. All credential fields are empty. No GUI was used, and no
sample was built by hand from the DDL.

`.gitattributes` marks this directory `-text`: the CSV is CRLF and the JSON has no trailing
newline, and git must not normalise either.

## Inventory

| Path | Made by | What it shows |
|---|---|---|
| `fresh-profile/xdg-config/hatate-linux/config.json` | `Settings.load()` on an empty profile | the 103-key config with every default (`log_file_path` holds the temp profile's path) |
| `fresh-profile/xdg-config/hatate-linux/session.db` | `session_db.save_entries(entries, None)` | the SQLite session: six synthetic entries, `meta` rows |
| `fresh-profile/xdg-config/hatate-linux/search_cache/<hash>.json` | `search_cache.save_cached_result` | one found result (2 candidates) and one not-found result |
| `fresh-profile/xdg-config/hatate-linux/matched_urls.log` | `logger.log_matched_url` | one log line |
| `fresh-profile/xdg-config/hatate-linux/mcp_audit.jsonl` | `mcp_audit.record` | one audit line; the `token` argument is shown redacted |
| `fresh-profile/xdg-config/hatate-linux/saucenao_quota_pause.json` | `saucenao.record_quota_pause` | the quota-pause state |
| `fresh-profile/xdg-config/hatate-linux/crash.log.running`, `last_clean_exit` | `crashlog.mark_running`, `mark_clean_shutdown` | epoch-seconds markers |
| `fresh-profile/xdg-config/hatate-linux/app.log` | `applog.setup_logging` + the calls above | the log format, with the generator's temp paths |
| `fresh-profile/xdg-config/hatate-linux/pawchive_index.db` | `PawchiveIndex().creators()` | the empty pawchive index (schema only) |
| `named-session/named-session.json` | `session.save_session(entries, path=…)` | a named-session JSON document (same six entries) |
| `exports/export.csv`, `exports/export.json` | `export.to_csv`, `export.to_json(generated_at=1790000000.0)` | the results export |
| `sidecars/append/example-1.png.txt`, `sidecars/replace/example-1.txt` | `sidecar.plan` + its `body` | tag sidecars in both naming styles |
| `legacy-migration/input/config.json` | **hand-written** (synthetic, empty credentials) | a schema-v1 config with the two legacy keys and an unknown key |
| `legacy-migration/input/session.json` | copy of `named-session.json` | a legacy single-document `session.json` |
| `legacy-migration/after-load/{config.json,config.json.bak,session.db}` | `Settings.load()` and `session.load_session()` on the two inputs | what the Python app turns them into |
| `schemas/session.db.schema.txt`, `schemas/pawchive_index.db.schema.txt` | `sqlite3 <db> .schema` | the stored DDL |
| `schemas/session.db.meta.txt` | `sqlite3 session.db "select * from meta order by key"` | the two `meta` rows |
| `schemas/file-modes.txt` | `stat -c %a` of the generated profile | the permissions the Python app produced (git cannot store them) |
| `behavior-probes.txt` | one subprocess per probe | raw output of the failure-mode probes |

Volatile values: timestamps (`saved_at`, `cached_at`, `paused_at`, log times), the temp-profile
path inside `config.json`'s `log_file_path` and `app.log`. Everything else is deterministic.

## Reproducing

The generator is scratch tooling and deliberately **not in this repository** (it is Python and
this tree is Rust-only). To regenerate, call the functions named in the table on a checkout of
the Python repo at `76c85c4` inside a fresh profile. `session.db` rows are: example-1 (good, two
candidates, tags of all five sources, sent and confirmed), example-2 (not found, cached,
upscale verdict "clear"), `ünïcode-3.webp` (error), example-4 (not searched), example-5
(searching, provisional, one candidate), example-6 (poor, one measured ascii2d candidate).

## Secret scan

The Phase 0.a grep, run over this directory and `docs/file-formats.md` before the PR:
`git grep -n -i -E 'cookie|sessionid|session_id|token|api[_-]?key|access[_-]?key|passw|authorization|bearer|set-cookie'`.
Every hit is a config **key name** with an empty value, a redaction marker, or prose in the
spec; the explanation per hit is in the PR description.
