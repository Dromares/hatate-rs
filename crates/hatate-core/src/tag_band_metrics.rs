//! Counting how many matches are thin, and whether anything could lend (`core/tag_band_metrics.py`).
//!
//! Read-and-log only: nothing here changes what the app does with tags (DAN-55). Two numbers are
//! recorded per entry and kept apart: `len(entry.tags)`, the union across every tag source, which
//! is what the status decision compares to `min_tags_for_good`; and `len(chosen.booru_tags)`, only
//! the tags the chosen post itself supplied. The lender side is measured through the real
//! [`candidates_that_may_lend`], so the eligible-lender rate is the rate under today's gate.
//!
//! **The file this writes is Python's.** [`Recorder`] appends one JSON object per line, written
//! the way `json.dumps(asdict(m), sort_keys=True)` writes it (sorted keys, `", "` and `": "`
//! separators, `ensure_ascii`, floats in `repr` form), and `tools/measure_tag_bands.py --jsonl`
//! reads it unchanged. The differential reads the same rows through that tool, used as a black box.
//!
//! Where it differs from Python: [`load_measurements`] reads a row whose field has the wrong JSON
//! type (a string for a count, say) as a damaged row and skips it with a warning, where Python's
//! dataclass stores whatever it was given. Rows this module and Python write never have one.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use serde_json::Value;
use tracing::{info, warn};

use crate::config::pyjson;
use crate::model::Candidate;
use crate::tag_borrowing::candidates_that_may_lend;

/// Set to a file path to turn live recording on. Absent, which is every normal run, and nothing in
/// this module is reached at all.
pub const ENV_OUTPUT_PATH: &str = "HATATE_TAG_BAND_METRICS";

pub const BUCKET_TAGLESS: &str = "0";
pub const BUCKET_UNDER_TAGGED: &str = "1..min-1";
pub const BUCKET_WELL_TAGGED: &str = ">=min";

pub const BUCKET_ORDER: [&str; 3] = [BUCKET_TAGLESS, BUCKET_UNDER_TAGGED, BUCKET_WELL_TAGGED];

/// `bucket_for`: which band a tag count falls in. The boundary is `min_tags_for_good`, not a
/// hard-coded five, because it is configurable.
pub fn bucket_for(tag_count: i64, min_tags_for_good: i64) -> &'static str {
    if tag_count <= 0 {
        return BUCKET_TAGLESS;
    }
    if tag_count < min_tags_for_good {
        return BUCKET_UNDER_TAGGED;
    }
    BUCKET_WELL_TAGGED
}

/// `EntryMeasurement`: one searched entry's tag counts and lender situation. Counts and engine and
/// site names only, so a recorded run carries nobody's library with it.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryMeasurement {
    pub entry_tag_count: i64,
    pub chosen_booru_tag_count: i64,
    pub chosen_engine_tag_count: i64,
    pub entry_bucket: String,
    pub chosen_booru_bucket: String,
    pub chosen_engine: Option<String>,
    pub chosen_site: Option<String>,
    pub chosen_similarity: Option<f64>,
    pub candidate_count: i64,
    pub eligible_lender_count: i64,
    pub best_lender_tag_count: i64,
    pub best_lender_engine: Option<String>,
    pub best_lender_site: Option<String>,
    pub best_lender_similarity: Option<f64>,
    pub lenders_with_tags: i64,
    pub best_tagged_lender_tag_count: i64,
    pub best_tagged_lender_engine: Option<String>,
    pub best_tagged_lender_site: Option<String>,
    pub tagged_lender_cross_engine: Option<bool>,
    pub already_borrowed: bool,
    pub all_matches_dead: bool,
    pub liveness_known: bool,
}

