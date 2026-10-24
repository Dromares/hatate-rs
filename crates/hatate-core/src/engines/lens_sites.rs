//! The two lookups Lens's recovery makes over the network (`boorus.rule34.find_post_by_search` and
//! `reddit.find_post`), behind [`LensSites`]: [`OnlineSites`] is the shipped implementation, over
//! the net layer.
//!
//! `search` runs the ranking on a blocking thread, so the lookups are synchronous calls that wait
//! on the runtime they were started from, as Python's are blocking calls from a worker thread.

use tokio::runtime::Handle;

use super::lens_rank::{LensSites, Rule34Hit, Unexpected};
use crate::boorus::rule34;
use crate::image_compare::Hash;
use crate::net::NetClient;
use crate::reddit::{self, FeedCache};

/// The default `timeout=15.0` of both Python functions.
pub const LOOKUP_TIMEOUT_SECONDS: f64 = 15.0;

/// The shipped lookups: rule34.xxx's listing page and Reddit's feeds on `net`. The Reddit feeds
/// read this session are kept (`_feed_cache`), so a second image from the same subreddit costs
/// no request.
pub struct OnlineSites {
    net: NetClient,
    feeds: FeedCache,
}

impl OnlineSites {
    pub fn new(net: NetClient) -> Self {
        OnlineSites {
            net,
            feeds: FeedCache::new(),
        }
    }
}

impl LensSites for OnlineSites {
    fn find_rule34_post(
        &self,
        tags: &str,
        local_hash: Option<&Hash>,
    ) -> Result<Option<Rule34Hit>, Unexpected> {
        let found = Handle::current().block_on(rule34::find_post_by_search(
            &self.net,
            tags,
            local_hash,
            LOOKUP_TIMEOUT_SECONDS,
        ));
        found
            .map(|hit| hit.map(|(url, thumb, distance)| (url, Some(thumb), distance)))
            .map_err(|bomb| Unexpected(bomb.to_string()))
    }

    fn find_reddit_post(
        &self,
        subreddit: &str,
        media_id: &str,
    ) -> Result<Option<String>, Unexpected> {
        Handle::current()
            .block_on(reddit::find_post(
                &self.net,
                &self.feeds,
                subreddit,
                media_id,
                LOOKUP_TIMEOUT_SECONDS,
            ))
            .map_err(|raised| Unexpected(raised.to_string()))
    }
}
