//! Real key events for the smoke, from Rust and with no new dependency: a minimal X11 client that
//! speaks the wire protocol over the display's Unix socket and injects keys through the XTEST
//! extension (`FakeInput`). The X server then delivers them to GTK exactly as it would a keyboard,
//! so a scenario exercises the whole path: X server, GDK, the window's key controllers.
//!
//! Why not an in-process event: GTK 4 has no public constructor for a `GdkEvent`, and emitting a
//! controller's `key-pressed` signal by hand skips the thing under test (whether GTK routes F10 at
//! all). Why not `xdotool`: it did not reach GTK here (DAN-2242), and it would be a tool the CI
//! image does not have.
//!
//! It only ever talks to `$DISPLAY`, and `Xtest::connect` refuses (before it opens the socket)
//! unless that display is demonstrably the headless server the runner started: see
//! `check_headless`. CLAUDE.md: never inject input into the board's live session.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

const KEYSYM_ALT_L: u32 = 0xffe9;
const KEYSYM_SHIFT_L: u32 = 0xffe1;
const KEYSYM_CONTROL_L: u32 = 0xffe3;
const KEYSYM_F10: u32 = 0xffc7;
const KEYSYM_ESCAPE: u32 = 0xff1b;
const KEYSYM_LEFT: u32 = 0xff51;
const KEYSYM_RIGHT: u32 = 0xff53;
const KEYSYM_TAB: u32 = 0xff09;
const KEYSYM_DOWN: u32 = 0xff54;
const KEYSYM_PAGE_DOWN: u32 = 0xff56;
const KEYSYM_END: u32 = 0xff57;
const KEYSYM_HOME: u32 = 0xff50;

pub struct Xtest {
    sock: UnixStream,
    root: u32,
    xtest_opcode: u8,
    first_keycode: u8,
    per_keycode: usize,
    keysyms: Vec<u32>,
}

/// A modifier held while a key is tapped.
#[derive(Clone, Copy, Debug)]
pub enum Mod {
    Alt,
    Shift,
    Ctrl,
}

/// A key, by what the keyboard map is asked for.
#[derive(Clone, Copy, Debug)]
pub enum Key {
    F10,
    Escape,
    Left,
    Right,
    Tab,
    Down,
    PageDown,
    End,
    Home,
    /// A letter or digit, by its ASCII (X keysyms for Latin-1 are the code points).
    Char(char),
}

impl Key {
    fn keysym(self) -> u32 {
        match self {
            Key::F10 => KEYSYM_F10,
            Key::Escape => KEYSYM_ESCAPE,
            Key::Left => KEYSYM_LEFT,
            Key::Right => KEYSYM_RIGHT,
            Key::Tab => KEYSYM_TAB,
            Key::Down => KEYSYM_DOWN,
            Key::PageDown => KEYSYM_PAGE_DOWN,
            Key::End => KEYSYM_END,
            Key::Home => KEYSYM_HOME,
            Key::Char(c) => c as u32,
        }
    }
}

fn pad4(n: usize) -> usize {
    (4 - n % 4) % 4
}

/// The MIT-MAGIC-COOKIE-1 for display `num` from `$XAUTHORITY` (what `xvfb-run` sets), if any.
fn cookie(num: &str) -> Option<Vec<u8>> {
    let path = std::env::var_os("XAUTHORITY")?;
    let data = std::fs::read(path).ok()?;
    let mut i = 0;
    let take_u16 = |i: &mut usize| -> Option<usize> {
        let v = u16::from_be_bytes([*data.get(*i)?, *data.get(*i + 1)?]) as usize;
        *i += 2;
        Some(v)
    };
    while i < data.len() {
        let _family = take_u16(&mut i)?;
        let n = take_u16(&mut i)?;
        i += n; // address
        let n = take_u16(&mut i)?;
        let number = data.get(i..i + n)?.to_vec();
        i += n;
        let n = take_u16(&mut i)?;
        let name = data.get(i..i + n)?.to_vec();
        i += n;
        let n = take_u16(&mut i)?;
        let value = data.get(i..i + n)?.to_vec();
        i += n;
        if name == b"MIT-MAGIC-COOKIE-1" && (number.is_empty() || number == num.as_bytes()) {
            return Some(value);
        }
    }
    None
}

/// The runner states the display is its own headless server with this variable (set by the CI
/// step and by the documented `xvfb-run` invocation, `docs/gtk-shell.md`).
pub const HEADLESS_FLAG: &str = "HATATE_SMOKE_XVFB";

