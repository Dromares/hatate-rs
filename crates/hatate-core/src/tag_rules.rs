//! User-configurable tag rules (`core/tag_rules.py`): namespace remapping, blacklist filtering,
//! the rating tag and the tags the app applies itself from how a search turned out.
//!
//! Only the part the engine runner and `search_image` call is here (DAN-2063): the remap, the
//! blacklist, `with_rating_tag`, `countable_tags` and `apply_outcome_tags` with the helpers they
//! use. `apply_inline_edit` and its result type stay in Phase 4 with the review table that calls
//! them.
//!
//! Order of operations is the caller's: remap first, then the blacklist, so a pattern is written
//! against the tag in its final displayed form.

use tracing::debug;

use crate::config::Settings;
use crate::model::{Entry, MatchStatus, Tag, TagSource};
use crate::pycompat;
use crate::pyfnmatch::fnmatchcase;

/// `OUTCOME_TAG_SOURCE`: the source every by-rule tag is written under. Nothing else in the app
/// writes it, which is what makes the rule idempotent and keeps the tags out of the count.
pub const OUTCOME_TAG_SOURCE: TagSource = TagSource::Hatate;

/// `DEFAULT_RATING_NAMESPACE`.
pub const DEFAULT_RATING_NAMESPACE: &str = "rating";

/// `apply_namespace_remap`: each tag's namespace rewritten by `settings.tag_namespace_remap`,
/// when enabled. A tag with no namespace, or one not in the table, passes through unchanged; a
/// rule that maps to an empty string strips the namespace.
pub fn apply_namespace_remap(tags: &[Tag], settings: &Settings) -> Vec<Tag> {
    if !settings.enable_tag_namespace_remap || settings.tag_namespace_remap.is_empty() {
        return tags.to_vec();
    }
    let remap = &settings.tag_namespace_remap;
    tags.iter()
        .map(|t| {
            let target = match t.namespace.as_deref() {
                Some(ns) if !ns.is_empty() => remap.get(ns),
                _ => None,
            };
            match target {
                Some(new_namespace) => Tag {
                    name: t.name.clone(),
                    source: t.source,
                    // "" (strip) -> no namespace
                    namespace: if new_namespace.is_empty() {
                        None
                    } else {
                        Some(new_namespace.clone())
                    },
                },
                None => t.clone(),
            }
        })
        .collect()
}

/// `with_rating_tag`: the list with the matched post's rating added as one namespaced tag, when
/// the user asked for it and the site stated one.
pub fn with_rating_tag(tags: &[Tag], rating: Option<&str>, settings: &Settings) -> Vec<Tag> {
    let rating = rating.unwrap_or("");
    if !settings.add_rating_tag || rating.is_empty() {
        return tags.to_vec();
    }
    let namespace = pycompat::strip(&settings.rating_tag_namespace);
    let namespace = if namespace.is_empty() {
        DEFAULT_RATING_NAMESPACE
    } else {
        namespace
    };
    let mut out = tags.to_vec();
    out.push(Tag {
        name: rating.to_owned(),
        namespace: Some(namespace.to_owned()),
        source: TagSource::Booru,
    });
    out
}

/// `_normalize`: lower-cased, with spaces and underscores the same character.
fn normalize(text: &str) -> String {
    pycompat::strip(text).to_lowercase().replace(' ', "_")
}

/// `is_tag_blacklisted`: whether one tag matches any pattern. A pattern with a colon matches the
/// full `namespace:name` form, one without matches the bare name in any namespace.
pub fn is_tag_blacklisted<S: AsRef<str>>(tag: &Tag, patterns: &[S]) -> bool {
    let name = normalize(&tag.name);
    let display = normalize(&tag.display());
    for raw_pattern in patterns {
        let pattern = normalize(raw_pattern.as_ref());
        if pattern.is_empty() {
            continue; // blank lines in the settings box
        }
        let target = if pattern.contains(':') {
            &display
        } else {
            &name
        };
        if fnmatchcase(target, &pattern) {
            return true;
        }
    }
    false
}

/// `apply_tag_blacklist`: the tags with the blacklisted ones removed, when the feature is on and
/// patterns are configured.
pub fn apply_tag_blacklist(tags: &[Tag], settings: &Settings) -> Vec<Tag> {
    if !settings.enable_tag_blacklist || settings.tag_blacklist.is_empty() {
        return tags.to_vec();
    }
    let patterns = &settings.tag_blacklist;
    let kept: Vec<Tag> = tags
        .iter()
        .filter(|t| !is_tag_blacklisted(t, patterns))
        .cloned()
        .collect();
    let dropped = tags.len() - kept.len();
    if dropped > 0 {
        debug!(
            "Tag blacklist dropped {dropped} of {} incoming tag(s)",
            tags.len()
        );
    }
    kept
}

/// `split_tag_text`: `"artist:someone"` -> `(Some("artist"), "someone")`. Splits on the first
/// colon only; an empty namespace side is no namespace.
pub fn split_tag_text(text: &str) -> (Option<String>, String) {
    match text.split_once(':') {
        Some((namespace, name)) => {
            let namespace = pycompat::strip(namespace);
            (
                if namespace.is_empty() {
                    None
                } else {
                    Some(namespace.to_owned())
                },
                pycompat::strip(name).to_owned(),
            )
        }
        None => (None, pycompat::strip(text).to_owned()),
    }
}

