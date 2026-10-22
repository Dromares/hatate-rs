//! Placeholder binary for the Hatate Rust rewrite. Work in progress.

/// Message printed by the placeholder binary.
fn banner() -> &'static str {
    "hatate-mcp: work-in-progress Rust rewrite, not yet a replacement for Hatate"
}

fn main() {
    println!("{}", banner());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_says_work_in_progress() {
        assert!(banner().contains("work-in-progress"));
    }
}
