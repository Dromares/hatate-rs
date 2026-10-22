//! Command line of the spike binary. Hand-parsed: the spike needs no CLI crate.

use crate::review::Mode;
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
  --tab-order            print the Tab focus chain, then quit";

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
    fn rejects_unknown_arguments_and_modes() {
        assert!(p("--bogus").is_err());
        assert!(p("--mode sideways").is_err());
        assert!(p("--rows").is_err());
    }
}
