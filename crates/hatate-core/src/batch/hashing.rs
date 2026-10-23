//! The file-hash pass (`workers/file_hash_worker.py` without Qt, and `hash_from_filename` /
//! `verify_filename_hashes` from `core/hydrus_tag_lookup.py`).
//!
//! Hashing is the dominant cost of adding a large batch, so it is done once, up front, before any
//! file is judged a duplicate. A batch added straight out of Hydrus's own file store already
//! carries every hash in its file names; [`split_by_filename_hash`] takes those hashes from the
//! names, but only after a small sample is read and checked, so a pile of files that merely look
//! hash-shaped is never trusted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use tracing::{info, warn};

use crate::hydrus_tag_lookup::hash_file;

/// Don't offer an estimate until this much of the work is done: the first couple of files are a
/// poor sample.
pub const MIN_FRACTION_BEFORE_ESTIMATING: f64 = 0.02;
/// ... or until this many seconds have passed.
pub const MIN_SECONDS_BEFORE_ESTIMATING: f64 = 1.5;
/// Progress is reported at most this often (seconds), always including the last update.
pub const PROGRESS_EMIT_INTERVAL_SECONDS: f64 = 0.1;

/// `HYDRUS_FILENAME_RE`: `^([0-9a-f]{64})(\.[A-Za-z0-9]+)?$` on the file's base name. Python's `$`
/// also matches just before one trailing newline, so a name that ends in `\n` matches.
pub fn hash_from_filename(path: &Path) -> String {
    let Some(base) = path.file_name() else {
        return String::new();
    };
    let base = base.to_string_lossy();
    let name = base.strip_suffix('\n').unwrap_or(&base);
    let bytes = name.as_bytes();
    if bytes.len() < 64 || !bytes[..64].iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return String::new();
    }
    let rest = &name[64..];
    let ok = match rest.strip_prefix('.') {
        None => rest.is_empty(),
        Some(ext) => !ext.is_empty() && ext.bytes().all(|b| b.is_ascii_alphanumeric()),
    };
    if ok { name[..64].to_owned() } else { String::new() }
}

/// `verify_filename_hashes`: reads a few of the files named like a hash and checks their real hash
/// against the name. `false` when there are none, when one cannot be read, or when one differs.
pub fn verify_filename_hashes(paths: &[PathBuf], sample_size: usize) -> bool {
    let candidates: Vec<&PathBuf> = paths
        .iter()
        .filter(|p| !hash_from_filename(p).is_empty())
        .collect();
    if candidates.is_empty() {
        return false;
    }
    let step = (candidates.len() / sample_size).max(1);
    for path in candidates.iter().step_by(step).take(sample_size) {
        let claimed = hash_from_filename(path);
        let actual = hash_file(path);
        if actual.is_empty() {
            warn!(
                "Could not verify {} - unreadable; not trusting filename hashes",
                path.display()
            );
            return false;
        }
        if actual != claimed {
            warn!(
                "{} is named like a SHA256 but its contents hash to {}… - filenames are NOT \
                 content hashes here, hashing everything properly",
                path.file_name().unwrap_or_default().to_string_lossy(),
                &actual[..12]
            );
            return false;
        }
    }
    info!(
        "Verified {} sampled file(s): filenames match their contents, so the rest of the batch \
         can take its hashes from the names without reading them",
        candidates.len().min(sample_size)
    );
    true
}

/// Where a file's SHA-256 comes from (`Settings.hash_source`). Anything but `"hydrus"` reads every
/// file, which is the slower but never-wrong option.
pub fn uses_filename_hashes(hash_source: &str) -> bool {
    hash_source == "hydrus"
}

/// `_split_by_filename_hash`: the hashes taken from names, and the files still to read.
pub fn split_by_filename_hash(
    paths: &[PathBuf],
    hash_source: &str,
) -> (HashMap<PathBuf, String>, Vec<PathBuf>) {
    if !uses_filename_hashes(hash_source) {
        tracing::debug!("hash_source={hash_source:?} - reading every file rather than using filenames");
        return (HashMap::new(), paths.to_vec());
    }
    let candidates: Vec<PathBuf> = paths
        .iter()
        .filter(|p| !hash_from_filename(p).is_empty())
        .cloned()
        .collect();
    if candidates.is_empty() {
        return (HashMap::new(), paths.to_vec());
    }
    info!(
        "{} of {} file(s) are named like a SHA256 - verifying a sample before trusting them",
        candidates.len(),
        paths.len()
    );
    if !verify_filename_hashes(&candidates, 3) {
        return (HashMap::new(), paths.to_vec());
    }
    let from_name: HashMap<PathBuf, String> = candidates
        .iter()
        .map(|p| (p.clone(), hash_from_filename(p)))
        .collect();
    let remaining: Vec<PathBuf> = paths
        .iter()
        .filter(|p| !from_name.contains_key(*p))
        .cloned()
        .collect();
    info!(
        "Taking {} hash(es) from filenames; {} file(s) still to hash",
        from_name.len(),
        remaining.len()
    );
    (from_name, remaining)
}

