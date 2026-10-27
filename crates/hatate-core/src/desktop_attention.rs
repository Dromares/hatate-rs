//! Whether the Lens browser window may call for the user, on KDE Plasma (`core/desktop_attention.py`
//! at `76c85c4`).
//!
//! The Lens window asks for attention during an ordinary search; KWin's focus-stealing prevention
//! turns that into "demands attention", and an auto-hide panel then stays up until the window is
//! clicked. So the window is kept quiet (off the taskbar, its attention flag cleared) except while
//! Google is asking for a robot check, when it is put back on the taskbar and flagged.
//!
//! Done with a short KWin script sent over D-Bus with `qdbus6`, which changes only the window whose
//! class is [`crate::lens_browser::WindowAttention`]'s argument. Anywhere that is not Plasma, or
//! without `qdbus6`, every call does nothing: this is a courtesy, and a search must never depend
//! on it. The script is a string constant ported from Python's.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use tracing::debug;

use crate::lens_browser::WindowAttention;
use crate::pyrepr::py_repr;

/// `SCRIPT_NAME`.
pub const SCRIPT_NAME: &str = "hatate-lens-attention";
/// `QDBUS_TIMEOUT`, seconds.
pub const QDBUS_TIMEOUT: Duration = Duration::from_secs(5);

/// `script_for(window_class, skip_taskbar, demands_attention)`: the KWin script.
pub fn script_for(window_class: &str, skip_taskbar: bool, demands_attention: bool) -> String {
    let js = |v: bool| if v { "true" } else { "false" };
    let class = py_repr(&Value::String(window_class.to_owned()));
    format!(
        "const ws = workspace.windowList ? workspace.windowList() : workspace.clientList();\n\
         for (const w of ws) {{\n  if (w.resourceClass !== {class}) continue;\n  \
         w.skipTaskbar = {};\n  w.demandsAttention = {};\n}}\n",
        js(skip_taskbar),
        js(demands_attention)
    )
}

static LOCK: Mutex<()> = Mutex::new(());
static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The Plasma implementation of [`WindowAttention`].
pub struct KdeAttention {
    /// `XDG_CURRENT_DESKTOP`.
    desktop: String,
    /// The directories `shutil.which` searches (`PATH`).
    path: Vec<PathBuf>,
}

impl KdeAttention {
    /// From the process environment.
    pub fn from_process() -> KdeAttention {
        let path = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        KdeAttention::new(
            std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            path,
        )
    }

    /// With the desktop name and the search path given (a test's).
    pub fn new(desktop: String, path: Vec<PathBuf>) -> KdeAttention {
        KdeAttention { desktop, path }
    }

    fn qdbus(&self) -> Option<PathBuf> {
        ["qdbus6", "qdbus-qt6"]
            .iter()
            .find_map(|name| self.path.iter().map(|d| d.join(name)).find(|p| is_exec(p)))
    }

    /// `available()`: Plasma, and a `qdbus6`.
    pub fn available(&self) -> bool {
        self.desktop.to_uppercase().contains("KDE") && self.qdbus().is_some()
    }

