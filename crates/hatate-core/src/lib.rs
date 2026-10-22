//! Shared library for the Hatate Rust rewrite. Work in progress.

pub mod config;
pub mod model;
pub mod paths;
pub mod session;

/// Crate name, used by the placeholder test below.
pub const NAME: &str = "hatate-core";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_set() {
        assert_eq!(NAME, "hatate-core");
    }
}
