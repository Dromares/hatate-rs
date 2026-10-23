//! The session's data: entries, candidates and tags (`docs/file-formats.md` 3.4, 3.4.1, 3.4.2).
//!
//! The field lists and their order are the stored shape: serialising an [`Entry`] gives the
//! 16 keys of 3.4, a [`Candidate`] the 20 keys of 3.4.1, a [`Tag`] the three keys of 3.4.2.
//! The derived `Deserialize` is the *strict* reading of that shape. Reading a `session.db`
//! or a session file does not use it: the Python app is lenient in specific ways (an unknown
//! status becomes `not_searched`, a missing candidate `url` drops the row, ...) and rule
//! 6.2 requires the same, so that lives in `session::codec`.

use serde::{Deserialize, Serialize};

use crate::config::Settings;

/// `MatchStatus` (`core/models.py:27-33`). Anything else in a file means `NotSearched`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchStatus {
    #[default]
    #[serde(rename = "not_searched")]
    NotSearched,
    #[serde(rename = "good")]
    Good,
    #[serde(rename = "poor")]
    Poor,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "error")]
    Error,
    /// A search was running. Sticky across a clean restart; reset by
    /// [`mark_interrupted_mid_search`] after an unclean one (spec 3.4, rule 6.2.6).
    #[serde(rename = "searching")]
    Searching,
}

impl MatchStatus {
    pub const ALL: [MatchStatus; 6] = [
        MatchStatus::NotSearched,
        MatchStatus::Good,
        MatchStatus::Poor,
        MatchStatus::NotFound,
        MatchStatus::Error,
        MatchStatus::Searching,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MatchStatus::NotSearched => "not_searched",
            MatchStatus::Good => "good",
            MatchStatus::Poor => "poor",
            MatchStatus::NotFound => "not_found",
            MatchStatus::Error => "error",
            MatchStatus::Searching => "searching",
        }
    }

    /// Exact match on one of the six values; `None` for anything else.
    pub fn parse(s: &str) -> Option<MatchStatus> {
        MatchStatus::ALL.into_iter().find(|m| m.as_str() == s)
    }
}

/// `TagSource` (`core/models.py:12-18`). Anything else in a file means `User`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TagSource {
    #[default]
    #[serde(rename = "User")]
    User,
    #[serde(rename = "Search engine")]
    SearchEngine,
    #[serde(rename = "Booru")]
    Booru,
    #[serde(rename = "Hatate-linux")]
    Hatate,
    #[serde(rename = "Hydrus")]
    Hydrus,
}

impl TagSource {
    pub const ALL: [TagSource; 5] = [
        TagSource::User,
        TagSource::SearchEngine,
        TagSource::Booru,
        TagSource::Hatate,
        TagSource::Hydrus,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TagSource::User => "User",
            TagSource::SearchEngine => "Search engine",
            TagSource::Booru => "Booru",
            TagSource::Hatate => "Hatate-linux",
            TagSource::Hydrus => "Hydrus",
        }
    }

    pub fn parse(s: &str) -> Option<TagSource> {
        TagSource::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

/// A tag on an entry (3.4.2): `name`, `namespace` and the source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub namespace: Option<String>,
    pub source: TagSource,
}

impl Tag {
    /// `(namespace, name)`, what makes two tags the same tag (`Tag.key`).
    fn key(&self) -> (&Option<String>, &str) {
        (&self.namespace, &self.name)
    }

    /// `namespace:name`, or `name` when there is no namespace (3.4.2).
    pub fn display(&self) -> String {
        match &self.namespace {
            Some(ns) if !ns.is_empty() => format!("{ns}:{}", self.name),
            _ => self.name.clone(),
        }
    }
}

/// A tag inside a candidate (`engine_tags`, `booru_tags`): no `source`, the array implies it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateTag {
    pub name: String,
    pub namespace: Option<String>,
}

impl CandidateTag {
    fn with_source(&self, source: TagSource) -> Tag {
        Tag {
            name: self.name.clone(),
            namespace: self.namespace.clone(),
            source,
        }
    }
}

