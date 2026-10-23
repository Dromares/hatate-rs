//! The MCP tool registry: names, tiers, gating and refusal text (`core/mcp_tools.py`).
//!
//! No network and no Qt: this is the part of the server that is pure data and pure logic. A
//! disabled tier refuses a call with a message naming the setting that would allow it, never a
//! stack trace and never a silent no-op.

use std::collections::BTreeSet;

use crate::config::McpSettings;

use super::McpError;

pub const TIER_READ: &str = "read";
pub const TIER_LOCAL_WRITE: &str = "local_write";
pub const TIER_RESEARCH: &str = "research";
pub const TIER_HYDRUS_WRITE: &str = "hydrus_write";
pub const TIER_DESTRUCTIVE: &str = "destructive";

/// A tool's tier. Python keeps it as one of five strings; [`Tier::as_str`] gives that string,
/// which is what the audit log and the refusal text carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Read,
    LocalWrite,
    Research,
    HydrusWrite,
    Destructive,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Read => TIER_READ,
            Tier::LocalWrite => TIER_LOCAL_WRITE,
            Tier::Research => TIER_RESEARCH,
            Tier::HydrusWrite => TIER_HYDRUS_WRITE,
            Tier::Destructive => TIER_DESTRUCTIVE,
        }
    }

    /// `DRY_RUNNABLE_TIERS`: the tiers that change something outside the in-memory queue. Read
    /// tools and the two reversible local writes have nothing useful for `dry_run` to simulate.
    pub fn is_dry_runnable(self) -> bool {
        matches!(self, Tier::Research | Tier::HydrusWrite | Tier::Destructive)
    }
}

/// The [`McpSettings`] switch that gates a tier (`ToolSpec.gate_field`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateField {
    AllowResearch,
    AllowHydrusWrites,
    AllowDestructive,
}

impl GateField {
    /// The settings attribute name, as `getattr` took it and as `tools/list` prints it.
    pub fn name(self) -> &'static str {
        match self {
            GateField::AllowResearch => "allow_research",
            GateField::AllowHydrusWrites => "allow_hydrus_writes",
            GateField::AllowDestructive => "allow_destructive",
        }
    }

    /// `getattr(settings, gate_field)`.
    pub fn is_on(self, settings: &McpSettings) -> bool {
        match self {
            GateField::AllowResearch => settings.allow_research,
            GateField::AllowHydrusWrites => settings.allow_hydrus_writes,
            GateField::AllowDestructive => settings.allow_destructive,
        }
    }
}

/// One MCP tool: its tier, the setting that gates it (if any), and the review action id it
/// mirrors (if any).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolSpec {
    pub name: &'static str,
    pub tier: Tier,
    pub gate_field: Option<GateField>,
    /// The checkbox label named in the refusal message.
    pub gate_label: &'static str,
    pub review_action_id: Option<&'static str>,
}

const fn spec(
    name: &'static str,
    tier: Tier,
    gate_field: Option<GateField>,
    gate_label: &'static str,
    review_action_id: Option<&'static str>,
) -> ToolSpec {
    ToolSpec {
        name,
        tier,
        gate_field,
        gate_label,
        review_action_id,
    }
}

const HYDRUS_LABEL: &str = "Let it send files and tags to Hydrus";
const DESTRUCTIVE_LABEL: &str = "Let it remove rows and reset results";

/// `TOOLS`, in registration order (which is the order `tools/list` prints).
pub static TOOLS: [ToolSpec; 14] = [
    spec("list_queue", Tier::Read, None, "", None),
    spec("get_entry", Tier::Read, None, "", None),
    spec("get_candidates", Tier::Read, None, "", None),
    spec("get_images", Tier::Read, None, "", None),
    spec("get_diff", Tier::Read, None, "", None),
    spec("list_actions", Tier::Read, None, "", None),
    spec("select_candidate", Tier::LocalWrite, None, "", None),
    spec(
        "toggle_reviewed",
        Tier::LocalWrite,
        None,
        "",
        Some("toggle_reviewed"),
    ),
    spec(
        "research",
        Tier::Research,
        Some(GateField::AllowResearch),
        "Let it re-search a stuck entry",
        Some("research"),
    ),
    spec(
        "send_upload",
        Tier::HydrusWrite,
        Some(GateField::AllowHydrusWrites),
        HYDRUS_LABEL,
        Some("send_upload"),
    ),
    spec(
        "send_url",
        Tier::HydrusWrite,
        Some(GateField::AllowHydrusWrites),
        HYDRUS_LABEL,
        Some("send_url"),
    ),
    spec(
        "download_send",
        Tier::HydrusWrite,
        Some(GateField::AllowHydrusWrites),
        HYDRUS_LABEL,
        Some("download_send"),
    ),
    spec(
        "remove_row",
        Tier::Destructive,
        Some(GateField::AllowDestructive),
        DESTRUCTIVE_LABEL,
        Some("remove_row"),
    ),
    spec(
        "reset_result",
        Tier::Destructive,
        Some(GateField::AllowDestructive),
        DESTRUCTIVE_LABEL,
        Some("reset_result"),
    ),
];

