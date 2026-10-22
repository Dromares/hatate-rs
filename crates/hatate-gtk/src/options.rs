//! Command line of the spike binary. Hand-parsed: the spike needs no CLI crate.

use crate::fonts::Route;
use crate::review::Mode;
use crate::theme::Scheme;
use std::path::PathBuf;

pub const USAGE: &str = "hatate-gtk (spike, work in progress)
  --tab queue|review     page to show (default queue)
  --rows N               queue rows (default 5000; 0 = empty queue)
  --left PATH --right PATH   images for Review (default: generated artwork)
  --mode side|wipe|diff  Review layout (default side)
  --size WxH|min         window size (default 1100x650; min = GTK's minimum)
  --select A..B          select queue rows A..=B after load
  --sort COLUMN[:desc]   sort by FILE, STATUS or MATCHES after load
  --shot PATH            write a PNG of the window once settled, then quit
  --measure              print MEASURE lines (first frame, fling, RSS), then quit
  --tab-order            print the Tab focus chain, then quit
  --scheme dark|light    colour scheme (default dark)
  --theme on|off         off = no Ryoku stylesheet, libadwaita as shipped (control run)
  --fonts DIR            load Space Grotesk, JetBrains Mono, Source Serif 4 and Ryoku Status
                         from DIR at run time (nothing is bundled in the tree)
  --font-route R         fc-add-dir (default) | conf-file | xdg-data-home | none
  --thumbs DIR           real thumbnails: write one JPEG per row under DIR (reused if present)
                         and decode them through the texture cache
  --thumb-cache N        texture cache size in thumbnails (default 1024)
  --thumb-workers N      decode threads (default 4)
  --open WHAT            open dropdown | menubar | dialog before the shot
  --inactive             draw the window in its inactive (backdrop) state
  --focus WHAT           focus queue | tab | canvas | slider | mode | dropdown before the shot
  --select-then-focus    with --focus queue: select row 0 first (focus ring on a selected row)
  --audit                focus-ring, popover and dialog pixel audits (AUDIT lines), then quit
  --listen SECS          run SECS seconds logging ACTION/FOCUS lines (for an external keyboard)";

#[derive(Debug, Clone)]
pub struct Options {
    pub tab: String,
    pub rows: u32,
    pub left: Option<PathBuf>,
    pub right: Option<PathBuf>,
    pub mode: Mode,
    pub size: Option<(i32, i32)>,
    pub select: Option<(u32, u32)>,
    pub sort: Option<(String, bool)>,
    pub shot: Option<PathBuf>,
    pub measure: bool,
    pub tab_order: bool,
    pub scheme: Scheme,
    pub theme: bool,
    pub fonts: Option<PathBuf>,
    pub font_route: Route,
    pub thumbs: Option<PathBuf>,
    pub thumb_cache: usize,
    pub thumb_workers: usize,
    pub open: Option<Open>,
    pub inactive: bool,
    pub focus: Option<String>,
    pub select_then_focus: bool,
    pub audit: bool,
    pub listen: Option<u64>,
}

/// Something to open before the shot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Open {
    Dropdown,
    Menubar,
    Dialog,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            tab: "queue".into(),
            rows: 5000,
            left: None,
            right: None,
            mode: Mode::SideBySide,
            size: Some((1100, 650)),
            select: None,
            sort: None,
            shot: None,
            measure: false,
            tab_order: false,
            scheme: Scheme::Dark,
            theme: true,
            fonts: None,
            font_route: Route::FcAddDir,
            thumbs: None,
            thumb_cache: 1024,
            thumb_workers: 4,
            open: None,
            inactive: false,
            focus: None,
            select_then_focus: false,
            audit: false,
            listen: None,
        }
    }
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut o = Options::default();
    let mut it = args.into_iter();
    let val = |name: &str, it: &mut dyn Iterator<Item = String>| {
        it.next().ok_or_else(|| format!("{name} needs a value"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--tab" => o.tab = val(&a, &mut it)?,
            "--rows" => {
                o.rows = val(&a, &mut it)?
                    .parse()
                    .map_err(|e| format!("--rows: {e}"))?
            }
            "--left" => o.left = Some(val(&a, &mut it)?.into()),
            "--right" => o.right = Some(val(&a, &mut it)?.into()),
            "--mode" => {
                let v = val(&a, &mut it)?;
                o.mode = Mode::parse(&v).ok_or(format!("--mode: unknown mode {v}"))?;
            }
            "--size" => {
                let v = val(&a, &mut it)?;
                o.size = if v == "min" {
                    None
                } else {
                    let (w, h) = v.split_once('x').ok_or("--size wants WxH or min")?;
                    Some((
                        w.parse().map_err(|e| format!("--size: {e}"))?,
                        h.parse().map_err(|e| format!("--size: {e}"))?,
                    ))
                };
            }
            "--select" => {
                let v = val(&a, &mut it)?;
                let (x, y) = v.split_once("..").ok_or("--select wants A..B")?;
                o.select = Some((
                    x.parse().map_err(|e| format!("--select: {e}"))?,
                    y.parse().map_err(|e| format!("--select: {e}"))?,
                ));
            }
            "--sort" => {
                let v = val(&a, &mut it)?;
                o.sort = Some(match v.strip_suffix(":desc") {
                    Some(c) => (c.to_string(), true),
                    None => (v, false),
                });
            }
            "--shot" => o.shot = Some(val(&a, &mut it)?.into()),
            "--measure" => o.measure = true,
            "--tab-order" => o.tab_order = true,
            "--scheme" => {
                let v = val(&a, &mut it)?;
                o.scheme = Scheme::parse(&v).ok_or(format!("--scheme: unknown scheme {v}"))?;
            }
            "--theme" => o.theme = val(&a, &mut it)? != "off",
            "--fonts" => o.fonts = Some(val(&a, &mut it)?.into()),
            "--font-route" => {
                let v = val(&a, &mut it)?;
                o.font_route =
                    Route::parse(&v).ok_or(format!("--font-route: unknown route {v}"))?;
            }
            "--thumbs" => o.thumbs = Some(val(&a, &mut it)?.into()),
            "--thumb-cache" => {
                o.thumb_cache = val(&a, &mut it)?
                    .parse()
                    .map_err(|e| format!("--thumb-cache: {e}"))?
            }
            "--thumb-workers" => {
                o.thumb_workers = val(&a, &mut it)?
                    .parse()
                    .map_err(|e| format!("--thumb-workers: {e}"))?
            }
            "--open" => {
                o.open = Some(match val(&a, &mut it)?.as_str() {
                    "dropdown" => Open::Dropdown,
                    "menubar" => Open::Menubar,
                    "dialog" => Open::Dialog,
                    v => return Err(format!("--open: unknown surface {v}")),
                })
            }
            "--inactive" => o.inactive = true,
            "--focus" => o.focus = Some(val(&a, &mut it)?),
            "--select-then-focus" => o.select_then_focus = true,
            "--audit" => o.audit = true,
            "--listen" => {
                o.listen = Some(
                    val(&a, &mut it)?
                        .parse()
                        .map_err(|e| format!("--listen: {e}"))?,
                )
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<Options, String> {
        parse(s.split_whitespace().map(String::from))
    }

    #[test]
    fn defaults_are_the_5000_row_queue() {
        let o = p("").unwrap();
        assert_eq!((o.tab.as_str(), o.rows), ("queue", 5000));
    }

    #[test]
    fn parses_the_scenarios() {
        let o = p("--tab review --mode wipe --size min --select 3..8 --sort STATUS:desc --rows 0")
            .unwrap();
        assert_eq!(o.mode, Mode::Wipe);
        assert_eq!(o.size, None);
        assert_eq!(o.select, Some((3, 8)));
        assert_eq!(o.sort, Some(("STATUS".into(), true)));
        assert_eq!(o.rows, 0);
    }

    #[test]
    fn parses_the_flip_spike_flags() {
        let o = p(
            "--scheme light --fonts /f --font-route conf-file --thumbs /t --thumb-cache 64 \
                   --open menubar --inactive --focus queue --audit",
        )
        .unwrap();
        assert_eq!(o.scheme, Scheme::Light);
        assert_eq!(o.font_route, Route::ConfFile);
        assert_eq!(o.thumb_cache, 64);
        assert_eq!(o.open, Some(Open::Menubar));
        assert!(o.inactive && o.audit && o.focus.as_deref() == Some("queue"));
        assert!(p("--font-route nope").is_err() && p("--open sideways").is_err());
    }

    #[test]
    fn rejects_unknown_arguments_and_modes() {
        assert!(p("--bogus").is_err());
        assert!(p("--mode sideways").is_err());
        assert!(p("--rows").is_err());
    }
}