/// One candidate match (3.4.1), the 20 keys in the order `_candidate_to_dict` writes them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub url: String,
    pub source_name: Option<String>,
    pub thumb_url: Option<String>,
    pub similarity: f64,
    pub similarity_measured: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub engine: String,
    pub engine_tags: Vec<CandidateTag>,
    pub booru_tags: Vec<CandidateTag>,
    pub direct_file_url: Option<String>,
    pub preview_url: Option<String>,
    pub remote_format: Option<String>,
    pub remote_size_bytes: Option<i64>,
    pub incomplete_reason: Option<String>,
    pub tags_borrowed_from: Option<String>,
    /// Tri-state: `None` is unknown, never "gone" (3.4.1).
    pub remote_available: Option<bool>,
    pub availability_checked_at: Option<f64>,
    pub restricted: Option<String>,
    /// The parser's `TAG_VERSION` for `url`; see [`tag_version_for_url`]. The Python app
    /// recomputes it on every write and ignores the stored one when it reads a session,
    /// so a candidate always carries the value for its own URL.
    pub tag_version: i64,
    /// `MatchCandidate.thumb_bytes`: the thumbnail the engine sent inline (Google Lens does).
    /// Session-only: `_candidate_to_dict` does not write it.
    #[serde(skip)]
    pub thumb_bytes: Option<Vec<u8>>,
    /// `MatchCandidate.page_index`: which image of a multi-image post this is. Session-only.
    #[serde(skip)]
    pub page_index: i64,
    /// `MatchCandidate.booru_tags_fetched`: whether the booru page has been tried yet. A session
    /// load always restores `False`; only the search cache restores it (see `cache`). Session-only.
    #[serde(skip)]
    pub booru_tags_fetched: bool,
    /// `MatchCandidate.remote_info_fetched`: whether the format/size HEAD has been tried yet (it
    /// may still have failed). Session-only.
    #[serde(skip)]
    pub remote_info_fetched: bool,
}

impl Candidate {
    pub fn new(url: impl Into<String>) -> Candidate {
        let url = url.into();
        let tag_version = tag_version_for_url(&url);
        Candidate {
            url,
            source_name: None,
            thumb_url: None,
            similarity: 0.0,
            similarity_measured: false,
            width: None,
            height: None,
            engine: "IQDB".to_owned(),
            engine_tags: Vec::new(),
            booru_tags: Vec::new(),
            direct_file_url: None,
            preview_url: None,
            remote_format: None,
            remote_size_bytes: None,
            incomplete_reason: None,
            tags_borrowed_from: None,
            remote_available: None,
            availability_checked_at: None,
            restricted: None,
            tag_version,
            thumb_bytes: None,
            page_index: 0,
            booru_tags_fetched: false,
            remote_info_fetched: false,
        }
    }

    /// Stores an availability verdict together with when it was reached (`now`, epoch seconds), so
    /// the date cannot drift from the verdict it dates. An inconclusive check clears the date too:
    /// "unknown as of last Tuesday" is not a fact worth keeping.
    pub fn record_availability(&mut self, available: Option<bool>, now: f64) {
        self.remote_available = available;
        self.availability_checked_at = available.map(|_| now);
    }
}

/// One image in the list (3.4): the 16 keys in the order `_entry_to_dict` writes them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub path: String,
    pub hydrus_hash: Option<String>,
    pub status: MatchStatus,
    pub error_message: Option<String>,
    pub result_source: Option<String>,
    pub searched_without_saucenao: bool,
    pub sent_to_hydrus: bool,
    pub hydrus_import_confirmed: bool,
    pub reviewed: bool,
    pub upscale_verdict: Option<String>,
    pub upscale_check_detail: Option<String>,
    pub local_width: Option<i64>,
    pub local_height: Option<i64>,
    pub selected_candidate_index: usize,
    pub candidates: Vec<Candidate>,
    pub tags: Vec<Tag>,
    /// The previous run died while this row was `searching` (see
    /// [`mark_interrupted_mid_search`]). Describes one launch, never stored (3.4).
    #[serde(skip)]
    pub interrupted_mid_search: bool,
    /// `ImageEntry.last_searched`: when `mark_searched` last ran (epoch seconds). Not stored
    /// (3.4 has no such key); it describes this run.
    #[serde(skip)]
    pub last_searched: Option<f64>,
    /// What Python's `ImageEntry.matched_url` still holds after a cached result's matches have all
    /// died: the URL of the match that was selected before they were dropped. Python keeps
    /// `matched_url` as a stored field and `select_candidate(-1)` changes nothing, so the row is
    /// left `good`/`poor` with no candidates and the dead URL, and the batch's matched-URL log
    /// writes it. Otherwise `matched_url` is the selected candidate's URL and is read from there
    /// ([`Entry::selected_candidate`]). Describes this run; not stored.
    #[serde(skip)]
    pub stale_matched_url: Option<String>,
    /// The same quirk for `ImageEntry.similarity` (Phase 4.g): the score of the match that was
    /// selected before a cached result's dead matches were dropped. The auto-import threshold
    /// reads it. Describes this run; not stored.
    #[serde(skip)]
    pub stale_similarity: Option<f64>,
    /// ... and for `ImageEntry.similarity_measured` (`score_is_measured` of that match).
    #[serde(skip)]
    pub stale_similarity_measured: bool,
    /// ... and for `ImageEntry.booru_name` (the stale match's `source_name`). The provenance note
    /// writes it as the site.
    #[serde(skip)]
    pub stale_source_name: Option<String>,
}