/// `countable_tags`: the tags that count towards `min_tags_for_good`, which is everything except
/// the app's own by-rule tags.
pub fn countable_tags(tags: &[Tag]) -> Vec<Tag> {
    tags.iter()
        .filter(|t| t.source != OUTCOME_TAG_SOURCE)
        .cloned()
        .collect()
}

/// `outcome_tags_for`: the by-rule tags an entry with this outcome should carry. `tag_count` is
/// the number of real tags on the entry, measured before any by-rule tag is added.
pub fn outcome_tags_for(status: MatchStatus, tag_count: usize, settings: &Settings) -> Vec<Tag> {
    let mut names: Vec<&str> = Vec::new();
    match status {
        MatchStatus::Good | MatchStatus::Poor => {
            names.extend(settings.tags_for_found.iter().map(String::as_str));
            // `tag_count < min_tags_for_good`, compared as the integers they are.
            if (tag_count as i64) < settings.match_conditions.min_tags_for_good {
                names.extend(settings.tags_for_low_tag_count.iter().map(String::as_str));
            }
        }
        MatchStatus::NotFound => {
            names.extend(settings.tags_for_not_found.iter().map(String::as_str));
        }
        MatchStatus::Error | MatchStatus::Searching | MatchStatus::NotSearched => {}
    }

    let mut tags: Vec<Tag> = Vec::new();
    let mut seen: Vec<(Option<String>, String)> = Vec::new();
    for raw in names {
        let (namespace, name) = split_tag_text(raw);
        if name.is_empty() {
            // Blank lines and a lone ":" in the settings box.
            continue;
        }
        let key = (namespace.clone(), name.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        tags.push(Tag {
            name,
            namespace,
            source: OUTCOME_TAG_SOURCE,
        });
    }
    tags
}

/// `apply_outcome_tags`: puts the by-rule tags for `entry.status` onto `entry`, replacing any left
/// by a previous search, and returns the tags applied. Not subject to the blacklist or the remap:
/// they came from this user's own settings.
pub fn apply_outcome_tags(entry: &mut Entry, settings: &Settings) -> Vec<Tag> {
    let tags = outcome_tags_for(entry.status, countable_tags(&entry.tags).len(), settings);
    if tags.is_empty() && !entry.tags.iter().any(|t| t.source == OUTCOME_TAG_SOURCE) {
        // Nothing to add and nothing to clear: the feature is off.
        return Vec::new();
    }
    entry.add_tags(tags.clone(), Some(OUTCOME_TAG_SOURCE));
    if !tags.is_empty() {
        debug!(
            "{}: applied {} by-rule tag(s) for status={}: {}",
            file_name(&entry.path),
            tags.len(),
            entry.status.as_str(),
            tags.iter().map(Tag::display).collect::<Vec<_>>().join(", ")
        );
    }
    tags
}

/// `ImageEntry.filename`: the last path component, for log lines.
fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OrderedMap;

    fn settings() -> Settings {
        let paths = crate::paths::Paths::from_lookup(|k| (k == "HOME").then(|| "/home/test".into()))
            .unwrap();
        Settings::with_defaults(&paths)
    }

    fn tag(name: &str, namespace: Option<&str>, source: TagSource) -> Tag {
        Tag {
            name: name.to_owned(),
            namespace: namespace.map(str::to_owned),
            source,
        }
    }

    #[test]
    fn remap_rewrites_and_strips_namespaces() {
        let mut s = settings();
        let tags = vec![
            tag("a", Some("artist"), TagSource::Booru),
            tag("b", Some("general"), TagSource::Booru),
            tag("c", None, TagSource::Booru),
            tag("d", Some("other"), TagSource::Booru),
        ];
        // Off by default: unchanged.
        assert_eq!(apply_namespace_remap(&tags, &s), tags);
        s.enable_tag_namespace_remap = true;
        s.tag_namespace_remap = OrderedMap(vec![
            ("artist".to_owned(), "creator".to_owned()),
            ("general".to_owned(), String::new()),
        ]);
        let out = apply_namespace_remap(&tags, &s);
        assert_eq!(out[0].namespace.as_deref(), Some("creator"));
        assert_eq!(out[1].namespace, None);
        assert_eq!(out[2], tags[2]);
        assert_eq!(out[3], tags[3]);
    }

    #[test]
    fn blacklist_matches_names_and_namespaced_forms() {
        let mut s = settings();
        s.enable_tag_blacklist = true;
        s.tag_blacklist = vec![
            "highres".into(),
            "Meta:Translated".into(),
            "bad id".into(),
            "".into(),
            "  ".into(),
            "*_request".into(),
        ];
        let tags = vec![
            tag("highres", None, TagSource::Booru),
            tag("highres", Some("meta"), TagSource::Booru),
            tag("translated", Some("meta"), TagSource::Booru),
            tag("translated", Some("other"), TagSource::Booru),
            tag("bad_id", None, TagSource::Booru),
            tag("commentary_request", None, TagSource::Booru),
            tag("keep", None, TagSource::Booru),
        ];
        let kept = apply_blacklist_names(&tags, &s);
        assert_eq!(kept, vec!["translated", "keep"]);
        s.enable_tag_blacklist = false;
        assert_eq!(apply_tag_blacklist(&tags, &s), tags);
    }

    fn apply_blacklist_names(tags: &[Tag], s: &Settings) -> Vec<String> {
        apply_tag_blacklist(tags, s)
            .iter()
            .map(|t| t.name.clone())
            .collect()
    }

    #[test]
    fn a_colon_pattern_never_matches_an_unnamespaced_tag() {
        let tag = tag("highres", None, TagSource::Booru);
        assert!(!is_tag_blacklisted(&tag, &["meta:highres"]));
        assert!(is_tag_blacklisted(&tag, &["h*"]));
    }

    #[test]
    fn rating_tag_uses_the_configured_namespace_or_the_default() {
        let mut s = settings();
        let tags = vec![tag("a", None, TagSource::Booru)];
        assert_eq!(with_rating_tag(&tags, Some("explicit"), &s), tags);
        s.add_rating_tag = true;
        assert_eq!(with_rating_tag(&tags, None, &s), tags);
        assert_eq!(with_rating_tag(&tags, Some(""), &s), tags);
        let out = with_rating_tag(&tags, Some("explicit"), &s);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].display(), "rating:explicit");
        assert_eq!(out[1].source, TagSource::Booru);
        s.rating_tag_namespace = "  ".into();
        assert_eq!(
            with_rating_tag(&tags, Some("safe"), &s)[1].display(),
            "rating:safe"
        );
        s.rating_tag_namespace = " age ".into();
        assert_eq!(
            with_rating_tag(&tags, Some("safe"), &s)[1].display(),
            "age:safe"
        );
    }

    #[test]
    fn split_tag_text_splits_on_the_first_colon() {
        assert_eq!(
            split_tag_text("series:Re:Zero"),
            (Some("series".into()), "Re:Zero".into())
        );
        assert_eq!(split_tag_text(" :name "), (None, "name".into()));
        assert_eq!(split_tag_text(" plain "), (None, "plain".into()));
        assert_eq!(split_tag_text(":"), (None, String::new()));
    }

    #[test]
    fn outcome_tags_follow_the_status() {
        let mut s = settings();
        s.tags_for_found = vec!["hatate:found".into(), "hatate:found".into(), ":".into()];
        s.tags_for_not_found = vec!["hatate:not found".into()];
        s.tags_for_low_tag_count = vec!["hatate:few tags".into()];
        let names = |status, count| -> Vec<String> {
            outcome_tags_for(status, count, &s)
                .iter()
                .map(Tag::display)
                .collect()
        };
        assert_eq!(names(MatchStatus::Good, 9), vec!["hatate:found"]);
        assert_eq!(
            names(MatchStatus::Poor, 1),
            vec!["hatate:found", "hatate:few tags"]
        );
        assert_eq!(names(MatchStatus::NotFound, 0), vec!["hatate:not found"]);
        assert!(names(MatchStatus::Error, 0).is_empty());
        assert!(names(MatchStatus::Searching, 0).is_empty());
        assert!(names(MatchStatus::NotSearched, 0).is_empty());
    }

    #[test]
    fn apply_outcome_tags_replaces_its_own_and_is_idempotent() {
        let mut s = settings();
        s.tags_for_not_found = vec!["hatate:not found".into()];
        s.tags_for_found = vec!["hatate:found".into()];
        let mut entry = Entry::new("/tmp/a.png");
        entry.status = MatchStatus::NotFound;
        assert_eq!(apply_outcome_tags(&mut entry, &s).len(), 1);
        assert_eq!(apply_outcome_tags(&mut entry, &s).len(), 1);
        assert_eq!(entry.tags.len(), 1);
        entry.status = MatchStatus::Good;
        entry
            .tags
            .push(tag("real", None, TagSource::Booru));
        apply_outcome_tags(&mut entry, &s);
        let shown: Vec<String> = entry.tags.iter().map(Tag::display).collect();
        assert_eq!(shown, vec!["real", "hatate:found"]);
        // Feature off and nothing to clear: nothing touched.
        let off = settings();
        let mut plain = Entry::new("/tmp/b.png");
        plain.status = MatchStatus::Good;
        assert!(apply_outcome_tags(&mut plain, &off).is_empty());
        assert!(plain.tags.is_empty());
        // ... but a leftover by-rule tag is cleared once the feature is off.
        plain.tags.push(tag("x", Some("hatate"), TagSource::Hatate));
        apply_outcome_tags(&mut plain, &off);
        assert!(plain.tags.is_empty());
    }

    #[test]
    fn countable_tags_leave_out_the_apps_own() {
        let tags = vec![
            tag("a", None, TagSource::Booru),
            tag("found", Some("hatate"), TagSource::Hatate),
            tag("b", None, TagSource::User),
        ];
        assert_eq!(countable_tags(&tags).len(), 2);
    }
}
