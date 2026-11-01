//! The headless Tab walk of the presubmit gate (DAN-2485, DAN-2499): `shot --tabs` walks Tab from the top of
//! each named surface under a virtual display, dark and light, and every stop must show a visible indicator
//! (a changed-pixel diff of the stop's own box). A stop with no indicator never appears in a shot, so the shot
//! set cannot catch it; this does.
//!
//! Needs a display, like `smoke`: runs under `xvfb-run` with `HATATE_GTK_SMOKE=1` and **fails closed**
//! with `CI` set. `HATATE_TAB_STATES=a,b` names the surfaces a PR added or changed (a PR that adds a window
//! or page lists it, and adds it to `TAB_STATES` in `examples/shot.rs`); `all` walks every surface (about 12
//! minutes). Without it a small default set runs, so a change to the shared focus CSS still reddens here.

use std::path::PathBuf;
use std::process::Command;

/// Surfaces walked when `HATATE_TAB_STATES` is unset: the main window, the review page and a Settings page.
const DEFAULT_STATES: &str = "empty,review,settings-8";

fn shot_binary() -> PathBuf {
    // target/<profile>/deps/presubmit_tabs-<hash> -> target/<profile>/examples/shot
    let exe = std::env::current_exe().unwrap();
    exe.parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("shot")
}

#[test]
fn every_tab_stop_of_the_named_surfaces_shows_an_indicator() {
    let ci = std::env::var_os("CI").is_some();
    let flag = std::env::var("HATATE_GTK_SMOKE").as_deref() == Ok("1");
    let display =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !flag {
        assert!(
            !ci,
            "CI is set but HATATE_GTK_SMOKE=1 is not: the Tab walk fails closed"
        );
        eprintln!("SKIPPED: the Tab walk needs a display; set HATATE_GTK_SMOKE=1 under xvfb-run");
        return;
    }
    assert!(
        display,
        "HATATE_GTK_SMOKE=1 but no DISPLAY or WAYLAND_DISPLAY: the Tab walk fails closed"
    );
    let shot = shot_binary();
    assert!(
        shot.is_file(),
        "{} is missing: run `cargo build -p hatate-gtk --example shot`",
        shot.display()
    );

    let wanted = std::env::var("HATATE_TAB_STATES").unwrap_or_else(|_| DEFAULT_STATES.to_string());
    let out = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(&shot);
    cmd.arg("--tabs").arg(out.path());
    if wanted != "all" {
        cmd.arg("--only").arg(&wanted);
    }
    let run = cmd.output().expect("run shot --tabs");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    eprintln!(
        "{}",
        stdout
            .lines()
            .filter(|l| l.starts_with("TABS "))
            .collect::<Vec<_>>()
            .join("\n")
    );

    let tsv =
        std::fs::read_to_string(out.path().join("tabs.tsv")).expect("shot --tabs wrote tabs.tsv");
    let rows: Vec<Vec<&str>> = tsv
        .lines()
        .skip(1)
        .map(|l| l.split('\t').collect())
        .collect();
    let bare: Vec<String> = rows
        .iter()
        .filter(|r| r.get(6) != Some(&"yes"))
        .map(|r| format!("{} {} stop {} {} (diffpx {})", r[0], r[1], r[2], r[3], r[5]))
        .collect();
    assert!(
        bare.is_empty(),
        "{} Tab stop(s) show no indicator:\n{}\n{stderr}",
        bare.len(),
        bare.join("\n")
    );
    // Every named surface was really walked, in both schemes, and has at least one stop.
    if wanted != "all" {
        for state in wanted.split(',') {
            for scheme in ["dark", "light"] {
                let n = rows
                    .iter()
                    .filter(|r| r[0] == state && r[1] == scheme)
                    .count();
                assert!(
                    n > 0,
                    "{state} {scheme}: the walk found no Tab stop\n{stderr}"
                );
            }
        }
    }
    assert!(
        run.status.success(),
        "shot --tabs failed:\n{stdout}\n{stderr}"
    );
}
