//! DAN-2145 (Phase 4.h): `LiveEngines::pawchive_index` over the 4.f index, through the engine
//! runner as `hatate batch` drives it. Every case of `_collect_pawchive_index` is here: index
//! missing, index empty, local file unreadable, matches found, none found, a database that
//! cannot be read; and `enable_pawchive_index` true and false. The Python-vs-Rust collector
//! differential over the same index files is the broad check
//! (`worktrees/DAN-2145-differential.log`). Local only: no request is made, no server is needed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use hatate_core::clock::ManualClock;
use hatate_core::config::Settings;
use hatate_core::engine_backends::LiveEngines;
use hatate_core::engine_runner::{EngineRunner, Engines, IndexAnswer, IndexError, engine_waves};
use hatate_core::image_compare::{Hash, Source};
use hatate_core::model::{Candidate, Entry};
use hatate_core::net::{NetClient, NetConfig, install_crypto_provider};
use hatate_core::paths::Paths;
use hatate_core::pawchive_index::{ImageRow, PawchiveIndex, fingerprints};
use sha2::{Digest, Sha256};

fn net() -> NetClient {
    install_crypto_provider();
    NetClient::new(NetConfig::default()).unwrap()
}

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(1_700_000_000.0, 100.0))
}

fn paths_in(home: &Path) -> Paths {
    let home = home.as_os_str().to_owned();
    Paths::from_lookup(|k| (k == "HOME").then(|| home.clone())).unwrap()
}

/// A picture with structure, unique per seed (the 4.f tests' generator).
fn picture(seed: u64, w: u32, h: u32) -> image::RgbImage {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as u8
    };
    let mut small = image::RgbImage::new(12, 16);
    for p in small.pixels_mut() {
        *p = image::Rgb([next(), next(), next()]);
    }
    image::imageops::resize(&small, w, h, image::imageops::FilterType::CatmullRom)
}

fn png(img: &image::RgbImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn jpeg(img: &image::RgbImage, quality: u8) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality)
        .encode_image(img)
        .unwrap();
    out
}

