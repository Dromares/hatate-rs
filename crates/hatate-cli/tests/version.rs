//! DAN-2501: `hatate --version` prints the workspace version, the one version source, and says
//! work in progress. The binary is run, not its `version_line` called.

use std::process::Command;

#[test]
fn version_flag_prints_the_workspace_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_hatate"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        text,
        format!(
            "hatate {} (work-in-progress Rust rewrite, not yet a replacement for Hatate)\n",
            hatate_core::version::VERSION
        )
    );
}