impl Default for EntryMeasurement {
    fn default() -> Self {
        EntryMeasurement {
            entry_tag_count: 0,
            chosen_booru_tag_count: 0,
            chosen_engine_tag_count: 0,
            entry_bucket: BUCKET_TAGLESS.to_owned(),
            chosen_booru_bucket: BUCKET_TAGLESS.to_owned(),
            chosen_engine: None,
            chosen_site: None,
            chosen_similarity: None,
            candidate_count: 0,
            eligible_lender_count: 0,
            best_lender_tag_count: 0,
            best_lender_engine: None,
            best_lender_site: None,
            best_lender_similarity: None,
            lenders_with_tags: 0,
            best_tagged_lender_tag_count: 0,
            best_tagged_lender_engine: None,
            best_tagged_lender_site: None,
            tagged_lender_cross_engine: None,
            already_borrowed: false,
            all_matches_dead: false,
            liveness_known: true,
        }
    }
}

/// `str(candidate.source_name)` when it is truthy.
fn site_of(candidate: &Candidate) -> Option<String> {
    candidate
        .source_name
        .clone()
        .filter(|name| !name.is_empty())
}

fn tag_count(candidate: &Candidate) -> i64 {
    candidate.booru_tags.len() as i64
}

/// `measure_entry`: measures one entry. Reads only; mutates nothing it is given.
pub fn measure_entry(
    entry_tag_count: i64,
    candidates: &[Candidate],
    chosen_index: usize,
    min_tags_for_good: i64,
    slack: f64,
    liveness_known: bool,
) -> EntryMeasurement {
    let mut m = EntryMeasurement {
        entry_tag_count,
        entry_bucket: bucket_for(entry_tag_count, min_tags_for_good).to_owned(),
        candidate_count: candidates.len() as i64,
        liveness_known,
        ..EntryMeasurement::default()
    };
    let Some(chosen) = candidates.get(chosen_index) else {
        m.all_matches_dead = candidates.is_empty();
        m.chosen_booru_bucket = bucket_for(0, min_tags_for_good).to_owned();
        return m;
    };

    m.chosen_booru_tag_count = tag_count(chosen);
    m.chosen_engine_tag_count = chosen.engine_tags.len() as i64;
    m.chosen_booru_bucket = bucket_for(m.chosen_booru_tag_count, min_tags_for_good).to_owned();
    m.chosen_engine = Some(chosen.engine.clone());
    m.chosen_site = site_of(chosen);
    m.chosen_similarity = Some(chosen.similarity);
    m.already_borrowed = chosen
        .tags_borrowed_from
        .as_deref()
        .is_some_and(|url| !url.is_empty());

    // The real gate, not a copy of it: whatever bounds borrowing today bounds this number too.
    let lenders = candidates_that_may_lend(candidates, chosen_index, slack);
    m.eligible_lender_count = lenders.len() as i64;
    let Some(&best_index) = lenders.first() else {
        return m;
    };

    let best = &candidates[best_index];
    m.best_lender_tag_count = tag_count(best);
    m.best_lender_engine = Some(best.engine.clone());
    m.best_lender_site = site_of(best);
    m.best_lender_similarity = Some(best.similarity);

    let tagged: Vec<&Candidate> = lenders
        .iter()
        .map(|&i| &candidates[i])
        .filter(|c| tag_count(c) > 0)
        .collect();
    m.lenders_with_tags = tagged.len() as i64;
    // `max(tagged, key=_tag_count)`: the first of equals.
    let mut richest: Option<&Candidate> = None;
    for c in &tagged {
        if richest.is_none_or(|r| tag_count(c) > tag_count(r)) {
            richest = Some(c);
        }
    }
    if let Some(richest) = richest {
        m.best_tagged_lender_tag_count = tag_count(richest);
        m.best_tagged_lender_engine = Some(richest.engine.clone());
        m.best_tagged_lender_site = site_of(richest);
        m.tagged_lender_cross_engine = Some(richest.engine != chosen.engine);
    }
    m
}

/// `BucketSummary`: the aggregate for one band. The maps keep their keys sorted, which is the
/// order `asdict` plus `sort_keys` writes them in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BucketSummary {
    pub bucket: String,
    pub entries: i64,
    pub with_eligible_lender: i64,
    pub with_tagged_lender: i64,
    pub cross_engine_lenders: i64,
    pub same_engine_lenders: i64,
    /// Sum of the richest tagged lender's tag count, over the entries that have one.
    pub lender_tag_total: i64,
    pub lender_tag_max: i64,
    pub already_borrowed: i64,
    pub chosen_sites: BTreeMap<String, i64>,
    pub eligible_lender_sites: BTreeMap<String, i64>,
    pub lender_sites: BTreeMap<String, i64>,
}