fn sha_of(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn hex16(h: &Hash) -> String {
    format!(
        "{:016x}{:016x}{:016x}{:016x}",
        h.0[0], h.0[1], h.0[2], h.0[3]
    )
}

fn row(post: u64, bytes: &[u8], artist_user: &str) -> ImageRow {
    let (h8, h16) = fingerprints(Source::Bytes(bytes)).unwrap();
    let (h8, h16) = (h8.unwrap(), h16.unwrap());
    let sha = sha_of(bytes);
    ImageRow {
        service: "fanbox".to_owned(),
        user_id: artist_user.to_owned(),
        post_id: post.to_string(),
        page_index: 0,
        path: format!("/{}/{}/{sha}.png", &sha[..2], &sha[2..4]),
        sha256: sha,
        title: Some("a post".to_owned()),
        dhash8: Some(format!("{:016x}", h8.0[3])),
        dhash16: Some(hex16(&h16)),
    }
}

struct Profile {
    _dir: tempfile::TempDir,
    paths: Paths,
}

impl Profile {
    fn new() -> Profile {
        let dir = tempfile::Builder::new()
            .prefix("hatate-dan2145-")
            .tempdir()
            .unwrap();
        let paths = paths_in(dir.path());
        Profile { _dir: dir, paths }
    }

    fn index_file(&self) -> PathBuf {
        self.paths.pawchive_index_db()
    }

    /// An index file in the profile holding `rows`.
    fn with_rows(&self, rows: &[ImageRow]) {
        PawchiveIndex::new(self.index_file())
            .add_images(rows)
            .unwrap();
    }

    fn engines(&self) -> LiveEngines {
        LiveEngines::for_profile(net(), clock(), &self.paths)
    }

    fn image(&self, name: &str, bytes: &[u8]) -> String {
        let path = self._dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }
}

/// What `_run_engine(PAWCHIVE_INDEX, ...)` leaves behind.
struct Collected {
    found: bool,
    candidates: Vec<Candidate>,
    errors: Vec<String>,
    answered: HashSet<String>,
}

async fn collect(engines: LiveEngines, entry: &Entry) -> Collected {
    let runner = EngineRunner::new(Arc::new(engines), clock());
    let settings = Arc::new(Settings::with_defaults(&paths_in(Path::new(
        "/nonexistent",
    ))));
    let (mut candidates, mut errors, mut answered) = (Vec::new(), Vec::new(), HashSet::new());
    let found = runner
        .run_engines_parallel(
            &["pawchiveindex"],
            entry,
            &settings,
            &mut candidates,
            &mut errors,
            None,
            None,
            None,
            Some(&mut answered),
        )
        .await
        .unwrap();
    Collected {
        found: found["pawchiveindex"],
        candidates,
        errors,
        answered,
    }
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

#[tokio::test]
async fn a_missing_index_file_is_empty_and_is_not_created() {
    let profile = Profile::new();
    let local = profile.image("a.png", &png(&picture(1, 64, 64)));
    let engines = profile.engines();

    let answer = engines.pawchive_index(Path::new(&local), None).await;

    assert_eq!(answer, Ok(IndexAnswer::Empty));
    assert!(
        !profile.index_file().exists(),
        "a search must not create the index"
    );
}

#[tokio::test]
async fn an_index_with_nothing_fingerprinted_is_empty_and_the_local_file_is_not_read() {
    let profile = Profile::new();
    // Schema only: opening the index through the 4.f module creates it with no rows.
    assert_eq!(
        PawchiveIndex::new(profile.index_file())
            .image_count()
            .unwrap(),
        0
    );
    PawchiveIndex::new(profile.index_file()).creators().unwrap();
    assert!(profile.index_file().exists());
    let engines = profile.engines();

    // The path does not exist: reading it would answer Unreadable, not Empty.
    let answer = engines
        .pawchive_index(Path::new("/nonexistent/never-read.png"), None)
        .await;

    assert_eq!(answer, Ok(IndexAnswer::Empty));
}

#[tokio::test]
async fn a_resized_copy_of_an_indexed_image_is_found_with_its_post_and_thumbnail() {
    let profile = Profile::new();
    let original = picture(7, 256, 256);
    profile.with_rows(&[
        row(11, &png(&original), "artist-1"),
        row(12, &png(&picture(8, 256, 256)), "artist-1"),
    ]);
    let local = profile.image(
        "copy.jpg",
        &jpeg(
            &image::imageops::resize(&original, 128, 128, image::imageops::FilterType::Triangle),
            80,
        ),
    );
    let engines = profile.engines();

    let answer = engines
        .pawchive_index(Path::new(&local), None)
        .await
        .unwrap();

    let IndexAnswer::Matches(matches) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(matches.len(), 1, "{matches:?}");
    let m = &matches[0];
    assert_eq!(m.url, "https://pawchive.pw/fanbox/user/artist-1/post/11");
    assert!(
        m.thumb_url
            .as_deref()
            .is_some_and(|t| t.starts_with("https://") && t.ends_with(".png")),
        "{m:?}"
    );
    assert!(!m.exact);
    assert!(
        m.similarity > 70.0 && m.similarity < 100.0,
        "{}",
        m.similarity
    );
}

#[tokio::test]
async fn the_hydrus_hash_makes_the_same_file_an_exact_match_at_100() {
    let profile = Profile::new();
    let bytes = png(&picture(21, 200, 200));
    profile.with_rows(&[row(30, &bytes, "artist-2")]);
    let local = profile.image("same.png", &bytes);
    let engines = profile.engines();

    let hash = sha_of(&bytes);
    let answer = engines
        .pawchive_index(Path::new(&local), Some(&hash))
        .await
        .unwrap();

    let IndexAnswer::Matches(matches) = answer else {
        panic!("{answer:?}");
    };
    assert_eq!(matches.len(), 1);
    assert!(matches[0].exact);
    assert_eq!(matches[0].similarity, 100.0);
}

#[tokio::test]
async fn an_image_the_index_does_not_know_is_a_search_that_found_nothing() {
    let profile = Profile::new();
    profile.with_rows(&[row(40, &png(&picture(31, 200, 200)), "artist-3")]);
    let local = profile.image("other.png", &png(&picture(32, 200, 200)));

    let collected = collect(profile.engines(), &Entry::new(local)).await;

    assert!(!collected.found);
    assert!(collected.candidates.is_empty());
    assert!(collected.errors.is_empty(), "{:?}", collected.errors);
    assert!(collected.answered.contains("pawchiveindex"));
}

#[tokio::test]
async fn through_the_runner_a_match_is_a_candidate_labelled_by_exactness() {
    let profile = Profile::new();
    let bytes = png(&picture(51, 200, 200));
    profile.with_rows(&[row(60, &bytes, "artist-4")]);
    let local = profile.image("c.png", &bytes);
    let mut entry = Entry::new(local);

    entry.hydrus_hash = Some(sha_of(&bytes));
    let with_hash = collect(profile.engines(), &entry).await;
    entry.hydrus_hash = None;
    let without = collect(profile.engines(), &entry).await;

    assert!(with_hash.found && without.found);
    assert!(with_hash.errors.is_empty() && without.errors.is_empty());
    let (a, b) = (&with_hash.candidates[0], &without.candidates[0]);
    assert_eq!(a.url, "https://pawchive.pw/fanbox/user/artist-4/post/60");
    assert_eq!(a.source_name.as_deref(), Some("Pawchive"));
    assert_eq!(a.similarity, 100.0);
    // Same SHA-256 is labelled as the exact engine; the fingerprint alone is the index engine.
    assert_eq!(b.engine, "Pawchive index");
    assert_ne!(a.engine, b.engine);
}

#[tokio::test]
async fn a_local_file_that_is_not_a_picture_is_unreadable_and_says_so() {
    let profile = Profile::new();
    profile.with_rows(&[row(70, &png(&picture(61, 64, 64)), "artist-5")]);
    let junk = profile.image("notes.png", b"this is not an image");
    let engines = profile.engines();

    assert_eq!(
        engines.pawchive_index(Path::new(&junk), None).await,
        Ok(IndexAnswer::Unreadable)
    );
    let collected = collect(profile.engines(), &Entry::new(junk)).await;
    assert!(!collected.found);
    assert_eq!(
        collected.errors,
        ["Pawchive index: could not read the local file"]
    );
    assert!(!collected.answered.contains("pawchiveindex"));

    let gone = profile.image("gone.png", b"x");
    std::fs::remove_file(&gone).unwrap();
    assert_eq!(
        engines.pawchive_index(Path::new(&gone), None).await,
        Ok(IndexAnswer::Unreadable)
    );
}

#[tokio::test]
async fn a_database_that_cannot_be_read_is_an_error_row_not_a_crash() {
    let profile = Profile::new();
    std::fs::create_dir_all(profile.index_file().parent().unwrap()).unwrap();
    std::fs::write(profile.index_file(), vec![b'x'; 4096]).unwrap();
    let local = profile.image("a.png", &png(&picture(81, 64, 64)));
    let engines = profile.engines();

    let Err(IndexError(text)) = engines.pawchive_index(Path::new(&local), None).await else {
        panic!("a damaged index file must be an error");
    };
    assert_eq!(text, "file is not a database");

    let collected = collect(profile.engines(), &Entry::new(local)).await;
    assert!(!collected.found);
    assert_eq!(collected.errors, ["Pawchive index: file is not a database"]);
    assert!(!collected.answered.contains("pawchiveindex"));
}

#[tokio::test]
async fn searching_never_writes_the_users_index() {
    let profile = Profile::new();
    let bytes = png(&picture(91, 200, 200));
    profile.with_rows(&[
        row(100, &bytes, "artist-6"),
        row(101, &png(&picture(92, 200, 200)), "artist-6"),
    ]);
    let local = profile.image("q.png", &bytes);
    let index = profile.index_file();
    let before = (std::fs::read(&index).unwrap(), modified(&index));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let engines = profile.engines();

    let hash = sha_of(&bytes);
    for hydrus_hash in [None, Some(hash.as_str())] {
        let answer = engines
            .pawchive_index(Path::new(&local), hydrus_hash)
            .await
            .unwrap();
        assert!(matches!(&answer, IndexAnswer::Matches(m) if !m.is_empty()));
    }
    // Unreadable and no-match paths open the file too.
    let junk = profile.image("junk.png", b"nope");
    engines
        .pawchive_index(Path::new(&junk), None)
        .await
        .unwrap();

    let after = (std::fs::read(&index).unwrap(), modified(&index));
    assert!(before.0 == after.0, "the index bytes changed");
    assert_eq!(before.1, after.1, "the index mtime changed");
    let dir = index.parent().unwrap();
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("pawchive_index"))
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["pawchive_index.db"],
        "no journal or WAL left behind"
    );
}

