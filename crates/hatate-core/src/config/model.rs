//! The typed settings: the 79 top-level and 24 nested keys of `docs/file-formats.md`
//! 2.5, in the dataclass field order of `core/config.py:127-510` (the order they are
//! written in, rule 6.1.8). Defaults are the Python dataclass defaults (2.1, 2.5).

use serde_json::{Map, Value};

use super::field::{Field, OrderedMap, Secret, Section, join_key};
use super::pyjson;
use super::report::LoadReport;
use crate::paths::Paths;

/// The schema version this build writes (`core/config.py:92`, rule 6.1.5).
pub const CURRENT_SCHEMA_VERSION: i64 = 13;

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn pairs(items: &[(&str, &str)]) -> OrderedMap<String> {
    OrderedMap(
        items
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    )
}

/// `DEFAULT_TAG_BLACKLIST` (`core/config.py:101-124`).
fn default_tag_blacklist() -> Vec<String> {
    strings(&[
        "highres",
        "absurdres",
        "lowres",
        "incredibly_absurdres",
        "tagme",
        "commentary",
        "commentary_request",
        "commentary_typo",
        "translated",
        "translation_request",
        "check_translation",
        "artist_request",
        "character_request",
        "source_request",
        "bad_id",
        "bad_pixiv_id",
        "bad_twitter_id",
        "md5_mismatch",
        "resolution_mismatch",
        "revision",
        "third-party_edit",
        "duplicate",
    ])
}

/// `ALL_SITE_OPTIONS` (`core/sites.py:126-128`): the site names sorted case-insensitively,
/// then `"Other"`.
fn default_enabled_sites() -> Vec<String> {
    strings(&[
        "AniList",
        "Anime-Pictures",
        "Danbooru",
        "DeviantArt",
        "e-Hentai",
        "e-shuushuu",
        "e621",
        "Gelbooru",
        "Konachan",
        "MangaDex",
        "MangaUpdates",
        "MyAnimeList",
        "Paheal",
        "Pawchive",
        "Pixiv",
        "Reddit",
        "Rule34",
        "Rule34.us",
        "Safebooru",
        "Sankaku Complex",
        "Toon34",
        "Twitter",
        "Xbooru",
        "Yande.re",
        "Zerochan",
        "Other",
    ])
}

