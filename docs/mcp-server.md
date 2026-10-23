# MCP server, audit log and stdio bridge (Phase 5.d, DAN-2166)

Work in progress. `hatate-rs` is not yet a replacement for Hatate. No subcommand starts this
server in this phase: Phase 6 wires it to Settings > MCP. What is here is the server, its tool
registry, the audit log and the stdio bridge, tested over scripted handlers. The tool bodies
(`gui/mcp_bridge.py`, `core/mcp_images.py`) are 5.e (DAN-2167).

Source of truth: `Dromares/Hatate-Linux-Redux` at `76c85c44fea050e06ae91c77ecc5b40e2f23f986`,
oracle `mcp 1.30.0` (the `mcp>=1.2,<2` pin of `requirements.txt`; pydantic 2.13.5, uvicorn 0.54.0,
starlette 1.7.0, requests 2.34.2, CPython 3.14.8).

## What is ported, and where it lives

| Python | Rust (`crates/hatate-core/src/mcp/`) |
|---|---|
| `core/mcp_tools.py`: tiers, `ToolSpec`, `TOOLS`, `mapped_review_action_ids`, `unaccounted_review_action_ids`, `refusal_message`, `gate`, `is_dry_runnable` | `tools.rs` |
| `core/mcp_audit.py`: `AUDIT_LOG_FILE`, `REDACTED_KEYS`, `REDACTED_VALUE`, `_redact`, `record`, `_iso_now`, `_append`, `_rotate_if_needed` | `audit.rs` (path: `Paths::mcp_audit_log()`, `mcp_audit.jsonl` under the config dir) |
| `core/mcp_content.py`: `to_content_blocks` and the `_image` key suffix rule | `content.rs` |
| `core/mcp_server.py`: `HOST`, `BIND_TIMEOUT_SECONDS`, `_HANDLER_METHOD_NAMES`, `dispatch` | `mod.rs`, `server.rs`, `handlers.rs` |
| `McpServerController` (`running`, `start`, `stop`, `restart`, `last_error`) | `server.rs` |
| `_build_asgi_app`, `_register_tools`, `_wrap_tool`, `_with_bearer_auth` | `protocol.rs` (the transport, as a function from a request to a reply), `schema.rs` (the registered tools and their `inputSchema`), `server.rs` (the hyper listener) |
| FastMCP's argument validation (`FuncMetadata.pre_parse_json` and pydantic lax mode) | `pyvalidate.rs`, `schema.rs` |
| `JSONRPCMessage.model_validate` and its error text | `envelope.rs` |
| `tools/mcp_stdio_bridge.py`: `parse_args`, `run`, `_write_line`, `_write_error`, `main`, `DEFAULT_PORT`, `REQUEST_TIMEOUT_SECONDS` | `bridge.rs`, and the `hatate mcp-stdio-bridge` subcommand (`crates/hatate-cli/src/bridge_cmd.rs`) |
| `_mcp_top_level_import_succeeds` | `server::mcp_top_level_import_succeeds()`, always `true`: the Python function asks whether the optional `mcp` package imports, and the Rust server is part of the build. The "package missing or too new" messages of `start()` have no Rust counterpart. |
| `McpSettings` (8 keys) | already ported (`config/model.rs`); no key added |

Not here: `_run_tool` (gate, dry-run and audit around each tool body), the tool bodies, the
main-thread invoker, `mcp_images.fetch_full_resolution_match`: 5.e. The scripted handlers of the
differential implement `_run_tool` themselves, from the public `tools::gate`,
`tools::is_dry_runnable` and `audit::record`. **For 5.e:** `_run_tool` records `outcome=result`,
and for `get_images` that result holds `bytes`, which `json.dumps(default=str)` writes as
`str(bytes)`; `content::py_bytes_repr` is that function if the audit line must match.

## The handler trait

`handlers::McpHandlers` has one typed method per tool, in the names of `_HANDLER_METHOD_NAMES`
(all fourteen are the identity), and `shutdown_invoker()` for `stop()`. Arguments arrive
validated and coerced; a result is a `ToolOutput` (insertion-ordered fields; a value that carries
image bytes is an `ImageValue`); a failure is `HandlerError(str(exc))`. Methods block: the server
runs each call on a blocking thread. `dispatch` calls the method for a `schema::ToolCall`.

