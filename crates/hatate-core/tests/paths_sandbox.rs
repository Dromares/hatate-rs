//! Paths resolve from the environment the process was started with, and the test
//! scaffolding keeps every other test off the real profile (DAN-1979 acceptance).

mod common;

use std::path::Path;
use std::process::Command;

use common::Sandbox;
use hatate_core::paths::Paths;

#[test]
fn sandbox_paths_are_inside_the_temp_dir() {
    for sb in [Sandbox::new(), Sandbox::home_only()] {
        sb.assert_sandboxed();
        assert!(sb.paths.config_file().starts_with(sb.dir.path()));
        assert!(sb.paths.session_db().starts_with(sb.dir.path()));
    }
}

#[test]
fn home_only_sandbox_uses_the_dot_config_default() {
    let sb = Sandbox::home_only();
    assert_eq!(
        sb.paths.config_dir(),
        sb.dir.path().join("home/.config/hatate-linux")
    );
    assert_eq!(
        sb.paths.cache_dir(),
        sb.dir.path().join("home/.cache/hatate-linux")
    );
}

/// The guard itself must bite: a profile under the real one has to be rejected.
#[test]
#[should_panic(expected = "sandbox")]
fn guard_rejects_the_real_profile() {
    let Ok(real) = Paths::from_env() else {
        panic!("no real profile to compare with: sandbox resolved to the real profile");
    };
    let sb = Sandbox::new();
    // Pretend a bug made the sandbox resolve like the real environment.
    let fake = Sandbox {
        dir: sb.dir,
        paths: real,
    };
    fake.assert_sandboxed();
}

/// `Paths::from_env` reads the real variables. Re-run this test binary as a child with a
/// scrubbed environment so the parent's variables cannot leak in.
#[test]
fn from_env_reads_the_process_environment() {
    if std::env::var_os("HATATE_TEST_CHILD").is_some() {
        let p = Paths::from_env().expect("paths");
        println!("CONFIG={}", p.config_dir().display());
        println!("CACHE={}", p.cache_dir().display());
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let run = |vars: &[(&str, &Path)]| {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "from_env_reads_the_process_environment",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("HATATE_TEST_CHILD", "1");
        for (k, v) in vars {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = String::from_utf8(out.stdout).unwrap();
        let get = |key: &str| {
            text.lines()
                .find_map(|l| l.split_once(key))
                .unwrap()
                .1
                .to_owned()
        };
        (get("CONFIG="), get("CACHE="))
    };
    let home = tmp.path().join("h");
    let xdg = tmp.path().join("x");
    let (config, cache) = run(&[("HOME", &home)]);
    assert_eq!(
        config,
        home.join(".config/hatate-linux").display().to_string()
    );
    assert_eq!(
        cache,
        home.join(".cache/hatate-linux").display().to_string()
    );
    let (config, cache) = run(&[
        ("HOME", &home),
        ("XDG_CONFIG_HOME", &xdg),
        ("XDG_CACHE_HOME", &xdg),
    ]);
    assert_eq!(config, xdg.join("hatate-linux").display().to_string());
    assert_eq!(cache, xdg.join("hatate-linux").display().to_string());
}

/// Spec 1.2 lists the names under `CONFIG_DIR`; the Rust-only file must not be one of them.
#[test]
fn rust_settings_file_collides_with_nothing_in_section_1_2() {
    let doc = common::doc();
    let start = doc.find("### 1.2 Files and sub-directories").unwrap();
    let end = doc[start..].find("### 1.3").unwrap() + start;
    let section = &doc[start..end];
    let names: Vec<&str> = section
        .lines()
        .filter(|l| l.starts_with("| `"))
        .flat_map(|l| l.split('|').nth(1).unwrap().split(','))
        .filter_map(|cell| cell.split('`').nth(1))
        .collect();
    assert!(
        names.contains(&"config.json") && names.contains(&"session.db"),
        "parsed the wrong table: {names:?}"
    );
    assert_eq!(names.len(), 19, "{names:?}");
    let rust = hatate_core::paths::RUST_SETTINGS_FILE_NAME;
    assert_eq!(rust, "hatate-rs.json");
    for n in &names {
        // The rotated logs are written as "x.log" plus ".1 .2 .3" in prose, not as names.
        assert_ne!(*n, rust, "collides with {n}");
        assert!(!n.starts_with("hatate-rs"), "{n}");
    }
    let sb = Sandbox::new();
    let p = &sb.paths;
    // Rule 6.1.1: every file and directory of 1.2 has an accessor.
    let known: Vec<String> = [
        p.config_file(),
        p.config_backup_file(),
        p.config_tmp_file(),
        p.config_unreadable_file(),
        p.session_db(),
        p.session_json(),
        p.search_cache_dir(),
        p.default_matched_urls_log(),
        p.app_log(),
        p.crash_log(),
        p.crash_running_marker(),
        p.last_clean_exit(),
        p.mcp_audit_log(),
        p.saucenao_quota_pause(),
        p.pawchive_index_db(),
        p.pawchive_creators(),
        p.lens_profile_dir(),
    ]
    .iter()
    .map(|f| f.file_name().unwrap().to_string_lossy().into_owned())
    .collect();
    for n in &names {
        let base = n.trim_end_matches('/').split('/').next().unwrap();
        if base.starts_with("session.db-") {
            continue; // SQLite's own WAL files, named by SQLite
        }
        assert!(known.iter().any(|k| k == base), "no accessor for {n}");
    }
    assert_eq!(
        sb.paths.rust_settings_file(),
        sb.paths.config_dir().join(rust)
    );
}