/// Declares a settings struct: its fields in file order, with defaults, plus the
/// tolerant reader and the Python-format writer. A key not listed here is not a setting
/// of this section, so a nested section can never grow a key Python would reject.
macro_rules! section {
    ($(#[$meta:meta])* $name:ident { $($(#[$fmeta:meta])* $field:ident : $ty:ty = $default:expr),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name {
            $($(#[$fmeta])* pub $field: $ty,)+
        }

        impl $name {
            /// The key names of this section, in file order.
            pub const KEYS: &'static [&'static str] = &[$(stringify!($field)),+];

            fn defaults() -> Self {
                Self { $($field: $default,)+ }
            }
        }

        impl Section for $name {
            fn read_object(&mut self, obj: &Map<String, Value>, prefix: &str, rep: &mut LoadReport) {
                for (k, v) in obj {
                    match k.as_str() {
                        $(stringify!($field) => Field::read(&mut self.$field, v, &join_key(prefix, k), rep),)+
                        _ => rep.unknown_key(prefix),
                    }
                }
            }

            fn write_object(&self, out: &mut String, depth: usize) {
                out.push('{');
                let mut first = true;
                $(
                    if !std::mem::take(&mut first) {
                        out.push(',');
                    }
                    pyjson::newline(out, depth + 1);
                    pyjson::string(out, stringify!($field));
                    out.push_str(": ");
                    Field::write(&self.$field, out, depth + 1);
                )+
                pyjson::newline(out, depth);
                out.push('}');
            }
        }

        impl Field for $name {
            fn read(&mut self, v: &Value, key: &str, rep: &mut LoadReport) {
                match v {
                    Value::Object(obj) => self.read_object(obj, key, rep),
                    _ => rep.wrong_type(key),
                }
            }

            fn write(&self, out: &mut String, depth: usize) {
                self.write_object(out, depth);
            }
        }
    };
}

section! {
    /// `match_conditions` (4 keys).
    MatchConditions {
        min_width_gain_percent: f64 = 10.0,
        min_height_gain_percent: f64 = 10.0,
        min_tags_for_good: i64 = 5,
        require_larger_than_local: bool = true,
    }
}

section! {
    /// `saucenao` (7 keys).
    SauceNaoSettings {
        api_key: Secret = Secret::default(),
        use_json_api: bool = true,
        min_similarity: f64 = 60.0,
        tag_types: Vec<String> = strings(&["character", "material"]),
        namespace: String = String::new(),
        /// Hand-edit only in the Python app; kept as found.
        dbmask: i64 = 999,
        pause_search_on_quota_exhausted: bool = true,
    }
}

section! {
    /// `hydrus` (5 keys).
    HydrusSettings {
        api_url: String = "http://127.0.0.1:45869".to_owned(),
        access_key: Secret = Secret::default(),
        verify_ssl: bool = true,
        tag_service_key: String = String::new(),
        /// Hand-edit only in the Python app; kept as found.
        timeout: f64 = 30.0,
    }
}

section! {
    /// `mcp` (8 keys).
    McpSettings {
        enabled: bool = false,
        port: i64 = 8787,
        token: Secret = Secret::default(),
        dry_run: bool = false,
        allow_research: bool = false,
        allow_hydrus_writes: bool = true,
        allow_destructive: bool = false,
        audit_log_max_bytes: i64 = 5_000_000,
    }
}

impl Default for MatchConditions {
    fn default() -> Self {
        Self::defaults()
    }
}
impl Default for SauceNaoSettings {
    fn default() -> Self {
        Self::defaults()
    }
}
impl Default for HydrusSettings {
    fn default() -> Self {
        Self::defaults()
    }
}
impl Default for McpSettings {
    fn default() -> Self {
        Self::defaults()
    }
}

section! {
    /// Everything in `config.json` (79 top-level keys, 24 more inside the four nested
    /// objects). Build one with [`Settings::with_defaults`] or [`Settings::load`].
    ///
    /// Credentials are [`Secret`]s, so `{:?}` of a `Settings` shows none of them.
    Settings {
        schema_version: i64 = CURRENT_SCHEMA_VERSION,
        delay_min_seconds: f64 = 45.0,
        search_timeout: f64 = 30.0,
        delay_max_seconds: f64 = 75.0,
        /// `engine_id -> [min_s, max_s]`.
        engine_delays: OrderedMap<Vec<f64>> = OrderedMap::default(),
        hash_source: String = "hydrus".to_owned(),
        thumbnail_source: String = "hydrus".to_owned(),
        lazy_thumbnails: bool = true,
        hash_workers: i64 = 1,
        retrieve_tags_from_booru: bool = true,
        drop_restricted_matches: bool = true,
        rank_matches_by_quality: bool = true,
        drop_dead_matches: bool = true,
        borrow_tags_from_other_matches: bool = true,
        borrow_tags_similarity_slack: f64 = 5.0,
        continue_without_saucenao_on_quota: bool = false,
        retry_failed_searches: bool = true,
        remove_after_import: bool = false,
        restore_session_on_start: bool = true,
        autosave_session: bool = true,
        autosave_interval_seconds: i64 = 120,
        search_cache_ttl_days: f64 = 30.0,
        use_search_cache: bool = true,
        sankaku_cookies: Secret = Secret::default(),
        animepictures_cookies: Secret = Secret::default(),
        deviantart_cookies: Secret = Secret::default(),
        e621_username: Secret = Secret::default(),
        e621_api_key: Secret = Secret::default(),
        pixiv_session_cookie: Secret = Secret::default(),
        enable_tag_namespace_remap: bool = false,
        tag_namespace_remap: OrderedMap<String> = pairs(&[("artist", "creator")]),
        enable_tag_blacklist: bool = false,
        tag_blacklist: Vec<String> = default_tag_blacklist(),
        add_rating_tag: bool = false,
        rating_tag_namespace: String = "rating".to_owned(),
        auto_import_enabled: bool = false,
        auto_import_min_similarity: f64 = 90.0,
        auto_import_method: String = "upload".to_owned(),
        url_import_confirm_timeout: f64 = 60.0,
        url_import_confirm_interval: f64 = 2.0,
        set_hydrus_duplicate_relationships: bool = false,
        write_hydrus_provenance_note: bool = false,
        hydrus_provenance_note_name: String = String::new(),
        enable_hydrus_tag_colors: bool = false,
        tag_namespace_colors: OrderedMap<String> = OrderedMap::default(),
        log_matched_urls: bool = true,
        /// Absolute path frozen at first save; never recomputed for an existing config (rule 6.1.9).
        log_file_path: String = String::new(),
        enabled_tag_sources: Vec<String> = strings(&["User", "Hydrus", "Booru", "Hatate-linux"]),
        filter_sent_tags_by_source: bool = false,
        sidecar_filename_style: String = "append".to_owned(),
        sidecar_overwrite: String = "ask".to_owned(),
        tags_for_found: Vec<String> = Vec::new(),
        tags_for_not_found: Vec<String> = Vec::new(),
        tags_for_low_tag_count: Vec<String> = Vec::new(),
        enabled_sites: Vec<String> = default_enabled_sites(),
        primary_engine: String = "iqdb".to_owned(),
        secondary_engine_mode: String = "always".to_owned(),
        fallback_below_similarity: f64 = 0.0,
        extras_only_as_fallback: bool = false,
        enable_ascii2d: bool = true,
        enable_tracemoe: bool = false,
        enable_iqdb3d: bool = false,
        enable_google_images: bool = false,
        enable_google_lens: bool = false,
        enable_yandex: bool = false,
        enable_pawchive: bool = false,
        enable_pawchive_index: bool = true,
        lens_search_whole_image: bool = true,
        google_images_api_key: Secret = Secret::default(),
        tracemoe_min_similarity: f64 = 85.0,
        saucenao: SauceNaoSettings = SauceNaoSettings::defaults(),
        hydrus: HydrusSettings = HydrusSettings::defaults(),
        match_conditions: MatchConditions = MatchConditions::defaults(),
        mcp: McpSettings = McpSettings::defaults(),
        /// `action_id -> key sequence`, only bindings that differ from the default.
        review_shortcuts: OrderedMap<String> = OrderedMap::default(),
        theme: String = "system".to_owned(),
        /// Declared and never used by the Python app. Kept, always `""` by default (spec 2.6).
        window_geometry: String = String::new(),
        /// Qt header state (base64). The Rust GUI cannot read it and must keep it verbatim (rule 6.1.6).
        table_header_state: String = String::new(),
        table_header_columns: i64 = 0,
    }
}

impl Settings {
    /// A fresh config, as the Python app builds it: the defaults with `log_file_path`
    /// pointing into `paths`' `CONFIG_DIR` (`core/config.py:394`).
    pub fn with_defaults(paths: &Paths) -> Self {
        let mut s = Self::defaults();
        s.log_file_path = paths
            .default_matched_urls_log()
            .to_string_lossy()
            .into_owned();
        s
    }

    /// The bytes `config.json` gets: `json.dumps(asdict(settings), indent=2)` (spec 2.1).
    pub fn to_json_string(&self) -> String {
        let mut out = String::with_capacity(4096);
        self.write_object(&mut out, 0);
        out
    }
}

/// Count of keys in the file: top level, then nested (spec 2.5: 79 + 24 = 103).
pub fn key_counts() -> (usize, usize) {
    let nested = SauceNaoSettings::KEYS.len()
        + HydrusSettings::KEYS.len()
        + MatchConditions::KEYS.len()
        + McpSettings::KEYS.len();
    (Settings::KEYS.len(), nested)
}
