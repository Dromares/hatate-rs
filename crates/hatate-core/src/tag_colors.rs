//! Colours for the tag list, matching Hydrus's namespace colour convention (`core/tag_colors.py`).
//!
//! Hydrus's Client API has no endpoint for a user's configured namespace colours (hydrusnetwork
//! issue 915), so these are Hydrus's commonly cited defaults: creator red and character green from
//! a Hydrus developer's description, the rest from a third-party client's documented palette. A
//! user whose Hydrus is recoloured overrides them in Settings > Tag Namespaces.

use crate::config::Settings;
use crate::model::Tag;

/// `DEFAULT_NAMESPACE_COLORS`, in the Python dict's order. Booru-side spellings (`artist`,
/// `copyright`) map to the colour of the Hydrus namespace that means the same.
pub const DEFAULT_NAMESPACE_COLORS: [(&str, &str); 8] = [
    ("creator", "#bb1800"),
    ("artist", "#bb1800"),
    ("character", "#00b401"),
    ("person", "#008f00"),
    ("series", "#bb2cb9"),
    ("copyright", "#bb2cb9"),
    ("studio", "#941100"),
    ("meta", "#676767"),
];

/// `UNNAMESPACED_COLOR`: Hydrus also colours unnamespaced tags distinctly from its default text.
pub const UNNAMESPACED_COLOR: &str = "#0088fb";

/// `get_tag_color`: a hex colour for this tag, or `None` to use the default text colour.
pub fn get_tag_color(tag: &Tag, settings: &Settings) -> Option<String> {
    if !settings.enable_hydrus_tag_colors {
        return None;
    }
    let overrides = &settings.tag_namespace_colors;
    match tag.namespace.as_deref().filter(|ns| !ns.is_empty()) {
        Some(ns) => match overrides.get(ns) {
            Some(color) => Some(color.clone()),
            None => DEFAULT_NAMESPACE_COLORS
                .iter()
                .find(|(name, _)| *name == ns)
                .map(|(_, color)| (*color).to_owned()),
        },
        None => Some(
            overrides
                .get("")
                .cloned()
                .unwrap_or_else(|| UNNAMESPACED_COLOR.to_owned()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OrderedMap;
    use crate::model::TagSource;

    fn test_settings() -> crate::config::Settings {
        let home = std::ffi::OsString::from("/nonexistent");
        let paths = crate::paths::Paths::from_lookup(|k| (k == "HOME").then(|| home.clone()))
            .expect("HOME is set");
        crate::config::Settings::with_defaults(&paths)
    }

    fn tag(ns: Option<&str>) -> Tag {
        Tag {
            name: "x".to_owned(),
            namespace: ns.map(str::to_owned),
            source: TagSource::User,
        }
    }

    fn settings(on: bool, overrides: &[(&str, &str)]) -> Settings {
        let mut s = test_settings();
        s.enable_hydrus_tag_colors = on;
        s.tag_namespace_colors = OrderedMap(
            overrides
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        );
        s
    }

    #[test]
    fn nothing_is_coloured_unless_the_setting_is_on() {
        assert_eq!(
            get_tag_color(&tag(Some("creator")), &settings(false, &[])),
            None
        );
    }

    #[test]
    fn a_namespace_takes_its_override_then_its_default() {
        let s = settings(true, &[("creator", "#111111")]);
        assert_eq!(
            get_tag_color(&tag(Some("creator")), &s).as_deref(),
            Some("#111111")
        );
        assert_eq!(
            get_tag_color(&tag(Some("copyright")), &s).as_deref(),
            Some("#bb2cb9")
        );
        assert_eq!(get_tag_color(&tag(Some("unheard-of")), &s), None);
    }

    #[test]
    fn an_unnamespaced_tag_takes_the_blank_override_or_the_fallback() {
        assert_eq!(
            get_tag_color(&tag(None), &settings(true, &[])).as_deref(),
            Some(UNNAMESPACED_COLOR)
        );
        assert_eq!(
            get_tag_color(&tag(Some("")), &settings(true, &[("", "#222222")])).as_deref(),
            Some("#222222"),
            "an empty namespace is no namespace"
        );
    }

    #[test]
    fn every_default_is_a_six_digit_hex_string() {
        for (_, color) in DEFAULT_NAMESPACE_COLORS
            .iter()
            .chain([&("", UNNAMESPACED_COLOR)])
        {
            assert!(color.len() == 7 && color.starts_with('#'));
            assert!(color[1..].bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }
}
