//! The `hatate` command. `batch` is experimental, and `mcp-stdio-bridge` is a client of the MCP
//! server (it does not start one).

mod args;
mod batch_cmd;
mod bridge_cmd;

use args::{Command, parse};

/// Message printed when no command is given.
fn banner() -> &'static str {
    "hatate-cli: the command line of the Rust rewrite of Hatate"
}

/// What `--version` prints: the one version source (`hatate_core::version::VERSION`).
fn version_line() -> String {
    format!(
        "hatate {} (Rust rewrite of Hatate)",
        hatate_core::version::VERSION
    )
}

fn main() {
    // A panic in any command lands in `crash.log`. Without a home directory there is no profile,
    // and the default hook alone reports it.
    if let Ok(paths) = hatate_core::paths::Paths::from_env() {
        hatate_core::crash_marker::install_panic_hook(&paths, "hatate");
    }
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
        Ok(Command::Version) => println!("{}", version_line()),
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
    fn banner_names_the_rewrite_and_claims_nothing_else() {
        let text = banner();
        assert!(text.contains("Rust rewrite of Hatate"), "{text}");
        assert!(!text.contains("work-in-progress"), "{text}");
    }

    #[test]
    fn version_is_0_2_x_and_says_what_this_is() {
        let line = version_line();
        assert!(line.starts_with("hatate 0.2."), "{line}");
        // The number is the one version source's, not a copy (DAN-2501).
        assert!(
            line.starts_with(&format!("hatate {} (", hatate_core::version::VERSION)),
            "{line}"
        );
        assert!(line.ends_with("(Rust rewrite of Hatate)"), "{line}");
        assert!(!line.contains("work-in-progress"), "{line}");
    }
}
