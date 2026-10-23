//! The command line: `hatate [--help] | hatate batch [-v] <dir>`.

use std::path::PathBuf;

pub const USAGE: &str = "\
usage: hatate batch [-v|-vv] <dir>
       hatate --help

  batch <dir>   (experimental) add every image under <dir> to the saved session, search the
                ones not yet searched on IQDB, SauceNAO and the other engines the settings
                turn on, and save the session, which the Python app opens. Needs no display.
                Progress goes to stderr. Stop with Ctrl-C or SIGTERM: the session is saved
                first. Hydrus auto-import is not in this build.
  -v, -vv       log at info / debug level to stderr

exit status: 0 finished, 1 error, 2 usage, 3 paused (SauceNAO's daily allowance ran out),
             130 stopped by SIGINT, 143 stopped by SIGTERM";

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    /// No arguments.
    Banner,
    Help,
    Batch(BatchOptions),
}

#[derive(Debug, PartialEq, Eq)]
pub struct BatchOptions {
    pub dir: PathBuf,
    /// 0 = warnings only, 1 = info, 2 = debug.
    pub verbosity: u8,
}

pub fn parse(argv: &[String]) -> Result<Command, String> {
    let Some(first) = argv.first() else {
        return Ok(Command::Banner);
    };
    match first.as_str() {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "batch" => {
            let mut dir: Option<PathBuf> = None;
            let mut verbosity = 0u8;
            for arg in &argv[1..] {
                match arg.as_str() {
                    "-h" | "--help" => return Ok(Command::Help),
                    "-v" => verbosity = verbosity.max(1),
                    "-vv" => verbosity = 2,
                    other if other.starts_with('-') && other.len() > 1 => {
                        return Err(format!("batch: unknown option {other}"));
                    }
                    other => {
                        if dir.replace(PathBuf::from(other)).is_some() {
                            return Err("batch: only one folder at a time".to_owned());
                        }
                    }
                }
            }
            match dir {
                Some(dir) => Ok(Command::Batch(BatchOptions { dir, verbosity })),
                None => Err("batch: which folder?".to_owned()),
            }
        }
        other => Err(format!("unknown command {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(&args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn no_arguments_is_the_banner() {
        assert_eq!(p(&[]), Ok(Command::Banner));
    }

    #[test]
    fn help_in_any_spelling() {
        assert_eq!(p(&["--help"]), Ok(Command::Help));
        assert_eq!(p(&["-h"]), Ok(Command::Help));
        assert_eq!(p(&["batch", "--help"]), Ok(Command::Help));
    }

    #[test]
    fn batch_takes_one_folder_and_a_verbosity() {
        assert_eq!(
            p(&["batch", "-v", "/pics"]),
            Ok(Command::Batch(BatchOptions {
                dir: "/pics".into(),
                verbosity: 1
            }))
        );
        assert_eq!(
            p(&["batch", "/pics", "-vv"]),
            Ok(Command::Batch(BatchOptions {
                dir: "/pics".into(),
                verbosity: 2
            }))
        );
    }

    #[test]
    fn batch_without_a_folder_or_with_two_is_a_usage_error() {
        assert!(p(&["batch"]).is_err());
        assert!(p(&["batch", "a", "b"]).is_err());
        assert!(p(&["batch", "--session", "x"]).is_err());
        assert!(p(&["frobnicate"]).is_err());
    }
}
