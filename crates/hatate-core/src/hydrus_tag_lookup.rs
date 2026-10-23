//! `core/hydrus_tag_lookup.py` and the logic of `workers/hydrus_lookup_worker.py` (without Qt):
//! auto-importing the tags Hydrus already has for a file, keyed by the file's own SHA-256, the
//! hash Hydrus uses to identify files. `hash_file` came with Phase 3.h (DAN-2064); the rest is
//! Phase 4.a (DAN-2126).

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};
use tracing::{debug, info, warn};

use crate::config::Settings;
use crate::hydrus_client::{HydrusClient, HydrusError};
use crate::model::{Entry, Tag, TagSource};
use crate::tag_rules::apply_namespace_remap;

/// `TagLookupResult`: how many entries received tags, and an error string when the lookup itself
/// failed. A `0` with no error means "Hydrus had nothing for these hashes", which is not a
/// failure and must stay distinguishable from one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagLookupResult {
    pub tagged_count: usize,
    pub error: Option<String>,
}

impl TagLookupResult {
    fn ok(tagged_count: usize) -> Self {
        TagLookupResult {
            tagged_count,
            error: None,
        }
    }

    fn failed(error: String) -> Self {
        TagLookupResult {
            tagged_count: 0,
            error: Some(error),
        }
    }
}

/// `HYDRUS_FILENAME_RE`: `^([0-9a-f]{64})(\.[A-Za-z0-9]+)?$`. Python's `$` also matches before a
/// final newline, so the pattern allows one.
static HYDRUS_FILENAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([0-9a-f]{64})(\.[A-Za-z0-9]+)?\n?$").expect("hydrus filename pattern")
});

/// `os.path.basename`: everything after the last `/`.
fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `hash_from_filename`: the SHA-256 taken from the FILENAME, for files in Hydrus's own file
/// store (which names every file after its hash). `""` when the name is not a 64-hex-character
/// SHA-256. The name is trusted to match the contents here, so a caller must treat this as an
/// optimisation to be verified ([`verify_filename_hashes`]), not a fact.
pub fn hash_from_filename(path: &str) -> String {
    HYDRUS_FILENAME_RE
        .captures(basename(path))
        .map(|m| m[1].to_owned())
        .unwrap_or_default()
}

/// `sample_size=3`, the default of `verify_filename_hashes`.
pub const FILENAME_HASH_SAMPLE_SIZE: usize = 3;

/// `verify_filename_hashes(paths)`: reads a few of the given files and checks their real hash
/// against the one in the filename, so a batch of merely hash-shaped names is not trusted.
pub fn verify_filename_hashes(paths: &[String]) -> bool {
    verify_filename_hashes_sampled(paths, FILENAME_HASH_SAMPLE_SIZE)
}

/// `verify_filename_hashes(paths, sample_size)`.
pub fn verify_filename_hashes_sampled(paths: &[String], sample_size: usize) -> bool {
    let candidates: Vec<&String> = paths
        .iter()
        .filter(|p| !hash_from_filename(p).is_empty())
        .collect();
    if candidates.is_empty() {
        return false;
    }
    let step = (candidates.len() / sample_size.max(1)).max(1);
    let sample: Vec<&&String> = candidates.iter().step_by(step).take(sample_size).collect();
    for path in &sample {
        let claimed = hash_from_filename(path);
        let actual = hash_file(Path::new(path.as_str()));
        if actual.is_empty() {
            warn!("Could not verify {path} - unreadable; not trusting filename hashes");
            return false;
        }
        if actual != claimed {
            warn!(
                "{} is named like a SHA256 but its contents hash to {}\u{2026} - filenames are NOT content hashes here, hashing everything properly",
                basename(path),
                actual.chars().take(12).collect::<String>()
            );
            return false;
        }
    }
    info!(
        "Verified {} sampled file(s): filenames match their contents, so the rest of the batch can take its hashes from the names without reading them",
        sample.len()
    );
    true
}

