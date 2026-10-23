//! Shared library for the Hatate Rust rewrite. Work in progress.

pub mod availability;
pub mod batch;
pub mod boorus;
pub mod cache;
pub mod clock;
pub mod config;
pub mod crash_marker;
pub mod engine_backends;
pub mod engine_ids;
pub mod engine_runner;
pub mod engines;
pub mod error;
mod fsutil;
pub mod hard_timeout;
pub mod hydrus_client;
pub mod hydrus_tag_lookup;
pub mod hydrus_url;
pub mod image_compare;
pub mod image_diff;
pub mod image_prep;
pub mod localtime;
pub mod lru;
pub mod model;
pub mod multipage;
pub mod multipage_resolve;
pub mod net;
pub mod parser_health;
pub mod paths;
pub mod pawchive_index;
pub mod progress;
mod pyalpha;
mod pycase;
mod pycompat;
mod pycsv;
mod pyfnmatch;
mod pyimage;
pub mod pyjpeg;
mod pymeta;
mod pynumpy;
pub mod pyraster;
mod pyrepr;
pub mod pyresponse;
pub mod pyurl;
pub mod quota_pause;
pub mod ranking;
pub mod reddit;
pub mod remote;
pub mod rotating_log;
pub mod search_engine;
pub mod session;
pub mod similarity_check;
pub mod sites;
pub mod tag_band_metrics;
pub mod tag_borrowing;
pub mod tag_rules;
pub mod timeutil;
pub mod upscale_detect;

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
