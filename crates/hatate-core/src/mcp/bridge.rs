//! The stdio bridge (`tools/mcp_stdio_bridge.py`): relays one JSON-RPC message per line from
//! stdin to the embedded server over streamable-HTTP, and the server's replies back to stdout.
//!
//! The relay is Python's, line for line: the same URL, headers, session-id handling, SSE
//! `data:` extraction, and the same JSON-RPC error line when the server cannot be reached.
//! What changes is the program a client config launches (`hatate mcp-stdio-bridge`); see
//! `docs/mcp-server.md`.

use std::io::{BufRead, Write};
use std::time::Duration;

use serde_json::{Value, json};

use crate::hydrus_client::py_dumps;
use crate::pycompat::strip;

/// Loopback only, like [`super::HOST`]: the bridge never takes a `--host`.
pub const HOST: &str = super::HOST;
pub const DEFAULT_PORT: i64 = 8787;
/// The streamable-HTTP transport can hold a request open while a tool call runs (a Hydrus upload,
/// a re-search), so this is generous: a timeout here reads to the AI as the call having failed.
pub const REQUEST_TIMEOUT_SECONDS: f64 = 120.0;

/// Parsed command line (`argparse.Namespace`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub port: i64,
    pub token: String,
}

/// Why `parse_args` did not return arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgsOutcome {
    /// `-h`/`--help`: print the text, exit 0.
    Help(String),
    /// argparse's usage error: print the text on stderr, exit 2.
    UsageError(String),
    /// A Python exception out of `parse_args` itself (a non-integer `$HATATE_MCP_PORT`): exit 1.
    Failure(String),
}

const PROG: &str = "hatate mcp-stdio-bridge";

fn usage() -> String {
    format!("usage: {PROG} [-h] [--port PORT] [--token TOKEN]")
}

/// Python's `int(str)`: surrounding whitespace, a sign, digits with single underscores between.
pub fn py_int_str(s: &str) -> Option<i64> {
    let t = strip(s);
    let (neg, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if body.is_empty() || body.starts_with('_') || body.ends_with('_') || body.contains("__") {
        return None;
    }
    let digits: String = body.chars().filter(|c| *c != '_').collect();
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: i64 = digits.parse().ok()?;
    Some(if neg { -n } else { n })
}

/// `parse_args(argv)` with the environment given as a lookup. `--port`/`--token` accept any
/// unambiguous prefix and `--name=value`, as argparse does.
pub fn parse_args<F: Fn(&str) -> Option<String>>(
    argv: &[String],
    env: F,
) -> Result<Args, ArgsOutcome> {
    let default_port = match env("HATATE_MCP_PORT") {
        None => DEFAULT_PORT,
        Some(raw) => py_int_str(&raw).ok_or_else(|| {
            ArgsOutcome::Failure(format!(
                "ValueError: invalid literal for int() with base 10: {}",
                crate::pyrepr::py_repr(&json!(raw))
            ))
        })?,
    };
    let mut port = default_port;
    let mut token = env("HATATE_MCP_TOKEN").unwrap_or_default();
    let help = || {
        format!(
            "{}\n\nBridges a stdio-only MCP client (Claude Desktop and similar) to the app's embedded MCP server, which only speaks streamable-HTTP over loopback\n\noptions:\n  -h, --help     show this help message and exit\n  --port PORT    Port the embedded server is listening on (default {DEFAULT_PORT}, or $HATATE_MCP_PORT)\n  --token TOKEN  Bearer token from Settings > MCP (or $HATATE_MCP_TOKEN)\n",
            usage()
        )
    };
    let error = |message: String| {
        ArgsOutcome::UsageError(format!("{}\n{PROG}: error: {message}\n", usage()))
    };
    let mut extras: Vec<String> = Vec::new();
    let mut i = 0;
    let mut only_positionals = false;
    while i < argv.len() {
        let arg = &argv[i];
        i += 1;
        if only_positionals || !arg.starts_with('-') || arg == "-" || looks_negative(arg) {
            extras.push(arg.clone());
            continue;
        }
        if arg == "--" {
            only_positionals = true;
            continue;
        }
        if arg == "-h" || arg == "--help" {
            return Err(ArgsOutcome::Help(help()));
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if arg.starts_with("--") => (n.to_owned(), Some(v.to_owned())),
            _ => (arg.clone(), None),
        };
        let matches: Vec<&str> = ["--help", "--port", "--token"]
            .into_iter()
            .filter(|o| name.len() > 2 && o.starts_with(name.as_str()))
            .collect();
        let option = match matches.as_slice() {
            [one] => *one,
            [] => {
                extras.push(arg.clone());
                continue;
            }
            many => {
                return Err(error(format!(
                    "ambiguous option: {name} could match {}",
                    many.join(", ")
                )));
            }
        };
        if option == "--help" {
            return Err(ArgsOutcome::Help(help()));
        }
        let value = match inline {
            Some(v) => v,
            None => match argv.get(i) {
                Some(next) if !next.starts_with('-') || next == "-" || looks_negative(next) => {
                    i += 1;
                    next.clone()
                }
                _ => return Err(error(format!("argument {option}: expected one argument"))),
            },
        };
        if option == "--port" {
            port = py_int_str(&value).ok_or_else(|| {
                error(format!(
                    "argument --port: invalid int value: {}",
                    crate::pyrepr::py_repr(&json!(value))
                ))
            })?;
        } else {
            token = value;
        }
    }
    if !extras.is_empty() {
        return Err(error(format!(
            "unrecognized arguments: {}",
            extras.join(" ")
        )));
    }
    Ok(Args { port, token })
}

/// argparse treats `-5` as a value, not an option, when no option looks like a number.
fn looks_negative(arg: &str) -> bool {
    arg.strip_prefix('-').is_some_and(|rest| {
        !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_digit() || c == '.')
            && rest.chars().any(|c| c.is_ascii_digit())
    })
}