/// `TOOLS_BY_NAME[name]`.
pub fn tool(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|t| t.name == name)
}

/// `EXCLUDED_REVIEW_ACTION_IDS`: shortcut action ids deliberately left out of the tool surface
/// (superseded by `get_images`/`get_diff`, pointless away from the machine, or relative
/// navigation where every tool here takes a `row` instead).
pub const EXCLUDED_REVIEW_ACTION_IDS: [&str; 9] = [
    "compare",
    "open_match",
    "show_in_hydrus",
    "next_row",
    "prev_row",
    "next_unreviewed",
    "prev_unreviewed",
    "next_candidate",
    "prev_candidate",
];

/// The ids of `core/shortcuts.py`'s `REVIEW_ACTIONS` at `76c85c4`, which Python reads as
/// `ACTIONS_BY_ID`. The shortcuts registry itself belongs to the GUI (Phase 6); this list is
/// the copy [`unaccounted_review_action_ids`] checks the tool surface against.
pub const REVIEW_ACTION_IDS: [&str; 16] = [
    "next_row",
    "prev_row",
    "next_unreviewed",
    "prev_unreviewed",
    "next_candidate",
    "prev_candidate",
    "compare",
    "open_match",
    "show_in_hydrus",
    "send_upload",
    "send_url",
    "download_send",
    "research",
    "reset_result",
    "toggle_reviewed",
    "remove_row",
];

/// `mapped_review_action_ids()`: the review action ids that have a tool mapped onto them.
pub fn mapped_review_action_ids() -> BTreeSet<&'static str> {
    TOOLS.iter().filter_map(|t| t.review_action_id).collect()
}

/// `unaccounted_review_action_ids()`: review action ids that are neither mapped to a tool nor in
/// the documented exclusion list, sorted. Empty means parity holds.
pub fn unaccounted_review_action_ids() -> Vec<&'static str> {
    unaccounted_in(&REVIEW_ACTION_IDS)
}