impl BucketSummary {
    fn new(bucket: &str) -> Self {
        BucketSummary {
            bucket: bucket.to_owned(),
            ..BucketSummary::default()
        }
    }

    pub fn eligible_lender_rate(&self) -> f64 {
        if self.entries != 0 {
            self.with_eligible_lender as f64 / self.entries as f64
        } else {
            0.0
        }
    }

    pub fn tagged_lender_rate(&self) -> f64 {
        if self.entries != 0 {
            self.with_tagged_lender as f64 / self.entries as f64
        } else {
            0.0
        }
    }

    pub fn mean_lender_tags(&self) -> f64 {
        let n = self.with_tagged_lender;
        if n != 0 {
            self.lender_tag_total as f64 / n as f64
        } else {
            0.0
        }
    }
}

fn accumulate(summary: &mut BucketSummary, m: &EntryMeasurement) {
    summary.entries += 1;
    if m.already_borrowed {
        summary.already_borrowed += 1;
    }
    if let Some(site) = m.chosen_site.as_deref().filter(|s| !s.is_empty()) {
        *summary.chosen_sites.entry(site.to_owned()).or_insert(0) += 1;
    }
    if m.eligible_lender_count != 0 {
        summary.with_eligible_lender += 1;
        if let Some(site) = m.best_lender_site.as_deref().filter(|s| !s.is_empty()) {
            *summary
                .eligible_lender_sites
                .entry(site.to_owned())
                .or_insert(0) += 1;
        }
    }
    if m.best_tagged_lender_tag_count > 0 {
        summary.with_tagged_lender += 1;
        summary.lender_tag_total += m.best_tagged_lender_tag_count;
        summary.lender_tag_max = summary.lender_tag_max.max(m.best_tagged_lender_tag_count);
        if m.tagged_lender_cross_engine == Some(true) {
            summary.cross_engine_lenders += 1;
        } else {
            summary.same_engine_lenders += 1;
        }
        if let Some(site) = m
            .best_tagged_lender_site
            .as_deref()
            .filter(|s| !s.is_empty())
        {
            *summary.lender_sites.entry(site.to_owned()).or_insert(0) += 1;
        }
    }
}

/// How `summarize` buckets: on `len(entry.tags)` or on `len(chosen.booru_tags)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucketing {
    /// `by="entry"`: the number the status logic uses, so "how many rows look thin to the user".
    Entry,
    /// `by="chosen_booru"`: "how many posts are thin at the source".
    ChosenBooru,
}

impl Bucketing {
    /// The word `summarize(by=...)` takes. `Err` carries Python's `ValueError` text.
    pub fn parse(by: &str) -> Result<Bucketing, String> {
        match by {
            "entry" => Ok(Bucketing::Entry),
            "chosen_booru" => Ok(Bucketing::ChosenBooru),
            other => Err(format!(
                "unknown bucketing: {}",
                crate::pyrepr::py_repr(&Value::String(other.to_owned()))
            )),
        }
    }
}

/// `summarize`: aggregates per band, in [`BUCKET_ORDER`].
pub fn summarize(measurements: &[EntryMeasurement], by: Bucketing) -> Vec<BucketSummary> {
    let mut out: Vec<BucketSummary> = BUCKET_ORDER.iter().map(|b| BucketSummary::new(b)).collect();
    for m in measurements {
        let key = match by {
            Bucketing::Entry => &m.entry_bucket,
            Bucketing::ChosenBooru => &m.chosen_booru_bucket,
        };
        if let Some(summary) = out.iter_mut().find(|s| &s.bucket == key) {
            accumulate(summary, m);
        }
        // Python indexes a dict by the bucket name and so raises `KeyError` on a name outside the
        // three; measurements this crate makes always carry one of them.
    }
    out
}

