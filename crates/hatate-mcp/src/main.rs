//! Placeholder binary for the Hatate Rust rewrite. Work in progress.

// The panic hook (DAN-2501). This placeholder has no dependencies, so the one std-only source
// file is included instead of linking `hatate-core`.
#[allow(dead_code)] // `arm` and the entry text are for the binaries that open a section
#[path = "../../hatate-core/src/crash_report.rs"]
mod crash_report;

/// Message printed by the placeholder binary.
fn banner() -> &'static str {
    "hatate-mcp: work-in-progress Rust rewrite, not yet a replacement for Hatate"
}

fn main() {
    // A panic lands in `crash.log`, as in every binary (DAN-2501).
    if let Some(crash_log) = crash_report::crash_log_path(&|name| std::env::var_os(name)) {
        crash_report::install(crash_log, "hatate-mcp", env!("CARGO_PKG_VERSION"));
    }
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