impl Entry {
    pub fn new(path: impl Into<String>) -> Entry {
        Entry {
            path: path.into(),
            hydrus_hash: None,
            status: MatchStatus::NotSearched,
            error_message: None,
            result_source: None,
            searched_without_saucenao: false,
            sent_to_hydrus: false,
            hydrus_import_confirmed: false,
            reviewed: false,
            upscale_verdict: None,
            upscale_check_detail: None,
            local_width: None,
            local_height: None,
            selected_candidate_index: 0,
            candidates: Vec::new(),
            tags: Vec::new(),
            interrupted_mid_search: false,
            last_searched: None,
            stale_matched_url: None,
            stale_similarity: None,
            stale_similarity_measured: false,
            stale_source_name: None,
        }
    }

    /// `ImageEntry.filename`: the last `/`-separated part of the path, for log lines and messages.
    pub fn filename(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The active candidate, if the index points at one.
    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected_candidate_index)
    }

    /// `ImageEntry.matched_url`: the selected match's URL, or the one a cached result's dead
    /// matches left behind ([`Entry::stale_matched_url`]). `None` when the entry has no match.
    pub fn matched_url(&self) -> Option<&str> {
        self.selected_candidate()
            .map(|c| c.url.as_str())
            .or(self.stale_matched_url.as_deref())
    }

    /// `ImageEntry.similarity`: the selected match's score, or the one a cached result's dead
    /// matches left behind ([`Entry::stale_similarity`]). `None` when the entry has no match.
    pub fn similarity(&self) -> Option<f64> {
        self.selected_candidate()
            .map(|c| c.similarity)
            .or(self.stale_similarity)
    }

    /// `ImageEntry.similarity_measured`: whether [`Entry::similarity`] is a real comparison
    /// (`MatchCandidate.score_is_measured` of the selected match, or of the stale one).
    pub fn similarity_measured(&self) -> bool {
        match self.selected_candidate() {
            Some(c) => c.similarity_measured || crate::engine_ids::reports_real_similarity(Some(&c.engine)),
            None => self.stale_similarity_measured,
        }
    }

    /// `ImageEntry.booru_name`: the selected match's `source_name`, or the stale one's.
    pub fn booru_name(&self) -> Option<&str> {
        match self.selected_candidate() {
            Some(c) => c.source_name.as_deref(),
            None => self.stale_source_name.as_deref(),
        }
    }

    /// `ImageEntry.select_candidate` (`core/models.py:279-298`) as far as it touches stored
    /// state: make `index` the active candidate and **replace** the search-engine and booru
    /// tags with that candidate's own, keeping the others. An index out of range changes
    /// nothing. (`matched_url`, `similarity` and the other derived fields are read through
    /// [`Entry::selected_candidate`] instead of being copied.)
    pub fn select_candidate(&mut self, index: usize) {
        if index >= self.candidates.len() {
            return;
        }
        self.selected_candidate_index = index;
        let engine: Vec<Tag> = self.candidates[index]
            .engine_tags
            .iter()
            .map(|t| t.with_source(TagSource::SearchEngine))
            .collect();
        let booru: Vec<Tag> = self.candidates[index]
            .booru_tags
            .iter()
            .map(|t| t.with_source(TagSource::Booru))
            .collect();
        self.add_tags(engine, Some(TagSource::SearchEngine));
        self.add_tags(booru, Some(TagSource::Booru));
    }

    /// `ImageEntry.add_tags`: optionally drop every tag of `replace_source` first, then append
    /// the new ones that are not already present by `(namespace, name)`.
    pub fn add_tags(&mut self, new_tags: Vec<Tag>, replace_source: Option<TagSource>) {
        if let Some(source) = replace_source {
            self.tags.retain(|t| t.source != source);
        }
        for tag in new_tags {
            if !self.tags.iter().any(|t| t.key() == tag.key()) {
                self.tags.push(tag);
            }
        }
    }
}

