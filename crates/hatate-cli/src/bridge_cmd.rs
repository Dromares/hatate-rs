//! `hatate mcp-stdio-bridge`: `tools/mcp_stdio_bridge.py`'s `main()`. A client of the embedded MCP
//! server on 127.0.0.1, for MCP clients that only speak stdio. It does not start the server.

use std::io::{self, Write};

use hatate_core::mcp::bridge::{
    ArgsOutcome, REQUEST_TIMEOUT_SECONDS, parse_args, run as run_bridge,
};

/// Exit status: 0 the input ended, 1 a Python-style failure, 2 usage (including no token).
pub fn run(argv: &[String]) -> i32 {
    let args = match parse_args(argv, |name| std::env::var(name).ok()) {
        Ok(args) => args,
        Err(ArgsOutcome::Help(text)) => {
            print!("{text}");
            return 0;
        }
        Err(ArgsOutcome::UsageError(text)) => {
            eprint!("{text}");
            return 2;
        }
        Err(ArgsOutcome::Failure(text)) => {
            eprintln!("{text}");
            return 1;
        }
    };
    let stdin = io::stdin();
    let stdout = io::stdout();
    let code = run_bridge(
        args.port,
        &args.token,
        REQUEST_TIMEOUT_SECONDS,
        stdin.lock(),
        stdout.lock(),
        io::stderr(),
    );
    let _ = io::stdout().flush();
    code
}
