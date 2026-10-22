//! Safebooru (`core/boorus/safebooru.py`).
//!
//! Safebooru runs on the same software lineage as Gelbooru, so its pages follow the same layout
//! and this module reuses [`gelbooru`]'s readers. It stays a module of its own, not another host
//! in Gelbooru's list, so it can diverge if Safebooru's markup ever does.
//!
//! The one place it differs from rule34 and Xbooru is `parse_file_url`: it delegates to
//! Gelbooru's API-first hook, so it reads the same `api_post` Gelbooru does. It has no Data-API
//! path of its own.

use tracing::debug;

use super::host::on_host;
use super::{Context, SiteParser, gelbooru};
use crate::model::Tag;

const HOST: &str = "safebooru.org";

/// What the registry knows about this site: one entry per hook the module defines.
pub static PARSER: SiteParser = SiteParser {
    parse_file_url: Some(parse_file_url),
    parse_preview_url: Some(|body, url, _| parse_preview_url(body, url)),
    parse_dimensions: Some(|body, url, _| parse_dimensions(body, url)),
    parse_rating: Some(|body, url, _| parse_rating(body, url)),
    ..SiteParser::new("safebooru", matches, |body, url, _| parse(body, url))
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

/// The Statistics sidebar's "Rating:" line, the same shared Danbooru 1.x markup the size scrape
/// already reads.
pub fn parse_rating(html: &str, url: &str) -> Option<String> {
    gelbooru::parse_rating(html, url)
}

pub fn parse_file_url(html: &str, url: &str, ctx: &Context) -> Option<String> {
    gelbooru::parse_file_url(html, url, ctx)
}

/// The ORIGINAL's size, off the Statistics sidebar's "Size: WxH".
///
/// The page-only Gelbooru helper, not the API-first one: that path shares Gelbooru's 401 latch,
/// and Safebooru's own API is still public.
pub fn parse_dimensions(html: &str, url: &str) -> (Option<i64>, Option<i64>) {
    gelbooru::parse_dimensions_from_html(html, url)
}

/// The post page's own `#image`: Safebooru's sample where it made one, and the original where it
/// did not. Read from the HTML so it costs no extra request.
pub fn parse_preview_url(html: &str, url: &str) -> Option<String> {
    gelbooru::parse_preview_url_from_html(html, url)
}
