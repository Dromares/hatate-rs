//! Shared library for the Hatate Rust rewrite. Work in progress.

pub mod applog;
pub mod availability;
pub mod batch;
pub mod boorus;
pub mod cache;
pub mod clock;
pub mod config;
pub mod crash_marker;
pub mod engine_alerts;
pub mod engine_backends;
pub mod engine_ids;
pub mod engine_runner;
pub mod engines;
pub mod entry_filter;
pub mod error;
pub mod eta;
pub mod export;
mod fsutil;
pub mod hard_timeout;
pub mod hydrus_client;
pub mod hydrus_import;
pub mod hydrus_import_poll;
pub mod hydrus_reconcile;
pub mod hydrus_tag_lookup;
pub mod hydrus_url;
pub mod image_compare;
pub mod image_diff;
pub mod image_prep;
pub mod lens_browser;
pub mod localtime;
pub mod lru;
pub mod mcp;
pub mod mcp_images;
pub mod missing_files;
pub mod model;
pub mod multipage;
pub mod multipage_resolve;
pub mod net;
pub mod parser_health;
pub mod paths;
pub mod pawchive_index;
pub mod preview_text;
pub mod progress;
pub mod provenance_note;
mod pyalpha;
mod pycase;
mod pycompat;
mod pycsv;
mod pyfnmatch;
mod pyimage;
mod pyinflate;
pub mod pyjpeg;
mod pymeta;
mod pynumpy;
mod pypng;
pub mod pyraster;
mod pyrepr;
pub mod pyresponse;
pub mod pyurl;
pub mod queue;
pub mod quota_pause;
pub mod ranking;
pub mod reddit;
pub mod remote;
pub mod rotating_log;
pub mod run_state;
pub mod search_engine;
pub mod session;
pub mod settings_search;
pub mod shortcuts;
pub mod sidecar;
pub mod similarity_check;
pub mod similarity_display;
pub mod sites;
pub mod status_tier;
pub mod stop_record;
pub mod tag_band_metrics;
pub mod tag_borrowing;
pub mod tag_colors;
pub mod tag_rules;
pub mod thumb;
pub mod timeutil;
pub mod upscale_detect;
pub mod viewport;

#[cfg(test)]
mod error_shape_tests;

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