/// Why key injection is refused, or `Ok` when the display is demonstrably the headless server the
/// runner started. Two independent conditions, both required, because neither alone is enough:
///
/// 1. `HATATE_SMOKE_XVFB=1`: an explicit statement by the runner. Alone it is only a promise, so
/// 2. `XAUTHORITY` names a file `xvfb-run` created (`<tmp>/xvfb-run.XXXXXX/Xauthority`, with a random suffix on some distributions).
///    A live desktop's cookie is `~/.Xauthority`, `$XDG_RUNTIME_DIR/xauth_*` or the display
///    manager's; none of those sits in an `xvfb-run.*` directory, so `HATATE_SMOKE_XVFB=1
///    DISPLAY=:0` is still refused. The X vendor string cannot do this job: Xvfb and a real Xorg
///    both answer "The X.Org Foundation".
///
/// A hand-started `Xvfb` (no `xvfb-run`) or `xvfb-run -f` is refused too; the documented invocation
/// is the only supported one. Pure, so it is unit-tested without an X server (`xtest_guard.rs`).
pub fn check_headless(
    display: Option<&str>,
    flag: Option<&str>,
    xauthority: Option<&std::ffi::OsStr>,
) -> Result<(), String> {
    let display = display.ok_or_else(|| "DISPLAY is not set".to_owned())?;
    if flag != Some("1") {
        return Err(format!(
            "refusing to inject keys into DISPLAY={display}: {HEADLESS_FLAG}=1 is not set (it \
             states that the display is a headless Xvfb the runner started; run under xvfb-run)"
        ));
    }
    let auth = std::path::Path::new(xauthority.ok_or_else(|| {
        format!("refusing to inject keys into DISPLAY={display}: XAUTHORITY is not set, so it is not an xvfb-run display")
    })?);
    let file = auth.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let dir = auth
        .parent()
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    if !(file.starts_with("Xauthority") && dir.starts_with("xvfb-run.")) {
        return Err(format!(
            "refusing to inject keys into DISPLAY={display}: XAUTHORITY={} is not a file xvfb-run \
             created, so the display may be a live session",
            auth.display()
        ));
    }
    Ok(())
}

impl Xtest {
    /// Connects to `$DISPLAY` (`:N` or `:N.S`, the local Unix socket only) and finds XTEST. Refuses
    /// first, with no socket opened, unless `check_headless` passes.
    pub fn connect() -> Result<Xtest, String> {
        check_headless(
            std::env::var("DISPLAY").ok().as_deref(),
            std::env::var(HEADLESS_FLAG).ok().as_deref(),
            std::env::var_os("XAUTHORITY").as_deref(),
        )?;
        let display = std::env::var("DISPLAY").map_err(|_| "DISPLAY is not set".to_owned())?;
        let num = display
            .strip_prefix(':')
            .ok_or_else(|| format!("DISPLAY {display:?} is not a local display"))?
            .split('.')
            .next()
            .unwrap_or("0")
            .to_owned();
        let mut sock = UnixStream::connect(format!("/tmp/.X11-unix/X{num}"))
            .map_err(|e| format!("connect to the X socket: {e}"))?;

        let auth = cookie(&num).unwrap_or_default();
        let name: &[u8] = if auth.is_empty() {
            b""
        } else {
            b"MIT-MAGIC-COOKIE-1"
        };
        let mut req = vec![b'l', 0, 11, 0, 0, 0];
        req.extend((name.len() as u16).to_le_bytes());
        req.extend((auth.len() as u16).to_le_bytes());
        req.extend([0, 0]);
        req.extend(name);
        req.extend(std::iter::repeat_n(0, pad4(name.len())));
        req.extend(&auth);
        req.extend(std::iter::repeat_n(0, pad4(auth.len())));
        sock.write_all(&req).map_err(|e| e.to_string())?;

        let mut head = [0u8; 8];
        sock.read_exact(&mut head).map_err(|e| e.to_string())?;
        let more = u16::from_le_bytes([head[6], head[7]]) as usize * 4;
        let mut body = vec![0u8; more];
        sock.read_exact(&mut body).map_err(|e| e.to_string())?;
        if head[0] != 1 {
            return Err(format!(
                "the X server refused the connection: {}",
                String::from_utf8_lossy(&body[..body.len().min(60)])
            ));
        }
        let u32_at = |o: usize| u32::from_le_bytes(body[o..o + 4].try_into().unwrap());
        let vendor_len = u16::from_le_bytes([body[16], body[17]]) as usize;
        let formats = body[21] as usize;
        let first_keycode = body[26];
        let screens_at = 32 + vendor_len + pad4(vendor_len) + formats * 8;
        let root = u32_at(screens_at);

        let mut x = Xtest {
            sock,
            root,
            xtest_opcode: 0,
            first_keycode,
            per_keycode: 0,
            keysyms: Vec::new(),
        };
        x.xtest_opcode = x.query_extension("XTEST")?;
        x.load_keymap(first_keycode, body[27])?;
        Ok(x)
    }