## Why not `rmcp` for the transport

`rmcp` 3.5.0 is on the plan's dependency list. This ticket's bar is that a client sees the same
bytes the Python server sends, and `rmcp`'s `StreamableHttpService` does not produce them:

- its SSE events are `Sse::default().data(..)` with an `id:` and priming/`retry:` events
  (`src/transport/common/server_side_http.rs`, `sse_stream_response`), where FastMCP sends
  `event: message\r\ndata: <json>\r\n\r\n` and nothing else;
- the error bodies, statuses and messages of its transport checks are its own, and FastMCP's
  `-32602 "Invalid request parameters"` with `data: ""`, its pydantic argument-validation text
  (`Error executing tool x: 1 validation error for xArguments ...`) and its "Validation error:"
  envelope text belong to the Python SDK, not to the MCP protocol.

So `protocol.rs` is a port of `mcp.server.streamable_http`, `streamable_http_manager`,
`transport_security`, `lowlevel.server` and `fastmcp.server` at 1.30.0, on `hyper` (already in the
lockfile through `reqwest`). `rmcp` is not a dependency. New direct dependencies, each already in
`Cargo.lock` except the last: `hyper` (`server`, `http1`), `hyper-util`, `http-body-util`,
`getrandom` (`uuid4().hex` session ids), `base64` (image blocks), and `httpdate` (hyper's server
needs it; it is the one new crate).

## What the server does (all measured against the Python server)

