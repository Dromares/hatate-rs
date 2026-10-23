//! `hatate batch <dir>`: the headless run (Phase 3.i, DAN-2065).
//!
//! * [`intake`]: `core/file_intake.py` and `core/formats.py`, which files enter the list.
//! * [`hashing`]: `workers/file_hash_worker.py`, the hash pass.
//! * [`matched_urls`]: `core/logger.py`, the matched-URL log.
//! * [`run`]: `workers/search_worker.py` and the autosave, the loop itself.

pub mod hashing;
pub mod intake;
pub mod matched_urls;
pub mod run;

pub use run::{
    Autosave, BatchEnv, Ending, Outcome, Reporter, SessionSink, Silent, run_batch, search_pending,
};