/// A row the previous run was searching when it died (`gui/main_window.py:176-191`): set back
/// to unsearched and flagged. Only called after an **unclean** shutdown; after a clean one a
/// `searching` row stays as it is. Returns how many rows were reset (rule 6.2.6).
pub fn mark_interrupted_mid_search(entries: &mut [Entry]) -> usize {
    let mut marked = 0;
    for entry in entries.iter_mut() {
        if entry.status == MatchStatus::Searching {
            entry.status = MatchStatus::NotSearched;
            entry.interrupted_mid_search = true;
            marked += 1;
        }
    }
    marked
}

/// `_tag_version_for(url)` (`core/search_cache.py:48-67`): what the site parser claiming this
/// URL says its tag output is worth. Only MangaDex chapter URLs have one (`1`), so every
/// other URL is `0` (rule 6.3, `core/boorus/mangadex.py:62-80`).
pub fn tag_version_for_url(url: &str) -> i64 {
    i64::from(is_mangadex_chapter(url))
}

fn is_mangadex_chapter(url: &str) -> bool {
    let host = netloc(url).to_ascii_lowercase();
    (host == "mangadex.org" || host.ends_with(".mangadex.org")) && has_chapter_uuid(url)
}

/// `urlparse(url).netloc`: the text between `//` and the next `/`, `?` or `#`, when the URL
/// has an authority. Python first drops leading control characters and spaces and removes
/// tabs and newlines anywhere.
fn netloc(url: &str) -> String {
    let cleaned: String = url
        .trim_start_matches(|c: char| c <= ' ')
        .chars()
        .filter(|c| !matches!(c, '\t' | '\r' | '\n'))
        .collect();
    let mut rest = cleaned.as_str();
    if let Some((scheme, after)) = rest.split_once(':') {
        let mut chars = scheme.chars();
        let scheme_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic())
            && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if scheme_ok {
            rest = after;
        }
    }
    match rest.strip_prefix("//") {
        Some(authority) => {
            let end = authority.find(['/', '?', '#']).unwrap_or(authority.len());
            authority[..end].to_owned()
        }
        None => String::new(),
    }
}

