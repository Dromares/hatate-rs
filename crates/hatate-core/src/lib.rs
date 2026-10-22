//! Shared library for the Hatate Rust rewrite. Work in progress.

pub mod boorus;
pub mod cache;
pub mod config;
pub mod crash_marker;
pub mod engine_ids;
pub mod engines;
mod fsutil;
pub mod hard_timeout;
pub mod image_prep;
pub mod model;
pub mod net;
pub mod paths;
mod pyalpha;
mod pycase;
mod pycompat;
mod pycsv;
mod pyimage;
mod pyrepr;
pub mod pyurl;
pub mod quota_pause;
pub mod reddit;
pub mod rotating_log;
pub mod session;
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