Same path (`/mcp`), host (`127.0.0.1`, never configurable), port and bearer token. Every request
needs `Authorization: Bearer <token>` or it gets `401`, `text/plain`, body `Unauthorized`
(compared as bytes, last header wins). Then, in FastMCP's order: unknown session id `404`;
content type, `Host` (`127.0.0.1:*`, `localhost:*`, `[::1]:*`, so a bare host is `421`) and
`Origin` checks (`400`/`421`/`403`, plain text); `Accept` `406`; content type `415`; body parse
`400` with CPython's `JSONDecodeError` text; envelope validation `400` with pydantic's text for all
four JSON-RPC models; missing session `400`; unsupported `MCP-Protocol-Version` `400`. A notification
or a client response is `202` with an empty body. A request is answered as one SSE event.
`initialize` negotiates one of `2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25` (anything
else is answered with the latest) and opens a session; `tools/list` prints the fourteen tools with
FastMCP's `inputSchema`, key order included; `tools/call` returns `isError` results for unknown
tools, bad arguments (pydantic 2.13 text, values coerced as pydantic's lax mode does) and raising
handlers; `ping`, `prompts/list`, `resources/list`, `resources/templates/list`, `prompts/get`,
`resources/read` and the methods FastMCP registers no handler for answer as FastMCP does. `GET`
opens the held-open event stream (one per session, `409` for a second), `DELETE` ends the session,
other methods are `405`, `/mcp/` is a `307`, any other path is `404`.

`start()` binds before it returns. A busy port is
`MCP server did not start listening on port N within 5s (port already in use?)`, a blank token is
`Blank bearer token: refusing to start the MCP server`, a token with a character beyond U+00FF is
`Could not start the MCP server: 'latin-1' codec can't encode character ...`, an out-of-range port
is `Could not start the MCP server: bind(): port must be 0-65535.`; `last_error_kind()` gives the
kind (`ErrorKind::McpServerStart`, `McpPortBusy`).

## Client configuration

**Streamable-HTTP clients are unchanged**: the same URL (`http://127.0.0.1:<port>/mcp`) and the
same `Authorization: Bearer <token>` header.

**Stdio-only clients change their command** (class B1, granted by the ticket). The Python
bridge was launched as `python3 tools/mcp_stdio_bridge.py`; the Rust one is a `hatate`
subcommand, so the client config names a different program:

```json
{
  "mcpServers": {
    "hatate-linux": {
      "command": "/path/to/hatate",
      "args": ["mcp-stdio-bridge"],
      "env": {"HATATE_MCP_PORT": "8787", "HATATE_MCP_TOKEN": "<your-token>"}
    }
  }
}
```

`hatate mcp-stdio-bridge [--port PORT] [--token TOKEN]` takes the same two flags and the same
`HATATE_MCP_PORT` / `HATATE_MCP_TOKEN` fallbacks, and relays one JSON-RPC message per line exactly
as the Python script does. It is a client of the server and does not start it. It speaks plain
http to `127.0.0.1` only; there is no flag or variable that points it elsewhere.

## Differential against the Python server

The harness lives outside the repository (the tree is Rust only): a Rust driver with scripted
handlers, and Python scripts that run the real `McpServerController` at `76c85c4`, drive both
servers with the same raw HTTP, and compare. Log: `worktrees/DAN-2166-differential.log`.

| Part | Cases | Differing |
|---|---|---|
| MCP wire, raw HTTP, seeds 1 to 3 (protocol, envelopes, parse errors, UTF-16 and UTF-32 bodies with and without a byte-order mark, headers, methods, paths, all fourteen tools under six gate configurations with dry-run and live flips, 200 generated argument sets per tool per seed, audit files after each group including rotation at 900, 1 and 0 bytes) | 3 x 3,337 | 39 (13 distinct inputs: J1, J2) |
| Direct: `gate`, `refusal_message`, `is_dry_runnable`, registry, `mapped`/`unaccounted_review_action_ids` (also over mutated action sets), `_redact`, `to_content_blocks`, audit line, seeds 1 to 3 | 3 x 1,246 | 0 |
| Controller: disabled, blank token, unencodable token, invalid ports, busy port (twice), start while running, restart, restart onto a busy port, stop twice | 21 | 0 |
| Bridge transport against a scripted server (SSE and JSON bodies, charsets, session id, blank lines, redirects, server down for each id shape, read timeout, closed without reply) | 49 | 3 (H1) |
| Bridge command line against the real binary | 41 | 6 (P1) |

## Classes (differences that are not matched)

Granted by the ticket: **B1** the stdio client config command (above); **V1**
`serverInfo.version` (Python reports the `mcp` package's `1.30.0`, this build its own crate
version). **T1** the pydantic `title` keys are reproduced, so the grant is not used.

Not matched, with the counts in the log:

- **J1, JSON text that Python accepts and this server cannot hold (6 inputs).** Python's
  `json.loads` accepts `NaN`, `Infinity` and `-Infinity`, a lone surrogate escape (`"\ud800"`)
  and nesting past 128 levels; `serde_json` does not. Python then answers `400 -32602` with
  pydantic's envelope text; Rust answers `400 -32700 Parse error: ...`. The same holds for a
  lone surrogate that arrives as text rather than as an escape: a UTF-16 body with an unpaired
  surrogate, and UTF-8 bytes that encode one (`ED A0 80`, which `surrogatepass` lets through).
  Python answers `200`; Rust answers `500 Error handling POST request`, because a Rust `str`
  cannot hold the character. Ruled an accepted class by Cloud on DAN-2190.
  UTF-16 and UTF-32 bodies are not in this class: `json.loads(bytes)` picks the encoding from
  a byte-order mark or from where the NUL bytes of the first four bytes fall, and the server
  does the same (`decode_json_bytes`). All 22 probe inputs (each of UTF-16 LE/BE and UTF-32
  LE/BE with and without a mark, each with a plain ping and a tool call, a UTF-8 mark, odd lengths, a code point past U+10FFFF, two- and
  three-byte bodies) match to the byte.
- **J2, integers beyond 64 bits (7 inputs).** Python passes them to the handler; the Rust handler
  arguments are `i64`, so the call returns `Error executing tool x: argument out of range`.
  `9223372036854775807` and `-9223372036854775808` match.
- **H1, HTTP-library error text in the bridge (3 inputs).** For a reply that is a garbage status
  line, a body shorter than its `Content-Length`, or malformed chunking, `requests` reports
  `BadStatusLine(...)`, `IncompleteRead(n bytes read, m more expected)` and `InvalidChunkLength(...)`;
  the Rust bridge reports a generic `Connection aborted` text. Refusal, timeout, and a closed
  connection match to the byte.
- **P1, `--help` text (6 inputs: `-h`, `--help`, `-hx`, `--h`, `--he`, `--hel`).** The usage line
  and the option lines match. The description paragraph is Python's docstring, which names
  `core/mcp_server.py`; the Rust help describes the program in its own words.

Also not compared: the `Date`, `Server`, `Connection`, `Transfer-Encoding` and `Content-Length`
headers (HTTP framing), the SSE keep-alive comment line that `sse-starlette` sends every 15 s
while a tool call runs, and the bridge's own request headers `User-Agent` and `Accept-Encoding`.

## Python test mapping

| Python test (at `76c85c4`) | Rust test, or why none |
|---|---|
| `test_mcp_audit.py` `TestAppending` (6 tests) | `audit::tests::creates_the_file_and_its_parent_and_writes_one_object_per_line`, `a_refusal_a_dry_run_and_a_candidate_index_are_logged`, `iso_format_matches_strftime`, `line_format_is_pythons_json_dumps_with_the_fixed_key_order` |
| `TestRedaction` (5) | `redaction_covers_every_key_nested_and_case_insensitive`, `secrets_never_reach_the_file` |
| `TestRotation` (2) | `rotates_once_the_cap_is_reached_and_keeps_one_generation`, `a_write_failure_is_swallowed` |
| `test_mcp_tools.py` `TestEveryToolIsAccountedFor` (3) | `tools::tests::tool_names_are_unique_and_findable`; "a gated tool names a real `McpSettings` field" is the `GateField` enum, checked by the compiler |
| the review-action coverage tests (5) | `tools::tests::nothing_is_unaccounted_for_and_the_lists_do_not_overlap` |
| `TestGating` (7) | `tools::tests::gating_follows_the_defaults_and_the_live_settings` |
| `TestRefusalMessage` (4) | `tools::tests::refusal_text_matches_the_spec_verbatim_and_names_no_attribute` |
| `TestDryRun` (3) | `tools::tests::dry_run_applies_to_the_three_write_tiers_only` |
| `test_mcp_server.py` `TestHandlerMethodNamesAreComplete` (3) | `handlers::tests::every_tool_has_a_handler_method_name_and_no_stray_entries`; "exists on a real handlers object" is the trait |
| `TestDispatch` (2) | `handlers::tests::dispatch_calls_the_matching_method_with_the_validated_arguments`, `every_registered_tool_dispatches_without_error` |
| `TestServerLifecycle` (6) | `tests/mcp_server.rs`: `start_refuses_disabled_blank_and_unencodable_tokens`, `a_busy_port_is_reported_then_stop_and_restart_work`. The "missing optional package" test has no Rust counterpart (see `_mcp_top_level_import_succeeds`) |
| `TestPortBusyDetection` (2) | `a_busy_port_is_reported_then_stop_and_restart_work` (a real busy socket; Python's test uses a fake uvicorn that raises, so its "Address already in use" text is the fake's) |
| `TestMissingOrIncompatiblePackage` (4), `TestInstallCommandIsPinnedEverywhere` (2) | none: they test the optional `mcp` package and its pinned `pip install` command; the Rust server is part of the build |
| `TestRealToolRegistration` (1) | `tests/mcp_server.rs`: `every_tool_is_called_through_the_wire_and_a_raising_handler_is_an_error_result`, `initialize_and_tools_list_match_the_python_server` |
| `TestContentBlocks` (3) | `content::tests` (`a_plain_result_becomes_one_text_block`, `an_image_key_becomes_an_image_block_plus_its_metadata`, `a_failed_fetch_with_no_bytes_produces_no_image_block`) |
| `test_mcp_bridge.py`, `test_mcp_images.py` | 5.e (DAN-2167): `gui/mcp_bridge.py` and `core/mcp_images.py` |
| `test_mcp_config.py` | `McpSettings` is already ported and tested (`config` tests) |
| `test_mcp_settings_tab.py` and the MCP lines of `test_type_voice.py` | Phase 6 (the Settings > MCP page) |
| none exists for `tools/mcp_stdio_bridge.py` | `tests/mcp_bridge.rs` and the bridge unit tests are new, written against the differential |
