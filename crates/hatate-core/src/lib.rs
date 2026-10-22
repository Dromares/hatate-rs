//! Shared library for the Hatate Rust rewrite. Work in progress.

pub mod availability;
pub mod boorus;
pub mod cache;
pub mod clock;
pub mod config;
pub mod crash_marker;
pub mod engine_ids;
pub mod engines;
mod fsutil;
pub mod hard_timeout;
pub mod hydrus_url;
pub mod image_compare;
pub mod image_prep;
pub mod lru;
pub mod model;
pub mod multipage;
pub mod multipage_resolve;
pub mod net;
pub mod parser_health;
pub mod paths;
pub mod progress;
mod pyalpha;
mod pycase;
mod pycompat;
mod pycsv;
mod pyimage;
pub mod pyjpeg;
pub mod pyraster;
mod pyrepr;
pub mod pyresponse;
pub mod pyurl;
pub mod quota_pause;
pub mod reddit;
pub mod remote;
pub mod rotating_log;
pub mod session;
pub mod similarity_check;
pub mod sites;
pub mod timeutil;

/// Crate name, used by the placeholder test below.
pub const NAME: &str = "hatate-core";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_set() {
        assert_eq!(NAME, "hatate-core");
    }
}
