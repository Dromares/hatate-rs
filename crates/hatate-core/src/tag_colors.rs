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