/// `format_table`: the summary as a markdown table.
pub fn format_table(summary: &[BucketSummary], title: &str) -> String {
    let mut lines = vec![
        format!("### {title}"),
        String::new(),
        "| band | entries | eligible lender | with tagged lender | mean lender tags | max | \
         cross-engine | same-engine | already borrowed |"
            .to_owned(),
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- |".to_owned(),
    ];
    for name in BUCKET_ORDER {
        let Some(s) = summary.iter().find(|s| s.bucket == name) else {
            continue;
        };
        lines.push(format!(
            "| `{name}` | {} | {} ({}%) | {} ({}%) | {} | {} | {} | {} | {} |",
            s.entries,
            s.with_eligible_lender,
            fixed1(s.eligible_lender_rate() * 100.0),
            s.with_tagged_lender,
            fixed1(s.tagged_lender_rate() * 100.0),
            fixed1(s.mean_lender_tags()),
            s.lender_tag_max,
            s.cross_engine_lenders,
            s.same_engine_lenders,
            s.already_borrowed,
        ));
    }
    lines.join("\n")
}

/// Python's `f"{x:.1f}"`.
fn fixed1(x: f64) -> String {
    format!("{x:.1}")
}

/// `summary_to_dict`: JSON-ready, with the derived rates spelled out, keyed by band.
pub fn summary_to_value(summary: &[BucketSummary]) -> Value {
    let map = |m: &BTreeMap<String, i64>| -> Value {
        Value::Object(
            m.iter()
                .map(|(k, v)| (k.clone(), Value::from(*v)))
                .collect(),
        )
    };
    let mut out = serde_json::Map::new();
    for s in summary {
        let mut d = serde_json::Map::new();
        d.insert("bucket".into(), Value::from(s.bucket.clone()));
        d.insert("entries".into(), Value::from(s.entries));
        d.insert(
            "with_eligible_lender".into(),
            Value::from(s.with_eligible_lender),
        );
        d.insert(
            "with_tagged_lender".into(),
            Value::from(s.with_tagged_lender),
        );
        d.insert(
            "cross_engine_lenders".into(),
            Value::from(s.cross_engine_lenders),
        );
        d.insert(
            "same_engine_lenders".into(),
            Value::from(s.same_engine_lenders),
        );
        d.insert("lender_tag_total".into(), Value::from(s.lender_tag_total));
        d.insert("lender_tag_max".into(), Value::from(s.lender_tag_max));
        d.insert("already_borrowed".into(), Value::from(s.already_borrowed));
        d.insert("chosen_sites".into(), map(&s.chosen_sites));
        d.insert(
            "eligible_lender_sites".into(),
            map(&s.eligible_lender_sites),
        );
        d.insert("lender_sites".into(), map(&s.lender_sites));
        d.insert(
            "eligible_lender_rate".into(),
            Value::from(s.eligible_lender_rate()),
        );
        d.insert(
            "tagged_lender_rate".into(),
            Value::from(s.tagged_lender_rate()),
        );
        d.insert("mean_lender_tags".into(), Value::from(s.mean_lender_tags()));
        out.insert(s.bucket.clone(), Value::Object(d));
    }
    Value::Object(out)
}

/// One `key: value` pair of a row, value already in JSON text.
fn text_of_str(s: &str) -> String {
    let mut out = String::new();
    pyjson::string(&mut out, s);
    out
}

fn text_of_opt_str(s: &Option<String>) -> String {
    s.as_deref().map_or_else(|| "null".to_owned(), text_of_str)
}

fn text_of_float(x: f64) -> String {
    let mut out = String::new();
    pyjson::float(&mut out, x);
    out
}

fn text_of_opt_float(x: Option<f64>) -> String {
    x.map_or_else(|| "null".to_owned(), text_of_float)
}

fn text_of_bool(b: bool) -> String {
    if b { "true" } else { "false" }.to_owned()
}

