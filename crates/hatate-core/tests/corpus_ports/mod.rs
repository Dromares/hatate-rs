//! One file per ported parser: the map from each Python dotted callable to its Rust function.
//! Add the file here and its `PORT` to `PORTS` in `corpus_diff.rs`.

pub mod animepictures;
pub mod danbooru;
pub mod gelbooru;
pub mod joyreactor;
pub mod rule34;
pub mod safebooru;
pub mod sankaku;
pub mod xbooru;