/// [`unaccounted_review_action_ids`] over a given action registry, so a test can show that a new
/// action id landing without a change here is caught.
pub fn unaccounted_in(actions: &[&'static str]) -> Vec<&'static str> {
    let mapped = mapped_review_action_ids();
    let mut out: Vec<&'static str> = actions
        .iter()
        .copied()
        .filter(|id| !mapped.contains(id) && !EXCLUDED_REVIEW_ACTION_IDS.contains(id))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    out.sort_unstable();
    out
}

/// `_TIER_REFUSAL_COPY`: the tier label and its grammatical number. `is` for the singular
/// "Re-search", `are` for the plural noun phrases; deliberately not one hardcoded verb.
fn refusal_copy(tier: Tier) -> Option<(&'static str, &'static str)> {
    match tier {
        Tier::Research => Some(("Re-search", "is")),
        Tier::HydrusWrite => Some(("Hydrus writes", "are")),
        Tier::Destructive => Some(("Destructive actions", "are")),
        Tier::Read | Tier::LocalWrite => None,
    }
}

/// `refusal_message(spec)`: the exact text shown when a disabled tier refuses a call. `None`
/// for a spec of an ungated tier, where Python raises `KeyError`.
pub fn refusal_message(spec: &ToolSpec) -> Option<String> {
    let (tier_label, verb) = refusal_copy(spec.tier)?;
    Some(format!(
        "\"{}\" refused: {} {} off. Turn on \"{}\" in Settings \u{2192} MCP to allow it.",
        spec.name, tier_label, verb, spec.gate_label
    ))
}

/// `gate(tool_name, settings)`: `Ok(None)` if the call is allowed right now, else the refusal
/// message. Evaluated against the settings it is given on every call, so a switch flipped in
/// Settings takes effect on the next call. An unknown name is Python's `KeyError`.
pub fn gate(tool_name: &str, settings: &McpSettings) -> Result<Option<String>, McpError> {
    let spec = tool(tool_name).ok_or_else(|| McpError::unknown_tool(tool_name))?;
    let Some(field) = spec.gate_field else {
        return Ok(None);
    };
    if field.is_on(settings) {
        return Ok(None);
    }
    Ok(refusal_message(spec))
}

/// `is_dry_runnable(tool_name)`.
pub fn is_dry_runnable(tool_name: &str) -> Result<bool, McpError> {
    let spec = tool(tool_name).ok_or_else(|| McpError::unknown_tool(tool_name))?;
    Ok(spec.tier.is_dry_runnable())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> McpSettings {
        McpSettings::default()
    }

    #[test]
    fn tool_names_are_unique_and_findable() {
        let names: BTreeSet<_> = TOOLS.iter().map(|t| t.name).collect();
        assert_eq!(names.len(), TOOLS.len());
        for t in &TOOLS {
            assert_eq!(tool(t.name), Some(t));
        }
        assert_eq!(tool("nope"), None);
    }

    #[test]
    fn nothing_is_unaccounted_for_and_the_lists_do_not_overlap() {
        assert!(unaccounted_review_action_ids().is_empty());
        let mapped = mapped_review_action_ids();
        for id in EXCLUDED_REVIEW_ACTION_IDS {
            assert!(!mapped.contains(id), "{id} is both mapped and excluded");
            assert!(
                REVIEW_ACTION_IDS.contains(&id),
                "{id} is not a review action"
            );
        }
        for id in &mapped {
            assert!(
                REVIEW_ACTION_IDS.contains(id),
                "{id} is not a review action"
            );
        }
        // select_candidate is deliberately not a mapped review action.
        assert!(tool("select_candidate").unwrap().review_action_id.is_none());
        // A new action id with no tool and no exclusion is caught.
        let mut grown = REVIEW_ACTION_IDS.to_vec();
        grown.push("brand_new");
        assert_eq!(unaccounted_in(&grown), vec!["brand_new"]);
    }

    #[test]
    fn gating_follows_the_defaults_and_the_live_settings() {
        let mut s = settings();
        for t in TOOLS
            .iter()
            .filter(|t| matches!(t.tier, Tier::Read | Tier::LocalWrite))
        {
            assert_eq!(gate(t.name, &s).unwrap(), None);
        }
        assert!(gate("research", &s).unwrap().is_some());
        assert_eq!(gate("send_upload", &s).unwrap(), None);
        assert!(gate("remove_row", &s).unwrap().is_some());
        s.allow_research = true;
        assert_eq!(gate("research", &s).unwrap(), None);
        assert!(
            gate("remove_row", &s).unwrap().is_some(),
            "one tier on leaves another off"
        );
        s.allow_hydrus_writes = false;
        assert!(
            gate("send_url", &s)
                .unwrap()
                .unwrap()
                .contains("Hydrus writes are off")
        );
        assert!(gate("nope", &s).is_err());
    }

    #[test]
    fn refusal_text_matches_the_spec_verbatim_and_names_no_attribute() {
        let msg = refusal_message(tool("research").unwrap()).unwrap();
        assert_eq!(
            msg,
            "\"research\" refused: Re-search is off. Turn on \"Let it re-search a stuck entry\" in Settings \u{2192} MCP to allow it."
        );
        let msg = refusal_message(tool("remove_row").unwrap()).unwrap();
        assert_eq!(
            msg,
            "\"remove_row\" refused: Destructive actions are off. Turn on \"Let it remove rows and reset results\" in Settings \u{2192} MCP to allow it."
        );
        let all: BTreeSet<_> = TOOLS.iter().filter_map(refusal_message).collect();
        assert_eq!(all.len(), 6, "every gated tool has its own message");
        assert!(
            all.iter()
                .all(|m| !m.contains("allow_") && !m.contains("mcp."))
        );
        assert_eq!(refusal_message(tool("get_entry").unwrap()), None);
    }

    #[test]
    fn dry_run_applies_to_the_three_write_tiers_only() {
        for t in &TOOLS {
            let expect = matches!(
                t.tier,
                Tier::Research | Tier::HydrusWrite | Tier::Destructive
            );
            assert_eq!(is_dry_runnable(t.name).unwrap(), expect, "{}", t.name);
        }
        assert!(is_dry_runnable("nope").is_err());
    }
}