    fn apply(&self, window_class: &str, skip_taskbar: bool, demands_attention: bool) {
        if !self.available() {
            return;
        }
        let Some(qdbus) = self.qdbus() else { return };
        let script = script_for(window_class, skip_taskbar, demands_attention);
        let _guard = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let file = std::env::temp_dir().join(format!(
            "hatate-kwin-{}-{}.js",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let result = std::fs::write(&file, script).and_then(|()| {
            let base = |extra: &[&OsString]| -> Vec<OsString> {
                let mut v: Vec<OsString> = vec!["org.kde.KWin".into(), "/Scripting".into()];
                v.extend(extra.iter().map(|s| (*s).clone()));
                v
            };
            let name: OsString = SCRIPT_NAME.into();
            let unload: OsString = "org.kde.kwin.Scripting.unloadScript".into();
            let load: OsString = "org.kde.kwin.Scripting.loadScript".into();
            let start: OsString = "org.kde.kwin.Scripting.start".into();
            let path: OsString = file.clone().into_os_string();
            // A previous run's script of the same name would block loading.
            run(&qdbus, &base(&[&unload, &name]))?;
            run(&qdbus, &base(&[&load, &path, &name]))?;
            run(&qdbus, &base(&[&start]))?;
            run(&qdbus, &base(&[&unload, &name]))
        });
        if let Err(e) = result {
            debug!("Could not set the Lens window's taskbar state: {e}");
        }
        let _ = std::fs::remove_file(&file);
    }
}

impl WindowAttention for KdeAttention {
    /// `quiet`: off the taskbar, and any attention it has asked for dropped.
    fn quiet(&self, window_class: &str) {
        self.apply(window_class, true, false);
    }

    /// `ask_for_input`: back on the taskbar and flagged, so the panel shows it.
    fn ask_for_input(&self, window_class: &str) {
        self.apply(window_class, false, true);
    }
}

fn is_exec(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// `subprocess.run(command, capture_output=True, timeout=QDBUS_TIMEOUT, check=False)`: a
/// non-zero exit is not an error; a command that cannot start, or outlives the timeout (it is
/// killed), is.
fn run(program: &Path, args: &[OsString]) -> std::io::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if started.elapsed() > QDBUS_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "qdbus did not answer",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A `qdbus6` that appends its arguments, and the script file's text at `loadScript`, to a log.
    fn fake_qdbus(dir: &Path, log: &Path) -> PathBuf {
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let p = bin.join("qdbus6");
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\necho \"$@\" >> '{log}'\ncase \"$3\" in *loadScript) cat \"$4\" >> '{log}';; esac\n",
                log = log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    #[test]
    fn the_script_is_pythons_with_the_class_repr_quoted() {
        assert_eq!(
            script_for("lens_browser", true, false),
            "const ws = workspace.windowList ? workspace.windowList() : workspace.clientList();\n\
             for (const w of ws) {\n  if (w.resourceClass !== 'lens_browser') continue;\n  \
             w.skipTaskbar = true;\n  w.demandsAttention = false;\n}\n"
        );
        // Python's repr picks the quote that needs no escape.
        assert!(script_for("it's", false, true).contains("!== \"it's\""));
    }

    #[test]
    fn off_plasma_or_without_qdbus_every_call_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        let bin = fake_qdbus(dir.path(), &log);
        // Not KDE.
        KdeAttention::new("GNOME".into(), vec![bin.clone()]).quiet("lens_browser");
        // KDE, no qdbus on the path.
        KdeAttention::new("KDE".into(), vec![dir.path().join("nowhere")]).ask_for_input("x");
        assert!(!log.exists());
        assert!(!KdeAttention::new("GNOME".into(), vec![bin.clone()]).available());
        assert!(KdeAttention::new("KDE".into(), vec![bin]).available());
    }

    #[test]
    fn quiet_and_ask_load_start_and_unload_the_kwin_script_in_python_order() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("calls.log");
        let bin = fake_qdbus(dir.path(), &log);
        let kde = KdeAttention::new("KDE:plasma".into(), vec![bin]);
        kde.quiet("lens_browser");
        let said = std::fs::read_to_string(&log).unwrap();
        let calls: Vec<&str> = said
            .lines()
            .filter(|l| l.starts_with("org.kde.KWin"))
            .collect();
        assert_eq!(calls.len(), 4);
        assert_eq!(
            calls[0],
            "org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript hatate-lens-attention"
        );
        assert!(calls[1].starts_with("org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "));
        assert!(calls[1].ends_with(" hatate-lens-attention"));
        assert_eq!(
            calls[2],
            "org.kde.KWin /Scripting org.kde.kwin.Scripting.start"
        );
        assert_eq!(calls[3], calls[0]);
        assert!(said.contains("w.skipTaskbar = true;"));
        assert!(said.contains("w.demandsAttention = false;"));
        std::fs::remove_file(&log).unwrap();
        kde.ask_for_input("lens_browser");
        let said = std::fs::read_to_string(&log).unwrap();
        assert!(said.contains("w.skipTaskbar = false;"));
        assert!(said.contains("w.demandsAttention = true;"));
    }
}
