//! `cargo run -p xtask -- <task>`: workspace tooling, never shipped, Rust only (CLAUDE.md).
//!
//! - `doc-cites convert [--write]`: rewrite every old `file.rs:NNN` cite in `docs/*.md` into the
//!   function form (`file.rs` `fn name` `fragment`) at the tree's current sources, and list the
//!   cites that need a hand fix (the cited line is blank, a comment, or outside any fn).
//! - `appimage [--out DIR] [--dl DIR] [--no-build]`: builds `Hatate-x86_64.AppImage` from a release
//!   build (`xtask/src/appimage.rs`; the pinned inputs are in `packaging/appimage/pins.toml`).

mod appimage;
mod doc_cites;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["doc-cites", "convert", rest @ ..] => doc_cites::convert(rest.contains(&"--write")),
        ["appimage", rest @ ..] => appimage::build(rest),
        _ => {
            eprintln!(
                "usage: cargo run -p xtask -- doc-cites convert [--write] | appimage [--out DIR] [--dl DIR] [--no-build]"
            );
            2
        }
    };
    std::process::exit(code);
}
