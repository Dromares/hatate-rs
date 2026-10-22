//! `config.json`: the typed settings, their migrations and the read/write path.
//! Contract: `docs/file-formats.md` section 2 and rules 6.1.
//!
//! What is deliberately *not* here: anything Rust-only. `config.json` carries exactly the
//! 103 keys the Python app knows (rule 6.1.4); Rust-only settings go to
//! [`Paths::rust_settings_file`](crate::paths::Paths::rust_settings_file).

mod field;
mod migrate;
mod model;
pub(crate) mod pyjson;
mod report;
mod store;

pub use field::{OrderedMap, Secret};
pub use model::{
    CURRENT_SCHEMA_VERSION, HydrusSettings, MatchConditions, McpSettings, SauceNaoSettings,
    Settings, key_counts,
};
pub use report::{LoadReport, Notice, SaveError, Source, Unreadable};
pub use store::Loaded;