    fn reply(&mut self) -> Result<Vec<u8>, String> {
        loop {
            let mut head = [0u8; 32];
            self.sock.read_exact(&mut head).map_err(|e| e.to_string())?;
            match head[0] {
                0 => return Err(format!("X error code {} on request {}", head[1], head[10])),
                1 => {
                    let extra = u32::from_le_bytes(head[4..8].try_into().unwrap()) as usize * 4;
                    let mut out = head.to_vec();
                    out.resize(32 + extra, 0);
                    self.sock
                        .read_exact(&mut out[32..])
                        .map_err(|e| e.to_string())?;
                    return Ok(out);
                }
                _ => {} // an event: not ours
            }
        }
    }

    fn query_extension(&mut self, name: &str) -> Result<u8, String> {
        let n = name.len();
        let mut req = vec![98, 0];
        req.extend(((8 + n + pad4(n)) as u16 / 4).to_le_bytes());
        req.extend((n as u16).to_le_bytes());
        req.extend([0, 0]);
        req.extend(name.as_bytes());
        req.extend(std::iter::repeat_n(0, pad4(n)));
        self.sock.write_all(&req).map_err(|e| e.to_string())?;
        let r = self.reply()?;
        if r[8] == 0 {
            return Err(format!("the X server has no {name} extension"));
        }
        Ok(r[9])
    }

    fn load_keymap(&mut self, first: u8, last: u8) -> Result<(), String> {
        let count = last - first + 1;
        let req = [101, 0, 2, 0, first, count, 0, 0];
        self.sock.write_all(&req).map_err(|e| e.to_string())?;
        let r = self.reply()?;
        self.per_keycode = r[1] as usize;
        self.keysyms = r[32..]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        Ok(())
    }

    fn keycode(&self, keysym: u32) -> Result<u8, String> {
        // Column 0 is the unshifted keysym, which is what a plain letter or F-key sits on.
        (0..self.keysyms.len() / self.per_keycode.max(1))
            .find(|k| self.keysyms[k * self.per_keycode] == keysym)
            .map(|k| self.first_keycode + k as u8)
            .ok_or_else(|| format!("no keycode for keysym {keysym:#x}"))
    }

    fn fake(&mut self, kind: u8, detail: u8, x: i16, y: i16) -> Result<(), String> {
        let mut req = vec![self.xtest_opcode, 2, 9, 0, kind, detail, 0, 0];
        req.extend(0u32.to_le_bytes()); // time: now
        req.extend(self.root.to_le_bytes());
        req.extend([0u8; 8]);
        req.extend(x.to_le_bytes());
        req.extend(y.to_le_bytes());
        req.extend([0u8; 8]);
        self.sock.write_all(&req).map_err(|e| e.to_string())
    }

    /// Puts the pointer at `(x, y)` on the root, then gives the keyboard focus to the top-level
    /// window under it. There is no window manager under `xvfb-run`, so nothing else would: without
    /// both, GTK sees no focus and a key goes nowhere.
    pub fn focus_window_at(&mut self, x: i16, y: i16) -> Result<(), String> {
        self.fake(6, 0, x, y)?;
        self.sync()?;
        let q = [38, 0, 2, 0];
        let mut req = q.to_vec();
        req.extend(self.root.to_le_bytes());
        self.sock.write_all(&req).map_err(|e| e.to_string())?;
        let r = self.reply()?;
        let child = u32::from_le_bytes(r[12..16].try_into().unwrap());
        if child == 0 {
            return Err(format!("no window under the pointer at {x},{y}"));
        }
        // SetInputFocus(focus = child, revert-to = Parent, time = CurrentTime)
        let mut req = vec![42, 2, 3, 0];
        req.extend(child.to_le_bytes());
        req.extend(0u32.to_le_bytes());
        self.sock.write_all(&req).map_err(|e| e.to_string())?;
        self.sync()
    }

    /// A round trip (`GetInputFocus`): returns once the server has handled every request before
    /// it, and surfaces any X error those raised.
    fn sync(&mut self) -> Result<(), String> {
        self.sock
            .write_all(&[43, 0, 1, 0])
            .map_err(|e| e.to_string())?;
        self.reply().map(|_| ())
    }

    /// Presses and releases `key` with `mods` held.
    pub fn tap(&mut self, key: Key, mods: &[Mod]) -> Result<(), String> {
        let code = self.keycode(key.keysym())?;
        let held = mods
            .iter()
            .map(|m| {
                self.keycode(match m {
                    Mod::Alt => KEYSYM_ALT_L,
                    Mod::Shift => KEYSYM_SHIFT_L,
                    Mod::Ctrl => KEYSYM_CONTROL_L,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        for m in &held {
            self.fake(2, *m, 0, 0)?;
        }
        self.fake(2, code, 0, 0)?;
        self.fake(3, code, 0, 0)?;
        for m in held.iter().rev() {
            self.fake(3, *m, 0, 0)?;
        }
        self.sync()
    }
}
