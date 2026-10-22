//! xbooru.com (`core/boorus/xbooru.py`).
//!
//! Gelbooru-family software, so the tag-list markup is read by [`gelbooru`]. Its own module, not
//! another host in Gelbooru's list, because Gelbooru's Data API now 401s without credentials and
//! remembers that for the rest of the session: sharing that flag would skip this host's
//! still-public API after the first Gelbooru 401. That latch and the per-module memo live in the
//! fetch layer; the hooks here are given this host's own `api_post` in the [`Context`] and
//! otherwise read what Gelbooru's page-only readers read.

use tracing::debug;

use super::host::on_host;
use super::{Context, SiteParser, gelbooru};
use crate::model::Tag;

const HOST: &str = "xbooru.com";

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(parse_file_url),
    parse_preview_url: Some(parse_preview_url),
    parse_dimensions: Some(parse_dimensions),
    parse_rating: Some(|body, url, _| parse_rating(body, url)),
    ..SiteParser::new("xbooru", matches, |body, url, _| parse(body, url))
};

pub fn matches(url: &str) -> bool {
    on_host(url, &[HOST])
}

pub fn parse(html: &str, url: &str) -> Vec<Tag> {
    let tags = gelbooru::parse(html, url);
    if tags.is_empty() {
        debug!("No tags extracted from {url} (markup may differ from Gelbooru's)");
    }
    tags
}

/// The Statistics sidebar's "Rating:" line: identical markup to Gelbooru's, and read from the
/// page so it costs no extra request.
pub fn parse_rating(html: &str, url: &str) -> Option<String> {
    gelbooru::parse_rating(html, url)
}

pub fn parse_file_url(html: &str, url: &str, ctx: &Context) -> Option<String> {
    gelbooru::parse_file_url(html, url, ctx)
}

pub fn parse_preview_url(html: &str, url: &str, ctx: &Context) -> Option<String> {
    gelbooru::parse_preview_url(html, url, ctx)
}

pub fn parse_dimensions(html: &str, url: &str, ctx: &Context) -> (Option<i64>, Option<i64>) {
    gelbooru::parse_dimensions(html, url, ctx)
}
