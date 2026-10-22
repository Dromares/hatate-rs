//! One file per ported parser: the map from each Python dotted callable to its Rust function.
//! Add the file here and its `PORT` to `PORTS` in `corpus_diff.rs`.

pub mod danbooru;
pub mod e621;
pub mod eshuushuu;
pub mod gelbooru;
pub mod mangadex;
pub mod moebooru;
pub mod paheal;
pub mod rule34;
pub mod rule34us;
pub mod safebooru;
pub mod xbooru;
pub mod zerochan;