/// `str.splitlines()`: Python also breaks at `\v`, `\f`, `\x1c`-`\x1e`, `\x85`, `\u2028`, `\u2029`.
pub fn splitlines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let is_break = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !is_break {
            continue;
        }
        out.push(&text[start..i]);
        let mut end = i + c.len_utf8();
        if c == '\r' && chars.peek().is_some_and(|(_, n)| *n == '\n') {
            chars.next();
            end += 1;
        }
        start = end;
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// `response.text`, the way `requests` decodes it: the header's charset, else `ISO-8859-1` for a
/// `text/*` type and UTF-8 for `application/json`, else UTF-8; undecodable bytes become U+FFFD.
pub fn response_text(content_type: &str, body: &[u8]) -> String {
    let lower = content_type.to_lowercase();
    let charset = content_type.split(';').skip(1).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        (k.trim().eq_ignore_ascii_case("charset"))
            .then(|| v.trim().trim_matches(['"', '\'']).to_lowercase())
    });
    let encoding = match charset {
        Some(c) => c,
        None if lower.contains("text") => "iso-8859-1".to_owned(),
        None if lower.contains("application/json") => "utf-8".to_owned(),
        None => "utf-8".to_owned(),
    };
    match encoding.as_str() {
        "iso-8859-1" | "latin-1" | "latin1" | "iso8859-1" | "l1" | "8859" | "cp819" => {
            body.iter().map(|b| char::from(*b)).collect()
        }
        "ascii" | "us-ascii" => body
            .iter()
            .map(|b| {
                if *b < 0x80 {
                    char::from(*b)
                } else {
                    '\u{fffd}'
                }
            })
            .collect(),
        _ => String::from_utf8_lossy(body).into_owned(),
    }
}

fn write_line<W: Write>(out: &mut W, text: &str) {
    if !text.is_empty() {
        let _ = out.write_all(format!("{text}\n").as_bytes());
        let _ = out.flush();
    }
}

/// `_write_error(original_request, message)`: hands the client a JSON-RPC error rather than
/// dropping the request, so a client waiting on a specific id sees that id fail.
pub fn write_error<W: Write>(out: &mut W, original_request: &str, message: &str) {
    let request_id = serde_json::from_str::<Value>(original_request)
        .ok()
        .and_then(|v| {
            v.as_object()
                .map(|m| m.get("id").cloned().unwrap_or(Value::Null))
        })
        .unwrap_or(Value::Null);
    let line = py_dumps(&json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {"code": -32000, "message": message},
    }));
    write_line(out, &line);
}

