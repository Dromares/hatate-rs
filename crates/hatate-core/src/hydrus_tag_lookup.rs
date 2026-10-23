//! `core/hydrus_tag_lookup.py`, `hash_file` only (Phase 3.h, DAN-2064): the SHA-256 `search_image`
//! reads for the cache key. The rest of that module is the Hydrus client's, which stays in
//! Phase 4.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};
use tracing::warn;

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
}
