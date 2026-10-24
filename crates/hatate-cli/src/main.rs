//! The `hatate` command. Work in progress: `batch` is experimental, and `mcp-stdio-bridge` is a
//! client of the MCP server (it does not start one).

mod args;
mod batch_cmd;
mod bridge_cmd;

use args::{Command, parse};

/// Message printed when no command is given.
fn banner() -> &'static str {
    "hatate-cli: work-in-progress Rust rewrite, not yet a replacement for Hatate"
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match parse(&argv) {
        Ok(Command::Banner) => {
            println!("{}", banner());
            println!("{}", args::USAGE);
        }
        Ok(Command::Help) => {
            println!("{}", banner());
            println!("{}", args::USAGE);
        }
        Ok(Command::Batch(opts)) => std::process::exit(batch_cmd::run(opts)),
        Ok(Command::McpStdioBridge(rest)) => std::process::exit(bridge_cmd::run(&rest)),
        Err(message) => {
            eprintln!("hatate: {message}");
            eprintln!("{}", args::USAGE);
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_says_work_in_progress() {
        assert!(banner().contains("work-in-progress"));
    }
}