/// What `requests` raises for a failed exchange, as `str(exc)`.
fn requests_error_text(error: &reqwest::Error, port: i64, timeout: f64) -> String {
    let pool = format!("HTTPConnectionPool(host='{HOST}', port={port})");
    if error.is_timeout() && !error.is_connect() {
        return format!(
            "{pool}: Read timed out. (read timeout={})",
            crate::pyrepr::py_repr(&json!(timeout))
        );
    }
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(e) = source {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            if let Some(code) = io.raw_os_error() {
                let text = io.to_string();
                let text = text
                    .rsplit_once(" (os error")
                    .map_or(text.as_str(), |(t, _)| t);
                return format!(
                    "{pool}: Max retries exceeded with url: /mcp (Caused by NewConnectionError(\"HTTPConnection(host='{HOST}', port={port}): Failed to establish a new connection: [Errno {code}] {text}\"))"
                );
            }
        }
        source = e.source();
    }
    "('Connection aborted.', RemoteDisconnected('Remote end closed connection without response'))"
        .to_owned()
}

/// `run(port, token)`, with the timeout a parameter so a test need not wait 120 s. Returns the
/// process exit code.
pub fn run<R: BufRead, W: Write, E: Write>(
    port: i64,
    token: &str,
    timeout_seconds: f64,
    input: R,
    mut out: W,
    mut err: E,
) -> i32 {
    if token.is_empty() {
        let _ = writeln!(
            err,
            "mcp_stdio_bridge: no token given (--token or $HATATE_MCP_TOKEN) - the server will refuse every request"
        );
        return 2;
    }
    // The URL is plain http on loopback, so no TLS is spoken, but reqwest is built with no
    // built-in provider and refuses to build a client without one (CLAUDE.md, network clients).
    crate::net::install_crypto_provider();
    let url = format!("http://{HOST}:{port}/mcp");
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return 1,
    };
    let timeout = Duration::from_secs_f64(timeout_seconds);
    let client = match reqwest::Client::builder()
        .connect_timeout(timeout)
        .read_timeout(timeout)
        .build()
    {
        Ok(c) => c,
        Err(_) => return 1,
    };
    let mut session_id: Option<String> = None;
    for line in input.lines() {
        let Ok(line) = line else { break };
        let line = strip(&line).to_owned();
        if line.is_empty() {
            continue;
        }
        let mut request = client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(line.clone());
        if let Some(id) = &session_id {
            request = request.header("Mcp-Session-Id", id);
        }
        let exchange = runtime.block_on(async {
            let response = request.send().await?;
            let status = response.status();
            let headers = response.headers().clone();
            let body = response.bytes().await?;
            Ok::<_, reqwest::Error>((status, headers, body))
        });
        let (_, headers, body) = match exchange {
            Ok(parts) => parts,
            Err(e) => {
                let text = requests_error_text(&e, port, timeout_seconds);
                write_error(
                    &mut out,
                    &line,
                    &format!("Could not reach the app's MCP server at {url}: {text}"),
                );
                continue;
            }
        };
        if let Some(id) = headers.get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            session_id = Some(id.to_owned());
        }
        let content_type = headers
            .get("content-type")
            .map(|v| {
                v.as_bytes()
                    .iter()
                    .map(|b| char::from(*b))
                    .collect::<String>()
            })
            .unwrap_or_default();
        if content_type.contains("text/event-stream") {
            for event_line in splitlines(&response_text(&content_type, &body)) {
                if let Some(data) = event_line.strip_prefix("data:") {
                    write_line(&mut out, strip(data));
                }
            }
        } else if !body.is_empty() {
            write_line(&mut out, strip(&response_text(&content_type, &body)));
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn flags_and_environment_fallbacks() {
        assert_eq!(
            parse_args(&args(&[]), no_env).unwrap(),
            Args {
                port: 8787,
                token: String::new()
            }
        );
        assert_eq!(
            parse_args(&args(&["--port", "9", "--token", "t"]), no_env).unwrap(),
            Args {
                port: 9,
                token: "t".into()
            }
        );
        assert_eq!(
            parse_args(&args(&["--port=10", "--tok=u"]), no_env).unwrap(),
            Args {
                port: 10,
                token: "u".into()
            }
        );
        let env = |k: &str| match k {
            "HATATE_MCP_PORT" => Some("4242".to_owned()),
            "HATATE_MCP_TOKEN" => Some("envtok".to_owned()),
            _ => None,
        };
        assert_eq!(
            parse_args(&args(&[]), env).unwrap(),
            Args {
                port: 4242,
                token: "envtok".into()
            }
        );
        assert_eq!(parse_args(&args(&["--port", "-5"]), env).unwrap().port, -5);
        assert_eq!(
            parse_args(&args(&["--token", "flag"]), env).unwrap().token,
            "flag",
            "a flag beats the environment"
        );
    }

    #[test]
    fn bad_input_is_argparses_usage_error_and_a_bad_env_port_is_a_failure() {
        assert!(
            matches!(parse_args(&args(&["--port", "x"]), no_env), Err(ArgsOutcome::UsageError(m)) if m.contains("argument --port: invalid int value: 'x'"))
        );
        assert!(
            matches!(parse_args(&args(&["--port"]), no_env), Err(ArgsOutcome::UsageError(m)) if m.contains("expected one argument"))
        );
        assert!(matches!(
            parse_args(&args(&["--token", "--port"]), no_env),
            Err(ArgsOutcome::UsageError(_))
        ));
        assert!(
            matches!(parse_args(&args(&["stray", "x"]), no_env), Err(ArgsOutcome::UsageError(m)) if m.contains("unrecognized arguments: stray x"))
        );
        assert!(matches!(
            parse_args(&args(&["-h"]), no_env),
            Err(ArgsOutcome::Help(_))
        ));
        let bad = |k: &str| (k == "HATATE_MCP_PORT").then(|| "abc".to_owned());
        assert!(
            matches!(parse_args(&args(&[]), bad), Err(ArgsOutcome::Failure(m)) if m.contains("invalid literal for int() with base 10: 'abc'"))
        );
    }

    #[test]
    fn py_int_accepts_what_python_accepts() {
        assert_eq!(py_int_str(" 1_000 "), Some(1000));
        assert_eq!(py_int_str("+7"), Some(7));
        assert_eq!(py_int_str("-0"), Some(0));
        for bad in ["", "1__0", "_1", "1_", "1.0", "0x1", "1e3", "--1"] {
            assert_eq!(py_int_str(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn splitlines_breaks_where_python_does() {
        assert_eq!(splitlines("a\nb\r\nc\rd"), vec!["a", "b", "c", "d"]);
        assert_eq!(
            splitlines("a\u{b}b\u{c}c\u{1c}d\u{85}e\u{2028}f\u{2029}g"),
            vec!["a", "b", "c", "d", "e", "f", "g"]
        );
        assert_eq!(splitlines("a\n"), vec!["a"]);
        assert_eq!(splitlines("\n\n"), vec!["", ""]);
        assert!(splitlines("").is_empty());
    }

    #[test]
    fn text_decoding_follows_requests() {
        let utf8 = "é".as_bytes();
        assert_eq!(
            response_text("text/event-stream", utf8),
            "Ã©",
            "text/* without a charset is latin-1"
        );
        assert_eq!(response_text("application/json", utf8), "é");
        assert_eq!(response_text("text/event-stream; charset=utf-8", utf8), "é");
        assert_eq!(response_text("text/plain", b"\xff"), "ÿ");
        assert_eq!(response_text("application/json", b"\xff"), "\u{fffd}");
    }

    #[test]
    fn write_error_is_json_dumps_of_a_jsonrpc_error() {
        let mut out = Vec::new();
        write_error(
            &mut out,
            r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
            "no \u{e9}",
        );
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "{\"jsonrpc\": \"2.0\", \"id\": 7, \"error\": {\"code\": -32000, \"message\": \"no \\u00e9\"}}\n"
        );
        for original in ["not json", "5", "[1]", "{}", r#"{"id":null}"#] {
            let mut out = Vec::new();
            write_error(&mut out, original, "m");
            assert!(
                String::from_utf8(out).unwrap().contains("\"id\": null"),
                "{original}"
            );
        }
    }

    #[test]
    fn no_token_exits_2_before_reading_anything() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(run(8787, "", 1.0, &b"{}\n"[..], &mut out, &mut err), 2);
        assert!(out.is_empty());
        assert!(String::from_utf8(err).unwrap().contains("no token given"));
    }
}
