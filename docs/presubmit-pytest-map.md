# Python tests the presubmit walk found unnamed (DAN-2499)

The presubmit gate (`crates/hatate-doccite/tests/presubmit.rs`) walks `tests/test_*.py` of the Python repo at
`76c85c4`, follows `setUp`, class helpers and called helpers, and requires every test that touches a `gui/`
symbol to be named in a doc. At `b4f1487` (the base of this change) 868 tests touch one, and 8 were named
nowhere: the docs mapped the **symbols** (`header_layout_is_usable`, `_research_rows_with_opposite_engine`) but
never the tests that pin them. This doc maps those 8.

| Python test | Rust assertion | Disposition |
|---|---|---|
| `TestHeaderLayoutProvenance.test_layout_from_this_build_is_applied` | `crates/hatate-core/src/table_cells.rs` `fn column_widths_and_the_saved_layout_check` `assert!(header_layout_is_usable("state", 12, None));` | pinned: a saved layout recording the table's own column count is applied |
| `TestHeaderLayoutProvenance.test_layout_with_an_unrecorded_count_is_discarded` | `crates/hatate-core/src/table_cells.rs` `fn column_widths_and_the_saved_layout_check` `assert!(!header_layout_is_usable("state", 0, None));` | pinned: a recorded count of 0 (saved before the count existed) is discarded |
| `TestHeaderLayoutProvenance.test_layout_from_a_different_column_count_is_discarded` | `crates/hatate-core/src/table_cells.rs` `fn column_widths_and_the_saved_layout_check` `assert!(!header_layout_is_usable("state", 10, None));` | pinned for one wrong count (10 against 12); Python also tries the table's count -2, -1, +1 and +2, which the Rust test does not |
| `TestHeaderLayoutProvenance.test_no_saved_layout_is_not_an_error` | `crates/hatate-core/src/table_cells.rs` `fn column_widths_and_the_saved_layout_check` `assert!(!header_layout_is_usable("", 12, None));` | pinned: an empty saved state is "not usable", not an error |
| `TestEngineOverrideSelection.test_unsearched_rows_use_the_engine_not_yet_tried` | `crates/hatate-core/src/engine_ids.rs` `fn unsearched_rows_use_the_engine_not_yet_tried` `assert_eq!(opposite_engine(None, SAUCENAO), IQDB);` | pinned |
| `TestEngineOverrideSelection.test_fallback_follows_the_configured_primary` | `crates/hatate-core/src/engine_ids.rs` `fn unsearched_rows_use_the_engine_not_yet_tried` `assert_eq!(opposite_engine(None, IQDB), SAUCENAO);` | pinned |
| `TestEngineOverrideSelection.test_matched_rows_still_flip_per_row` | `crates/hatate-core/src/engine_ids.rs` `fn unsearched_rows_use_the_engine_not_yet_tried` `assert_eq!(opposite_engine(Some("SauceNAO"), SAUCENAO), IQDB);` | pinned for the per-row decision (`opposite_engine`); the loop that applies it to each selected row is `crates/hatate-gtk/src/search_run.rs` `fn research_rows_with_opposite_engine` `opposite_engine(current.as_deref(), &primary).to_owned(),` |
| `TestEngineOverrideSelection.test_mixed_selection_is_handled_row_by_row` | `crates/hatate-gtk/src/search_run.rs` `fn research_rows_with_opposite_engine` `opposite_engine(current.as_deref(), &primary).to_owned(),` | **not asserted**: the loop decides per row, but no Rust test selects a mix of searched and unsearched rows (the smoke scenario `re_search_uses_the_engine_it_was_asked_for` re-searches one row at a time). A Phase 7 rider, not a defect found here |