impl EntryMeasurement {
    /// `json.dumps(asdict(m), sort_keys=True)`.
    pub fn to_json_line(&self) -> String {
        let mut fields: Vec<(&str, String)> = vec![
            ("entry_tag_count", self.entry_tag_count.to_string()),
            (
                "chosen_booru_tag_count",
                self.chosen_booru_tag_count.to_string(),
            ),
            (
                "chosen_engine_tag_count",
                self.chosen_engine_tag_count.to_string(),
            ),
            ("entry_bucket", text_of_str(&self.entry_bucket)),
            (
                "chosen_booru_bucket",
                text_of_str(&self.chosen_booru_bucket),
            ),
            ("chosen_engine", text_of_opt_str(&self.chosen_engine)),
            ("chosen_site", text_of_opt_str(&self.chosen_site)),
            (
                "chosen_similarity",
                text_of_opt_float(self.chosen_similarity),
            ),
            ("candidate_count", self.candidate_count.to_string()),
            (
                "eligible_lender_count",
                self.eligible_lender_count.to_string(),
            ),
            (
                "best_lender_tag_count",
                self.best_lender_tag_count.to_string(),
            ),
            (
                "best_lender_engine",
                text_of_opt_str(&self.best_lender_engine),
            ),
            ("best_lender_site", text_of_opt_str(&self.best_lender_site)),
            (
                "best_lender_similarity",
                text_of_opt_float(self.best_lender_similarity),
            ),
            ("lenders_with_tags", self.lenders_with_tags.to_string()),
            (
                "best_tagged_lender_tag_count",
                self.best_tagged_lender_tag_count.to_string(),
            ),
            (
                "best_tagged_lender_engine",
                text_of_opt_str(&self.best_tagged_lender_engine),
            ),
            (
                "best_tagged_lender_site",
                text_of_opt_str(&self.best_tagged_lender_site),
            ),
            (
                "tagged_lender_cross_engine",
                self.tagged_lender_cross_engine
                    .map_or_else(|| "null".to_owned(), text_of_bool),
            ),
            ("already_borrowed", text_of_bool(self.already_borrowed)),
            ("all_matches_dead", text_of_bool(self.all_matches_dead)),
            ("liveness_known", text_of_bool(self.liveness_known)),
        ];
        fields.sort_by(|a, b| a.0.cmp(b.0));
        let body: Vec<String> = fields
            .iter()
            .map(|(k, v)| format!("{}: {v}", text_of_str(k)))
            .collect();
        format!("{{{}}}", body.join(", "))
    }