#[tokio::test]
async fn the_setting_decides_whether_the_index_runs() {
    let profile = Profile::new();
    let mut settings = Settings::with_defaults(&profile.paths);
    settings.enable_pawchive = false;
    settings.enable_pawchive_index = true;
    assert!(engine_waves(&settings, None, false)[0].contains(&"pawchiveindex"));
    settings.enable_pawchive_index = false;
    assert!(!engine_waves(&settings, None, false)[0].contains(&"pawchiveindex"));
}

#[tokio::test]
async fn hatate_batch_builds_its_engines_over_the_profiles_index() {
    // `batch_cmd` calls `LiveEngines::for_profile`; a build that forgot the index would answer
    // Empty here even with rows in the profile's file.
    let profile = Profile::new();
    let bytes = png(&picture(111, 200, 200));
    profile.with_rows(&[row(120, &bytes, "artist-7")]);
    let local = profile.image("b.png", &bytes);

    let bare = LiveEngines::new(net(), clock());
    assert_eq!(
        bare.pawchive_index(Path::new(&local), None).await,
        Ok(IndexAnswer::Empty)
    );
    let answer = profile
        .engines()
        .pawchive_index(Path::new(&local), None)
        .await
        .unwrap();
    assert!(
        matches!(&answer, IndexAnswer::Matches(m) if m.len() == 1),
        "{answer:?}"
    );
}

/// Python's `sqlite3` says only "unable to open database file" when the index cannot be opened;
/// `rusqlite` appends the file's path, which must not reach `Entry.error_message`.
#[tokio::test]
async fn an_index_that_cannot_be_opened_says_so_without_its_path() {
    use std::os::unix::fs::PermissionsExt;

    let profile = Profile::new();
    let bytes = png(&picture(111, 200, 200));
    let local = profile.image("b.png", &bytes);
    let expect = Err(IndexError("unable to open database file".to_owned()));

    // A folder where the file should be.
    std::fs::create_dir_all(profile.index_file()).unwrap();
    let answer = profile
        .engines()
        .pawchive_index(Path::new(&local), None)
        .await;
    assert_eq!(answer, expect);
    std::fs::remove_dir(profile.index_file()).unwrap();

    // A file this user may not open.
    std::fs::write(profile.index_file(), b"").unwrap();
    std::fs::set_permissions(profile.index_file(), std::fs::Permissions::from_mode(0o000)).unwrap();
    let answer = profile
        .engines()
        .pawchive_index(Path::new(&local), None)
        .await;
    assert_eq!(answer, expect);
}