/// `hash_file`: the lowercase hex SHA-256 of a file's raw bytes, which is how Hydrus identifies
/// files. `""` when the file cannot be read (Python logs the `OSError` and returns it).
pub fn hash_file(path: &Path) -> String {
    let read = || -> std::io::Result<String> {
        let mut file = File::open(path)?;
        let mut digest = Sha256::new();
        let mut chunk = vec![0u8; 1024 * 1024];
        loop {
            let n = file.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            digest.update(&chunk[..n]);
        }
        Ok(digest
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    };
    match read() {
        Ok(hex) => hex,
        Err(e) => {
            warn!("Could not hash {}: {e}", path.display());
            String::new()
        }
    }
}

/// `parse_tag`: `namespace:name` split at the first colon.
pub fn parse_tag(raw: &str, source: TagSource) -> Tag {
    match raw.split_once(':') {
        Some((namespace, name)) => Tag {
            name: name.to_owned(),
            namespace: Some(namespace.to_owned()),
            source,
        },
        None => Tag {
            name: raw.to_owned(),
            namespace: None,
            source,
        },
    }
}

/// `_size_matches`: whether the local file is the size Hydrus recorded for the hash it was looked
/// up by. Unknown sizes pass: older Hydrus versions may not report one, and refusing to import
/// tags over a missing field would be worse than the risk it guards against.
fn size_matches(path: &str, hydrus_size: Option<i64>) -> bool {
    let Some(hydrus_size) = hydrus_size.filter(|s| *s != 0) else {
        return true;
    };
    let Ok(meta) = std::fs::metadata(path) else {
        return true; // can't check; don't punish the file for that
    };
    let local_size = meta.len();
    if i64::try_from(local_size).is_ok_and(|n| n == hydrus_size) {
        return true;
    }
    warn!(
        "{} is {local_size} bytes but Hydrus says that hash is {hydrus_size} bytes - the hash doesn't match this file, so its Hydrus tags were NOT imported. If this file came from a batch using filename-derived hashes, turn that off in Settings > General and re-add it.",
        basename(path)
    );
    false
}

/// `apply_existing_hydrus_tags`: for each entry, hashes its local file and checks whether Hydrus
/// already has tags for that exact file; if so, imports them as `TagSource::Hydrus` tags with the
/// configured namespace remap applied, as freshly searched tags get. Mutates the entries in
/// place. Returns how many entries received tags, plus an error string when the lookup itself
/// failed, so a caller can tell that from Hydrus not recognising any hash. No-ops when no access
/// key is set.
///
/// `Err` is what Python lets escape as a raw exception: an answer shaped in a way the client does
/// not read ([`crate::error::ErrorKind::UnexpectedResponse`]). [`run_tag_lookup`] turns it into a
/// result, as the worker does.
pub async fn apply_existing_hydrus_tags(
    entries: &mut [Entry],
    settings: &Settings,
) -> Result<TagLookupResult, HydrusError> {
    if !settings.hydrus.is_configured() {
        debug!(
            "No Hydrus access key configured, skipping tag lookup for {} file(s)",
            entries.len()
        );
        return Ok(TagLookupResult::ok(0));
    }

    // Hash off the async threads: a batch of large files takes a while to read.
    let unhashed: Vec<(usize, String)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.hydrus_hash.as_deref().is_none_or(str::is_empty))
        .map(|(i, e)| (i, e.path.clone()))
        .collect();
    let hashed = tokio::task::spawn_blocking(move || {
        unhashed
            .into_iter()
            .map(|(i, path)| (i, hash_file(Path::new(&path))))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let mut by_index: Vec<Option<String>> = vec![None; entries.len()];
    for (i, hash) in hashed {
        by_index[i] = Some(hash);
    }

    let mut hash_to_entries: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, entry) in entries.iter_mut().enumerate() {
        let file_hash = match entry.hydrus_hash.as_deref() {
            Some(h) if !h.is_empty() => h.to_owned(),
            _ => by_index[i].take().unwrap_or_default(),
        };
        if file_hash.is_empty() {
            continue;
        }
        entry.hydrus_hash = Some(file_hash.clone());
        match hash_to_entries.iter_mut().find(|(h, _)| *h == file_hash) {
            Some((_, group)) => group.push(i),
            None => hash_to_entries.push((file_hash, vec![i])),
        }
    }

    debug!(
        "Hashed {}/{} file(s) into {} unique hash(es)",
        hash_to_entries.iter().map(|(_, g)| g.len()).sum::<usize>(),
        entries.len(),
        hash_to_entries.len()
    );

    if hash_to_entries.is_empty() {
        return Ok(TagLookupResult::ok(0));
    }

    let hashes: Vec<String> = hash_to_entries.iter().map(|(h, _)| h.clone()).collect();
    let metadata_by_hash = match HydrusClient::new(&settings.hydrus) {
        Ok(client) => client.get_tags_and_sizes_for_hashes(&hashes).await,
        Err(e) => Err(e),
    };
    let metadata_by_hash = match metadata_by_hash {
        Ok(found) => found,
        Err(e) if e.kind() != crate::error::ErrorKind::UnexpectedResponse => {
            warn!("Hydrus tag lookup failed: {e}");
            return Ok(TagLookupResult::failed(e.message().to_owned()));
        }
        Err(e) => return Err(e),
    };

    debug!(
        "Hydrus recognized {}/{} of those hashes",
        metadata_by_hash.len(),
        hash_to_entries.len()
    );

    let mut tagged_count = 0;
    for (file_hash, info) in metadata_by_hash.iter() {
        if info.tags.is_empty() {
            continue;
        }
        let parsed: Vec<Tag> = info
            .tags
            .iter()
            .map(|name| parse_tag(name, TagSource::Hydrus))
            .collect();
        let tag_objs = apply_namespace_remap(&parsed, settings);
        let group = hash_to_entries
            .iter()
            .find(|(h, _)| h == file_hash)
            .map(|(_, g)| g.as_slice())
            .unwrap_or_default();
        for &i in group {
            let entry = &mut entries[i];
            // For a hash computed from the file's contents the sizes always agree, so a
            // mismatch means the hash came from somewhere else (the filename shortcut on a file
            // whose name lies). Importing would put another file's tags on this one, silently
            // and permanently, so it is skipped and reported instead.
            if !size_matches(&entry.path, info.size) {
                continue;
            }
            entry.add_tags(tag_objs.clone(), Some(TagSource::Hydrus));
            tagged_count += 1;
        }
    }

    info!(
        "Auto-imported existing Hydrus tags for {tagged_count}/{} file(s)",
        entries.len()
    );
    Ok(TagLookupResult::ok(tagged_count))
}

/// `HydrusTagLookupWorker.run`, without Qt: hashes newly added files and imports any tags
/// Hydrus already has for them. An exception out of the lookup becomes a result carrying its
/// text. The caller runs this off the UI thread and delivers the result (`done.emit`).
pub async fn run_tag_lookup(entries: &mut [Entry], settings: &Settings) -> TagLookupResult {
    debug!(
        "Checking Hydrus for existing tags on {} newly-added file(s)",
        entries.len()
    );
    match apply_existing_hydrus_tags(entries, settings).await {
        Ok(result) => result,
        Err(e) => {
            warn!("Unexpected error during Hydrus tag lookup: {e}");
            TagLookupResult::failed(e.message().to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_the_raw_bytes_as_lowercase_hex() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            hash_file(&path),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn an_unreadable_file_or_a_directory_is_an_empty_hash() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(hash_file(&dir.path().join("missing")), "");
        assert_eq!(hash_file(dir.path()), "");
    }

    #[test]
    fn a_file_over_one_chunk_hashes_the_same_as_in_one_piece() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.bin");
        let data: Vec<u8> = (0..2_500_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &data).unwrap();
        let want: String = Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hash_file(&path), want);
    }

    fn name_for(data: &[u8]) -> String {
        Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn a_hydrus_style_name_gives_its_hash_and_an_ordinary_one_gives_nothing() {
        let h = "ab".repeat(32);
        assert_eq!(hash_from_filename(&format!("/x/fa1/{h}.jpg")), h);
        assert_eq!(hash_from_filename(&h), h);
        assert_eq!(
            hash_from_filename(&format!("{h}.jpg\n")),
            h,
            "Python's `$` allows a final newline"
        );
        for name in ["my_picture", "abc123", "IMG_0042", "/x/dir.jpg/", ""] {
            assert_eq!(hash_from_filename(name), "", "{name}");
        }
        assert_eq!(hash_from_filename(&format!("{}.jpg", h.to_uppercase())), "");
        assert_eq!(hash_from_filename(&format!("{h}.")), "");
        assert_eq!(hash_from_filename(&format!("{h}.tar.gz")), "");
        assert_eq!(hash_from_filename(&format!("a{h}")), "");
    }

    #[test]
    fn the_derived_hash_equals_the_real_one() {
        let dir = tempfile::tempdir().unwrap();
        let name = name_for(b"some real bytes here");
        let path = dir.path().join(format!("{name}.jpg"));
        std::fs::write(&path, b"some real bytes here").unwrap();
        assert_eq!(hash_from_filename(path.to_str().unwrap()), hash_file(&path));
    }

    #[test]
    fn verification_passes_on_a_genuine_store_and_rejects_names_that_lie() {
        let dir = tempfile::tempdir().unwrap();
        let mut real = Vec::new();
        for i in 0..10 {
            let data = format!("file {i}");
            let name = name_for(data.as_bytes());
            let path = dir.path().join(format!("{name}.jpg"));
            std::fs::write(&path, data).unwrap();
            real.push(path.to_str().unwrap().to_owned());
        }
        assert!(verify_filename_hashes(&real));
        let mut liars = Vec::new();
        for i in 0..10 {
            let name = name_for(format!("name {i}").as_bytes());
            let path = dir.path().join(format!("{name}.png"));
            std::fs::write(&path, format!("entirely different content {i}")).unwrap();
            liars.push(path.to_str().unwrap().to_owned());
        }
        assert!(!verify_filename_hashes(&liars));
    }

    #[test]
    fn verification_declines_when_there_is_nothing_to_verify_or_a_file_is_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let ordinary: Vec<String> = (0..3)
            .map(|i| {
                dir.path()
                    .join(format!("ordinary{i}.jpg"))
                    .to_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert!(!verify_filename_hashes(&ordinary));
        assert!(!verify_filename_hashes(&[]));
        let missing = vec![
            dir.path()
                .join(format!("{}.jpg", "cd".repeat(32)))
                .to_str()
                .unwrap()
                .to_owned(),
        ];
        assert!(!verify_filename_hashes(&missing));
    }

    #[test]
    fn the_sample_is_every_nth_candidate_up_to_the_sample_size() {
        // Ten candidates, sample 3: step 3, so candidates 0, 3 and 6 are read. Only those three
        // are real; the rest lie, and the batch still passes, as Python's does.
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..10 {
            let data = format!("file {i}");
            let name = name_for(data.as_bytes());
            let path = dir.path().join(format!("{name}.jpg"));
            let real = matches!(i, 0 | 3 | 6);
            std::fs::write(&path, if real { data } else { "lie".to_owned() }).unwrap();
            paths.push(path.to_str().unwrap().to_owned());
        }
        assert!(verify_filename_hashes(&paths));
        assert!(!verify_filename_hashes_sampled(&paths, 10));
    }

    #[test]
    fn a_tag_splits_at_the_first_colon_only() {
        let t = parse_tag("character:re:zero", TagSource::Hydrus);
        assert_eq!(
            (t.namespace.as_deref(), t.name.as_str()),
            (Some("character"), "re:zero")
        );
        let t = parse_tag("1girl", TagSource::Hydrus);
        assert_eq!((t.namespace, t.name.as_str()), (None, "1girl"));
        let t = parse_tag(":x", TagSource::Hydrus);
        assert_eq!((t.namespace.as_deref(), t.name.as_str()), (Some(""), "x"));
    }

    #[test]
    fn a_size_that_is_unknown_zero_or_unreadable_passes_and_a_different_one_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.jpg");
        std::fs::write(&path, vec![b'x'; 1000]).unwrap();
        let p = path.to_str().unwrap();
        assert!(size_matches(p, Some(1000)));
        assert!(!size_matches(p, Some(9999)));
        assert!(size_matches(p, None));
        assert!(size_matches(p, Some(0)));
        assert!(size_matches("/no/such/file", Some(5)));
    }
}
