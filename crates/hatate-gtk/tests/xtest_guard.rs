//! The key injector refuses a display that is not demonstrably the headless Xvfb the runner started
//! (DAN-2264). No X server needed: `check_headless` is pure and `Xtest::connect` calls it before it
//! opens any socket, so these run in every `cargo test`, not only under Xvfb.

#[allow(dead_code)]
#[path = "common/xtest.rs"]
mod xtest;

use std::ffi::OsStr;
use xtest::check_headless;

const XVFB_AUTH: &str = "/tmp/xvfb-run.a1B2c3/Xauthority.d4E5f6";

fn refused(display: Option<&str>, flag: Option<&str>, auth: Option<&str>) -> String {
    check_headless(display, flag, auth.map(OsStr::new)).expect_err("must refuse")
}

#[test]
fn the_documented_xvfb_run_environment_passes() {
    assert!(check_headless(Some(":99"), Some("1"), Some(OsStr::new(XVFB_AUTH))).is_ok());
    assert!(
        check_headless(
            Some(":99.0"),
            Some("1"),
            Some(OsStr::new("/var/tmp/xvfb-run.X/Xauthority.Y"))
        )
        .is_ok()
    );
}

#[test]
fn a_live_session_is_refused_without_the_flag() {
    let why = refused(Some(":0"), None, Some("/home/u/.Xauthority"));
    assert!(why.contains("HATATE_SMOKE_XVFB=1 is not set"), "{why}");
    // The flag must be exactly "1": not "0", not empty, not "true".
    for flag in ["0", "", "true"] {
        let why = refused(Some(":99"), Some(flag), Some(XVFB_AUTH));
        assert!(why.contains("is not set"), "{flag:?}: {why}");
    }
}

#[test]
fn the_flag_alone_does_not_open_a_live_session() {
    // `HATATE_SMOKE_XVFB=1 DISPLAY=:0` with the desktop's own cookie, or none at all.
    for auth in [
        Some("/home/u/.Xauthority"),
        Some("/run/user/1000/xauth_AbCdEf"),
        Some("/run/user/1000/gdm/Xauthority"),
        Some("/tmp/Xauthority.d4E5f6"), // right file name, not in an xvfb-run directory
        Some("/tmp/xvfb-run.a1B2c3/.Xauthority"), // right directory, not xvfb-run's file name
        Some("/"),
        None,
    ] {
        let why = refused(Some(":0"), Some("1"), auth);
        assert!(
            why.contains("refusing to inject keys into DISPLAY=:0"),
            "{auth:?}: {why}"
        );
    }
}

#[test]
fn no_display_is_refused() {
    assert_eq!(
        refused(None, Some("1"), Some(XVFB_AUTH)),
        "DISPLAY is not set"
    );
}

#[test]
fn connect_refuses_before_opening_any_socket() {
    // Whatever DISPLAY is, `connect` must return the refusal and never reach the socket code.
    // SAFETY: this binary's tests run in one process and only this one touches these variables.
    unsafe {
        std::env::remove_var(xtest::HEADLESS_FLAG);
        std::env::set_var("DISPLAY", ":0");
    }
    let why = match xtest::Xtest::connect() {
        Ok(_) => panic!("connect must refuse"),
        Err(why) => why,
    };
    assert!(why.contains("refusing to inject keys"), "{why}");
}