/// `/chapter/` followed by a UUID (`CHAPTER_RE`), anywhere in the URL; hex digits of either case.
fn has_chapter_uuid(url: &str) -> bool {
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let bytes = url.as_bytes();
    let needle = b"/chapter/";
    let mut from = 0;
    while let Some(at) = bytes[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
    {
        let mut i = at + needle.len();
        let mut ok = true;
        for (n, len) in GROUPS.into_iter().enumerate() {
            if n > 0 {
                if bytes.get(i) != Some(&b'-') {
                    ok = false;
                    break;
                }
                i += 1;
            }
            let end = i + len;
            if end > bytes.len() || !bytes[i..end].iter().all(u8::is_ascii_hexdigit) {
                ok = false;
                break;
            }
            i = end;
        }
        if ok {
            return true;
        }
        from = at + 1;
    }
    false
}

/// `enabled_tag_sources`: the sources named by `settings.enabled_tag_sources`, in `TagSource`
/// order. A name that is no source (a stale config, a rename) is dropped.
pub fn enabled_tag_sources(settings: &Settings) -> Vec<TagSource> {
    TagSource::ALL
        .into_iter()
        .filter(|s| settings.enabled_tag_sources.iter().any(|n| n == s.as_str()))
        .collect()
}

/// `tags_to_send`: the tags that leave the app for an entry (every Hydrus send and every export).
/// Every tag, unless the user switched `filter_sent_tags_by_source` on; then only those whose
/// source is ticked, which can be none at all.
pub fn tags_to_send<'a>(entry: &'a Entry, settings: Option<&Settings>) -> Vec<&'a Tag> {
    let Some(settings) = settings.filter(|s| s.filter_sent_tags_by_source) else {
        return entry.tags.iter().collect();
    };
    let enabled = enabled_tag_sources(settings);
    entry
        .tags
        .iter()
        .filter(|t| enabled.contains(&t.source))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(ns: Option<&str>, name: &str, source: TagSource) -> Tag {
        Tag {
            name: name.to_owned(),
            namespace: ns.map(str::to_owned),
            source,
        }
    }

    fn ctag(ns: Option<&str>, name: &str) -> CandidateTag {
        CandidateTag {
            name: name.to_owned(),
            namespace: ns.map(str::to_owned),
        }
    }

    #[test]
    fn status_and_source_domains() {
        for m in MatchStatus::ALL {
            assert_eq!(MatchStatus::parse(m.as_str()), Some(m));
        }
        for t in TagSource::ALL {
            assert_eq!(TagSource::parse(t.as_str()), Some(t));
        }
        assert_eq!(MatchStatus::parse("Good"), None);
        assert_eq!(MatchStatus::parse(""), None);
        assert_eq!(TagSource::parse("user"), None);
        assert_eq!(TagSource::parse("Hatate-Linux"), None);
    }

    #[test]
    fn select_replaces_engine_and_booru_tags_only() {
        let mut e = Entry::new("/a");
        let mut c0 = Candidate::new("https://a.example.invalid/1");
        c0.engine_tags = vec![ctag(None, "e0")];
        c0.booru_tags = vec![ctag(Some("artist"), "b0")];
        let mut c1 = Candidate::new("https://a.example.invalid/2");
        c1.engine_tags = vec![ctag(None, "e1")];
        c1.booru_tags = vec![ctag(None, "e1")]; // same key as its own engine tag
        e.candidates = vec![c0, c1];
        e.tags = vec![
            tag(None, "mine", TagSource::User),
            tag(None, "old", TagSource::Booru),
        ];
        e.select_candidate(0);
        let names: Vec<_> = e.tags.iter().map(|t| t.display()).collect();
        assert_eq!(names, ["mine", "e0", "artist:b0"]);
        e.select_candidate(1);
        // Candidate 0's tags are gone; the booru copy of `e1` is skipped as a duplicate.
        let got: Vec<_> = e.tags.iter().map(|t| (t.display(), t.source)).collect();
        assert_eq!(
            got,
            [
                ("mine".to_owned(), TagSource::User),
                ("e1".to_owned(), TagSource::SearchEngine)
            ]
        );
        assert_eq!(e.selected_candidate_index, 1);
        // Out of range: nothing changes.
        e.select_candidate(7);
        assert_eq!(e.selected_candidate_index, 1);
    }

    #[test]
    fn interrupted_search_is_reset_and_flagged() {
        let mut list = vec![Entry::new("/a"), Entry::new("/b"), Entry::new("/c")];
        list[1].status = MatchStatus::Searching;
        list[2].status = MatchStatus::Good;
        assert_eq!(mark_interrupted_mid_search(&mut list), 1);
        assert_eq!(list[1].status, MatchStatus::NotSearched);
        assert!(list[1].interrupted_mid_search);
        assert_eq!(list[2].status, MatchStatus::Good);
        assert!(!list[0].interrupted_mid_search);
    }

    #[test]
    fn tag_version_follows_the_mangadex_parser() {
        let id = "0123abcd-4567-89AB-cdef-0123456789ab";
        assert_eq!(
            tag_version_for_url(&format!("https://mangadex.org/chapter/{id}")),
            1
        );
        assert_eq!(
            tag_version_for_url(&format!("https://www.mangadex.org/chapter/{id}/3")),
            1
        );
        assert_eq!(
            tag_version_for_url(&format!("HTTPS://MangaDex.ORG/chapter/{id}")),
            1
        );
        // A title page is classification-only; another host or a port does not match.
        assert_eq!(
            tag_version_for_url(&format!("https://mangadex.org/title/{id}")),
            0
        );
        assert_eq!(
            tag_version_for_url(&format!("https://mangadex.org:443/chapter/{id}")),
            0
        );
        assert_eq!(
            tag_version_for_url(&format!("https://notmangadex.org/chapter/{id}")),
            0
        );
        assert_eq!(
            tag_version_for_url("https://mangadex.org/chapter/not-a-uuid"),
            0
        );
        assert_eq!(tag_version_for_url(""), 0);
        assert_eq!(tag_version_for_url("https://danbooru.donmai.us/posts/1"), 0);
    }

    #[test]
    fn stored_shape_has_the_documented_keys_in_order() {
        let e = Entry::new("/a");
        let v = serde_json::to_value(&e).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys,
            [
                "path",
                "hydrus_hash",
                "status",
                "error_message",
                "result_source",
                "searched_without_saucenao",
                "sent_to_hydrus",
                "hydrus_import_confirmed",
                "reviewed",
                "upscale_verdict",
                "upscale_check_detail",
                "local_width",
                "local_height",
                "selected_candidate_index",
                "candidates",
                "tags"
            ]
        );
        let c = serde_json::to_value(Candidate::new("u")).unwrap();
        assert_eq!(c.as_object().unwrap().len(), 20);
    }
}