    /// The dataclass built from a row with `EntryMeasurement(**{k: v for k in known})`: absent
    /// fields keep their defaults and unknown ones are ignored. `None` when a present field has a
    /// JSON type this struct cannot hold (see the module note).
    fn from_row(row: &serde_json::Map<String, Value>) -> Option<EntryMeasurement> {
        let mut m = EntryMeasurement::default();
        let int = |key: &str, into: &mut i64| -> Option<()> {
            match row.get(key) {
                None => Some(()),
                Some(v) => v.as_i64().map(|n| *into = n),
            }
        };
        let text = |key: &str, into: &mut String| -> Option<()> {
            match row.get(key) {
                None => Some(()),
                Some(Value::String(s)) => {
                    *into = s.clone();
                    Some(())
                }
                Some(_) => None,
            }
        };
        let opt_text = |key: &str, into: &mut Option<String>| -> Option<()> {
            match row.get(key) {
                None => Some(()),
                Some(Value::Null) => {
                    *into = None;
                    Some(())
                }
                Some(Value::String(s)) => {
                    *into = Some(s.clone());
                    Some(())
                }
                Some(_) => None,
            }
        };
        let opt_float = |key: &str, into: &mut Option<f64>| -> Option<()> {
            match row.get(key) {
                None => Some(()),
                Some(Value::Null) => {
                    *into = None;
                    Some(())
                }
                Some(v) => v.as_f64().map(|x| *into = Some(x)),
            }
        };
        let flag = |key: &str, into: &mut bool| -> Option<()> {
            match row.get(key) {
                None => Some(()),
                Some(Value::Bool(b)) => {
                    *into = *b;
                    Some(())
                }
                Some(_) => None,
            }
        };
        int("entry_tag_count", &mut m.entry_tag_count)?;
        int("chosen_booru_tag_count", &mut m.chosen_booru_tag_count)?;
        int("chosen_engine_tag_count", &mut m.chosen_engine_tag_count)?;
        text("entry_bucket", &mut m.entry_bucket)?;
        text("chosen_booru_bucket", &mut m.chosen_booru_bucket)?;
        opt_text("chosen_engine", &mut m.chosen_engine)?;
        opt_text("chosen_site", &mut m.chosen_site)?;
        opt_float("chosen_similarity", &mut m.chosen_similarity)?;
        int("candidate_count", &mut m.candidate_count)?;
        int("eligible_lender_count", &mut m.eligible_lender_count)?;
        int("best_lender_tag_count", &mut m.best_lender_tag_count)?;
        opt_text("best_lender_engine", &mut m.best_lender_engine)?;
        opt_text("best_lender_site", &mut m.best_lender_site)?;
        opt_float("best_lender_similarity", &mut m.best_lender_similarity)?;
        int("lenders_with_tags", &mut m.lenders_with_tags)?;
        int(
            "best_tagged_lender_tag_count",
            &mut m.best_tagged_lender_tag_count,
        )?;
        opt_text(
            "best_tagged_lender_engine",
            &mut m.best_tagged_lender_engine,
        )?;
        opt_text("best_tagged_lender_site", &mut m.best_tagged_lender_site)?;
        match row.get("tagged_lender_cross_engine") {
            None => {}
            Some(Value::Null) => m.tagged_lender_cross_engine = None,
            Some(Value::Bool(b)) => m.tagged_lender_cross_engine = Some(*b),
            Some(_) => return None,
        }
        flag("already_borrowed", &mut m.already_borrowed)?;
        flag("all_matches_dead", &mut m.all_matches_dead)?;
        flag("liveness_known", &mut m.liveness_known)?;
        Some(m)
    }
}

/// `Recorder`: appends one JSON object per measured entry. JSONL and append-only on purpose: a
/// library run is long, and a run that dies halfway leaves the rows it already produced.
#[derive(Debug)]
pub struct Recorder {
    pub path: PathBuf,
    written: std::sync::atomic::AtomicUsize,
}

impl Recorder {
    pub fn new(path: impl Into<PathBuf>) -> Recorder {
        Recorder {
            path: path.into(),
            written: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Rows written so far (`Recorder.written`).
    pub fn written(&self) -> usize {
        self.written.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// `Recorder.record`. A file that cannot be written is a warning, never an error:
    /// instrumentation must not take a search down with it.
    pub fn record(&self, measurement: &EntryMeasurement) {
        let line = format!("{}\n", measurement.to_json_line());
        let write = || -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            file.write_all(line.as_bytes())
        };
        match write() {
            Ok(()) => {
                self.written
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            Err(e) => warn!(
                "could not write tag-band measurement to {}: {e}",
                self.path.display()
            ),
        }
    }

    /// `Recorder.from_env`: a recorder when the variable names a file, otherwise `None`. `lookup`
    /// reads the variable (the process environment, normally).
    pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Option<Recorder> {
        let value = lookup(ENV_OUTPUT_PATH)?;
        let path = crate::pycompat::strip(&value);
        if path.is_empty() {
            return None;
        }
        info!("recording tag-band measurements to {path}");
        Some(Recorder::new(path))
    }
}

/// `load_measurements`: reads a JSONL file back, skipping any row a killed run truncated. A file
/// that cannot be opened is an `Err` (Python raises `OSError`).
pub fn load_measurements(path: &std::path::Path) -> std::io::Result<Vec<EntryMeasurement>> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    for line in text.lines() {
        let line = crate::pycompat::strip(line);
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(row)) => match EntryMeasurement::from_row(&row) {
                Some(m) => out.push(m),
                None => warn!("skipping unreadable measurement row in {}", path.display()),
            },
            _ => warn!("skipping unreadable measurement row in {}", path.display()),
        }
    }
    Ok(out)
}