/// `_estimate_remaining`: seconds left, from BYTES processed rather than file count (file sizes
/// range from well under a megabyte to hundreds). `-1.0` when there is not yet enough data.
pub fn estimate_remaining(total_bytes: u64, bytes_done: u64, elapsed: f64) -> f64 {
    if total_bytes == 0
        || bytes_done == 0
        || elapsed < MIN_SECONDS_BEFORE_ESTIMATING
        || (bytes_done as f64 / total_bytes as f64) < MIN_FRACTION_BEFORE_ESTIMATING
    {
        return -1.0;
    }
    let rate = bytes_done as f64 / elapsed;
    let remaining = total_bytes.saturating_sub(bytes_done) as f64;
    if rate > 0.0 { remaining / rate } else { -1.0 }
}

/// `progress(done, total, eta_seconds)`.
pub type OnHashProgress<'a> = &'a (dyn Fn(usize, usize, f64) + Send + Sync);

/// Hashes `paths` (`FileHashWorker.run`). `workers` is `Settings.hash_workers` (1 means
/// sequential). Returns `{path: hash}` for every file that could be hashed, or `None` when `stop`
/// was raised before the end (the Python worker then emits nothing).
pub fn hash_files(
    paths: &[PathBuf],
    workers: i64,
    hash_source: &str,
    stop: &AtomicBool,
    progress: Option<OnHashProgress<'_>>,
) -> Option<HashMap<PathBuf, String>> {
    let total = paths.len();
    let sizes: HashMap<&Path, u64> = paths
        .iter()
        .map(|p| (p.as_path(), std::fs::metadata(p).map_or(0, |m| m.len())))
        .collect();
    let total_bytes: u64 = sizes.values().sum();
    let workers = workers.max(1) as usize;
    info!(
        "Hashing {total} file(s) ({:.1} MB) using {workers} worker(s)",
        total_bytes as f64 / 1024.0 / 1024.0
    );
    let start = Instant::now();

    let (from_name, to_read) = split_by_filename_hash(paths, hash_source);
    let bytes_to_read: u64 = to_read.iter().map(|p| sizes.get(p.as_path()).copied().unwrap_or(0)).sum();

    let emitter = ProgressEmitter::new(progress);
    let offset = from_name.len();
    let mut result = from_name;
    if !to_read.is_empty() {
        let run = if workers == 1 {
            run_sequential(&to_read, &sizes, offset, total, bytes_to_read, stop, &emitter)
        } else {
            run_parallel(&to_read, &sizes, workers, offset, total, bytes_to_read, stop, &emitter)
        };
        result.extend(run);
        if stop.load(Ordering::SeqCst) {
            return None;
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    let mb = bytes_to_read as f64 / 1024.0 / 1024.0;
    info!(
        "Finished hashing {}/{total} file(s) in {elapsed:.1}s using {workers} worker(s) - read \
         {mb:.1} MB at {:.1} MB/s",
        result.len(),
        if elapsed > 0.0 { mb / elapsed } else { 0.0 }
    );
    Some(result)
}

struct ProgressEmitter<'a> {
    sink: Option<OnHashProgress<'a>>,
    last: Mutex<Option<Instant>>,
}

impl<'a> ProgressEmitter<'a> {
    fn new(sink: Option<OnHashProgress<'a>>) -> Self {
        ProgressEmitter { sink, last: Mutex::new(None) }
    }

    /// `_emit_progress`: throttled, but always emits the final update so the bar cannot be left
    /// short of the end.
    fn emit(&self, done: usize, total: usize, total_bytes: u64, bytes_done: u64, elapsed: f64) {
        let Some(sink) = self.sink else { return };
        let now = Instant::now();
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if done < total
            && last.is_some_and(|t| now.duration_since(t).as_secs_f64() < PROGRESS_EMIT_INTERVAL_SECONDS)
        {
            return;
        }
        *last = Some(now);
        drop(last);
        sink(done, total, estimate_remaining(total_bytes, bytes_done, elapsed));
    }
}

fn run_sequential(
    paths: &[PathBuf],
    sizes: &HashMap<&Path, u64>,
    offset: usize,
    total: usize,
    total_bytes: u64,
    stop: &AtomicBool,
    emitter: &ProgressEmitter<'_>,
) -> HashMap<PathBuf, String> {
    let mut result = HashMap::new();
    let start = Instant::now();
    let mut bytes_done = 0;
    for (i, path) in paths.iter().enumerate() {
        if stop.load(Ordering::SeqCst) {
            info!("File hashing stopped early at {}/{total}", offset + i);
            break;
        }
        bytes_done += sizes.get(path.as_path()).copied().unwrap_or(0);
        let h = hash_file(path);
        if !h.is_empty() {
            result.insert(path.clone(), h);
        }
        emitter.emit(offset + i + 1, total, total_bytes, bytes_done, start.elapsed().as_secs_f64());
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn run_parallel(
    paths: &[PathBuf],
    sizes: &HashMap<&Path, u64>,
    workers: usize,
    offset: usize,
    total: usize,
    total_bytes: u64,
    stop: &AtomicBool,
    emitter: &ProgressEmitter<'_>,
) -> HashMap<PathBuf, String> {
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(offset);
    let bytes_done = std::sync::atomic::AtomicU64::new(0);
    let result = Mutex::new(HashMap::new());
    let start = Instant::now();
    // A bounded pool rather than one thread per file: thousands of concurrent reads help nothing
    // and can exhaust handles.
    std::thread::scope(|scope| {
        for _ in 0..workers.min(paths.len()) {
            scope.spawn(|| {
                loop {
                    if stop.load(Ordering::SeqCst) {
                        info!(
                            "File hashing stopped early at {}/{total}",
                            done.load(Ordering::SeqCst)
                        );
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(path) = paths.get(i) else { return };
                    let h = hash_file(path);
                    if !h.is_empty() {
                        result.lock().unwrap_or_else(|e| e.into_inner()).insert(path.clone(), h);
                    }
                    let size = sizes.get(path.as_path()).copied().unwrap_or(0);
                    let b = bytes_done.fetch_add(size, Ordering::SeqCst) + size;
                    let d = done.fetch_add(1, Ordering::SeqCst) + 1;
                    emitter.emit(d, total, total_bytes, b, start.elapsed().as_secs_f64());
                }
            });
        }
    });
    result.into_inner().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn sha_hex(data: &[u8]) -> String {
        Sha256::digest(data).iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn a_hydrus_style_name_yields_its_hash() {
        let h = "ab".repeat(32);
        assert_eq!(hash_from_filename(Path::new(&format!("/x/fa1/{h}.png"))), h);
        assert_eq!(hash_from_filename(Path::new(&h)), h);
        assert_eq!(hash_from_filename(Path::new(&format!("{h}\n"))), h);
    }

    #[test]
    fn other_names_yield_nothing() {
        let h = "ab".repeat(32);
        for name in [
            "photo.png".to_owned(),
            format!("{}.png", &h[..63]),
            format!("{h}0.png"),
            format!("{}.png", h.to_uppercase()),
            format!("{h}."),
            format!("{h}.tar.gz"),
            format!("{h}.p ng"),
        ] {
            assert_eq!(hash_from_filename(Path::new(&name)), "", "{name}");
        }
    }

    fn named_file(dir: &Path, data: &[u8], claim: &str) -> PathBuf {
        let p = dir.join(format!("{claim}.png"));
        std::fs::write(&p, data).unwrap();
        p
    }

    #[test]
    fn names_that_match_their_contents_are_taken_without_reading_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let a = named_file(dir.path(), b"one", &sha_hex(b"one"));
        let b = named_file(dir.path(), b"two", &sha_hex(b"two"));
        let plain = dir.path().join("plain.png");
        std::fs::write(&plain, b"three").unwrap();
        let paths = vec![a.clone(), b.clone(), plain.clone()];
        let stop = AtomicBool::new(false);
        let got = hash_files(&paths, 1, "hydrus", &stop, None).unwrap();
        assert_eq!(got[&a], sha_hex(b"one"));
        assert_eq!(got[&b], sha_hex(b"two"));
        assert_eq!(got[&plain], sha_hex(b"three"));
    }

    #[test]
    fn a_lying_name_is_caught_by_the_sample_and_everything_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let liar = named_file(dir.path(), b"real contents", &"cd".repeat(32));
        let stop = AtomicBool::new(false);
        let got = hash_files(std::slice::from_ref(&liar), 1, "hydrus", &stop, None).unwrap();
        assert_eq!(got[&liar], sha_hex(b"real contents"));
    }

    #[test]
    fn local_hash_source_never_trusts_a_name() {
        let dir = tempfile::tempdir().unwrap();
        let liar = named_file(dir.path(), b"x", &sha_hex(b"not x"));
        let (from_name, to_read) = split_by_filename_hash(std::slice::from_ref(&liar), "local");
        assert!(from_name.is_empty());
        assert_eq!(to_read, [liar]);
    }

    #[test]
    fn an_unreadable_sampled_file_is_not_trusted() {
        let h = sha_hex(b"x");
        let paths = vec![PathBuf::from(format!("/nonexistent/{h}.png"))];
        assert!(!verify_filename_hashes(&paths, 3));
        assert!(!verify_filename_hashes(&[PathBuf::from("/x/plain.png")], 3));
    }

    #[test]
    fn the_sample_is_spread_across_the_candidates() {
        // With 9 candidates and a sample of 3 the step is 3: indexes 0, 3, 6 are read, so a
        // lying file at index 3 is caught and one at index 1 is not.
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..9u8 {
            let data = vec![i; 4];
            paths.push(named_file(dir.path(), &data, &sha_hex(&data)));
        }
        assert!(verify_filename_hashes(&paths, 3));
        std::fs::write(&paths[1], b"tampered").unwrap();
        assert!(verify_filename_hashes(&paths, 3), "index 1 is not sampled");
        std::fs::write(&paths[3], b"tampered").unwrap();
        assert!(!verify_filename_hashes(&paths, 3), "index 3 is sampled");
    }

    #[test]
    fn parallel_and_sequential_agree() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..20u8 {
            let p = dir.path().join(format!("f{i}.png"));
            std::fs::write(&p, vec![i; 100 + i as usize]).unwrap();
            paths.push(p);
        }
        paths.push(dir.path().join("missing.png"));
        let stop = AtomicBool::new(false);
        let seq = hash_files(&paths, 1, "local", &stop, None).unwrap();
        let par = hash_files(&paths, 4, "local", &stop, None).unwrap();
        assert_eq!(seq, par);
        assert_eq!(seq.len(), 20, "the missing file has no hash");
    }

    #[test]
    fn a_raised_stop_flag_gives_no_result() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.png");
        std::fs::write(&p, b"x").unwrap();
        let stop = AtomicBool::new(true);
        assert!(hash_files(std::slice::from_ref(&p), 1, "local", &stop, None).is_none());
        assert!(hash_files(std::slice::from_ref(&p), 3, "local", &stop, None).is_none());
    }

    #[test]
    fn progress_always_reports_the_last_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..5u8 {
            let p = dir.path().join(format!("f{i}.png"));
            std::fs::write(&p, [i]).unwrap();
            paths.push(p);
        }
        let seen = Mutex::new(Vec::new());
        let sink = |done: usize, total: usize, _eta: f64| seen.lock().unwrap().push((done, total));
        let stop = AtomicBool::new(false);
        hash_files(&paths, 1, "local", &stop, Some(&sink)).unwrap();
        assert_eq!(seen.lock().unwrap().last(), Some(&(5, 5)));
    }

    #[test]
    fn the_estimate_waits_for_enough_data() {
        assert_eq!(estimate_remaining(0, 10, 5.0), -1.0);
        assert_eq!(estimate_remaining(1000, 0, 5.0), -1.0);
        assert_eq!(estimate_remaining(1000, 500, 1.0), -1.0, "too early in seconds");
        assert_eq!(estimate_remaining(1000, 10, 5.0), -1.0, "under 2% done");
        assert_eq!(estimate_remaining(1000, 500, 2.0), 2.0);
        assert_eq!(estimate_remaining(1000, 1000, 2.0), 0.0);
    }
}
