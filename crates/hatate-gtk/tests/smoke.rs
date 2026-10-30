//! The window, headless: every scenario builds the real `MainWindow` over a temp profile with a
//! scripted prompter, drives it through its `gio::Action`s and model calls, and reads what it did
//! back. One program, because GTK must run on the main thread and the default test harness does
//! not give a test that.
//!
//! Needs a display. It runs under `xvfb-run` with `HATATE_GTK_SMOKE=1`. **Fails closed**: with
//! `CI` set, a missing flag or a missing display is a failure, not a skip (DAN-1974 item 1). Key
//! injection additionally needs `HATATE_SMOKE_XVFB=1` and an `xvfb-run` display (DAN-2264).

mod common;

use common::*;
use gtk4::gio;
use gtk4::prelude::*;
use hatate_core::engines::saucenao::SauceNaoQuota;
use hatate_core::model::MatchStatus;
use hatate_core::quota_pause::{QuotaPause, QuotaPauseFile};
use hatate_core::session::{LoadOptions, SessionStore, named};
use hatate_core::table_menu::Action;
use hatate_gtk::dialogs::{Asked, Buttons, Kind, Response};
use hatate_gtk::menu::{self, Owner};
use hatate_gtk::shell::{EMPTY_GHOST_GLYPH, IDLE_ETA, IDLE_PROGRESS, RUN_FAILED_NOTE};
use libadwaita as adw;
use std::cell::Cell;
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

type Scenario = (&'static str, fn(&adw::Application));

const SCENARIOS: &[Scenario] = &[
    (
        "settings_engine_line_lens_warning_and_browse_follow_python",
        settings_engine_line_lens_warning_and_browse_follow_python,
    ),
    (
        "settings_dialog_opens_on_the_stored_values_and_ok_saves_what_python_would",
        settings_dialog_opens_on_the_stored_values_and_ok_saves_what_python_would,
    ),
    (
        "settings_dialog_widgets_hand_back_the_values_they_were_given",
        settings_dialog_widgets_hand_back_the_values_they_were_given,
    ),
    (
        "settings_window_keeps_its_stated_minimum",
        settings_window_keeps_its_stated_minimum,
    ),
    (
        "settings_window_asks_its_questions_over_itself",
        settings_window_asks_its_questions_over_itself,
    ),
    (
        "settings_highlight_second_call_inside_the_window_still_restores",
        settings_highlight_second_call_inside_the_window_still_restores,
    ),
    (
        "settings_find_box_lists_and_jumps",
        settings_find_box_lists_and_jumps,
    ),
    (
        "review_page_follows_the_selection",
        review_page_follows_the_selection,
    ),
    (
        "edit_conditions_stores_and_saves_on_ok_and_nothing_on_cancel",
        edit_conditions_stores_and_saves_on_ok_and_nothing_on_cancel,
    ),
    (
        "review_tag_panel_adds_removes_and_edits_in_place",
        review_tag_panel_adds_removes_and_edits_in_place,
    ),
    (
        "review_candidate_picker_switches_the_match_and_the_fetch_stays_off_the_main_loop",
        review_candidate_picker_switches_the_match_and_the_fetch_stays_off_the_main_loop,
    ),
    (
        "review_keys_step_mark_skip_and_open_the_match",
        review_keys_step_mark_skip_and_open_the_match,
    ),
    (
        "review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide",
        review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide,
    ),
    (
        "the_readout_line_under_the_header_is_not_cut_and_carries_the_size_chip",
        the_readout_line_under_the_header_is_not_cut_and_carries_the_size_chip,
    ),
    (
        "no_clipping_wrapper_on_the_review_page_is_a_tab_stop",
        no_clipping_wrapper_on_the_review_page_is_a_tab_stop,
    ),
    (
        "tab_walks_the_review_page_from_the_header_to_the_footer",
        tab_walks_the_review_page_from_the_header_to_the_footer,
    ),
    (
        "review_with_a_not_found_row_selected_keeps_the_stated_minimum",
        review_with_a_not_found_row_selected_keeps_the_stated_minimum,
    ),
    (
        "review_with_add_tags_open_keeps_the_stated_minimum",
        review_with_add_tags_open_keeps_the_stated_minimum,
    ),
    (
        "review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection",
        review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection,
    ),
    (
        "review_wipe_with_an_unreadable_local_file_shows_the_error_state",
        review_wipe_with_an_unreadable_local_file_shows_the_error_state,
    ),
    (
        "review_wipe_error_state_returns_to_fit_on_re_entering_wipe",
        review_wipe_error_state_returns_to_fit_on_re_entering_wipe,
    ),
    (
        "review_wipe_waits_on_a_stalled_site_without_blocking_the_window",
        review_wipe_waits_on_a_stalled_site_without_blocking_the_window,
    ),
    (
        "review_wipe_chrome_names_its_ends_and_zoom_buttons_and_a_fallback_warns",
        review_wipe_chrome_names_its_ends_and_zoom_buttons_and_a_fallback_warns,
    ),
    (
        "review_zoom_readout_is_blank_while_nothing_is_loaded",
        review_zoom_readout_is_blank_while_nothing_is_loaded,
    ),
    (
        "review_differences_button_is_live_again_after_an_unopenable_row",
        review_differences_button_is_live_again_after_an_unopenable_row,
    ),
    (
        "review_comparison_states_keep_the_stated_minimum",
        review_comparison_states_keep_the_stated_minimum,
    ),
    (
        "compare_dialog_loads_the_pair_and_switches_modes",
        compare_dialog_loads_the_pair_and_switches_modes,
    ),
    (
        "the_differences_pane_has_zero_chroma_and_the_wipe_pane_shows_the_pictures_colours",
        the_differences_pane_has_zero_chroma_and_the_wipe_pane_shows_the_pictures_colours,
    ),
    (
        "compare_dialog_answers_in_a_box_when_there_is_nothing_to_compare",
        compare_dialog_answers_in_a_box_when_there_is_nothing_to_compare,
    ),
    (
        "the_readout_shows_in_the_row_under_the_header_once_it_has_dropped_there",
        the_readout_shows_in_the_row_under_the_header_once_it_has_dropped_there,
    ),
    (
        "a_fresh_hint_bar_asks_for_its_first_group_only",
        a_fresh_hint_bar_asks_for_its_first_group_only,
    ),
    (
        "window_menus_and_deferred_actions",
        window_menus_and_deferred_actions,
    ),
    ("empty_state_and_idle_strip", empty_state_and_idle_strip),
    (
        "run_strip_and_sent_count_follow_the_list",
        run_strip_and_sent_count_follow_the_list,
    ),
    ("add_files_hashes_and_dedupes", add_files_hashes_and_dedupes),
    (
        "add_folder_scans_and_drop_filters",
        add_folder_scans_and_drop_filters,
    ),
    (
        "a_second_add_waits_and_the_window_keeps_working",
        a_second_add_waits_and_the_window_keeps_working,
    ),
    ("restore_clean_session", restore_clean_session),
    (
        "restore_after_a_crash_shows_the_banner",
        restore_after_a_crash_shows_the_banner,
    ),
    ("nothing_survived_variant", nothing_survived_variant),
    ("missing_files_dialog", missing_files_dialog),
    ("save_session_as", save_session_as),
    ("open_session", open_session),
    (
        "clear_session_matches_the_empty_list_quirk",
        clear_session_matches_the_empty_list_quirk,
    ),
    ("persisted_quota_pause", persisted_quota_pause),
    (
        "theme_action_saves_the_setting",
        theme_action_saves_the_setting,
    ),
    (
        "close_saves_and_marks_a_clean_exit",
        close_saves_and_marks_a_clean_exit,
    ),
    (
        "close_writes_an_empty_list_over_the_old_rows",
        close_writes_an_empty_list_over_the_old_rows,
    ),
    (
        "autosave_writes_the_list_off_the_main_thread",
        autosave_writes_the_list_off_the_main_thread,
    ),
    ("log_viewer_and_open_log", log_viewer_and_open_log),
    (
        "fonts_cover_the_status_glyphs_and_kanji",
        fonts_cover_the_status_glyphs_and_kanji,
    ),
    (
        "minimum_size_fits_every_state_in_both_schemes",
        minimum_size_fits_every_state_in_both_schemes,
    ),
    ("start_search_runs_the_batch", start_search_runs_the_batch),
    (
        "start_search_with_nothing_to_do_or_one_running",
        start_search_with_nothing_to_do_or_one_running,
    ),
    (
        "stop_search_finishes_the_image_under_way",
        stop_search_finishes_the_image_under_way,
    ),
    (
        "a_quota_pause_shows_the_banner_and_continue_resumes",
        a_quota_pause_shows_the_banner_and_continue_resumes,
    ),
    (
        "engine_alerts_reach_the_user_only_during_a_run",
        engine_alerts_reach_the_user_only_during_a_run,
    ),
    (
        "the_window_keeps_working_while_an_engine_stalls",
        the_window_keeps_working_while_an_engine_stalls,
    ),
    (
        "the_sites_menu_toggles_and_saves",
        the_sites_menu_toggles_and_saves,
    ),
    (
        "a_failed_run_is_shown_as_failed_not_as_a_crash",
        a_failed_run_is_shown_as_failed_not_as_a_crash,
    ),
    (
        "the_availability_check_offers_to_drop_the_dead_matches",
        the_availability_check_offers_to_drop_the_dead_matches,
    ),
    (
        "the_upscale_check_marks_rows_and_summarises",
        the_upscale_check_marks_rows_and_summarises,
    ),
    (
        "starting_a_search_clears_the_stopped_note",
        starting_a_search_clears_the_stopped_note,
    ),
    (
        "parser_health_dialog_and_warning",
        parser_health_dialog_and_warning,
    ),
    (
        "activity_page_states_the_engines_and_tails_the_log",
        activity_page_states_the_engines_and_tails_the_log,
    ),
    (
        "lens_connection_is_said_in_the_status_line",
        lens_connection_is_said_in_the_status_line,
    ),
    (
        "re_search_uses_the_engine_it_was_asked_for",
        re_search_uses_the_engine_it_was_asked_for,
    ),
    (
        "design_follow_ups_paint_as_specified",
        design_follow_ups_paint_as_specified,
    ),
    (
        "a_dialog_shows_the_ring_on_its_default",
        a_dialog_shows_the_ring_on_its_default,
    ),
    ("menubar_keyboard_path", menubar_keyboard_path),
    (
        "the_log_viewer_opens_on_close_and_selects_in_ink",
        the_log_viewer_opens_on_close_and_selects_in_ink,
    ),
    (
        "queue_view_holds_5000_rows_and_filters_without_touching_the_queue",
        queue_view_holds_5000_rows_and_filters_without_touching_the_queue,
    ),
    (
        "queue_filter_bar_narrows_by_text_and_menus_reads_hidden_and_clears",
        queue_filter_bar_narrows_by_text_and_menus_reads_hidden_and_clears,
    ),
    (
        "queue_select_by_status_under_a_filter_never_selects_a_hidden_row",
        queue_select_by_status_under_a_filter_never_selects_a_hidden_row,
    ),
    (
        "queue_clear_re_ticks_every_filter_menu_so_the_ticks_summary_and_count_agree",
        queue_clear_re_ticks_every_filter_menu_so_the_ticks_summary_and_count_agree,
    ),
    (
        "queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row",
        queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row,
    ),
    (
        "queue_view_thumbnails_load_fail_and_stay_off_the_main_loop",
        queue_view_thumbnails_load_fail_and_stay_off_the_main_loop,
    ),
    (
        "queue_view_thumbnails_stay_off_when_the_source_is_off",
        queue_view_thumbnails_stay_off_when_the_source_is_off,
    ),
    (
        "queue_header_layout_is_saved_on_close_and_restored_repaired_or_discarded",
        queue_header_layout_is_saved_on_close_and_restored_repaired_or_discarded,
    ),
    (
        "queue_header_titles_are_not_clipped_at_the_minimum_window_size",
        queue_header_titles_are_not_clipped_at_the_minimum_window_size,
    ),
    (
        "queue_status_and_file_cells_fit_and_sent_is_in_view_at_both_window_sizes",
        queue_status_and_file_cells_fit_and_sent_is_in_view_at_both_window_sizes,
    ),
    (
        "queue_scrollbar_is_10_px_wide_and_sent_still_fits_beside_it",
        queue_scrollbar_is_10_px_wide_and_sent_still_fits_beside_it,
    ),
    (
        "settings_prose_wraps_to_the_page_and_every_page_keeps_its_bar_at_rest",
        settings_prose_wraps_to_the_page_and_every_page_keeps_its_bar_at_rest,
    ),
    (
        "clear_search_cache_label_carries_the_cache_size",
        clear_search_cache_label_carries_the_cache_size,
    ),
    (
        "queue_context_menu_selects_marks_resets_and_removes",
        queue_context_menu_selects_marks_resets_and_removes,
    ),
    (
        "queue_view_header_cycle_sorts_the_queue_in_place_and_restores_the_order",
        queue_view_header_cycle_sorts_the_queue_in_place_and_restores_the_order,
    ),
    (
        "export_results_writes_the_format_the_name_says",
        export_results_writes_the_format_the_name_says,
    ),
    (
        "write_tag_files_asks_then_writes_and_keeps_what_is_there",
        write_tag_files_asks_then_writes_and_keeps_what_is_there,
    ),
    (
        "clear_search_cache_asks_then_removes_the_saved_results",
        clear_search_cache_asks_then_removes_the_saved_results,
    ),
    (
        "hydrus_send_uploads_the_selected_rows_and_marks_them_sent",
        hydrus_send_uploads_the_selected_rows_and_marks_them_sent,
    ),
    (
        "hydrus_send_works_over_https_with_the_users_verify_setting",
        hydrus_send_works_over_https_with_the_users_verify_setting,
    ),
    (
        "hydrus_send_reports_what_failed_in_one_box_and_leaves_the_rows_unsent",
        hydrus_send_reports_what_failed_in_one_box_and_leaves_the_rows_unsent,
    ),
    (
        "hydrus_actions_ask_for_a_selection_and_a_key_before_anything_goes_out",
        hydrus_actions_ask_for_a_selection_and_a_key_before_anything_goes_out,
    ),
    (
        "hydrus_delete_defaults_to_cancel_and_only_deletes_after_the_answer",
        hydrus_delete_defaults_to_cancel_and_only_deletes_after_the_answer,
    ),
    (
        "hydrus_show_opens_a_page_and_explains_a_refusal",
        hydrus_show_opens_a_page_and_explains_a_refusal,
    ),
    (
        "hydrus_reconcile_confirms_queued_rows_and_says_when_nothing_waits",
        hydrus_reconcile_confirms_queued_rows_and_says_when_nothing_waits,
    ),
    (
        "pawchive_and_query_windows_keep_their_stated_minimum",
        pawchive_and_query_windows_keep_their_stated_minimum,
    ),
    (
        "query_and_pawchive_windows_follow_the_design_read",
        query_and_pawchive_windows_follow_the_design_read,
    ),
    (
        "tab_walks_the_query_hydrus_and_pawchive_windows_to_their_footers",
        tab_walks_the_query_hydrus_and_pawchive_windows_to_their_footers,
    ),
    (
        "pawchive_artist_columns_share_one_edge_in_the_header_and_every_row",
        pawchive_artist_columns_share_one_edge_in_the_header_and_every_row,
    ),
    (
        "message_detail_box_starts_at_its_top_left_and_is_indented",
        message_detail_box_starts_at_its_top_left_and_is_indented,
    ),
    (
        "hydrus_requests_leave_the_main_loop_running_while_the_server_stalls",
        hydrus_requests_leave_the_main_loop_running_while_the_server_stalls,
    ),
    (
        "hydrus_query_searches_lists_and_imports_what_the_list_does_not_hold",
        hydrus_query_searches_lists_and_imports_what_the_list_does_not_hold,
    ),
    (
        "hydrus_query_asks_for_a_tag_a_key_and_says_when_nothing_matched",
        hydrus_query_asks_for_a_tag_a_key_and_says_when_nothing_matched,
    ),
    (
        "pawchive_index_window_adds_by_link_searches_by_name_indexes_and_removes",
        pawchive_index_window_adds_by_link_searches_by_name_indexes_and_removes,
    ),
    (
        "pawchive_index_window_is_one_window_and_the_search_does_not_block_the_loop",
        pawchive_index_window_is_one_window_and_the_search_does_not_block_the_loop,
    ),
    (
        "hydrus_thumbnails_come_from_hydrus_for_files_it_holds_and_from_disk_for_the_rest",
        hydrus_thumbnails_come_from_hydrus_for_files_it_holds_and_from_disk_for_the_rest,
    ),
    (
        "hydrus_thumbnails_fall_back_to_the_file_when_hydrus_cannot_supply_one",
        hydrus_thumbnails_fall_back_to_the_file_when_hydrus_cannot_supply_one,
    ),
    (
        "hydrus_menu_entries_and_the_h_key_reach_their_handlers",
        hydrus_menu_entries_and_the_h_key_reach_their_handlers,
    ),
    (
        "sending_the_row_you_are_on_away_keeps_your_place",
        sending_the_row_you_are_on_away_keeps_your_place,
    ),
    (
        "mcp_server_starts_with_the_window_answers_over_http_and_stops_on_close",
        mcp_server_starts_with_the_window_answers_over_http_and_stops_on_close,
    ),
    (
        "mcp_server_stays_off_when_disabled_and_reports_a_held_port_and_a_blank_token",
        mcp_server_stays_off_when_disabled_and_reports_a_held_port_and_a_blank_token,
    ),
    (
        "mcp_tool_calls_change_the_list_and_the_table_follows",
        mcp_tool_calls_change_the_list_and_the_table_follows,
    ),
    (
        "mcp_research_runs_the_windows_own_search_and_the_tool_goes_on_when_it_ends",
        mcp_research_runs_the_windows_own_search_and_the_tool_goes_on_when_it_ends,
    ),
    (
        "mcp_send_leaves_the_main_loop_running_while_hydrus_stalls",
        mcp_send_leaves_the_main_loop_running_while_hydrus_stalls,
    ),
    (
        "settings_find_box_reaches_every_string_the_mcp_spec_lists",
        settings_find_box_reaches_every_string_the_mcp_spec_lists,
    ),
    (
        "settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict",
        settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict,
    ),
    (
        "settings_mcp_page_states_the_server_and_follows_the_boxes",
        settings_mcp_page_states_the_server_and_follows_the_boxes,
    ),
    (
        "settings_mcp_enable_summary_fires_on_a_real_change_only",
        settings_mcp_enable_summary_fires_on_a_real_change_only,
    ),
    (
        "settings_mcp_token_buttons_restart_and_ok_rebind_the_server",
        settings_mcp_token_buttons_restart_and_ok_rebind_the_server,
    ),
    (
        "settings_mcp_audit_table_follows_the_log_newest_first",
        settings_mcp_audit_table_follows_the_log_newest_first,
    ),
];

fn main() {
    let ci = std::env::var_os("CI").is_some();
    let flag = std::env::var("HATATE_GTK_SMOKE").as_deref() == Ok("1");
    let display =
        std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !flag {
        if ci {
            eprintln!("hatate-gtk smoke: CI is set but HATATE_GTK_SMOKE=1 is not: failing closed");
            std::process::exit(1);
        }
        println!("hatate-gtk smoke: skipped (set HATATE_GTK_SMOKE=1 under a display)");
        return;
    }
    if !display {
        eprintln!("hatate-gtk smoke: no DISPLAY or WAYLAND_DISPLAY: failing closed");
        std::process::exit(1);
    }
    // Before GTK starts: fontconfig reads its configuration once, at first use.
    let fonts = tempfile::tempdir().expect("font dir");
    hatate_gtk::fonts::load(fonts.path()).expect("the bundled fonts load");

    let failed = Rc::new(Cell::new(0usize));
    let f = failed.clone();
    let code = hatate_gtk::app::with_application(move |app| {
        // `HATATE_SMOKE_ONLY=<substring>` runs just the scenarios whose name contains it.
        let only = std::env::var("HATATE_SMOKE_ONLY").ok();
        for (name, scenario) in SCENARIOS {
            if only.as_deref().is_some_and(|o| !name.contains(o)) {
                continue;
            }
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| scenario(app)));
            match result {
                Ok(()) => println!("scenario {name} ... ok"),
                Err(e) => {
                    let why = e
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                        .unwrap_or_default();
                    println!("scenario {name} ... FAILED: {why}");
                    f.set(f.get() + 1);
                }
            }
        }
        app.quit();
    });
    let _ = code;
    if failed.get() > 0 {
        eprintln!("hatate-gtk smoke: {} scenario(s) failed", failed.get());
        std::process::exit(1);
    }
    println!("hatate-gtk smoke: {} scenarios ok", SCENARIOS.len());
}

fn fx(app: &adw::Application) -> Fx {
    Fx::new(app, Opts::default())
}

fn window_menus_and_deferred_actions(app: &adw::Application) {
    let f = fx(app);
    assert!(f.w.win.title().unwrap().contains("work in progress"));
    // Every action in the menus exists; a later ticket's is disabled, this ticket's is enabled.
    for (name, owner) in menu::actions() {
        if name == "site" {
            // The Check entries stand for one boolean action per site.
            for i in 0..menu::all_site_options().len() {
                let a = f.w.win.lookup_action(&menu::site_action(i));
                assert!(a.is_some_and(|a| a.is_enabled()), "site-{i}");
            }
            continue;
        }
        let action =
            f.w.win
                .lookup_action(name)
                .unwrap_or_else(|| panic!("no action {name}"));
        let enabled = action.is_enabled();
        match owner {
            Owner::Later(t) => assert!(!enabled, "{name} ({t}) must be disabled"),
            Owner::Shell => assert!(enabled, "{name} must be enabled"),
        }
    }
    // Start Search is wired (6.e): its action is enabled and the button follows it.
    assert!(
        f.w.win
            .lookup_action("toggle-search")
            .is_some_and(|a| a.is_enabled())
    );
    f.close();
}

/// Every label and button text under `root`, in tree order: what the screen says, as the user
/// reads it (the CSS `caps` class changes the paint, not the text).
fn texts_under(root: &gtk4::Widget) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(w) = stack.pop() {
        if let Some(l) = w.downcast_ref::<gtk4::Label>() {
            out.push(l.text().to_string());
        } else if let Some(b) = w.downcast_ref::<gtk4::Button>() {
            if let Some(t) = b.label() {
                out.push(t.to_string());
            }
        }
        let mut child = w.last_child();
        while let Some(c) = child {
            child = c.prev_sibling();
            stack.push(c);
        }
    }
    out
}

fn empty_state_and_idle_strip(app: &adw::Application) {
    let f = fx(app);
    // The empty state's copy, literally (`_build_drop_zone`, `screen_title("Queue.")`): the heading,
    // the section label and the three action labels the Python empty-state tests read.
    let said = texts_under(f.w.win.upcast_ref());
    for want in [
        "Queue.",
        "01 // Queue",
        "Drop images here.",
        "Add files\u{2026}",
        "Add folder\u{2026}",
        "Query Hydrus\u{2026}",
    ] {
        assert!(
            said.iter().any(|t| t == want),
            "the empty state does not say {want:?}: {said:?}"
        );
    }
    assert_eq!(
        f.w.s.queue_stack.visible_child_name().as_deref(),
        Some("empty")
    );
    assert_eq!(f.w.s.shell.root.glyph(), EMPTY_GHOST_GLYPH);
    assert_eq!(f.say(), "Ready");
    assert_eq!(f.w.s.shell.run_progress_label.text(), IDLE_PROGRESS);
    assert_eq!(f.w.s.shell.run_eta_label.text(), IDLE_ETA);
    assert!(!f.w.s.run_banner.root.is_visible());
    assert!(!f.w.s.quota_banner.root.is_visible());
    f.close();
}

/// `_refresh_sent_count_label`, `_idle_run_text` and the idle branch of
/// `_refresh_run_progress_label`: the strip's count and the status bar's `N sent` read the list
/// (found by the action differential: the first cut never refreshed them).
fn run_strip_and_sent_count_follow_the_list(app: &adw::Application) {
    let mut rows = vec![
        entry("/p/a.png", MatchStatus::NotSearched),
        entry("/p/b.png", MatchStatus::Good),
        entry("/p/c.png", MatchStatus::Poor),
        entry("/p/d.png", MatchStatus::NotFound),
    ];
    rows[1].sent_to_hydrus = true;
    rows[1].hydrus_import_confirmed = true;
    rows[2].sent_to_hydrus = true; // queued with Hydrus, not yet confirmed
    let f = Fx::new(
        app,
        Opts {
            seed: rows,
            ..Opts::default()
        },
    );
    let sh = &f.w.s.shell;
    assert_eq!(sh.run_progress_label.text(), "1 not searched");
    assert_eq!(sh.sent_count_label.text(), "1 sent (1 queued)");
    assert_eq!(sh.run_eta_label.text(), IDLE_ETA);
    // Reviewing a row adds the "to review" figure, which is quiet until something is reviewed.
    {
        let mut q = f.w.s.queue.borrow_mut();
        let id = q.id_at(0).unwrap();
        q.entry_mut(id).unwrap().reviewed = true;
    }
    f.w.list_changed();
    assert_eq!(
        sh.sent_count_label.text(),
        "1 sent (1 queued) \u{b7} 1 to review"
    );
    // An empty list: nothing queued, nothing sent.
    *f.w.s.queue.borrow_mut() =
        hatate_core::queue::QueueModel::new(Vec::new(), f.w.s.settings.clone());
    f.w.list_changed();
    assert_eq!(sh.run_progress_label.text(), IDLE_PROGRESS);
    assert_eq!(sh.sent_count_label.text(), "");
    f.close();
}

fn add_files_hashes_and_dedupes(app: &adw::Application) {
    let f = fx(app);
    let imgs = make_images(&f.dir.path().join("pics"), &["a.png", "b.jpg", "c.webp"]);
    f.script.push(Answer::Files(imgs.clone()));
    f.activate("add-files");
    assert!(
        pump_until(10_000, || f.len() == 3),
        "the three files were added"
    );
    assert_eq!(f.say(), "Added 3 file(s), 3 total");
    assert_eq!(
        f.w.s.queue_stack.visible_child_name().as_deref(),
        Some("rows")
    );
    assert_eq!(f.w.s.shell.root.glyph(), "\u{529B}");
    assert!(
        matches!(&f.script.asked.borrow()[0], Asked::OpenFiles { title, .. } if title == "Add images")
    );
    for e in f.w.s.queue.borrow().entries() {
        assert!(
            e.hydrus_hash.as_deref().is_some_and(|h| h.len() == 64),
            "hash filled"
        );
        assert_eq!(e.status, MatchStatus::NotSearched);
    }
    // Everything already there: a box says so, and nothing is added.
    f.script.push(Answer::Files(imgs.clone()));
    f.activate("add-files");
    assert!(pump_until(10_000, || f.script.count_shown() == 1));
    let shown = f.script.last_shown();
    assert_eq!(
        (
            shown.kind,
            shown.title.as_str(),
            shown.text.as_str(),
            shown.buttons
        ),
        (
            Kind::Information,
            "Add Files",
            "All 3 file(s) are already in the list.",
            Buttons::Ok
        )
    );
    assert_eq!(f.len(), 3);
    // One new, two known: the status says so.
    let more = make_images(&f.dir.path().join("pics"), &["d.gif"]);
    let mut batch = imgs[..2].to_vec();
    batch.extend(more);
    f.script.push(Answer::Files(batch));
    f.activate("add-files");
    assert!(pump_until(10_000, || f.len() == 4));
    assert_eq!(f.say(), "Added 1 file(s), 4 total (2 duplicate(s) skipped)");
    // Same content under another name is a duplicate too (the hash round).
    let copy = f.dir.path().join("pics").join("copy-of-a.png");
    std::fs::copy(&imgs[0], &copy).unwrap();
    f.script.push(Answer::Files(vec![copy]));
    f.activate("add-files");
    assert!(pump_until(10_000, || f.script.count_shown() == 2));
    assert_eq!(
        f.script.last_shown().text,
        "All 1 file(s) are already in the list."
    );
    // Nothing chosen: no job, no change.
    f.script.push(Answer::Files(vec![]));
    f.activate("add-files");
    pump_for(50);
    assert_eq!(f.len(), 4);
    f.close();
}

fn add_folder_scans_and_drop_filters(app: &adw::Application) {
    let f = fx(app);
    let folder = f.dir.path().join("lib");
    make_images(&folder.join("sub"), &["x.png", "y.mp4"]);
    make_images(&folder, &["top.jpg", "notes.txt"]);
    f.script.push(Answer::Path(Some(folder.clone())));
    f.activate("add-folder");
    assert!(
        pump_until(10_000, || f.len() == 3),
        "png, mp4 and jpg, not the .txt"
    );
    assert!(
        matches!(&f.script.asked.borrow()[0], Asked::Folder { title } if title == "Add folder")
    );
    // Cancelled chooser: nothing.
    f.script.push(Answer::Path(None));
    f.activate("add-folder");
    pump_for(50);
    assert_eq!(f.len(), 3);
    // A drop keeps folders (scanned) and files with an image extension.
    let other = f.dir.path().join("drop");
    let imgs = make_images(&other, &["p.png", "q.txt"]);
    f.w.drop_paths(imgs);
    assert!(pump_until(10_000, || f.len() == 4));
    f.close();
}

fn a_second_add_waits_and_the_window_keeps_working(app: &adw::Application) {
    let f = fx(app);
    // A file that never answers: a named pipe with no writer yet.
    let fifo = f.dir.path().join("stalled.png");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    f.script.push(Answer::Files(vec![fifo.clone()]));
    f.activate("add-files");
    assert!(
        pump_until(2_000, || f.say().starts_with("Hashing")),
        "{}",
        f.say()
    );
    assert_eq!(f.say(), "Hashing 1 file(s)\u{2026}");
    // Another add is refused while this one runs.
    let other = make_images(&f.dir.path().join("o"), &["o.png"]);
    f.script.push(Answer::Files(other));
    f.activate("add-files");
    let shown = f.script.last_shown();
    assert_eq!(shown.title, "Add Files");
    assert_eq!(
        shown.text,
        "Please wait for the current batch to finish being added."
    );
    // And the window still handles actions: a theme switch, the log viewer.
    f.activate("view-logs");
    assert!(f.w.s.log_viewer.borrow().is_some());
    pump_for(100);
    // Release the pipe: the hash completes and the file is added.
    let writer = std::thread::spawn({
        let fifo = fifo.clone();
        move || std::fs::write(fifo, b"released").unwrap()
    });
    assert!(pump_until(10_000, || f.len() == 1));
    writer.join().unwrap();
    assert_eq!(f.say(), "Added 1 file(s), 1 total");
    f.close();
}

fn seeded(n: usize, status: MatchStatus) -> Vec<hatate_core::model::Entry> {
    (0..n)
        .map(|i| entry(&format!("/lib/img{i}.png"), status))
        .collect()
}

fn restore_clean_session(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            ..Opts::default()
        },
    );
    assert_eq!(f.len(), 3);
    assert_eq!(f.say(), "Restored 3 image(s) from your last session");
    assert!(!f.w.s.run_banner.root.is_visible());
    assert_eq!(
        f.w.s.queue_stack.visible_child_name().as_deref(),
        Some("rows")
    );
    f.close();
    // `restore_session_on_start` off: nothing is read.
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            tweak: Some(Box::new(|s| s.restore_session_on_start = false)),
            ..Opts::default()
        },
    );
    assert_eq!(f.len(), 0);
    f.close();
}

fn restore_after_a_crash_shows_the_banner(app: &adw::Application) {
    let mut rows = seeded(4, MatchStatus::Good);
    rows[0].sent_to_hydrus = true;
    rows[0].hydrus_import_confirmed = true;
    rows[1].status = MatchStatus::Searching;
    rows[2].status = MatchStatus::NotSearched;
    let f = Fx::new(
        app,
        Opts {
            unclean: true,
            seed: rows,
            ..Opts::default()
        },
    );
    assert_eq!(f.len(), 4);
    assert!(f.w.s.run_banner.root.is_visible());
    let body = f.w.s.run_banner.parts.body.text().to_string();
    assert_eq!(
        body,
        "Hatate did not exit cleanly during this run. Everything up to the last autosave is safe. \
         1 of 4 images are already sent to Hydrus, 1 is decided and waiting for your review, and 2 \
         were never reached."
    );
    // The mid-search row went back to unsearched and is flagged.
    let q = f.w.s.queue.borrow();
    assert_eq!(q.entries()[1].status, MatchStatus::NotSearched);
    assert!(q.entries()[1].interrupted_mid_search);
    drop(q);
    // The banner replaces the status-bar line rather than joining it.
    assert_eq!(f.say(), "Ready");
    assert!(f.w.s.shell.run_note_label.text().starts_with("stopped "));
    f.close();
}

fn nothing_survived_variant(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            unclean: true,
            empty_session_file: true,
            ..Opts::default()
        },
    );
    assert_eq!(f.len(), 0);
    assert_eq!(
        f.say(),
        "Run interrupted \u{2014} nothing survived from the last session"
    );
    assert_eq!(f.w.s.shell.run_note_label.text(), RUN_FAILED_NOTE);
    assert!(f.w.s.nothing_survived.borrow().is_some());
    assert!(!f.w.s.run_banner.root.is_visible());
    // Its button goes back to the ordinary drop zone and clears the note.
    let btn =
        f.w.s
            .nothing_survived
            .borrow()
            .as_ref()
            .unwrap()
            .new_search
            .clone();
    btn.emit_clicked();
    assert!(f.w.s.nothing_survived.borrow().is_none());
    assert_eq!(
        f.w.s.queue_stack.visible_child_name().as_deref(),
        Some("empty")
    );
    assert_eq!(f.w.s.shell.run_note_label.text(), "");
    f.close();
    // A crash with no session file at all shows nothing: "never saved anything".
    let f = Fx::new(
        app,
        Opts {
            unclean: true,
            ..Opts::default()
        },
    );
    assert!(f.w.s.nothing_survived.borrow().is_none());
    f.close();
}

fn missing_files_dialog(app: &adw::Application) {
    let missing: hatate_gtk::window::MissingFiles =
        Arc::new(|paths, _| paths.iter().take(2).cloned().collect::<HashSet<String>>());
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(5, MatchStatus::Good),
            missing: Some(missing),
            ..Opts::default()
        },
    );
    assert!(pump_until(5_000, || f.script.count_shown() == 1));
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.kind, shown.title.as_str(), shown.buttons),
        (Kind::Information, "Missing files", Buttons::Ok)
    );
    assert!(shown.text.starts_with("2 of 5 image(s) in this session are no longer on disk - most likely deleted from Hydrus since the session was saved.\n\nThey're still listed, marked \"missing\" in the File column"));
    assert_eq!(
        f.say(),
        "2 image(s) in this list are no longer on disk - see the File column"
    );
    f.close();
    // Nothing missing: nothing said.
    let none: hatate_gtk::window::MissingFiles = Arc::new(|_, _| HashSet::new());
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(2, MatchStatus::Good),
            missing: Some(none),
            ..Opts::default()
        },
    );
    pump_for(200);
    assert_eq!(f.script.count_shown(), 0);
    f.close();
}

fn save_session_as(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let target = f.dir.path().join("kept");
    f.script.push(Answer::Path(Some(target.clone())));
    f.activate("save-session-as");
    let saved = f.dir.path().join("kept.json");
    assert!(saved.exists(), "the .json suffix is added");
    assert_eq!(named::entry_count(&saved), Some(3));
    assert_eq!(f.say(), "Saved 3 image(s) to kept.json");
    let asked = f.script.asked.borrow().last().cloned().unwrap();
    let Asked::SaveFile {
        title,
        default_name,
        filter,
    } = asked
    else {
        panic!("not a save chooser")
    };
    assert_eq!(title, "Save Session As");
    assert!(
        default_name.contains("hatate-session-") && default_name.ends_with(".json"),
        "{default_name}"
    );
    assert_eq!(filter, "Session files (*.json);;All files (*)");
    // A cancelled chooser saves nothing; an unwritable path warns with Python's words.
    f.script.push(Answer::Path(None));
    f.activate("save-session-as");
    let bad = PathBuf::from("/nonexistent-dir-for-test/x.json");
    f.script.push(Answer::Path(Some(bad.clone())));
    f.activate("save-session-as");
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.kind, shown.title.as_str()),
        (Kind::Warning, "Save Session")
    );
    assert_eq!(
        shown.text,
        format!(
            "Could not write the session to:\n{}\n\nSee Help > View Logs for details.",
            bad.display()
        )
    );
    f.close();
    // An empty list says so and asks nothing.
    let f = fx(app);
    f.activate("save-session-as");
    assert_eq!(
        f.script.last_shown().text,
        "There's nothing in the list to save."
    );
    assert!(f.script.asked.borrow().is_empty());
    f.close();
}

fn open_session(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(2, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let theirs = f.dir.path().join("theirs.json");
    let rows = seeded(5, MatchStatus::Poor);
    named::save(&theirs, &rows, NOW).unwrap();
    // Replacing a non-empty list asks first, and the default answer is No.
    f.script.push(Answer::Path(Some(theirs.clone())));
    f.script.push(Answer::Msg(Response::No));
    f.activate("open-session");
    let q = f.script.last_shown();
    assert_eq!(
        (q.kind, q.title.as_str(), q.buttons, q.default),
        (Kind::Question, "Open Session", Buttons::YesNo, Response::No)
    );
    assert_eq!(
        q.text,
        "Replace the current list of 2 image(s) with 5 image(s) from theirs.json?\n\nAnything unsaved in the current list will be lost. Your files and anything already sent to Hydrus are untouched."
    );
    assert_eq!(f.len(), 2, "No keeps the list");
    f.script.push(Answer::Path(Some(theirs.clone())));
    f.script.push(Answer::Msg(Response::Yes));
    f.activate("open-session");
    assert_eq!(f.len(), 5);
    assert_eq!(f.say(), "Opened theirs.json - 5 image(s)");
    // Not a session: a warning with the path.
    let junk = f.dir.path().join("junk.json");
    std::fs::write(&junk, "[1, 2]").unwrap();
    f.script.push(Answer::Path(Some(junk.clone())));
    f.activate("open-session");
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.kind, shown.title.as_str()),
        (Kind::Warning, "Open Session")
    );
    assert_eq!(
        shown.text,
        format!(
            "That doesn't look like a session file this build can read:\n{}\n\nSee Help > View Logs for details.",
            junk.display()
        )
    );
    // A readable session with no entries.
    let empty = f.dir.path().join("empty.json");
    named::save(&empty, &[], NOW).unwrap();
    f.script.push(Answer::Path(Some(empty.clone())));
    f.script.push(Answer::Msg(Response::Yes));
    f.activate("open-session");
    assert_eq!(
        f.script.last_shown().text,
        format!("No usable entries were found in:\n{}", empty.display())
    );
    assert_eq!(f.len(), 5);
    // While a search runs it refuses.
    f.w.s.queue.borrow_mut().set_search_running(true);
    f.activate("open-session");
    assert_eq!(
        f.script.last_shown().text,
        "A search is running. Stop it before opening a different session."
    );
    f.close();
}

fn clear_session_matches_the_empty_list_quirk(app: &adw::Application) {
    // Python's `clear_session()` runs before the question when the list is empty, so a No still
    // leaves the saved session discarded. The list is empty here because restore is off.
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            tweak: Some(Box::new(|s| s.restore_session_on_start = false)),
            ..Opts::default()
        },
    );
    assert_eq!(f.saved_count(), 3);
    f.script.push(Answer::Msg(Response::No));
    f.activate("clear-session");
    let q = f.script.last_shown();
    assert_eq!(
        (q.title.as_str(), q.default),
        ("Clear Saved Session", Response::No)
    );
    assert_eq!(
        q.text,
        "Empty the current list and discard the saved session?\n\nThis only affects this app's list - your files and anything already sent to Hydrus are untouched."
    );
    assert_eq!(
        f.saved_count(),
        0,
        "the session was discarded before the question"
    );
    f.close();
    // With a list, No changes nothing and Yes clears both.
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    f.script.push(Answer::Msg(Response::No));
    f.activate("clear-session");
    assert_eq!((f.len(), f.saved_count()), (3, 3));
    f.script.push(Answer::Msg(Response::Yes));
    f.activate("clear-session");
    assert_eq!((f.len(), f.saved_count()), (0, 0));
    assert_eq!(f.say(), "Cleared the list and the saved session");
    assert_eq!(
        f.w.s.queue_stack.visible_child_name().as_deref(),
        Some("empty")
    );
    f.close();
}

fn persisted_quota_pause(app: &adw::Application) {
    let live = QuotaPause {
        paused_at: NOW - 3600.0,
        searched: 12,
        remaining: 1234,
        reset_at: hatate_core::timeutil::next_utc_midnight_iso(NOW - 2.0 * 86_400.0),
    };
    // The reset is two days behind the clock: the allowance has come back.
    let reset = live.reset_epoch().unwrap();
    let setup = move |paths: &hatate_core::paths::Paths| {
        QuotaPauseFile::new(paths).record(&live).unwrap();
    };
    let f = Fx::new(
        app,
        Opts {
            setup: Some(Box::new(setup)),
            ..Opts::default()
        },
    );
    assert!(reset < NOW);
    assert!(!f.w.s.quota_banner.root.is_visible());
    assert!(
        QuotaPauseFile::new(&f.paths).read().is_none(),
        "a stale pause is deleted"
    );
    f.close();

    let fresh = QuotaPause {
        paused_at: NOW - 600.0,
        searched: 5,
        remaining: 1234,
        reset_at: hatate_core::timeutil::next_utc_midnight_iso(NOW),
    };
    let fresh2 = fresh.clone();
    let setup = move |paths: &hatate_core::paths::Paths| {
        QuotaPauseFile::new(paths).record(&fresh2).unwrap()
    };
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(2, MatchStatus::Good),
            setup: Some(Box::new(setup)),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    assert!(f.w.s.quota_banner.root.is_visible());
    assert_eq!(f.script.count_shown(), 0, "a pause on disk raises no modal");
    let quota = hatate_gtk::text::describe_quota_pause(&fresh, NOW);
    // The restore status and the quota text join with a middle dot.
    assert_eq!(
        f.say(),
        format!("Restored 2 image(s) from your last session \u{b7} {quota}")
    );
    // The strip's note and estimate, as literal text (a constant compared with itself would pass
    // whatever the constant said), and the count on a paused strip: nothing waits to be searched.
    assert_eq!(
        f.w.s.shell.run_note_label.text(),
        "paused \u{2014} quota exhausted"
    );
    assert_eq!(f.w.s.shell.run_eta_label.text(), "ETA paused");
    assert_eq!(f.w.s.shell.run_progress_label.text(), "nothing queued");
    // Dismissing the pause takes the note and the estimate back off the strip.
    f.w.hide_quota_banner();
    assert_eq!(f.w.s.shell.run_note_label.text(), "");
    f.w.refresh_run_progress_label();
    assert_eq!(f.w.s.shell.run_eta_label.text(), "ETA \u{2014}");
    assert!(
        f.w.s
            .quota_banner
            .parts
            .body
            .text()
            .contains("Nothing was lost.")
    );
    f.close();
}

/// `action_edit_conditions` and `MatchConditionsDialog`: the box opens on the stored values; OK
/// stores and saves them, Cancel leaves them.
fn edit_conditions_stores_and_saves_on_ok_and_nothing_on_cancel(app: &adw::Application) {
    use hatate_core::config::MatchConditions;
    let f = fx(app);
    let saved = |f: &Fx| {
        hatate_core::config::Settings::load(&f.paths)
            .settings
            .match_conditions
    };
    assert_eq!(saved(&f), MatchConditions::default());

    f.script.push(Answer::Conditions(None));
    f.w.activate_action("edit-conditions");
    assert!(matches!(
        f.script.asked.borrow().last(),
        Some(Asked::MatchConditions { opened_on }) if opened_on == "5 10 10 true"
    ));
    assert_eq!(
        f.w.s.settings.lock().unwrap().match_conditions,
        MatchConditions::default()
    );

    let new = MatchConditions {
        min_tags_for_good: 8,
        min_width_gain_percent: 12.5,
        min_height_gain_percent: 0.0,
        require_larger_than_local: false,
    };
    f.script.push(Answer::Conditions(Some(new.clone())));
    f.w.activate_action("edit-conditions");
    assert_eq!(f.w.s.settings.lock().unwrap().match_conditions, new);
    assert_eq!(saved(&f), new);

    // It reopens on what was stored.
    f.w.activate_action("edit-conditions");
    assert!(matches!(
        f.script.asked.borrow().last(),
        Some(Asked::MatchConditions { opened_on }) if opened_on == "8 12.5 0 false"
    ));
    f.close();
}

fn theme_action_saves_the_setting(app: &adw::Application) {
    let f = fx(app);
    let saved = |f: &Fx| hatate_core::config::Settings::load(&f.paths).settings.theme;
    assert_eq!(saved(&f), "system");
    f.w.set_theme("light");
    assert_eq!(saved(&f), "light");
    assert_eq!(f.w.s.scheme.get(), hatate_gtk::tokens::Scheme::Light);
    f.w.set_theme("dark");
    assert_eq!(
        (saved(&f), f.w.s.scheme.get()),
        ("dark".into(), hatate_gtk::tokens::Scheme::Dark)
    );
    // Not a mode: ignored, as `action_set_theme` does.
    f.w.set_theme("sepia");
    assert_eq!(saved(&f), "dark");
    // The menu action carries the state.
    gio::prelude::ActionGroupExt::activate_action(&f.w.win, "theme", Some(&"light".to_variant()));
    assert_eq!(saved(&f), "light");
    f.close();
}

fn close_saves_and_marks_a_clean_exit(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            setup: Some(Box::new(|p| {
                std::fs::write(p.crash_running_marker(), "1").unwrap();
            })),
            ..Opts::default()
        },
    );
    let imgs = make_images(&f.dir.path().join("p"), &["a.png", "b.png"]);
    f.script.push(Answer::Files(imgs));
    f.activate("add-files");
    assert!(pump_until(10_000, || f.len() == 2));
    assert_eq!(f.saved_count(), 0, "nothing saved yet");
    let started = hatate_core::crash_marker::CrashMarkers::begin(&f.paths, NOW);
    assert!(started.previous_run_unclean);
    *f.w.s.run_session.borrow_mut() = Some(started.session);
    f.w.close_teardown();
    assert!(f.w.s.closing.get());
    assert_eq!(f.saved_count(), 2, "the final save wrote the list");
    assert!(
        !f.paths.crash_running_marker().exists(),
        "the running marker is gone"
    );
    assert!(
        f.paths.last_clean_exit().exists(),
        "the clean exit is recorded"
    );
    // A second close does nothing more.
    f.w.close_teardown();
    f.close();
}

/// `closeEvent`'s `save_session(self.entries)` is a full write and is not skipped for an empty
/// list: a list emptied since the last save leaves an empty session, not the old rows (found by
/// the action differential, 13 scenarios).
fn close_writes_an_empty_list_over_the_old_rows(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(3, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    assert_eq!(f.len(), 3);
    assert_eq!(f.saved_count(), 3, "the seeded session is on disk");
    *f.w.s.queue.borrow_mut() =
        hatate_core::queue::QueueModel::new(Vec::new(), f.w.s.settings.clone());
    f.w.list_changed();
    f.w.close_teardown();
    assert_eq!(f.saved_count(), 0, "the final save wrote the empty list");
    f.close();
}

fn autosave_writes_the_list_off_the_main_thread(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(2, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    f.w.s
        .queue
        .borrow_mut()
        .push(entry("/lib/extra.png", MatchStatus::NotSearched));
    assert_eq!(f.saved_count(), 2);
    f.w.autosave_now();
    assert!(
        pump_until(10_000, || f.saved_count() == 3),
        "the autosave landed"
    );
    // An empty list is never written over a session that has contents.
    f.w.s.queue.replace(hatate_core::queue::QueueModel::new(
        vec![],
        f.w.s.settings.clone(),
    ));
    f.w.autosave_now();
    pump_for(150);
    assert_eq!(f.saved_count(), 3);
    f.close();
}

fn log_viewer_and_open_log(app: &adw::Application) {
    let f = fx(app);
    f.activate("view-logs");
    let v = f.w.s.log_viewer.borrow().clone().expect("viewer");
    assert_eq!(v.window.title().as_deref(), Some("Logs"));
    assert!(v.path_label.text().starts_with("Log file: "));
    assert!(v.path_label.text().ends_with("hatate-rs.log"));
    assert_eq!(v.body(), "(no log entries yet)");
    assert_eq!(v.crash_log_btn.label().as_deref(), Some("View Crash Log"));
    v.toggle_crash_log();
    assert!(v.path_label.text().starts_with("Crash log: "));
    assert_eq!(v.body(), "(no crash log yet - nothing has crashed)");
    assert_eq!(v.crash_log_btn.label().as_deref(), Some("View App Log"));
    // The crash banner's button opens it pre-switched.
    f.w.open_crash_log();
    assert!(
        f.w.s
            .log_viewer
            .borrow()
            .as_ref()
            .unwrap()
            .showing_crash_log()
    );
    // Open Matched URLs Log: absent, then present.
    f.activate("open-log");
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.title.as_str(), shown.text.as_str()),
        ("Matched URLs Log", "The log file doesn't exist yet.")
    );
    let log = PathBuf::from(f.w.s.settings.lock().unwrap().log_file_path.clone());
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(&log, "x").unwrap();
    f.activate("open-log");
    assert_eq!(*f.opened.borrow(), vec![log]);
    f.close();
}

fn fonts_cover_the_status_glyphs_and_kanji(app: &adw::Application) {
    let f = fx(app);
    let ctx = f.w.win.pango_context();
    let has = |family: &str, ch: char| {
        let desc = gtk4::pango::FontDescription::from_string(&format!("{family} 14"));
        let font = ctx
            .load_font(&desc)
            .unwrap_or_else(|| panic!("{family} did not load"));
        font.describe()
            .family()
            .is_some_and(|f| f.as_str().eq_ignore_ascii_case(family))
            && font.has_char(ch)
    };
    for ch in hatate_gtk::fonts::STATUS_GLYPHS {
        assert!(has("Ryoku Status", ch), "Ryoku Status lacks {ch:?}");
    }
    for ch in hatate_gtk::fonts::KANJI_GLYPHS {
        assert!(has("Ryoku Kanji", ch), "Ryoku Kanji lacks {ch:?}");
    }
    for family in ["Space Grotesk", "JetBrains Mono", "Source Serif 4"] {
        assert!(has(family, 'A'), "{family} did not load");
    }
    // `LoadOptions` is part of the public restore contract the scenarios rely on.
    let _ = (LoadOptions::at(NOW), SessionStore::new(&f.paths));
    f.close();
}

/// The chain of widgets that sets a width: each step is the child asking the most, with all the
/// children's minimums beside it. Printed so a host whose fonts differ from the author's can say
/// what its number was made of.
fn widest_chain(root: &gtk4::Widget) -> String {
    let mut out = Vec::new();
    let mut w = root.clone();
    for _ in 0..40 {
        let min = w.measure(gtk4::Orientation::Horizontal, -1).0;
        let label = w
            .downcast_ref::<gtk4::Label>()
            .map(|l| format!(" {:?}", l.text()))
            .unwrap_or_default();
        let mut kids = Vec::new();
        let mut best: Option<(i32, gtk4::Widget)> = None;
        let mut c = w.first_child();
        while let Some(child) = c {
            if child.is_visible() {
                let m = child.measure(gtk4::Orientation::Horizontal, -1).0;
                kids.push(m.to_string());
                if best.as_ref().is_none_or(|(b, _)| m > *b) {
                    best = Some((m, child.clone()));
                }
            }
            c = child.next_sibling();
        }
        out.push(format!(
            "{}[{}]={min}{label} ({})",
            w.type_().name(),
            w.css_name(),
            kids.join("+")
        ));
        match best {
            Some((_, b)) => w = b,
            None => break,
        }
    }
    out.join(" > ")
}

/// The content's own minimum, with no `set_size_request` floor: width, then height at that width.
fn content_min(f: &Fx) -> (i32, i32) {
    let content =
        libadwaita::prelude::AdwApplicationWindowExt::content(&f.w.win).expect("window content");
    let (w, _, _, _) = content.measure(gtk4::Orientation::Horizontal, -1);
    let (h, _, _, _) = content.measure(gtk4::Orientation::Vertical, w);
    (w, h)
}

/// Design gate 2.5: the stated minimum fits every state of the shell's surfaces, dark and light,
/// and the window really is that size when it is given it (outer rect recorded).
fn minimum_size_fits_every_state_in_both_schemes(app: &adw::Application) {
    use hatate_core::quota_pause::{QuotaPause, QuotaPauseFile};
    use hatate_gtk::window::{MIN_HEIGHT, MIN_WIDTH};
    let mut worst = (0, 0);
    for scheme in ["dark", "light"] {
        let tweak = move |s: &mut hatate_core::config::Settings| s.theme = scheme.to_owned();
        type Build = fn(&adw::Application, &str) -> Fx;
        let states: [(&str, Build, &str); 8] = [
            (
                "queue-empty",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "queue",
            ),
            (
                "queue-rows",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            seed: seeded(5, MatchStatus::Good),
                            missing: Some(Arc::new(|_, _| HashSet::new())),
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "queue",
            ),
            (
                "queue-crashed-banner",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            unclean: true,
                            seed: seeded(5, MatchStatus::Good),
                            missing: Some(Arc::new(|_, _| HashSet::new())),
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "queue",
            ),
            (
                "queue-nothing-survived",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            unclean: true,
                            empty_session_file: true,
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "queue",
            ),
            (
                "queue-quota-banner",
                |app, scheme| {
                    let fresh = QuotaPause {
                        paused_at: NOW - 600.0,
                        searched: 5,
                        remaining: 1234,
                        reset_at: hatate_core::timeutil::next_utc_midnight_iso(NOW),
                    };
                    Fx::new(
                        app,
                        Opts {
                            seed: seeded(5, MatchStatus::Good),
                            missing: Some(Arc::new(|_, _| HashSet::new())),
                            setup: Some(Box::new(move |p| {
                                QuotaPauseFile::new(p).record(&fresh).unwrap()
                            })),
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "queue",
            ),
            (
                "review",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "review",
            ),
            (
                "queue-failed-banner",
                |app, scheme| {
                    let f = Fx::new(
                        app,
                        Opts {
                            seed: seeded(5, MatchStatus::NotSearched),
                            missing: Some(Arc::new(|_, _| HashSet::new())),
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| c.theme = s.clone()
                            })),
                            ..Opts::default()
                        },
                    );
                    f.w.show_failed_banner(
                        "Hatate could not make its temporary folder for automatic imports. Check \
                         that the cache folder exists and can be written to.",
                    );
                    f
                },
                "queue",
            ),
            (
                "activity",
                |app, scheme| {
                    Fx::new(
                        app,
                        Opts {
                            tweak: Some(Box::new({
                                let s = scheme.to_owned();
                                move |c| {
                                    c.theme = s.clone();
                                    // Every extra on: the widest chip row the page can have.
                                    c.enable_tracemoe = true;
                                    c.enable_iqdb3d = true;
                                    c.enable_google_images = true;
                                    c.enable_google_lens = true;
                                    c.enable_yandex = true;
                                    c.enable_pawchive = true;
                                    c.secondary_engine_mode = "always".to_owned();
                                }
                            })),
                            ..Opts::default()
                        },
                    )
                },
                "activity",
            ),
        ];
        let _ = tweak;
        for (name, build, mode) in states {
            let f = build(app, scheme);
            f.w.set_mode(mode);
            pump_for(150);
            let (w, h) = content_min(&f);
            let _ = h;
            println!("MINSIZE {name} {scheme} content_min_width={w}");
            // The window at the stated minimum: its outer rect, and what the content is given
            // inside it. Height depends on width (wrapped prose), so the need is measured at the
            // width the content is actually given.
            f.w.win.set_default_size(1, 1);
            pump_for(200);
            let content = libadwaita::prelude::AdwApplicationWindowExt::content(&f.w.win).unwrap();
            let (cw, ch) = (content.width(), content.height());
            let (_, need_h_at_cw, _, _) = content.measure(gtk4::Orientation::Vertical, cw);
            // What the toplevel asks for before it knows a width (GTK 4.14's FlowBox answers this
            // with its chips stacked in one column: the Activity page then wanted 909 px, DAN-2324).
            let (need_h_unconstrained, _, _, _) = content.measure(gtk4::Orientation::Vertical, -1);
            println!(
                "OUTER {name} {scheme} window={}x{} content={cw}x{ch} needs={w}x{need_h_at_cw} unconstrained_min_h={need_h_unconstrained}",
                f.w.win.width(),
                f.w.win.height()
            );
            assert!(
                f.w.win.width() <= MIN_WIDTH && f.w.win.height() <= MIN_HEIGHT,
                "{name} ({scheme}): the window had to grow past the stated minimum {MIN_WIDTH}x{MIN_HEIGHT}"
            );
            assert!(
                cw >= w && ch >= need_h_at_cw,
                "{name} ({scheme}) needs {w}x{need_h_at_cw} but the stated minimum gives it {cw}x{ch}"
            );
            // The content must also fit INSIDE the window: a child given more than the window
            // has runs off its bottom edge, and `ch >= need` above cannot see that.
            assert!(
                ch <= f.w.win.height(),
                "{name} ({scheme}): the content is allocated {ch} px in a window {} px tall; it runs off the bottom",
                f.w.win.height()
            );
            assert!(
                need_h_unconstrained <= MIN_HEIGHT,
                "{name} ({scheme}): asked for {need_h_unconstrained} px before a width is known; the stated minimum is {MIN_HEIGHT}"
            );
            worst = (worst.0.max(w), worst.1.max(need_h_at_cw));
            f.close();
        }
    }
    println!(
        "MINSIZE worst content_min={}x{} stated={MIN_WIDTH}x{MIN_HEIGHT}",
        worst.0, worst.1
    );
}

// ---- 6.e: running searches from the window ---------------------------------------------------

/// Settings every search scenario starts from: IQDB only, no pacing wait, no cache, nothing that
/// would reach a network.
fn search_tweak() -> Tweak {
    Box::new(|s: &mut hatate_core::config::Settings| {
        s.primary_engine = "iqdb".to_owned();
        s.secondary_engine_mode = "disabled".to_owned();
        s.enable_ascii2d = false;
        s.enable_tracemoe = false;
        s.enable_pawchive_index = false;
        s.use_search_cache = false;
        s.delay_min_seconds = 0.0;
        s.delay_max_seconds = 0.0;
        s.match_conditions.require_larger_than_local = false;
    })
}

/// `n` unsearched rows whose files exist (a 4 x 3 PNG each), under `dir`.
fn unsearched(dir: &std::path::Path, n: usize) -> Vec<hatate_core::model::Entry> {
    std::fs::create_dir_all(dir).unwrap();
    (0..n)
        .map(|i| {
            let p = dir.join(format!("img{i}.png"));
            std::fs::write(&p, common::search::PNG).unwrap();
            entry(&p.to_string_lossy(), MatchStatus::NotSearched)
        })
        .collect()
}

fn search_fx(app: &adw::Application, pics: &std::path::Path, n: usize) -> Fx {
    Fx::new(
        app,
        Opts {
            seed: unsearched(pics, n),
            tweak: Some(search_tweak()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    )
}

fn statuses(f: &Fx) -> Vec<MatchStatus> {
    f.w.s
        .queue
        .borrow()
        .entries()
        .iter()
        .map(|e| e.status)
        .collect()
}

fn start_button(f: &Fx) -> String {
    f.w.s
        .shell
        .search_toggle_btn
        .label()
        .unwrap_or_default()
        .to_string()
}

/// `action_start_search` over three rows: the strip, the status line, the button and the rows
/// follow the worker, and the list is the searched one when it ends.
fn start_search_runs_the_batch(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 3);
    for _ in 0..3 {
        f.search
            .engines
            .iqdb
            .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    }
    assert_eq!(start_button(&f), "\u{25B6} Start Search");
    assert_eq!(f.w.s.shell.run_progress_label.text(), "3 not searched");
    f.activate("toggle-search");
    // At once: the gauge is at 0 of 3, the strip is live, the button says Stop, and the rows are
    // still the list's own until the worker reports.
    assert_eq!(start_button(&f), "\u{25A0} Stop Search");
    assert!(f.w.s.run_active.get());
    assert!(f.w.s.queue.borrow().search_running());
    assert_eq!(f.w.s.run_progress.get(), (0, 3));
    assert_eq!(f.w.s.shell.run_progress_label.text(), "0/3 searched");
    assert_eq!(f.say(), "Searching 3 image(s)\u{2026}");
    assert_eq!(f.search.resets.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        pump_until(10_000, || !f.w.s.run_active.get()),
        "the run never finished: {}",
        f.say()
    );
    assert_eq!(statuses(&f), vec![MatchStatus::NotFound; 3]);
    assert_eq!(f.w.s.run_progress.get(), (3, 3));
    assert_eq!(f.w.s.shell.run_progress_label.text(), "3/3 searched");
    assert_eq!(f.say(), "Search finished");
    assert_eq!(start_button(&f), "\u{25B6} Start Search");
    assert!(f.w.s.shell.search_toggle_btn.is_sensitive());
    assert!(!f.w.s.queue.borrow().search_running());
    // The ETA is not a lie about a run that is over: the strip is back to its idle reading.
    assert_eq!(f.w.s.shell.run_eta_label.text(), IDLE_ETA);
    // The engine order the scripted engines saw: one IQDB call per row.
    assert_eq!(f.search.engines.calls(), vec!["iqdb"; 3]);
    f.close();
}

/// `test_starting_a_search_clears_it` (test_interrupted_banner.py): after a crash the strip says
/// "stopped 2h 14m ago"; starting a search forgets that, because it describes the run before.
fn starting_a_search_clears_the_stopped_note(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = Fx::new(
        app,
        Opts {
            unclean: true,
            seed: unsearched(pics.path(), 2),
            tweak: Some(search_tweak()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    assert!(
        f.w.s.shell.run_note_label.text().starts_with("stopped "),
        "{}",
        f.w.s.shell.run_note_label.text()
    );
    for _ in 0..2 {
        f.search
            .engines
            .iqdb
            .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    }
    f.activate("toggle-search");
    assert_eq!(f.w.s.shell.run_note_label.text(), "");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    f.close();
}

/// `action_start_search`'s two refusals.
fn start_search_with_nothing_to_do_or_one_running(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = Fx::new(
        app,
        Opts {
            seed: vec![entry(
                &pics.path().join("done.png").to_string_lossy(),
                MatchStatus::Good,
            )],
            tweak: Some(search_tweak()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    f.activate("toggle-search");
    let shown = f.script.last_shown();
    assert_eq!(shown.title, "Search");
    assert_eq!(shown.text, "No unsearched images in the list.");
    assert_eq!(shown.buttons, Buttons::Ok);
    assert!(!f.w.s.queue.borrow().search_running());
    f.close();

    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 2);
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::After(
            std::time::Duration::from_millis(400),
            Ok(Vec::new()),
        ));
    f.activate("toggle-search");
    assert!(f.w.s.queue.borrow().search_running());
    // A second Start while one runs (the menu cannot do it, a shortcut or the MCP tool can).
    f.w.action_start_search();
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.title.as_str(), shown.text.as_str()),
        ("Search", "A search is already running.")
    );
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    f.close();
}

/// `action_stop_search`: "Stopping after current image\u{2026}", the button off until the worker
/// is done, and the image under way is finished and kept.
fn stop_search_finishes_the_image_under_way(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 3);
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::After(
            std::time::Duration::from_millis(300),
            Ok(Vec::new()),
        ));
    f.activate("toggle-search");
    // The first image is in the engine's hands; Stop now.
    pump_for(50);
    f.activate("toggle-search");
    assert_eq!(f.say(), "Stopping after current image\u{2026}");
    assert!(!f.w.s.shell.search_toggle_btn.is_sensitive());
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert_eq!(
        statuses(&f),
        vec![
            MatchStatus::NotFound,
            MatchStatus::NotSearched,
            MatchStatus::NotSearched
        ]
    );
    assert_eq!(f.say(), "Search finished");
    assert!(f.w.s.shell.search_toggle_btn.is_sensitive());
    assert_eq!(start_button(&f), "\u{25B6} Start Search");
    assert_eq!(f.w.s.shell.run_progress_label.text(), "1/3 searched");
    // Interrupted in-session shows no banner: the user just did it.
    assert!(!f.w.s.run_banner.root.is_visible());
    f.close();
}

/// A daily quota that runs out mid-batch: no modal, the banner with both actions, the pause on
/// disk and on the strip; "Continue with other engines" resumes without SauceNAO for this run
/// only.
fn a_quota_pause_shows_the_banner_and_continue_resumes(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = Fx::new(
        app,
        Opts {
            seed: unsearched(pics.path(), 3),
            tweak: Some(Box::new(|s: &mut hatate_core::config::Settings| {
                search_tweak()(s);
                s.primary_engine = "saucenao".to_owned();
                s.saucenao.pause_search_on_quota_exhausted = true;
                s.continue_without_saucenao_on_quota = false;
            })),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    for _ in 0..3 {
        f.search
            .engines
            .saucenao
            .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    }
    // The quota SauceNAO's last response reported: the strip's readout is "SauceNAO " (no colon)
    // plus the daily count, and its tooltip is the burst window (DAN-2326).
    *f.search.last_quota.lock().unwrap() = Some(SauceNaoQuota {
        short_remaining: Some(4),
        short_limit: Some(6),
        long_remaining: Some(198),
        long_limit: Some(200),
    });
    // The allowance is spent after the first request.
    f.search
        .quota_after_calls
        .store(1, std::sync::atomic::Ordering::SeqCst);
    f.activate("toggle-search");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    let readout = &f.w.s.shell.saucenao_quota_label;
    assert_eq!(readout.text(), "SauceNAO 2/200 used today");
    assert_eq!(
        readout.root.tooltip_text().as_deref(),
        Some("2/6 used in the current ~30s window")
    );
    // The banner's Review button works now (6.f): no reason to give for a disabled button.
    assert!(f.w.s.quota_banner.review.is_sensitive());
    assert_eq!(f.w.s.quota_banner.review.tooltip_text(), None);
    assert_eq!(
        statuses(&f),
        vec![
            MatchStatus::NotFound,
            MatchStatus::NotSearched,
            MatchStatus::NotSearched
        ]
    );
    assert_eq!(f.script.count_shown(), 0, "a quota pause raises no modal");
    assert!(f.w.s.quota_banner.root.is_visible());
    assert!(f.w.s.quota_paused.get());
    assert_eq!(
        f.w.s.shell.run_note_label.text(),
        "paused \u{2014} quota exhausted"
    );
    // `test_review_is_the_primary_and_continue_is_the_secondary`, the half a test can read: the
    // second action's words, and that the first is not offered while nothing has been decided
    // (the one row searched is a NotFound). Which is primary and their order are judged in the
    // shots.
    assert_eq!(
        f.w.s.quota_banner.keep_going.label().as_deref(),
        Some("Continue with other engines")
    );
    assert!(!f.w.s.quota_banner.review.is_visible());
    assert_eq!(f.w.s.shell.run_eta_label.text(), "ETA paused");
    let pause = QuotaPauseFile::new(&f.paths)
        .read()
        .expect("the pause is on disk");
    assert_eq!((pause.searched, pause.remaining), (1, 2));
    let texts = texts_under(f.w.s.quota_banner.root.upcast_ref());
    assert!(
        texts.iter().any(|t| t.contains("2 ")),
        "the banner counts what is left: {texts:?}"
    );
    // The run ending does not wipe the pause from the strip: the status line is the run's, the
    // note and the banner are the pause's.
    assert_eq!(f.say(), "Search finished");

    // Continue with other engines: SauceNAO is left out of this run, the saved setting is not
    // touched, the banner and the persisted pause go.
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    f.w.s.quota_banner.keep_going.emit_clicked();
    assert!(!f.w.s.quota_banner.root.is_visible());
    assert!(!f.w.s.quota_paused.get());
    assert!(QuotaPauseFile::new(&f.paths).read().is_none());
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert_eq!(statuses(&f), vec![MatchStatus::NotFound; 3]);
    assert!(
        !f.w.s
            .settings
            .lock()
            .unwrap()
            .continue_without_saucenao_on_quota,
        "the saved setting is exactly as the user has it"
    );
    f.close();
}

/// `_engine_alert_forwarder` / `_on_engine_alert` / `engine_alerts.unsubscribe`: the channel is
/// open for the length of a run, an alert becomes a warning box and a status line, and an
/// unsubscribed listener stops hearing.
fn engine_alerts_reach_the_user_only_during_a_run(app: &adw::Application) {
    use hatate_core::engine_alerts::EngineAlert;
    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 1);
    let alert = |key: &str| EngineAlert {
        key: key.to_owned(),
        engine: "Google Lens".to_owned(),
        title: "Google Lens needs a browser".to_owned(),
        body: "Lens cannot run: no browser was found.".to_owned(),
        remedy: "install chromium".to_owned(),
    };
    // Before a run nobody is listening.
    assert!(f.search.alerts.raise_alert(&alert("before")));
    pump_for(30);
    assert_eq!(f.script.count_shown(), 0);

    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::After(
            std::time::Duration::from_millis(250),
            Ok(Vec::new()),
        ));
    f.activate("toggle-search");
    pump_for(30);
    assert!(f.search.alerts.raise_alert(&alert("during")));
    assert!(pump_until(2_000, || f.script.count_shown() == 1));
    let shown = f.script.last_shown();
    assert_eq!(shown.kind, Kind::Warning);
    assert_eq!(shown.title, "Google Lens needs a browser");
    assert_eq!(
        shown.text,
        "Lens cannot run: no browser was found.\n\ninstall chromium"
    );
    assert_eq!(shown.buttons, Buttons::Ok);
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    // The listener comes off when the run ends (`test_an_unsubscribed_listener_stops_hearing`).
    assert!(f.search.alerts.raise_alert(&alert("after")));
    pump_for(30);
    assert_eq!(f.script.count_shown(), 1);
    f.close();
}

/// Nothing the engine does blocks the main loop: while the engine sits on a stalled answer the
/// window still handles actions and its timers.
fn the_window_keeps_working_while_an_engine_stalls(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 1);
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::After(
            std::time::Duration::from_millis(700),
            Ok(Vec::new()),
        ));
    let ticks = Rc::new(Cell::new(0u32));
    let t = ticks.clone();
    let id = gtk4::glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        t.set(t.get() + 1);
        gtk4::glib::ControlFlow::Continue
    });
    f.activate("toggle-search");
    pump_for(100);
    assert!(f.w.s.run_active.get(), "the engine is still stalled");
    let before = ticks.get();
    // An action handled while the search waits.
    f.w.set_theme("light");
    assert_eq!(f.w.s.settings.lock().unwrap().theme, "light");
    pump_for(200);
    assert!(
        ticks.get() >= before + 4,
        "the main loop kept turning: {before} -> {}",
        ticks.get()
    );
    assert!(f.w.s.run_active.get());
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    id.remove();
    f.close();
}

/// `_build_sites_menu`, `_on_site_toggled`, `_set_all_sites`: one check per site, the setting
/// follows each click and is saved, Select None / Select All set every check at once.
fn the_sites_menu_toggles_and_saves(app: &adw::Application) {
    let f = Fx::new(
        app,
        Opts {
            tweak: Some(Box::new(|s: &mut hatate_core::config::Settings| {
                s.enabled_sites = vec!["Danbooru".to_owned(), "Pixiv".to_owned()];
            })),
            ..Opts::default()
        },
    );
    let all = menu::all_site_options();
    let state = |i: usize| -> bool {
        f.w.win
            .lookup_action(&menu::site_action(i))
            .and_then(|a| a.state())
            .and_then(|v| v.get::<bool>())
            .expect("a boolean site action")
    };
    for (i, site) in all.iter().enumerate() {
        assert_eq!(state(i), site == "Danbooru" || site == "Pixiv", "{site}");
    }
    let danbooru = all.iter().position(|s| s == "Danbooru").unwrap();
    let gelbooru = all.iter().position(|s| s == "Gelbooru").unwrap();
    // Clicking a site turns it off, and the saved config has it so.
    f.activate(&menu::site_action(danbooru));
    assert!(!state(danbooru));
    let saved = |paths: &hatate_core::paths::Paths| {
        hatate_core::config::Settings::load(paths)
            .settings
            .enabled_sites
    };
    assert_eq!(saved(&f.paths), vec!["Pixiv".to_owned()]);
    // And on: the list is in ALL_SITE_OPTIONS order, not click order.
    f.activate(&menu::site_action(gelbooru));
    f.activate(&menu::site_action(danbooru));
    let want: Vec<String> = all
        .iter()
        .filter(|s| ["Danbooru", "Gelbooru", "Pixiv"].contains(&s.as_str()))
        .cloned()
        .collect();
    assert_eq!(saved(&f.paths), want);
    f.activate("sites-select-none");
    assert!((0..all.len()).all(|i| !state(i)));
    assert!(saved(&f.paths).is_empty());
    f.activate("sites-select-all");
    assert!((0..all.len()).all(state));
    assert_eq!(saved(&f.paths), all);
    f.close();
}

/// `_research_rows_with_engine` and `_with_opposite_engine`: the named engine runs whatever the
/// configured order says, with the cache bypassed and the saved row overwritten.
fn re_search_uses_the_engine_it_was_asked_for(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let mut rows = unsearched(pics.path(), 2);
    rows[0].status = MatchStatus::NotFound;
    let f = Fx::new(
        app,
        Opts {
            seed: rows,
            tweak: Some(search_tweak()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let ids: Vec<_> = {
        let q = f.w.s.queue.borrow();
        (0..2).map(|r| q.id_at(r).unwrap()).collect()
    };
    f.search
        .engines
        .saucenao
        .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    f.w.run_menu_action(Action::ResearchWithEngine("saucenao"), &ids[..1]);
    assert_eq!(f.say(), "Searching (SauceNAO only) 1 image(s)\u{2026}");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert_eq!(f.search.engines.calls(), vec!["saucenao"]);
    // Primary is IQDB, so a row with no match flips to SauceNAO.
    f.search
        .engines
        .saucenao
        .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    f.w.run_menu_action(Action::ResearchWithOppositeEngine, &ids[1..]);
    assert_eq!(f.say(), "Searching (opposite engine) 1 image(s)\u{2026}");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert_eq!(f.search.engines.calls(), vec!["saucenao", "saucenao"]);
    // Plain re-search: the configured engine, "Re-searching".
    f.search
        .engines
        .iqdb
        .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    f.w.run_menu_action(Action::Research, &ids[..1]);
    assert_eq!(f.say(), "Re-searching 1 image(s)\u{2026}");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert_eq!(f.search.engines.calls()[2], "iqdb");
    f.close();
}

/// `Ending::Failed`: the worker dies on a staging directory that cannot be made. The window shows
/// a failed run (banner, strip note, counts), the message holds no path and no OS text, nothing
/// crashed, the rows stay as they were, and Dismiss clears it.
fn a_failed_run_is_shown_as_failed_not_as_a_crash(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = Fx::new(
        app,
        Opts {
            seed: unsearched(pics.path(), 2),
            tweak: Some(Box::new(|s: &mut hatate_core::config::Settings| {
                search_tweak()(s);
                s.auto_import_enabled = true;
                s.auto_import_method = "download_send".to_owned();
                s.hydrus.access_key = "synthetic-key".into();
            })),
            setup: Some(Box::new(|paths: &hatate_core::paths::Paths| {
                // A file where the staging directory has to be.
                let staging = paths.auto_import_staging_dir();
                std::fs::create_dir_all(staging.parent().unwrap()).unwrap();
                std::fs::write(&staging, b"in the way").unwrap();
            })),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    f.activate("toggle-search");
    assert!(pump_until(10_000, || !f.w.s.run_active.get()));
    assert!(f.w.s.failed_banner.root.is_visible());
    let said = texts_under(f.w.s.failed_banner.root.upcast_ref());
    let all = said.join(" | ");
    assert!(all.contains("Run failed"), "{all}");
    assert!(
        all.contains("Everything searched so far is saved, and 2 were not reached."),
        "{all}"
    );
    assert!(
        all.contains("Searched") && all.contains("Not searched"),
        "{all}"
    );
    let body = f.w.s.failed_banner.parts.body.text().to_string();
    for bad in ["/", "os error", "Not a directory"] {
        assert!(!body.contains(bad), "the banner body shows {bad:?}: {body}");
    }
    assert!(
        !all.contains(&*f.dir.path().to_string_lossy()),
        "no path: {all}"
    );
    assert_eq!(
        f.w.s.shell.run_note_label.text(),
        "last run failed \u{2014} see log"
    );
    assert_eq!(statuses(&f), vec![MatchStatus::NotSearched; 2]);
    assert_eq!(
        f.script.count_shown(),
        0,
        "a failed run is not a modal crash"
    );
    assert!(!f.w.s.queue.borrow().search_running());
    f.w.s.failed_banner.dismiss.emit_clicked();
    assert!(!f.w.s.failed_banner.root.is_visible());
    assert_eq!(f.w.s.shell.run_note_label.text(), "");
    f.close();
}

/// A row with `n` matches whose URLs the scripted host answers by path.
fn row_with_matches(path: &str, urls: &[&str]) -> hatate_core::model::Entry {
    let mut e = entry(path, MatchStatus::Good);
    e.candidates = urls
        .iter()
        .map(|u| hatate_core::model::Candidate::new(*u))
        .collect();
    e
}

/// `_check_match_availability` / `_on_availability_finished` / `_remove_dead_candidates`: the
/// refusals, the all-clear text, the Yes/No offer with the dead list, and Yes dropping them.
fn the_availability_check_offers_to_drop_the_dead_matches(app: &adw::Application) {
    // No match: said so, nothing started.
    let f = Fx::new(
        app,
        Opts {
            seed: vec![entry("/pics/none.png", MatchStatus::NotFound)],
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let id = f.w.s.queue.borrow().id_at(0).unwrap();
    f.w.run_menu_action(Action::CheckAvailability, &[id]);
    let shown = f.script.last_shown();
    assert_eq!(shown.title, "Check Match Availability");
    assert_eq!(shown.text, "This image has no matches to check.");
    f.close();

    // Every match is `unreachable.invalid`-free: the pre-set verdicts stand in for a network the
    // test never touches; the check itself is `hatate_core`'s (tests/availability_https.rs). Here
    // the shell is driven with the answer the worker would hand back.
    let f = Fx::new(
        app,
        Opts {
            seed: vec![row_with_matches(
                "/pics/two.png",
                &["https://a.test/1", "https://a.test/2"],
            )],
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let id = f.w.s.queue.borrow().id_at(0).unwrap();
    let mut done = f.w.s.queue.borrow().entry(id).cloned().unwrap();
    done.candidates[0].remote_available = Some(false);
    done.candidates[1].remote_available = None;
    f.script.push(Answer::Msg(Response::Yes));
    f.w.on_availability_finished(
        id,
        done,
        hatate_core::availability::EntryCheck {
            gone: 1,
            checked: 2,
        },
    );
    let shown = f.script.last_shown();
    assert_eq!(shown.kind, Kind::Question);
    assert_eq!(shown.buttons, Buttons::YesNo);
    assert!(
        shown.text.starts_with(
            "1 of 2 match(es) are no longer available.\n\nRemove them from this image's match list?"
        ),
        "{}",
        shown.text
    );
    assert!(
        shown
            .text
            .contains("(1 other(s) couldn't be checked conclusively and will be kept.)")
    );
    assert!(shown.text.contains("a.test \u{2014} https://a.test/1"));
    assert_eq!(shown.default, Response::No);
    assert_eq!(f.say(), "Removed 1 unavailable match(es)");
    let urls: Vec<String> =
        f.w.s
            .queue
            .borrow()
            .entry(id)
            .unwrap()
            .candidates
            .iter()
            .map(|c| c.url.clone())
            .collect();
    assert_eq!(urls, ["https://a.test/2"]);

    // No: kept, and said so.
    let mut done = f.w.s.queue.borrow().entry(id).cloned().unwrap();
    done.candidates[0].remote_available = Some(false);
    f.script.push(Answer::Msg(Response::No));
    f.w.on_availability_finished(
        id,
        done,
        hatate_core::availability::EntryCheck {
            gone: 1,
            checked: 1,
        },
    );
    assert_eq!(f.say(), "Availability check finished: 1 gone (kept)");
    assert_eq!(f.w.s.queue.borrow().entry(id).unwrap().candidates.len(), 1);

    // Nothing gone: the all-clear.
    let mut done = f.w.s.queue.borrow().entry(id).cloned().unwrap();
    done.candidates[0].remote_available = Some(true);
    f.w.on_availability_finished(
        id,
        done,
        hatate_core::availability::EntryCheck {
            gone: 0,
            checked: 1,
        },
    );
    assert_eq!(
        f.script.last_shown().text,
        "All 1 match(es) still appear to be available."
    );
    assert_eq!(f.say(), "Availability check finished: nothing gone");

    // A window that is closing says nothing (`test_availability_finished_is_a_no_op_while_closing`).
    let shown_before = f.script.count_shown();
    f.w.s.closing.set(true);
    let mut done = f.w.s.queue.borrow().entry(id).cloned().unwrap();
    done.candidates[0].remote_available = Some(false);
    f.w.on_availability_finished(
        id,
        done,
        hatate_core::availability::EntryCheck {
            gone: 1,
            checked: 1,
        },
    );
    assert_eq!(f.script.count_shown(), shown_before);
    f.close();
}

/// `_check_rows_for_upscaling`: the worker runs the real heuristics over a synthetic PNG, each row
/// gets a verdict and its detail, and the summary says how many were flagged.
fn the_upscale_check_marks_rows_and_summarises(app: &adw::Application) {
    let pics = tempfile::tempdir().unwrap();
    let f = search_fx(app, pics.path(), 2);
    let ids: Vec<_> = {
        let q = f.w.s.queue.borrow();
        (0..2).map(|r| q.id_at(r).unwrap()).collect()
    };
    f.w.run_menu_action(Action::CheckUpscaling, &ids);
    assert_eq!(f.say(), "Checking 2 image(s) for upscaling\u{2026}");
    // Running: a second request is refused.
    f.w.run_menu_action(Action::CheckUpscaling, &ids);
    assert_eq!(f.script.last_shown().text, "A check is already running.");
    assert!(pump_until(20_000, || f.script.count_shown() >= 2));
    {
        let q = f.w.s.queue.borrow();
        for e in q.entries() {
            assert!(
                matches!(e.upscale_verdict.as_deref(), Some("flagged" | "clear")),
                "{:?}",
                e.upscale_verdict
            );
            assert!(
                e.upscale_check_detail
                    .as_deref()
                    .is_some_and(|d| !d.is_empty())
            );
            assert_eq!((e.local_width, e.local_height), (Some(4), Some(3)));
        }
    }
    let shown = f.script.last_shown();
    assert_eq!(shown.title, "Upscale Check Results");
    assert!(
        shown
            .text
            .contains(" of 2 image(s) show possible signs of upscaling.")
    );
    assert!(shown.text.contains("img0.png:\n"));
    assert!(f.say().starts_with("Upscale check finished: "));
    f.close();
}

/// A probe that answers from a script after a pause, so a test can watch the window meanwhile.
struct ScriptedProbe {
    results: Vec<hatate_gtk::parser_health::ProbeResult>,
    pause: std::time::Duration,
}

impl hatate_gtk::parser_health::ProbeRunner for ScriptedProbe {
    fn run(
        &self,
        _timeout: f64,
        on_progress: &dyn Fn(usize, usize, &str),
    ) -> Vec<hatate_gtk::parser_health::ProbeResult> {
        for (i, r) in self.results.iter().enumerate() {
            on_progress(i, self.results.len(), &r.site);
            std::thread::sleep(self.pause);
        }
        self.results.clone()
    }
}

/// `action_parser_health`, `ParserHealthDialog`, `_warn_about_broken_parsers`.
fn parser_health_dialog_and_warning(app: &adw::Application) {
    use hatate_gtk::parser_health::ProbeResult;
    let probe = |site: &str, ok: bool, problem: Option<&str>| ProbeResult {
        site: site.to_owned(),
        ok,
        tags: 7,
        width: Some(800),
        height: Some(600),
        has_preview: true,
        has_file_url: true,
        problem: problem.map(str::to_owned),
        restricted: None,
    };
    let f = Fx::new_with(app, Opts::default(), |env| {
        env.probe = Some(Arc::new(ScriptedProbe {
            results: vec![
                probe("danbooru", true, None),
                probe("e621", false, Some("fetched, but produced no tags")),
            ],
            pause: std::time::Duration::from_millis(150),
        }));
    });
    assert!(f.w.win.lookup_action("parser-health").unwrap().is_enabled());
    f.activate("parser-health");
    let dialog =
        f.w.s
            .parser_health_dialog
            .borrow()
            .clone()
            .expect("the dialog is up");
    let cells = dialog.texts();
    assert_eq!(
        cells[0],
        ["Site", "This session", "Live check", "Last problem"]
    );
    assert_eq!(cells.len(), 1 + 13, "every site, before anything is known");
    assert!(
        cells[1..]
            .iter()
            .all(|r| r[1] == "not used yet" && r[2] == "\u{2014}")
    );

    // The live check runs off the main thread: the window's timers keep turning while it waits,
    // the button is off, and the progress and the summary line follow.
    let ticks = Rc::new(Cell::new(0u32));
    let t = ticks.clone();
    let id = gtk4::glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        t.set(t.get() + 1);
        gtk4::glib::ControlFlow::Continue
    });
    dialog.check_button.emit_clicked();
    assert!(!dialog.check_button.is_sensitive());
    assert!(dialog.progress.is_visible());
    assert_eq!(dialog.status.text(), "Checking\u{2026}");
    pump_for(120);
    assert!(ticks.get() >= 3, "the loop kept turning: {}", ticks.get());
    assert!(pump_until(5_000, || dialog.check_button.is_sensitive()));
    id.remove();
    assert!(!dialog.progress.is_visible());
    assert!(
        dialog
            .status
            .text()
            .starts_with("1 of 2 sites have a problem: e621. A site being down")
    );
    let rows = dialog.texts();
    let e621 = rows.iter().find(|r| r[0] == "e621").unwrap();
    assert_eq!(e621[2], "fetched, but produced no tags");
    assert_eq!(e621[3], "fetched, but produced no tags");
    let dan = rows.iter().find(|r| r[0] == "danbooru").unwrap();
    assert_eq!(dan[2], "7 tags, 800x600, preview, file");
    dialog.window.close();

    // `_warn_about_broken_parsers`: once per site, only after three empties and no success.
    let backend = f.w.s.search.backend_if_built().unwrap();
    let health = &backend.engine.booru.health;
    for _ in 0..2 {
        health.record_empty("rule34", "https://rule34.xxx/p/1");
    }
    f.w.warn_about_broken_parsers();
    assert_eq!(f.say(), "Ready", "two empties are not yet a fault");
    health.record_empty("rule34", "https://rule34.xxx/p/1");
    f.w.warn_about_broken_parsers();
    assert_eq!(
        f.say(),
        "rule34: no tags in 3 attempts - see Help > Parser Health"
    );
    f.w.say("Ready");
    f.w.warn_about_broken_parsers();
    assert_eq!(f.say(), "Ready", "reported once");
    f.close();

    // Without a live check the button says so and stays off.
    let f = fx(app);
    f.activate("parser-health");
    let dialog = f.w.s.parser_health_dialog.borrow().clone().unwrap();
    pump_for(150);
    // Opening it must not leave a site name looking picked: no label holds a selection, and the
    // first focus is on Close (DAN-2297 item A).
    let mut labels = Vec::new();
    let mut stack = vec![dialog.window.child().unwrap()];
    while let Some(w) = stack.pop() {
        if let Some(l) = w.downcast_ref::<gtk4::Label>() {
            labels.push(l.clone());
        }
        let mut c = w.first_child();
        while let Some(x) = c {
            c = x.next_sibling();
            stack.push(x);
        }
    }
    assert!(
        labels.iter().any(|l| l.is_selectable()),
        "the site names stay selectable so they can be copied"
    );
    assert!(
        labels.iter().all(|l| l.selection_bounds().is_none()),
        "a site name opened already selected"
    );
    let focus =
        gtk4::prelude::GtkWindowExt::focus(&dialog.window).expect("the dialog has a first focus");
    assert_eq!(
        focus
            .downcast_ref::<gtk4::Button>()
            .and_then(|b| b.label())
            .as_deref(),
        Some("Close"),
        "first focus is Close"
    );
    assert!(!dialog.check_button.is_sensitive());
    assert_eq!(
        dialog.texts()[0],
        ["Site", "This session", "Last problem"],
        "no Live check column when no live check runs"
    );
    assert_eq!(
        dialog.check_button.tooltip_text().unwrap(),
        "The live check is not part of this build yet."
    );
    // And the same words as a caption, so the reason is on screen without a hover.
    assert!(
        texts_under(dialog.window.child().unwrap().upcast_ref())
            .iter()
            .any(|t| t == "The live check is not part of this build yet.")
    );
    dialog.window.close();
    f.close();
}

/// A log source whose text the test changes.
struct MovingLog(std::cell::RefCell<String>);

impl hatate_gtk::logview::LogSource for MovingLog {
    fn log_path(&self) -> PathBuf {
        PathBuf::from("/synthetic/logs/hatate-rs.log")
    }
    fn recent_log(&self) -> String {
        self.0.borrow().clone()
    }
    fn crash_log_path(&self) -> PathBuf {
        PathBuf::from("/synthetic/logs/crash.log")
    }
    fn recent_crash_log(&self) -> String {
        String::new()
    }
}

/// `ActivityViewMixin`: the engine line-up read from the settings, the pipeline sentence, the log
/// pane that tails only while the page is on screen and the follow toggle is on.
fn activity_page_states_the_engines_and_tails_the_log(app: &adw::Application) {
    let log = Rc::new(MovingLog(std::cell::RefCell::new(String::new())));
    let l2 = log.clone();
    let f = Fx::new_with(
        app,
        Opts {
            tweak: Some(Box::new(|s: &mut hatate_core::config::Settings| {
                s.primary_engine = "iqdb".to_owned();
                s.secondary_engine_mode = "fallback".to_owned();
                s.enable_ascii2d = true;
                s.extras_only_as_fallback = true;
                s.delay_min_seconds = 45.0;
                s.delay_max_seconds = 75.0;
                s.use_search_cache = false;
            })),
            ..Opts::default()
        },
        move |env| env.logs = Some(l2),
    );
    let page = f.w.s.activity.clone();
    // Built with the window: the chips and the sentences are there before the page is shown.
    let chips: Vec<(String, bool)> = page.chip_texts().into_iter().map(|c| (c.0, c.1)).collect();
    assert_eq!(chips.len(), 10);
    assert_eq!(chips[0], ("IQDB \u{b7} primary".to_owned(), true));
    assert_eq!(chips[1], ("SauceNAO \u{b7} fallback".to_owned(), true));
    assert_eq!(chips[2], ("ascii2d \u{b7} fallback".to_owned(), true));
    assert_eq!(chips[3], ("trace.moe \u{b7} off".to_owned(), false));
    assert_eq!(page.chip_texts()[3].2, "Switched off in Settings > Engine.");
    assert_eq!(page.chip_texts()[0].2, "Runs on every image.");
    assert!(page.pipeline.text().contains("IQDB"));
    assert_eq!(
        page.summary.text(),
        "Paced at 45\u{2013}75s an image.  The search cache is off."
    );
    assert!(
        page.footer
            .text()
            .ends_with("/synthetic/logs/hatate-rs.log")
    );
    assert_eq!(page.log_text(), hatate_gtk::activity::NO_LOG_YET);

    // Off the page, nothing tails.
    assert!(page.timer.borrow().is_none());
    f.w.set_mode("activity");
    assert!(page.timer.borrow().is_some(), "on the page the log follows");
    *log.0.borrow_mut() = "first line\nsecond line".to_owned();
    f.w.tail_log();
    assert_eq!(page.log_text(), "first line\nsecond line");
    // The follow toggle off stops the tail; on, it reads at once and starts again.
    page.follow.set_active(false);
    assert!(page.timer.borrow().is_none());
    *log.0.borrow_mut() = "first line\nsecond line\nthird".to_owned();
    page.follow.set_active(true);
    assert_eq!(page.log_text(), "first line\nsecond line\nthird");
    assert!(page.timer.borrow().is_some());
    // The folder button hands the log's folder to the desktop.
    page.open_folder.emit_clicked();
    assert_eq!(
        f.opened.borrow().last().unwrap(),
        &PathBuf::from("/synthetic/logs")
    );
    // Leaving the page stops the tail; Show in Queue goes to Queue.
    page.show_in_queue.emit_clicked();
    assert_eq!(f.w.s.shell.current_mode(), "queue");
    assert!(page.timer.borrow().is_none());
    // Back on the page the engines are restated from the settings as they are now.
    f.w.s.settings.lock().unwrap().enable_tracemoe = true;
    f.w.set_mode("activity");
    assert_eq!(page.chip_texts()[3].0, "trace.moe \u{b7} fallback");
    // DAN-2297 ruling 2: the chips are not Tab stops. Walking Tab from the top of the page never
    // lands in the chip row (the tooltips stay for the pointer), and the walk gets past it to the
    // controls after it: marking only the FlowBox's wrappers unfocusable left the walk stuck on
    // Start Search.
    pump_for(150);
    f.w.win.set_focus_visible(true);
    let mut landed = Vec::new();
    for _ in 0..12 {
        f.w.win.child_focus(gtk4::DirectionType::TabForward);
        let w = gtk4::prelude::GtkWindowExt::focus(&f.w.win).expect("Tab leaves focus somewhere");
        assert!(
            !w.is_ancestor(page.chips.upcast_ref::<gtk4::Widget>())
                && &w != page.chips.upcast_ref::<gtk4::Widget>(),
            "a chip took the Tab focus"
        );
        landed.push(w);
    }
    assert!(
        landed.windows(2).any(|p| p[0] != p[1]),
        "Tab never moved off one widget"
    );
    assert!(
        landed.contains(&page.parser_health.clone().upcast::<gtk4::Widget>()),
        "Tab never reached the control after the chips"
    );
    f.close();
}

/// The first widget under `w` (itself included) with the CSS class `class`.
fn find_class(w: &gtk4::Widget, class: &str) -> Option<gtk4::Widget> {
    if w.has_css_class(class) {
        return Some(w.clone());
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(found) = find_class(&child, class) {
            return Some(found);
        }
        c = child.next_sibling();
    }
    None
}

/// The first button under `w` whose label reads `text`.
fn find_button(w: &gtk4::Widget, text: &str) -> Option<gtk4::Button> {
    if let Some(b) = w.downcast_ref::<gtk4::Button>() {
        if b.label().as_deref() == Some(text) {
            return Some(b.clone());
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(found) = find_button(&child, text) {
            return Some(found);
        }
        c = child.next_sibling();
    }
    None
}

/// What the widget draws, `margin` px of room around its box included, as straight RGBA
/// (`width`, `height`, bytes); the widget's own box starts at `(margin, margin)`.
fn render(w: &gtk4::Widget, margin: i32) -> (usize, usize, Vec<u8>) {
    use gtk4::gdk;
    use gtk4::graphene::Rect;
    let paintable = gtk4::WidgetPaintable::new(Some(w));
    // The paintable's own size is the widget's border box; `Widget::width` is the content box inside
    // the CSS padding and border, and asking for that scales the picture down.
    let (bw, bh) = (
        gdk::prelude::PaintableExt::intrinsic_width(&paintable),
        gdk::prelude::PaintableExt::intrinsic_height(&paintable),
    );
    let snap = gtk4::Snapshot::new();
    gdk::prelude::PaintableExt::snapshot(&paintable, &snap, f64::from(bw), f64::from(bh));
    let node = snap.to_node().expect("snapshot");
    let renderer = w
        .native()
        .and_then(|n| n.renderer())
        .expect("a realized widget has a renderer");
    let (vw, vh) = (bw + 2 * margin, bh + 2 * margin);
    let view = Rect::new(-margin as f32, -margin as f32, vw as f32, vh as f32);
    let tex = renderer.render_texture(&node, Some(&view));
    let mut dl = gdk::TextureDownloader::new(&tex);
    dl.set_format(gdk::MemoryFormat::R8g8b8a8);
    let (bytes, stride) = dl.download_bytes();
    assert_eq!(stride, vw as usize * 4);
    (vw as usize, vh as usize, bytes.to_vec())
}

fn px(img: &(usize, usize, Vec<u8>), x: usize, y: usize) -> [u8; 4] {
    let i = (y * img.0 + x) * 4;
    [img.2[i], img.2[i + 1], img.2[i + 2], img.2[i + 3]]
}

/// DAN-2242: what the design read measured on the shots, asserted on the widgets' own pixels, in
/// both schemes: the ghost kanji's box is 420 px tall (fix 1), the Queue frame is the raised fill
/// (fix 2), the plain `.card` draws no outline outside its box (fix 3), and a hollow control's
/// border is `ink-46` at rest and `ink-58` under the pointer (fix 4).
fn design_follow_ups_paint_as_specified(app: &adw::Application) {
    use hatate_gtk::tokens::{Palette, Scheme};
    for (name, scheme) in [("dark", Scheme::Dark), ("light", Scheme::Light)] {
        let f = Fx::new(
            app,
            Opts {
                tweak: Some(Box::new(move |c| c.theme = name.to_owned())),
                ..Opts::default()
            },
        );
        f.w.win.set_default_size(1100, 800);
        pump_for(400);
        let pal = Palette::of(scheme);
        let root = f.w.win.clone().upcast::<gtk4::Widget>();

        let ghost = find_class(&root, "ghost-kanji").expect("the ghost kanji");
        assert_eq!(
            ghost.height(),
            420,
            "{name}: the ghost label box is 420 px (line-height: 1 of a 420 px face), not the font's own 1.44 em"
        );

        let zone = find_class(&root, "drop-zone").expect("the empty-state frame");
        let img = render(&zone, 0);
        let want = pal.raised.hex();
        let at = |x, y| {
            let p = px(&img, x, y);
            format!("#{:02x}{:02x}{:02x}", p[0], p[1], p[2])
        };
        assert_eq!(
            at(16, img.1 - 16),
            want,
            "{name}: the frame interior is the raised fill"
        );
        assert_eq!(px(&img, 16, img.1 - 16)[3], 255, "{name}: and opaque");

        // The card: libadwaita's own `.card` draws a ring and a shadow outside its box, which a
        // paintable of the card alone does not hold. Read the margin around it from its parent.
        let card = find_class(&root, "card").expect("the Queue card");
        // The nearest ancestor with room around the card.
        let mut up = card.parent();
        let (parent, cx, cy) = loop {
            let p = up.expect("an ancestor with room around the card");
            let at = card
                .compute_point(&p, &gtk4::graphene::Point::new(0.0, 0.0))
                .expect("the card's place in its ancestor");
            if at.x() >= 20.0 && at.y() >= 20.0 {
                break (p, at.x() as usize, at.y() as usize);
            }
            up = p.parent();
        };
        let img = render(&parent, 0);
        let mid_y = cy + card.height() as usize / 2;
        let right = cx + card.width() as usize;
        assert!(right + 20 <= img.0, "{name}: room right of the card");
        let (ref_l, ref_r) = (px(&img, cx - 18, mid_y), px(&img, right + 18, mid_y));
        for d in 1..=8 {
            assert_eq!(
                px(&img, cx - d, mid_y),
                ref_l,
                "{name}: the page is plain {d}px left of the card (no outline or shadow)"
            );
            assert_eq!(
                px(&img, right + d - 1, mid_y),
                ref_r,
                "{name}: the page is plain {d}px right of the card"
            );
        }

        // A hollow control's border: the left edge column of the button, alpha only.
        let folder = find_button(&root, "Add folder\u{2026}").expect("Add folder");
        let edge_alpha = |b: &gtk4::Button| {
            let img = render(b.upcast_ref(), 0);
            f64::from(px(&img, 0, img.1 / 2)[3]) / 255.0
        };
        assert!(
            (edge_alpha(&folder) - pal.ink_46.3).abs() < 0.02,
            "{name}: Add folder's border is ink-46 ({}) at rest, got {}",
            pal.ink_46.3,
            edge_alpha(&folder)
        );
        // `.card` sets `border-radius: 0` for the framed variant: no 6.c page draws one, but
        // `widgets::card(_, true)` is the API the later pages build on, and libadwaita's own
        // `.card` would round its hairline to radius 12 (the corner pixel would then be clear).
        {
            let framed = hatate_gtk::widgets::card(Some("Framed"), true);
            let host = gtk4::Window::new();
            host.set_default_size(300, 120);
            host.set_child(Some(&framed));
            host.present();
            pump_for(300);
            let img = render(framed.upcast_ref(), 0);
            let corner = f64::from(px(&img, 0, 0)[3]) / 255.0;
            assert!(
                (corner - pal.ink_18.3).abs() < 0.02,
                "{name}: a framed card's corner is a square ink-18 hairline ({}), got alpha {corner}",
                pal.ink_18.3
            );
            host.close();
        }
        // D3 (DAN-2245): a disabled hollow control keeps its border at ink-46. libadwaita fades a
        // disabled button with `opacity` / `filter: opacity()`, which would take the edge to half of
        // this; the rule that cancels it (`opacity: 1; filter: none`) is what this reads.
        let query = find_button(&root, "Query Hydrus\u{2026}").expect("Query Hydrus");
        // Query Hydrus is live since 6.g; the sample of a disabled hollow control is made disabled
        // here, which is what the rule below is about.
        query.set_sensitive(false);
        assert!(!query.is_sensitive());
        assert!(
            (edge_alpha(&query) - pal.ink_46.3).abs() < 0.02,
            "{name}: a disabled hollow control keeps its ink-46 border ({}), got {}",
            pal.ink_46.3,
            edge_alpha(&query)
        );
        folder.set_state_flags(gtk4::StateFlags::PRELIGHT, false);
        pump_for(100);
        // The hover wash (ink-10) is painted under the border, so the edge pixel is the two over each other.
        let under_wash = 1.0 - (1.0 - pal.ink_58.3) * (1.0 - pal.ink_10.3);
        assert!(
            (edge_alpha(&folder) - under_wash).abs() < 0.02,
            "{name}: and ink-58 over the ink-10 wash ({under_wash:.3}) under the pointer, got {}",
            edge_alpha(&folder)
        );
        f.close();
    }
}

/// DAN-2242 fix 6 (design gate 2.6): the production prompter shows the default button's focus ring
/// as soon as the dialog opens, and the default of a destructive question is No.
fn a_dialog_shows_the_ring_on_its_default(app: &adw::Application) {
    use hatate_gtk::dialogs::{AdwPrompter, Prompter};
    let f = fx(app);
    assert!(
        !f.w.win.upcast_ref::<gtk4::Window>().gets_focus_visible(),
        "no key has been pressed yet"
    );
    let prompter = AdwPrompter::new(&f.w.win);
    let answered = Rc::new(Cell::new(None));
    let a = answered.clone();
    prompter.message(
        Kind::Question,
        "Clear Saved Session",
        "Empty the current list?",
        Buttons::YesNo,
        Box::new(move |r| a.set(Some(r))),
    );
    pump_for(400);
    assert!(
        f.w.win.upcast_ref::<gtk4::Window>().gets_focus_visible(),
        "the ring is drawn on the dialog's default button the moment it opens"
    );
    let focused = gtk4::prelude::GtkWindowExt::focus(f.w.win.upcast_ref::<gtk4::Window>())
        .expect("focus is inside the dialog");
    let mut label = String::new();
    let mut stack = vec![focused];
    while let Some(w) = stack.pop() {
        if let Some(l) = w.downcast_ref::<gtk4::Label>() {
            label = l.text().to_string();
            break;
        }
        let mut c = w.first_child();
        while let Some(ch) = c {
            stack.push(ch.clone());
            c = ch.next_sibling();
        }
    }
    assert_eq!(
        label, "No",
        "the default button, and the focused one, is No"
    );
    assert_eq!(answered.get(), None, "nothing answered the question");
    f.close();
}

/// `LensConnection` reaches the window from the Lens client's thread through the inbox and is said
/// in the status line; `Ready` says nothing (Lens answered, nothing to report).
fn lens_connection_is_said_in_the_status_line(app: &adw::Application) {
    use hatate_core::engines::lens_search::LensConnection as C;
    use hatate_gtk::search_run::BusEvent;
    let f = fx(app);
    let bus = f.w.s.search.bus.clone();
    let say_after = |c: C| {
        f.w.say("Ready");
        // From another thread, as the Lens client posts it.
        let b = bus.clone();
        std::thread::spawn(move || b.post(BusEvent::Lens(c)))
            .join()
            .unwrap();
        pump_for(60);
        f.say()
    };
    assert_eq!(
        say_after(C::NotReachable),
        "Google Lens: the browser could not be reached - see Help > View Logs"
    );
    assert_eq!(
        say_after(C::WaitingForLens),
        "Google Lens: waiting for an answer\u{2026}"
    );
    assert_eq!(
        say_after(C::Blocked),
        "Google Lens: standing down for this run"
    );
    assert_eq!(say_after(C::Ready), "Ready");
    assert_eq!(f.w.s.search.lens.get(), Some(C::Ready));
    f.close();
}

/// DAN-2245 item 1: the log viewer opens with focus on Close, nothing selected, and selected text
/// (the path line, the log body; focused or not) is ink on an ink-18 wash, at least 4.5:1, not
/// libadwaita's accent blue (3.48:1 dark, 3.40:1 light under the path line), wherever the toolkit
/// paints a selection in this headless session (GTK 4.24 does; 4.14 paints none here, and its
/// numbers come from the shot set).
fn the_log_viewer_opens_on_close_and_selects_in_ink(app: &adw::Application) {
    use hatate_gtk::tokens::{Palette, Rgba, Scheme, contrast};
    for (name, scheme) in [("dark", Scheme::Dark), ("light", Scheme::Light)] {
        let f = Fx::new(
            app,
            Opts {
                tweak: Some(Box::new(move |c| c.theme = name.to_owned())),
                ..Opts::default()
            },
        );
        f.activate("view-logs");
        pump_for(400);
        let viewer = f.w.s.log_viewer.borrow().clone().expect("the log viewer");
        let pal = Palette::of(scheme);
        let focus = GtkWindowExt::focus(&viewer.window).expect("the viewer has a focus");
        assert_eq!(
            focus
                .downcast_ref::<gtk4::Button>()
                .and_then(|b| b.label())
                .as_deref(),
            Some("Close"),
            "{name}: the viewer opens on Close, not on the selectable path line"
        );
        assert_eq!(
            viewer.path_label.selection_bounds(),
            None,
            "{name}: and nothing is selected"
        );

        // What a selection looks like: the background pixel is the wash over the surface under it,
        // and the pixel farthest from it (a glyph) clears 4.5:1.
        // The widget's own render has no surface under it: lay each pixel over the one it sits on.
        // An unfocused selection is not painted at all by every toolkit (GTK 4.14 draws none for a label
        // that is not focused), so it is only read when something is painted; a focused one must be.
        // Every state is read before the scenario fails, so one run on a new toolkit lists them all.
        let problems = std::cell::RefCell::new(Vec::<String>::new());
        let check = |what: &str,
                     w: &gtk4::Widget,
                     surface: Rgba,
                     crop: (usize, usize),
                     must_paint: bool| {
            let to_rgba = |p: [u8; 4]| {
                Rgba(
                    f64::from(p[0]),
                    f64::from(p[1]),
                    f64::from(p[2]),
                    f64::from(p[3]) / 255.0,
                )
                .over(surface)
            };
            pump_for(150);
            let img = render(w, 0);
            let wash = pal.ink_18.over(surface);
            // The most common pixel of a selected line is its background.
            let mut seen: std::collections::HashMap<[u8; 4], usize> = Default::default();
            let (cw, ch) = (img.0.min(crop.0), img.1.min(crop.1));
            for i in 0..cw * ch {
                *seen.entry(px(&img, i % cw, i / cw)).or_default() += 1;
            }
            let bg = to_rgba(*seen.iter().max_by_key(|(_, n)| **n).expect("pixels").0);
            if bg.hex() == surface.hex() {
                // GTK 4.14 paints no selection in a label or view that this headless session has not
                // made "active" (found by hosted ci on DAN-2245): nothing to measure. The rule itself
                // is pinned in `theme::tests` and the shot set measures what each toolkit draws.
                eprintln!(
                    "note: {name}: {what}: no selection is painted on this toolkit ({must_paint})"
                );
                return;
            }
            if (bg.0 - wash.0).abs() > 1.0
                || (bg.1 - wash.1).abs() > 1.0
                || (bg.2 - wash.2).abs() > 1.0
            {
                problems.borrow_mut().push(format!(
                    "{name}: {what}: the selection is the ink-18 wash {}, got {}",
                    wash.hex(),
                    bg.hex()
                ));
            }
            let worst = (0..cw * ch)
                .map(|i| contrast(to_rgba(px(&img, i % cw, i / cw)), wash))
                .fold(0.0_f64, f64::max);
            if worst < 4.5 {
                problems.borrow_mut().push(format!(
                    "{name}: {what}: selected text reaches {worst:.2}:1, below 4.5"
                ));
            }
        };
        // The log body is one short line in a tall view: read the first line's box only.
        // The path line, selected while Close holds focus (the window is never active under Xvfb,
        // so this is the backdrop state), then focused.
        viewer.path_label.select_region(0, -1);
        check(
            "path line",
            viewer.path_label.upcast_ref(),
            pal.page,
            (usize::MAX, usize::MAX),
            false,
        );
        viewer.path_label.grab_focus();
        viewer.path_label.select_region(0, -1);
        check(
            "path line, focused",
            viewer.path_label.upcast_ref(),
            pal.page,
            (usize::MAX, usize::MAX),
            true,
        );
        // The log body.
        let buffer = viewer.text.buffer();
        buffer.select_range(&buffer.start_iter(), &buffer.end_iter());
        check(
            "log body",
            viewer.text.upcast_ref(),
            pal.sunken,
            (130, 14),
            false,
        );
        viewer.text.grab_focus();
        buffer.select_range(&buffer.start_iter(), &buffer.end_iter());
        check(
            "log body, focused",
            viewer.text.upcast_ref(),
            pal.sunken,
            (130, 14),
            true,
        );
        assert!(
            problems.borrow().is_empty(),
            "{}",
            problems.borrow().join("; ")
        );
        viewer.window.close();
        f.close();
    }
}

fn queue_view_holds_5000_rows_and_filters_without_touching_the_queue(app: &adw::Application) {
    use hatate_core::entry_filter::EntryFilter;
    use hatate_core::table_cells::Column;
    let mut seed = seeded(5000, MatchStatus::NotSearched);
    for (i, e) in seed.iter_mut().enumerate() {
        e.status = MatchStatus::ALL[i % MatchStatus::ALL.len()];
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    assert_eq!(view.total_count(), 5000);
    assert_eq!(view.visible_count(), 5000);
    assert_eq!(view.cell(0, Column::File).unwrap().text, "img0.png");
    assert_eq!(view.cell(1, Column::Status).unwrap().text, "\u{25cf} Good");
    // A filter hides rows from the table, never from the queue.
    view.set_filter(EntryFilter {
        statuses: Some([MatchStatus::Good].into_iter().collect()),
        ..EntryFilter::default()
    });
    assert_eq!(view.visible_count(), 5000 / 6 + usize::from(5000 % 6 > 1));
    assert_eq!(f.w.s.queue.borrow().len(), 5000);
    assert_eq!(view.cell(0, Column::File).unwrap().text, "img1.png");
    // The selection resolves through the screen, so a hidden row cannot be reached.
    view.select_row(0);
    let id = view.selected_ids()[0];
    assert_eq!(
        f.w.s.queue.borrow().entry(id).unwrap().path,
        "/lib/img1.png"
    );
    view.set_filter(EntryFilter::default());
    assert_eq!(
        view.selected_ids(),
        vec![id],
        "the selection survives a reset"
    );
    assert_eq!(view.row_of(id), Some(1));
    f.close();
}

fn queue_filter_bar_narrows_by_text_and_menus_reads_hidden_and_clears(app: &adw::Application) {
    let mut seed = seeded(12, MatchStatus::NotSearched);
    for (i, e) in seed.iter_mut().enumerate() {
        e.status = MatchStatus::ALL[i % 3];
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let bar = f.w.s.filter_bar.clone();
    assert_eq!(view.visible_count(), 12);
    assert_eq!(bar.count.label().as_str(), "", "no filter, no reading");
    assert!(!bar.clear.is_sensitive());
    assert_eq!(
        bar.button("Status").label().unwrap().as_str(),
        "Status: All"
    );

    // Typing narrows the list by filename and the reading says how many are held back.
    bar.text.set_text("img1");
    assert_eq!(view.visible_count(), 3, "img1, img10 and img11");
    assert_eq!(bar.count.label().as_str(), "9 hidden");
    assert!(bar.clear.is_sensitive());
    assert_eq!(
        bar.count.tooltip_text().unwrap().as_str(),
        "showing 3 of 12 (the rest are hidden, not removed)"
    );
    bar.clear.emit_clicked();
    assert_eq!(view.visible_count(), 12);
    assert_eq!(bar.text.text().as_str(), "");
    assert_eq!(bar.count.label().as_str(), "");

    // The Status menu: None empties the list, ticking one back isolates it, All restores it.
    let status = bar.button("Status");
    status.activate_action("fstatus.none", None).unwrap();
    assert_eq!(view.visible_count(), 0);
    assert_eq!(status.label().unwrap().as_str(), "Status: 0 selected");
    status.activate_action("fstatus.v0", None).unwrap();
    assert_eq!(view.visible_count(), 4);
    assert_eq!(status.label().unwrap().as_str(), "Status: Not searched");
    status.activate_action("fstatus.all", None).unwrap();
    assert_eq!(view.visible_count(), 12);
    assert_eq!(status.label().unwrap().as_str(), "Status: All");
    // A filter hides rows from the table, never from the queue.
    assert_eq!(f.w.s.queue.borrow().len(), 12);
    f.close();
}

fn queue_select_by_status_under_a_filter_never_selects_a_hidden_row(app: &adw::Application) {
    // DAN-2310 class 1: Python selects the rows its filter hides, and Remove / Reset / Mark
    // reviewed then act on rows nobody can see. Here a hidden row is never selected.
    let mut seed = seeded(12, MatchStatus::NotSearched);
    for (i, e) in seed.iter_mut().enumerate() {
        e.status = MatchStatus::ALL[i % 3];
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let bar = f.w.s.filter_bar.clone();
    // What a save would write: the rows, their statuses and their reviewed marks.
    let snapshot = |f: &Fx| -> Vec<(String, MatchStatus, bool)> {
        f.w.s
            .queue
            .borrow()
            .entries()
            .iter()
            .map(|e| (e.path.clone(), e.status, e.reviewed))
            .collect()
    };
    let before = snapshot(&f);

    // "img1" shows img1, img10 and img11 (statuses 1, 1, 2); the four rows of status 0 are hidden.
    bar.text.set_text("img1");
    assert_eq!(view.visible_count(), 3);
    let hidden_status = MatchStatus::ALL[0];
    f.w.select_rows_by_status(hidden_status);
    assert!(view.selected_ids().is_empty(), "a hidden row was selected");
    assert_eq!(
        f.say(),
        format!(
            "No visible images matching status \"{}\" (4 hidden by the filter)",
            hidden_status.label()
        )
    );
    // Remove and Mark reviewed act on the (empty) selection and change nothing.
    let none = view.selected_ids();
    f.w.remove_rows(&none);
    f.w.set_rows_reviewed(&none, true);
    assert_eq!(
        snapshot(&f),
        before,
        "the queue changed under an empty selection"
    );

    // A status that is partly visible selects only the visible rows and counts only those.
    let part = MatchStatus::ALL[1];
    f.w.select_rows_by_status(part);
    assert_eq!(
        view.selected_ids().len(),
        2,
        "img1 and img10, not img4 or img7"
    );
    assert_eq!(
        f.say(),
        format!("Selected 2 image(s) matching status \"{}\"", part.label())
    );
    // Reset on every visible row leaves the hidden ones as they were
    // (`test_resetting_only_resets_visible_rows`).
    f.w.select_rows_by_predicate("every visible row", |_| true);
    let shown_ids = view.selected_ids();
    assert_eq!(shown_ids.len(), 3, "img1, img10 and img11");
    let before_reset = snapshot(&f);
    f.script.push(Answer::Msg(Response::Yes));
    f.w.reset_rows(&shown_ids);
    for ((path, status, _), (_, old_status, _)) in snapshot(&f).iter().zip(&before_reset) {
        if path.contains("img1") {
            assert_eq!(*status, MatchStatus::NotSearched, "{path} is visible");
        } else {
            assert_eq!(
                status, old_status,
                "reset a row the filter had hidden: {path}"
            );
        }
    }
    // With no filter nothing is hidden, so the old wording stands.
    bar.clear.emit_clicked();
    f.w.select_rows_by_predicate("nothing", |_| false);
    assert_eq!(f.say(), "No images matching nothing");
    f.close();
}

fn queue_clear_re_ticks_every_filter_menu_so_the_ticks_summary_and_count_agree(
    app: &adw::Application,
) {
    // DAN-2310 class 2: Python's Clear leaves the Status / Site / Upscale ticks as they were,
    // so toggling a stale tick does the opposite of what it shows. Here the ticks follow the filter.
    let mut seed = seeded(12, MatchStatus::NotSearched);
    for (i, e) in seed.iter_mut().enumerate() {
        e.status = MatchStatus::ALL[i % 3];
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let bar = f.w.s.filter_bar.clone();
    let status = bar.button("Status");
    status.activate_action("fstatus.v0", None).unwrap();
    assert!(!bar.ticked("Status", 0), "the tick was not removed");
    assert_eq!(view.visible_count(), 8);
    assert_eq!(bar.count.label().as_str(), "4 hidden");
    bar.text.set_text("img1");
    assert!(bar.clear.is_sensitive());

    bar.clear.emit_clicked();
    for prefix in ["Status", "Site", "Upscale"] {
        for n in 0..bar.present(prefix).len() {
            assert!(
                bar.ticked(prefix, n),
                "{prefix} entry {n} is still unticked after Clear"
            );
        }
    }
    assert_eq!(status.label().unwrap().as_str(), "Status: All");
    assert_eq!(bar.text.text().as_str(), "");
    assert_eq!(view.visible_count(), 12);
    assert_eq!(bar.count.label().as_str(), "");
    assert!(!bar.clear.is_sensitive());
    f.close();
}

fn queue_remove_lands_on_the_next_row_that_survives_and_sort_keeps_the_top_selected_row(
    app: &adw::Application,
) {
    use hatate_core::table_cells::Column;
    let f = Fx::new(
        app,
        Opts {
            seed: seeded(5, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let ids = view.visible_ids();
    let path_of = |id| f.w.s.queue.borrow().entry(id).unwrap().path.clone();

    // `_successor_surviving`: removing the selected row selects the one that followed it.
    view.select_row(1);
    f.script.push(Answer::Msg(Response::Remove));
    f.w.remove_rows(&[ids[1]]);
    assert_eq!(
        view.selected_ids()
            .iter()
            .map(|i| path_of(*i))
            .collect::<Vec<_>>(),
        ["/lib/img2.png"]
    );
    // The last row has nothing below it, so the nearest one above is taken.
    view.select_row(3);
    f.script.push(Answer::Msg(Response::Remove));
    f.w.remove_rows(&[ids[4]]);
    assert_eq!(
        view.selected_ids()
            .iter()
            .map(|i| path_of(*i))
            .collect::<Vec<_>>(),
        ["/lib/img3.png"]
    );
    // A selection that survives is left as it is.
    view.select_row(0);
    f.script.push(Answer::Msg(Response::Remove));
    f.w.remove_rows(&[ids[2]]);
    assert_eq!(
        view.selected_ids()
            .iter()
            .map(|i| path_of(*i))
            .collect::<Vec<_>>(),
        ["/lib/img0.png"]
    );

    // `_apply_sort`: the top selected row is the current one and the only one left selected.
    let top = view.visible_ids();
    view.select_ids(&[top[0], top[1]]);
    view.on_header_clicked(Column::File);
    assert_eq!(view.selected_ids().len(), 1);
    assert_eq!(path_of(view.selected_ids()[0]), "/lib/img0.png");
    f.close();
}

fn queue_view_thumbnails_load_fail_and_stay_off_the_main_loop(app: &adw::Application) {
    use hatate_core::thumb::{FAILED_TOOLTIP, ThumbState};
    let pics = tempfile::tempdir().unwrap();
    let good = pics.path().join("good.png");
    image::RgbaImage::from_pixel(640, 320, image::Rgba([10, 120, 200, 255]))
        .save(&good)
        .unwrap();
    let text = pics.path().join("text.png");
    std::fs::write(&text, b"not a picture").unwrap();
    let mut seed = seeded(3, MatchStatus::Good);
    seed[0].path = good.to_string_lossy().into_owned();
    seed[1].path = text.to_string_lossy().into_owned();
    seed[2].path = pics
        .path()
        .join("missing.png")
        .to_string_lossy()
        .into_owned();
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let ids = view.visible_ids();
    // Every drawn row asks, so all three settle: one picture, two failures.
    assert!(
        pump_until(10_000, || ids.iter().all(|id| view.thumb_known(*id))),
        "the drawn rows were never decoded"
    );
    assert_eq!(
        ids.iter()
            .map(|id| view.thumb_state(*id))
            .collect::<Vec<_>>(),
        [ThumbState::Loaded, ThumbState::Failed, ThumbState::Failed]
    );
    assert_eq!(f.w.s.thumbs.in_flight(), 0);
    assert!(FAILED_TOOLTIP.len() > 10);
    // A row rebuilt from the cache keeps its picture without a second decode.
    view.rebuild();
    assert_eq!(view.thumb_state(ids[0]), ThumbState::Loaded);
    assert!(!f.w.s.thumbs.is_running());
    f.close();
}

fn queue_view_thumbnails_stay_off_when_the_source_is_off(app: &adw::Application) {
    // `test_off_reads_no_files_at_all_but_still_finishes`: no file is read, no pass is left
    // running, and the progress strip is not left showing a pass that never ends.
    let pics = tempfile::tempdir().unwrap();
    let good = pics.path().join("good.png");
    image::RgbaImage::from_pixel(64, 64, image::Rgba([10, 120, 200, 255]))
        .save(&good)
        .unwrap();
    let mut seed = seeded(2, MatchStatus::Good);
    seed[0].path = good.to_string_lossy().into_owned();
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            tweak: Some(Box::new(|s| s.thumbnail_source = "off".into())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    pump_for(500);
    for id in view.visible_ids() {
        assert!(
            !view.thumb_known(id),
            "a picture was decoded with the source off"
        );
    }
    assert_eq!(f.w.s.thumbs.in_flight(), 0);
    assert!(!f.w.s.thumbs.is_running());
    f.close();
}

fn queue_context_menu_selects_marks_resets_and_removes(app: &adw::Application) {
    use hatate_core::table_menu::Predicate;
    let mut seed = seeded(12, MatchStatus::NotSearched);
    for (i, e) in seed.iter_mut().enumerate() {
        e.status = MatchStatus::ALL[i % MatchStatus::ALL.len()];
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    // Nothing selected: only the four "Select by" menus.
    assert!(f.w.table_menu(&[]).rows.is_none());

    f.w.select_rows_by_status(MatchStatus::Good);
    assert_eq!(view.selected_ids().len(), 2);
    assert_eq!(
        f.say(),
        "Selected 2 image(s) matching status \"Found (good)\""
    );
    f.w.select_rows_by_predicate(Predicate::MissingFromDisk.description(), |e| {
        Predicate::MissingFromDisk.test(e)
    });
    assert!(
        view.selected_ids().is_empty(),
        "a new selection replaces the old one"
    );
    assert_eq!(f.say(), "No images matching Missing from disk");

    // The menu over those two rows says "2", and its wired entries have handlers.
    f.w.select_rows_by_status(MatchStatus::Good);
    let ids = view.selected_ids();
    let rows = f.w.table_menu(&ids).rows.unwrap();
    assert!(rows.tail.iter().any(|l| matches!(l,
        hatate_core::table_menu::Line::Item(i) if i.label == "Remove 2 selected files")));

    // Reviewed: no question, the status line counts what is left to review.
    f.w.set_rows_reviewed(&ids, true);
    assert!(
        f.say()
            .starts_with("Marked 2 image(s) as reviewed \u{2014} ")
    );
    f.w.set_rows_reviewed(&ids, true);
    assert!(
        f.say().starts_with("Marked 2"),
        "nothing changed, nothing said"
    );

    // Reset asks first, and the question defaults to No.
    f.w.reset_rows(&ids);
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.buttons, shown.default),
        (Buttons::YesNo, Response::No)
    );
    assert!(
        shown
            .text
            .starts_with("Discard the search results for 2 images?")
    );
    assert!(
        view.cell(1, hatate_core::table_cells::Column::Status)
            .is_some()
    );
    f.script.push(Answer::Msg(Response::Yes));
    f.w.reset_rows(&ids);
    assert_eq!(f.say(), "Reset 2 images to Not searched");

    // Remove asks with Cancel as the default, and acts only on Remove.
    f.w.remove_rows(&ids);
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.buttons, shown.default),
        (Buttons::RemoveCancel, Response::Cancel)
    );
    assert_eq!(f.len(), 12, "the default answer removes nothing");
    f.script.push(Answer::Msg(Response::Remove));
    f.w.remove_rows(&ids);
    assert_eq!(f.len(), 10);
    assert_eq!(f.say(), "Removed 2 file(s) from the list");
    f.close();
}

fn queue_view_header_cycle_sorts_the_queue_in_place_and_restores_the_order(app: &adw::Application) {
    use hatate_core::table_cells::Column;
    let mut seed = seeded(4, MatchStatus::Good);
    for (e, name) in seed
        .iter_mut()
        .zip(["/lib/c.png", "/lib/A.png", "/lib/b.png", "/lib/d.png"])
    {
        e.path = name.to_owned();
    }
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let view = f.w.s.queue_view.clone();
    let names = || -> Vec<String> {
        f.w.s
            .queue
            .borrow()
            .entries()
            .iter()
            .map(|e| e.filename().to_owned())
            .collect()
    };
    assert_eq!(names(), ["c.png", "A.png", "b.png", "d.png"]);
    view.on_header_clicked(Column::File);
    assert_eq!(
        names(),
        ["A.png", "b.png", "c.png", "d.png"],
        "ascending, case-folded"
    );
    assert_eq!(view.cell(0, Column::File).unwrap().text, "A.png");
    view.on_header_clicked(Column::File);
    assert_eq!(names(), ["d.png", "c.png", "b.png", "A.png"], "descending");
    view.on_header_clicked(Column::File);
    assert_eq!(view.sort_state(), None);
    assert_eq!(
        names(),
        ["c.png", "A.png", "b.png", "d.png"],
        "back to insertion order"
    );
    f.close();
}

fn tagged_seed(dir: &std::path::Path, names: &[&str]) -> Vec<hatate_core::model::Entry> {
    make_images(dir, names)
        .into_iter()
        .map(|p| {
            let mut e = entry(&p.to_string_lossy(), MatchStatus::Good);
            e.tags = vec![hatate_core::model::Tag {
                name: "cat".to_owned(),
                namespace: None,
                source: hatate_core::model::TagSource::User,
            }];
            e
        })
        .collect()
}

fn export_results_writes_the_format_the_name_says(app: &adw::Application) {
    // An empty list says so and asks nothing.
    let f = fx(app);
    f.activate("export-results");
    assert_eq!(
        f.script.last_shown().text,
        "There's nothing in the list to export."
    );
    assert!(f.script.asked.borrow().is_empty());
    f.close();

    let f = Fx::new(
        app,
        Opts {
            seed: seeded(2, MatchStatus::Good),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    f.script.push(Answer::Path(Some(f.dir.path().join("out"))));
    f.activate("export-results");
    let csv = f.dir.path().join("out.csv");
    let text = std::fs::read_to_string(&csv).expect("the .csv is added to a bare name");
    assert!(
        text.contains("\r\n") && text.contains("img0.png"),
        "{text:?}"
    );
    assert_eq!(f.say(), "Exported 2 image(s) to out.csv");
    let Asked::SaveFile {
        title,
        default_name,
        filter,
    } = f.script.asked.borrow().last().cloned().unwrap()
    else {
        panic!("not a save chooser")
    };
    assert_eq!(title, "Export Results");
    assert!(
        default_name.contains("hatate-results-") && default_name.ends_with(".csv"),
        "{default_name}"
    );
    assert_eq!(
        filter,
        "CSV spreadsheet (*.csv);;JSON (*.json);;All files (*)"
    );
    // A name that says JSON, with no filter to say otherwise, writes JSON.
    f.script
        .push(Answer::Path(Some(f.dir.path().join("out.json"))));
    f.activate("export-results");
    let json = std::fs::read_to_string(f.dir.path().join("out.json")).unwrap();
    assert!(
        json.starts_with('{') && json.contains("\"count\": 2"),
        "{json:?}"
    );
    // An unwritable path warns and says nothing was exported.
    f.script.push(Answer::Path(Some(PathBuf::from(
        "/nonexistent-dir-for-test/x.csv",
    ))));
    f.activate("export-results");
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.kind, shown.title.as_str()),
        (Kind::Warning, "Export Results")
    );
    assert!(
        shown
            .text
            .starts_with("Could not write the results to:\n/nonexistent-dir-for-test/x.csv\n\n")
    );
    assert!(!shown.text.contains("os error"), "{}", shown.text);
    f.close();
}

fn write_tag_files_asks_then_writes_and_keeps_what_is_there(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let seed = tagged_seed(dir.path(), &["a.png", "b.png"]);
    let f = Fx::new(
        app,
        Opts {
            seed,
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    std::fs::write(dir.path().join("a.png.txt"), "mine\n").unwrap();
    // `ask` with a file in the way: the three-way box, Keep existing files the default.
    f.script.push(Answer::Msg(Response::Keep));
    f.activate("write-tag-files");
    let shown = f.script.last_shown();
    assert_eq!(shown.buttons, Buttons::KeepReplaceCancel);
    assert_eq!(shown.default, Response::Keep);
    assert!(
        shown.text.contains("1 of them already exist, for example:"),
        "{}",
        shown.text
    );
    assert!(shown.text.ends_with("This app cannot tell a tag file it wrote on an earlier run from a text file you wrote yourself."));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.png.txt")).unwrap(),
        "mine\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.png.txt")).unwrap(),
        "cat\n"
    );
    assert_eq!(
        f.say(),
        "Wrote 1 tag file(s), 1 already existed and were kept"
    );
    // Replace them replaces; Cancel writes nothing.
    f.script.push(Answer::Msg(Response::Cancel));
    std::fs::remove_file(dir.path().join("b.png.txt")).unwrap();
    f.activate("write-tag-files");
    assert!(!dir.path().join("b.png.txt").exists());
    f.script.push(Answer::Msg(Response::Replace));
    f.activate("write-tag-files");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.png.txt")).unwrap(),
        "cat\n"
    );
    assert_eq!(f.say(), "Wrote 2 tag file(s)");
    f.close();
}

fn clear_search_cache_asks_then_removes_the_saved_results(app: &adw::Application) {
    let f = fx(app);
    // Set at startup, before anything is cached.
    assert_eq!(
        f.w.s.clear_cache_item.label(),
        "Clear Search Cache (0 B)\u{2026}"
    );
    let cache = hatate_core::cache::SearchCache::new(&f.paths);
    std::fs::create_dir_all(cache.dir()).unwrap();
    for n in ["a", "b"] {
        std::fs::write(cache.dir().join(format!("{n}.json")), "{}").unwrap();
    }
    // The question defaults to No (design gate 2.6), so nothing is deleted without a Yes.
    f.activate("clear-search-cache");
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.buttons, shown.default),
        (Buttons::YesNo, Response::No)
    );
    assert_eq!(std::fs::read_dir(cache.dir()).unwrap().count(), 2);
    f.script.push(Answer::Msg(Response::Yes));
    f.activate("clear-search-cache");
    assert_eq!(
        f.script.last_shown().text,
        "Removed 2 cached search result(s)."
    );
    assert_eq!(std::fs::read_dir(cache.dir()).unwrap().count(), 0);
    f.close();
}

fn queue_header_layout_is_saved_on_close_and_restored_repaired_or_discarded(
    app: &adw::Application,
) {
    use hatate_core::table_cells::{
        COLUMNS, DEFAULT_COLUMN_WIDTH, HEADER_STATE_PREFIX, HeaderSection, encode_header_state,
    };
    let saved = |paths: &hatate_core::paths::Paths| {
        let s = hatate_core::config::Settings::load(paths).settings;
        (s.table_header_state, s.table_header_columns)
    };
    let with_state = |state: String, columns: i64| Opts {
        tweak: Some(Box::new(move |s| {
            s.table_header_state = state.clone();
            s.table_header_columns = columns;
        })),
        ..Opts::default()
    };

    // A usable layout, with one hidden and one too-narrow column: restored, then repaired.
    let mut sections = vec![
        HeaderSection {
            width: 130,
            hidden: false
        };
        COLUMNS.len()
    ];
    sections[2].hidden = true;
    sections[3].width = 10;
    let f = Fx::new(app, with_state(encode_header_state(&sections), 12));
    let got = f.w.s.queue_view.header_sections();
    assert!(!got[2].hidden, "the hidden column is shown again");
    assert_eq!(
        got[3].width, DEFAULT_COLUMN_WIDTH,
        "the narrow column is widened"
    );
    assert_eq!(
        got[4].width, 130,
        "an ordinary column keeps its saved width"
    );
    // A saved layout is not overwritten by the window changing size
    // (`test_a_saved_layout_is_not_overwritten_by_the_defaults`).
    f.w.win.set_default_size(1500, 900);
    pump_for(300);
    let resized = f.w.s.queue_view.header_sections();
    assert_eq!(
        resized[4].width, 130,
        "a resize rewrote a saved column width"
    );
    f.w.close_teardown();
    // Closing saves the live layout in this build's format, with the column count. Read before the
    // fixture closes: dropping it removes the profile.
    let (state, columns) = saved(&f.paths);
    f.close();
    assert!(state.starts_with(HEADER_STATE_PREFIX), "{state:?}");
    assert_eq!(columns, 12);
    assert_eq!(state.split(',').count(), 12);

    // A Qt blob of the right column count (what a Python build wrote): discarded, never parsed.
    let f = Fx::new(
        app,
        with_state(
            "AAAA/wAAAAAAAAABAAAAAQEAAAAAAAAAAAAAAAAAAAAAAADIAAA=".into(),
            12,
        ),
    );
    assert_eq!(f.w.s.settings.lock().unwrap().table_header_state, "");
    assert_eq!(f.w.s.settings.lock().unwrap().table_header_columns, 12);
    f.close();

    // Saved with another column count: both keys reset.
    let f = Fx::new(app, with_state(encode_header_state(&sections), 10));
    let s = f.w.s.settings.lock().unwrap().clone();
    assert_eq!(
        (s.table_header_state.as_str(), s.table_header_columns),
        ("", 0)
    );
    drop(s);
    f.close();
}

fn clear_search_cache_label_carries_the_cache_size(app: &adw::Application) {
    let f = fx(app);
    let cache = hatate_core::cache::SearchCache::new(&f.paths);
    std::fs::create_dir_all(cache.dir()).unwrap();
    std::fs::write(
        cache.dir().join(format!("{}.json", "a".repeat(64))),
        vec![b'x'; 1536],
    )
    .unwrap();
    // The label is refreshed when the menu is about to open; here the refresh is called directly.
    f.w.update_clear_cache_label();
    assert_eq!(
        f.w.s.clear_cache_item.label(),
        "Clear Search Cache (1.5 KB)\u{2026}"
    );
    f.close();
}

/// Every `gtk::Label` under `w` (itself included).
fn all_labels(w: &gtk4::Widget, out: &mut Vec<gtk4::Label>) {
    if let Some(l) = w.downcast_ref::<gtk4::Label>() {
        out.push(l.clone());
    }
    let mut child = w.first_child();
    while let Some(c) = child {
        all_labels(&c, out);
        child = c.next_sibling();
    }
}

/// DAN-1999 F6: the `THUMB` title (letter-spaced, in a 72 px column) was clipped at the minimum
/// window size. Every column title must be drawn whole: not ellipsized, and given at least the
/// width it asks for.
fn queue_header_titles_are_not_clipped_at_the_minimum_window_size(app: &adw::Application) {
    use hatate_core::table_cells::Column;
    for scheme in ["dark", "light"] {
        let s = scheme.to_owned();
        let f = Fx::new(
            app,
            Opts {
                seed: seeded(5, MatchStatus::Good),
                missing: Some(Arc::new(|_, _| HashSet::new())),
                tweak: Some(Box::new(move |c| c.theme = s.clone())),
                ..Opts::default()
            },
        );
        f.w.win.set_default_size(1, 1);
        pump_for(300);
        let mut labels = Vec::new();
        all_labels(f.w.s.queue_view.view.upcast_ref(), &mut labels);
        let mut seen = 0;
        for col in Column::ALL {
            let title = col.title();
            let Some(label) = labels
                .iter()
                .find(|l| l.text() == title && l.is_mapped() && l.width() > 0)
            else {
                panic!("{scheme}: no drawn header title {title:?}");
            };
            seen += 1;
            // The header cell is the enclosing button: the title must sit wholly inside it.
            let mut cell = label.parent();
            while let Some(c) = &cell {
                if c.css_name() == "button" {
                    break;
                }
                cell = c.parent();
            }
            let cell = cell.unwrap_or_else(|| panic!("{scheme}: header {title:?} has no cell"));
            let b = label.compute_bounds(&cell).expect("bounds");
            println!(
                "HEADER {scheme} {title} cell_width={} label_x={:.1}..{:.1} ellipsized={}",
                cell.width(),
                b.x(),
                b.x() + b.width(),
                label.layout().is_ellipsized()
            );
            assert!(
                !label.layout().is_ellipsized()
                    && b.x() >= -0.5
                    && b.x() + b.width() <= cell.width() as f32 + 0.5,
                "{scheme}: header {title:?} is clipped (cell {}, title {:.1}..{:.1})",
                cell.width(),
                b.x(),
                b.x() + b.width()
            );
        }
        assert_eq!(seen, Column::ALL.len());
        f.close();
    }
}

/// DAN-2319 F1 and F5: at the default (1090 x 790) and the minimum (1000 x 780) window sizes the
/// Status chip is drawn whole (`Unsearched`, `Not found` were cut to `Unsearc...`, `Not fou...`),
/// a File name that does not fit is cut in the middle, and the `SENT` column lies inside the
/// viewport at 1000.
fn queue_status_and_file_cells_fit_and_sent_is_in_view_at_both_window_sizes(
    app: &adw::Application,
) {
    use gtk4::pango::EllipsizeMode;
    use hatate_core::table_cells::Column;
    for (width, height) in [(1090, 790), (1000, 780)] {
        for scheme in ["dark", "light"] {
            let mut seed = seeded(MatchStatus::ALL.len(), MatchStatus::NotSearched);
            for (i, e) in seed.iter_mut().enumerate() {
                e.status = MatchStatus::ALL[i];
                e.path = format!("/lib/synthetic-{i:04}-a-long-file-name-that-cannot-fit.png");
            }
            let s = scheme.to_owned();
            let f = Fx::new(
                app,
                Opts {
                    seed,
                    missing: Some(Arc::new(|_, _| HashSet::new())),
                    tweak: Some(Box::new(move |c| c.theme = s.clone())),
                    ..Opts::default()
                },
            );
            f.w.win.set_default_size(width, height);
            // A mapped window does not grow to a new default size; the request does.
            // (The window's content is 10 px smaller than the frame the shots are named after.)
            f.w.win.set_size_request(width + 10, height + 10);
            pump_for(400);
            let mut labels = Vec::new();
            all_labels(f.w.s.queue_view.view.upcast_ref(), &mut labels);
            println!(
                "FIT {width}x{height} {scheme} window={}x{} widths={:?}",
                f.w.win.width(),
                f.w.win.height(),
                f.w.s
                    .queue_view
                    .header_sections()
                    .iter()
                    .map(|s| s.width)
                    .collect::<Vec<_>>()
            );
            let mut chips = 0;
            let mut chip_clipped = false;
            let mut file_modes = Vec::new();
            for label in labels.iter().filter(|l| l.is_mapped() && l.width() > 0) {
                let text = label.text();
                let is_chip = [
                    "Good",
                    "Review",
                    "Not found",
                    "Error",
                    "Searching",
                    "Unsearched",
                ]
                .iter()
                .any(|t| text.ends_with(t) && text.len() > t.len());
                if is_chip {
                    chips += 1;
                    println!(
                        "FIT {width}x{height} {scheme} chip {text:?} width={} natural={} ellipsized={}",
                        label.width(),
                        label.measure(gtk4::Orientation::Horizontal, -1).1,
                        label.layout().is_ellipsized()
                    );
                    chip_clipped |= label.layout().is_ellipsized();
                }
                if text.starts_with("synthetic-") {
                    file_modes.push((text.to_string(), label.ellipsize()));
                }
            }
            assert_eq!(
                chips,
                MatchStatus::ALL.len(),
                "{scheme}: every Status chip drawn"
            );
            assert!(
                !chip_clipped,
                "{width}x{height} {scheme}: a Status chip is ellipsized"
            );
            assert!(!file_modes.is_empty(), "{scheme}: File cells drawn");
            for (text, mode) in &file_modes {
                assert_eq!(
                    *mode,
                    EllipsizeMode::Middle,
                    "{width}x{height} {scheme}: File {text:?} must ellipsize in the middle"
                );
            }
            // The title of SENT sits inside the viewport (the scrolled window), not past its edge.
            let scroller = f.w.s.queue_view.scroller.clone();
            let sent = labels
                .iter()
                .find(|l| l.text() == Column::Sent.title() && l.is_mapped())
                .unwrap_or_else(|| panic!("{scheme}: no drawn SENT title"));
            let b = sent.compute_bounds(&scroller).expect("bounds");
            println!(
                "FIT {width}x{height} {scheme} SENT x={:.1}..{:.1} viewport={}",
                b.x(),
                b.x() + b.width(),
                scroller.width()
            );
            assert!(
                b.x() + b.width() <= scroller.width() as f32 + 0.5,
                "{width}x{height} {scheme}: SENT is off-screen"
            );
            // And the whole column, not only its title: Thumb to Sent end inside the viewport.
            let through_sent: i32 = f.w.s.queue_view.header_sections()[..=Column::Sent.index()]
                .iter()
                .map(|s| s.width)
                .sum();
            println!("FIT {width}x{height} {scheme} through_sent={through_sent}");
            assert!(
                through_sent <= scroller.width(),
                "{width}x{height} {scheme}: the SENT column ends at {through_sent}, past the viewport"
            );
            f.close();
        }
    }
}

/// The first widget under `w` whose CSS node is `name` and that is mapped.
fn find_css_node(w: &gtk4::Widget, name: &str, out: &mut Vec<gtk4::Widget>) {
    if w.css_name() == name && w.is_mapped() {
        out.push(w.clone());
    }
    let mut child = w.first_child();
    while let Some(c) = child {
        find_css_node(&c, name, out);
        child = c.next_sibling();
    }
}

/// DAN-2319 F2: the Queue's persistent scrollbar is 10 px wide (design gate 2.4, `tokens.css`),
/// not the toolkit's 22, and with 5,000 rows the `SENT` column still ends inside what is left of
/// the 1000 px window's viewport.
fn queue_scrollbar_is_10_px_wide_and_sent_still_fits_beside_it(app: &adw::Application) {
    use hatate_core::table_cells::Column;
    for scheme in ["dark", "light"] {
        let mut seed = seeded(5000, MatchStatus::NotSearched);
        for (i, e) in seed.iter_mut().enumerate() {
            e.status = MatchStatus::ALL[i % MatchStatus::ALL.len()];
        }
        let s = scheme.to_owned();
        let f = Fx::new(
            app,
            Opts {
                seed,
                missing: Some(Arc::new(|_, _| HashSet::new())),
                tweak: Some(Box::new(move |c| c.theme = s.clone())),
                ..Opts::default()
            },
        );
        f.w.win.set_default_size(1000, 780);
        f.w.win.set_size_request(1010, 790);
        pump_for(400);
        let scroller = f.w.s.queue_view.scroller.clone();
        let mut bars = Vec::new();
        find_css_node(scroller.upcast_ref(), "scrollbar", &mut bars);
        let vertical: Vec<_> = bars
            .iter()
            .filter(|b| b.has_css_class("vertical"))
            .collect();
        assert_eq!(vertical.len(), 1, "{scheme}: one vertical scrollbar");
        let bar_width = vertical[0].width();
        println!(
            "SCROLLBAR {scheme} width={bar_width} viewport={}",
            scroller.width()
        );
        assert_eq!(bar_width, 10, "{scheme}: the scrollbar is 10 px wide");
        let through_sent: i32 = f.w.s.queue_view.header_sections()[..=Column::Sent.index()]
            .iter()
            .map(|s| s.width)
            .sum();
        assert!(
            through_sent <= scroller.width() - bar_width,
            "{scheme}: SENT ends at {through_sent}, under the scrollbar ({} left)",
            scroller.width() - bar_width
        );
        f.close();
    }
}

/// The first label under `w` that is a `GtkModelButton` text, per visible popover menu: what menus
/// are open, by their first entry.
fn open_menu_first_entries(w: &gtk4::Widget, out: &mut Vec<String>) {
    if let Some(p) = w.downcast_ref::<gtk4::PopoverMenu>() {
        if p.is_visible() {
            out.push(first_model_button_text(p.upcast_ref()).unwrap_or_default());
        }
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        open_menu_first_entries(&child, out);
        c = child.next_sibling();
    }
}

fn first_model_button_text(w: &gtk4::Widget) -> Option<String> {
    if w.type_().name() == "GtkModelButton" {
        return w.property::<Option<String>>("text");
    }
    let mut c = w.first_child();
    while let Some(child) = c {
        if let Some(t) = first_model_button_text(&child) {
            return Some(t);
        }
        c = child.next_sibling();
    }
    None
}

/// DAN-2246: the menubar by keyboard, with real X key events (`common::xtest`) delivered to the
/// real window. GTK does Alt+mnemonic, Left and Right on its own and this pins that it still does;
/// `menubar_keys` adds F10 (open the first menu, again to close) and the return of focus to the
/// widget that had it before the bar was entered.
fn menubar_keyboard_path(app: &adw::Application) {
    use common::xtest::{Key, Mod, Xtest};
    let mut keys = match Xtest::connect() {
        Ok(x) => x,
        Err(why) => {
            assert!(
                std::env::var_os("CI").is_none(),
                "CI is set and the X server cannot inject keys: {why}: failing closed"
            );
            println!("menubar_keyboard_path: skipped ({why})");
            return;
        }
    };
    let f = fx(app);
    // Earlier scenarios leave their secondary windows mapped (the log viewer); they would hold the
    // keyboard. Only this scenario's window may be on screen.
    for w in gtk4::Window::list_toplevels() {
        if &w != f.w.win.upcast_ref::<gtk4::Widget>() {
            if let Ok(w) = w.downcast::<gtk4::Window>() {
                w.destroy();
            }
        }
    }
    f.w.win.set_default_size(1100, 800);
    pump_for(400);
    let root = f.w.win.clone().upcast::<gtk4::Widget>();
    let window = f.w.win.upcast_ref::<gtk4::Window>();
    let focus = || gtk4::prelude::GtkWindowExt::focus(window);
    let open_menus = || {
        let mut v = Vec::new();
        open_menu_first_entries(&root, &mut v);
        v
    };
    let files = "Add Files\u{2026}";
    let hydrus = "Send File + URL + Tags to Hydrus (upload)";

    // No window manager under xvfb: the pointer and the X focus put the keys on this window.
    keys.focus_window_at(300, 300).expect("focus the window");
    let add = find_button(&root, "Add files\u{2026}").expect("Add files");
    add.grab_focus();
    pump_for(200);
    let add_widget = add.clone().upcast::<gtk4::Widget>();
    assert_eq!(
        focus().as_ref(),
        Some(&add_widget),
        "Add files starts with focus"
    );

    let mut press = |key: Key, mods: &[Mod]| {
        keys.tap(key, mods).expect("inject");
        pump_for(400);
    };

    // The keys reach the window at all: Alt+F is GTK's own, and opens Files.
    press(Key::Char('f'), &[Mod::Alt]);
    assert_eq!(open_menus(), [files], "Alt+F opens Files");
    press(Key::Escape, &[]);
    assert!(open_menus().is_empty(), "Escape closes it");
    assert_eq!(
        focus().as_ref(),
        Some(&add_widget),
        "and focus returns to Add files"
    );

    // Shift+F10 is the context-menu key and Ctrl+F10 is not ours: neither opens a menu.
    press(Key::F10, &[Mod::Shift]);
    assert!(open_menus().is_empty(), "Shift+F10 does not open a menu");
    press(Key::F10, &[Mod::Ctrl]);
    assert!(open_menus().is_empty(), "Ctrl+F10 does not open a menu");

    // F10 opens the first menu.
    press(Key::F10, &[]);
    assert_eq!(open_menus(), [files], "F10 opens Files");
    // Right moves to the next menu, Left back.
    press(Key::Right, &[]);
    assert_eq!(
        open_menus(),
        [hydrus],
        "Right opens Hydrus and closes Files"
    );
    press(Key::Left, &[]);
    assert_eq!(open_menus(), [files], "Left goes back to Files");
    // F10 again closes, and focus goes back to where it was before F10.
    press(Key::F10, &[]);
    assert!(open_menus().is_empty(), "F10 a second time closes the menu");
    assert_eq!(
        focus().as_ref(),
        Some(&add_widget),
        "focus returns to Add files after F10, F10"
    );

    // Escape does the same after a trip through two menus.
    press(Key::F10, &[]);
    press(Key::Right, &[]);
    assert_eq!(open_menus(), [hydrus]);
    press(Key::Escape, &[]);
    assert!(open_menus().is_empty(), "Escape closes the Hydrus menu");
    assert_eq!(
        focus().as_ref(),
        Some(&add_widget),
        "Escape returns focus to the widget that had it before F10"
    );
    f.close();
}

// ---------------------------------------------------------------------------------------------
// Review (6.f, DAN-2219)
// ---------------------------------------------------------------------------------------------

/// Settings that keep a window test off the network: no booru page, no availability probe.
fn offline() -> Tweak {
    Box::new(|s: &mut hatate_core::config::Settings| {
        s.retrieve_tags_from_booru = false;
        s.drop_dead_matches = false;
    })
}

/// A candidate whose picture and tags are "already fetched", so selecting it asks nobody.
fn fetched(url: &str, similarity: f64, source: &str) -> hatate_core::model::Candidate {
    let mut c = hatate_core::model::Candidate::new(url);
    c.similarity = similarity;
    c.similarity_measured = true;
    c.engine = "IQDB".to_owned();
    c.source_name = Some(source.to_owned());
    c.width = Some(1400);
    c.height = Some(2200);
    c.thumb_bytes = Some(common::search::PNG.to_vec());
    c.booru_tags_fetched = true;
    c.remote_info_fetched = true;
    c
}

/// Three rows: a Good one with two matches and three tags, a Not found one, a Good one with one.
fn review_seed(dir: &std::path::Path) -> Vec<hatate_core::model::Entry> {
    use hatate_core::model::{Tag, TagSource};
    let png = |name: &str| {
        let p = dir.join(name);
        std::fs::write(&p, common::search::PNG).unwrap();
        p.to_string_lossy().into_owned()
    };
    let tag = |ns: Option<&str>, name: &str, source| Tag {
        name: name.to_owned(),
        namespace: ns.map(str::to_owned),
        source,
    };
    let mut a = entry(&png("a.png"), MatchStatus::Good);
    a.candidates = vec![
        fetched("https://danbooru.donmai.us/posts/1", 96.0, "Danbooru"),
        fetched("https://gelbooru.com/index.php?id=2", 80.0, "Gelbooru"),
    ];
    a.tags = vec![
        tag(None, "solo", TagSource::Booru),
        tag(Some("character"), "reimu", TagSource::User),
        tag(None, "sketch", TagSource::Hydrus),
    ];
    let b = entry(&png("b.png"), MatchStatus::NotFound);
    let mut c = entry(&png("c.png"), MatchStatus::Good);
    c.candidates = vec![fetched(
        "https://danbooru.donmai.us/posts/3",
        91.0,
        "Danbooru",
    )];
    vec![a, b, c]
}

fn review_fx(app: &adw::Application, dir: &std::path::Path) -> Fx {
    let f = Fx::new(
        app,
        Opts {
            seed: review_seed(dir),
            tweak: Some(offline()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    already_fetched(&f);
    f
}

/// A session keeps no picture bytes and no "fetched" flags (`thumb_bytes` is session-only), so a
/// restored match has everything to fetch. These rows are as a search just left them.
fn already_fetched(f: &Fx) {
    let mut q = f.w.s.queue.borrow_mut();
    let ids: Vec<_> = (0..q.len() as i64).filter_map(|i| q.id_at(i)).collect();
    for id in ids {
        for c in q.entry_mut(id).unwrap().candidates.iter_mut() {
            c.thumb_bytes = Some(common::search::PNG.to_vec());
            c.booru_tags_fetched = true;
            c.remote_info_fetched = true;
        }
    }
}

/// The text of each row of the tag list.
fn tag_texts(f: &Fx) -> Vec<String> {
    let list = &f.w.s.review.tag_list;
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(row) = list.row_at_index(i) {
        let child = row.child();
        out.push(
            child
                .clone()
                .and_downcast::<gtk4::Label>()
                .map(|l| l.text().to_string())
                .or_else(|| {
                    child
                        .and_downcast::<gtk4::EditableLabel>()
                        .map(|l| l.text().to_string())
                })
                .unwrap_or_default(),
        );
        i += 1;
    }
    out
}

/// `_update_preview`, `_set_review_captions`, `refresh_review`, `_refresh_tag_list` and the
/// position strip all follow the selected row (`test_selection_and_preview_handlers`).
fn review_page_follows_the_selection(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let f = review_fx(app, dir.path());
    let r = f.w.s.review.clone();
    f.w.set_mode("review");
    pump_for(100);

    // Nothing selected.
    assert_eq!(r.local.shown_text(), "No image selected");
    assert_eq!(r.matched.shown_text(), "No match yet");
    assert_eq!(r.match_label.text(), "No match");
    assert!(!r.candidate_combo.is_sensitive());
    assert!(!r.readout_root.is_visible());
    assert!(tag_texts(&f).is_empty());
    assert_eq!(r.matched_caption.left.text(), "Match");

    f.w.s.queue_view.select_row(0);
    assert!(pump_until(5_000, || r.local.shows_picture()));
    assert_eq!(r.match_label.text(), "Match 1 of 2");
    assert_eq!(r.candidate_model.n_items(), 2);
    assert_eq!(
        r.candidate_model.string(0).as_deref(),
        Some("Danbooru \u{2014} 96% (IQDB) \u{2014} https://danbooru.donmai.us/posts/1")
    );
    assert_eq!(
        r.candidate_combo.tooltip_text().as_deref(),
        Some("https://danbooru.donmai.us/posts/1")
    );
    assert!(r.matched.shows_picture());
    assert!(r.readout_root.is_visible());
    assert_eq!(r.readout_value.text(), "96%");
    assert_eq!(r.readout_label.text(), "Similarity");
    assert_eq!(r.matched_caption.left.text(), "Match \u{2014} Danbooru");
    assert_eq!(r.matched_caption.right.text(), "1400 \u{d7} 2200");
    assert_eq!(
        r.local_caption.right.text(),
        "4 \u{d7} 3 \u{b7} PNG \u{b7} 94 B"
    );
    assert_eq!(
        tag_texts(&f),
        ["[Booru] solo", "[User] character:reimu", "[Hydrus] sketch"]
    );
    assert_eq!(r.mark_btn.text(), "Mark reviewed");
    assert!(!r.prev_match_btn.is_sensitive() && r.next_match_btn.is_sensitive());
    assert!(!r.prev_btn.button.is_sensitive() && r.next_btn.button.is_sensitive());

    // A row with no match says why.
    f.w.s.queue_view.select_row(1);
    assert_eq!(r.matched.shown_text(), "No match found");
    assert_eq!(r.match_label.text(), "No match");
    assert_eq!(r.matched_caption.left.text(), "Match \u{2014} none");
    assert_eq!(r.matched_caption.right.text(), "\u{2014}");
    assert!(!r.candidate_combo.is_sensitive());
    assert!(!r.readout_root.is_visible());
    assert!(tag_texts(&f).is_empty());
    assert!(r.next_btn.button.is_sensitive() && r.prev_btn.button.is_sensitive());

    // The last row has no next.
    f.w.s.queue_view.select_row(2);
    assert!(!r.next_btn.button.is_sensitive());

    // Selecting nothing empties the page again.
    f.w.s.queue_view.select_ids(&[]);
    assert_eq!(r.local.shown_text(), "No image selected");
    assert_eq!(r.match_label.text(), "No match");
    f.close();
}

/// `_add_tags_to_current`, `_remove_selected_tags`, `_on_edit_tags_toggled`,
/// `_on_tag_item_edited`.
fn review_tag_panel_adds_removes_and_edits_in_place(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let f = review_fx(app, dir.path());
    let r = f.w.s.review.clone();
    f.w.s.queue_view.select_row(0);
    assert_eq!(tag_texts(&f).len(), 3);

    // Add tags: nothing selected says so, Cancel adds nothing, text adds user tags once each.
    f.w.s.queue_view.select_ids(&[]);
    f.w.add_tags_to_current();
    assert_eq!(
        f.script.last_shown().text,
        "Select at least one image first."
    );
    f.w.s.queue_view.select_row(0);
    f.script.reset();
    f.script.push(Answer::Text(None));
    f.w.add_tags_to_current();
    assert_eq!(tag_texts(&f).len(), 3);
    f.script.push(Answer::Text(Some(
        " series : touhou \n\nsolo\ncat\n".to_owned(),
    )));
    f.w.add_tags_to_current();
    assert!(matches!(
        f.script.asked.borrow().last(),
        Some(Asked::AddTags { prompt })
            if prompt == "Add tags to a.png (one per line, optionally namespace:tag):"
    ));
    assert_eq!(
        tag_texts(&f),
        [
            "[Booru] solo",
            "[User] character:reimu",
            "[Hydrus] sketch",
            "[User] series:touhou",
            "[User] cat"
        ],
        "solo was already there and is not added twice"
    );

    // Remove: the picked rows go, the others stay.
    r.tag_list.select_row(r.tag_list.row_at_index(3).as_ref());
    r.tag_list.select_row(r.tag_list.row_at_index(4).as_ref());
    f.w.remove_selected_tags();
    assert_eq!(tag_texts(&f).len(), 3);

    // Edit: plain `namespace:name` while editable.
    r.edit_tags_btn.set_active(true);
    assert_eq!(
        r.edit_tags_btn.label().as_deref(),
        Some("Editing Tags (click to stop)")
    );
    assert_eq!(
        tag_texts(&f),
        ["solo", "character:reimu", "sketch"],
        "no [Source] prefix while editing"
    );
    let rename = |row: i32, text: &str| {
        let field = r
            .tag_list
            .row_at_index(row)
            .and_then(|row| row.child())
            .and_downcast::<gtk4::EditableLabel>()
            .expect("an editable row");
        field.start_editing();
        field.set_text(text);
        field.stop_editing(true);
    };
    rename(2, "artist:someone");
    assert_eq!(
        tag_texts(&f),
        ["solo", "character:reimu", "artist:someone"],
        "renamed in place, source kept"
    );
    assert_eq!(
        f.w.s.queue.borrow().entries()[0].tags[2].source,
        hatate_core::model::TagSource::Hydrus
    );
    rename(0, "character:reimu");
    assert_eq!(
        tag_texts(&f),
        ["character:reimu", "artist:someone"],
        "a collision drops the older duplicate"
    );
    rename(1, "   ");
    assert_eq!(
        tag_texts(&f),
        ["character:reimu"],
        "clearing the text deletes the tag"
    );
    r.edit_tags_btn.set_active(false);
    assert_eq!(r.edit_tags_btn.label().as_deref(), Some("Edit Tags"));
    f.close();
}

/// `_on_candidate_combo_changed`, `_ensure_candidate_loaded`, `_on_candidate_fetched`: picking a
/// match makes it the entry's, and the fetch of its picture runs off the main loop.
fn review_candidate_picker_switches_the_match_and_the_fetch_stays_off_the_main_loop(
    app: &adw::Application,
) {
    use std::io::Read;
    use std::net::TcpListener;
    let dir = tempfile::tempdir().unwrap();
    // A site that accepts the connection and never answers: DeviantArt is on the list of hosts the
    // lazy fetch checks, and `host_overrides` sends that name to this socket.
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();
    let held = std::thread::spawn(move || {
        let (mut conn, _) = server.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = conn.read(&mut buf);
        // Hold it open until the test lets go.
        std::thread::sleep(std::time::Duration::from_millis(900));
    });
    let mut seed = review_seed(dir.path());
    let mut lazy = hatate_core::model::Candidate::new("http://www.deviantart.com/art/lazy-1");
    lazy.engine = "IQDB".to_owned();
    lazy.similarity = 70.0;
    seed[0].candidates.push(lazy);
    let f = Fx::new_with(
        app,
        Opts {
            seed,
            tweak: Some(Box::new(|s: &mut hatate_core::config::Settings| {
                s.retrieve_tags_from_booru = false;
                s.drop_dead_matches = true;
            })),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
        |env| {
            let mut net = hatate_core::net::NetConfig::default();
            net.host_overrides
                .push(("www.deviantart.com".to_owned(), addr));
            let paths = env.paths.clone();
            let (backend, _) = common::search::Scripted::with_net(&paths, net);
            env.search = Some(backend);
        },
    );
    already_fetched(&f);
    {
        // The third match is the one nobody has fetched yet.
        let mut q = f.w.s.queue.borrow_mut();
        let id = q.id_at(0).unwrap();
        let lazy = &mut q.entry_mut(id).unwrap().candidates[2];
        lazy.thumb_bytes = None;
        lazy.booru_tags_fetched = false;
    }
    let r = f.w.s.review.clone();
    f.w.s.queue_view.select_row(0);
    assert_eq!(r.candidate_model.n_items(), 3);

    // Picking the second match: it becomes the entry's, and the row follows.
    r.candidate_combo.set_selected(1);
    assert_eq!(
        f.w.s.queue.borrow().entries()[0].selected_candidate_index,
        1
    );
    assert_eq!(r.match_label.text(), "Match 2 of 3");
    assert_eq!(r.matched_caption.left.text(), "Match \u{2014} Gelbooru");
    assert!(r.prev_match_btn.is_sensitive() && r.next_match_btn.is_sensitive());
    assert_eq!(
        f.w.s
            .queue_view
            .cell(0, hatate_core::table_cells::Column::Booru)
            .map(|c| c.text),
        Some("Gelbooru".to_owned())
    );

    // The third has nothing fetched: the fetch goes to the site, which does not answer.
    let ticks = Rc::new(Cell::new(0u32));
    let t = ticks.clone();
    let tick = gtk4::glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        t.set(t.get() + 1);
        gtk4::glib::ControlFlow::Continue
    });
    r.next_match_btn.emit_clicked();
    assert_eq!(
        f.w.s.queue.borrow().entries()[0].selected_candidate_index,
        2
    );
    // On the last match Previous is live, Next and its bracket cap dim.
    assert!(r.prev_match_btn.is_sensitive() && !r.next_match_btn.is_sensitive());
    assert!(r.prev_match_key.is_sensitive() && !r.next_match_key.is_sensitive());
    assert_eq!(r.matched.shown_text(), "Loading picture\u{2026}");
    assert_eq!(
        f.say(),
        "Fetching picture and tags for the selected site\u{2026}"
    );
    assert!(f.w.s.candidate_worker.is_running());
    let before = ticks.get();
    // Actions are handled while it waits: stepping back and forth, marking a row.
    f.w.run_review_action("toggle_reviewed");
    assert!(f.w.s.queue.borrow().entries()[0].reviewed);
    pump_for(200);
    assert!(
        ticks.get() >= before + 4,
        "the main loop kept turning: {before} -> {}",
        ticks.get()
    );
    assert!(
        f.w.s.candidate_worker.is_running(),
        "still waiting on the site"
    );
    // The site lets go; the fetch ends and the status says so.
    assert!(pump_until(10_000, || f.say() == "Ready"));
    assert!(!f.w.s.candidate_worker.is_running());
    tick.remove();
    held.join().unwrap();
    f.close();
}

/// DAN-2219: the window opens 1100 px wide, and the Review page with a match selected must still
/// ask for no more than the stated minimum. Its keyed buttons put the key beside the label while
/// the column is wide, and the label's own minimum used to keep them that way (430 px of rail
/// instead of 340): a window opened wide could not be narrowed to 1010, and the minimum-size
/// scenario never saw it because it opens Review with an empty queue.
fn review_with_a_match_selected_fits_the_stated_minimum_after_opening_wide(app: &adw::Application) {
    use hatate_gtk::window::{MIN_HEIGHT, MIN_WIDTH};
    let dir = tempfile::tempdir().unwrap();
    let f = review_fx(app, dir.path());
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    assert!(pump_until(5_000, || f.w.s.review.local.shows_picture()));
    pump_for(200);
    let (w, h) = content_min(&f);
    println!("MINSIZE review-selected content_min={w}x{h}");
    let content = libadwaita::prelude::AdwApplicationWindowExt::content(&f.w.win).unwrap();
    println!("MINCHAIN {}", widest_chain(&content));
    assert!(
        w <= MIN_WIDTH && h <= MIN_HEIGHT,
        "Review with a match selected asks for {w}x{h}; the stated minimum is {MIN_WIDTH}x{MIN_HEIGHT}"
    );
    f.close();
}

/// The chain of widgets that sets a height at `width`: each step is the child asking the most.
/// Printed when a minimum-size test fails, so a host whose toolkit differs from the author's can
/// say what its number was made of.
fn tallest_chain(root: &gtk4::Widget, width: i32) -> String {
    let mut out = Vec::new();
    let mut w = root.clone();
    for _ in 0..40 {
        let min = w.measure(gtk4::Orientation::Vertical, width).0;
        let mut kids = Vec::new();
        let mut best: Option<(i32, gtk4::Widget)> = None;
        let mut c = w.first_child();
        while let Some(child) = c {
            if child.is_visible() {
                let m = child.measure(gtk4::Orientation::Vertical, width).0;
                kids.push(m.to_string());
                if best.as_ref().is_none_or(|(b, _)| m > *b) {
                    best = Some((m, child.clone()));
                }
            }
            c = child.next_sibling();
        }
        out.push(format!(
            "{}[{}]={min} ({})",
            w.type_().name(),
            w.css_name(),
            kids.join("+")
        ));
        match best {
            Some((_, b)) => w = b,
            None => break,
        }
    }
    out.join(" > ")
}

/// DAN-2219 (design read): at the stated minimum the readout sits on the line under the header, and
/// the words there were cut to "SIMILARIT…" with 600 px free on hosted `ci`. The line takes the
/// width of its words, and a match twice the local size says so in the chip ("▲ 2× larger"): the
/// chip was empty in the shots because the fixture gave the local file no size.
fn the_readout_line_under_the_header_is_not_cut_and_carries_the_size_chip(app: &adw::Application) {
    use gtk4::pango::EllipsizeMode;
    let dir = tempfile::tempdir().unwrap();
    let f = review_fx(app, dir.path());
    {
        let mut q = f.w.s.queue.borrow_mut();
        let id = q.id_at(0).unwrap();
        let e = q.entry_mut(id).unwrap();
        e.local_width = Some(640);
        e.local_height = Some(480);
        for c in e.candidates.iter_mut() {
            c.width = Some(1280);
            c.height = Some(960);
        }
    }
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    assert!(pump_until(5_000, || f.w.s.review.local.shows_picture()));
    smallest_window(&f);
    let r = f.w.s.review.clone();
    assert!(
        !r.readout_in_header(),
        "at the stated minimum the readout is on the line under the header"
    );
    assert_eq!(r.readout_diff.text(), "\u{25b2} 2\u{d7} larger");
    for l in [&r.readout_label, &r.readout_diff] {
        assert_eq!(
            l.ellipsize(),
            EllipsizeMode::None,
            "'{}' may be cut on the line under the header",
            l.text()
        );
        assert!(
            !l.layout().is_ellipsized(),
            "'{}' is drawn cut ({} px given)",
            l.text(),
            l.width()
        );
    }
    f.close();
}

/// DAN-2366: `give_way` wraps the Review title and the readout's parts in a `ScrolledWindow` that
/// only clips, and GTK4 scrolled windows take the Tab focus by default, so the first Tab on Review
/// landed on the static "01 // Compare" title. With a match selected the readout (and its wrapper)
/// is shown; the tag list's scroller is the one scrolled window that may be a Tab stop. The stops
/// are read off `is_focusable` (what the Tab chain keys on): `child_focus` in a headless window
/// stalls after the first stop, so a Tab walk here would not reach the wrappers.
fn no_clipping_wrapper_on_the_review_page_is_a_tab_stop(app: &adw::Application) {
    fn wrappers(w: &gtk4::Widget, out: &mut Vec<gtk4::Widget>) {
        if w.is::<gtk4::ScrolledWindow>() || w.is::<gtk4::Viewport>() {
            out.push(w.clone());
        }
        let mut c = w.first_child();
        while let Some(child) = c {
            wrappers(&child, out);
            c = child.next_sibling();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let f = review_fx(app, dir.path());
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    assert!(pump_until(5_000, || f.w.s.review.local.shows_picture()));
    pump_for(300);
    let r = f.w.s.review.clone();
    assert!(
        r.readout_root.is_visible() && r.readout_label.is_mapped(),
        "the readout is shown"
    );
    let mut found = Vec::new();
    wrappers(r.root.upcast_ref(), &mut found);
    let title: Vec<_> = found.iter().filter(|w| r.title.is_ancestor(*w)).collect();
    let readout: Vec<_> = found
        .iter()
        .filter(|w| r.readout_label.is_ancestor(*w))
        .collect();
    assert!(
        !title.is_empty() && !readout.is_empty(),
        "the title and the readout both sit in a clipping wrapper"
    );
    for (what, wrappers) in [("title", title), ("readout", readout)] {
        for w in wrappers {
            assert!(
                !w.is_focusable(),
                "{} around the {what} is a Tab stop",
                w.type_().name()
            );
        }
    }
    f.close();
}

/// DAN-2384: Tab leaves the header. A `GtkScrolledWindow` that is not focusable still answers
/// `child_focus` with `true` without moving focus, so the clipping wrappers of DAN-2366 stopped the
/// walk at the Show in Queue button, and nothing after it was reachable by Tab. Real key events,
/// as `review_keys` sends them.
fn tab_walks_the_review_page_from_the_header_to_the_footer(app: &adw::Application) {
    use common::xtest::{Key, Xtest};
    let dir = tempfile::tempdir().unwrap();
    let f = review_fx(app, dir.path());
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    assert!(pump_until(5_000, || f.w.s.review.local.shows_picture()));
    f.w.win.set_default_size(1100, 800);
    pump_for(300);
    let mut keys = match Xtest::connect() {
        Ok(x) => x,
        Err(why) => {
            assert!(
                std::env::var_os("CI").is_none(),
                "CI is set and the X server cannot inject keys: {why}: failing closed"
            );
            println!("tab_walk: skipped ({why})");
            f.close();
            return;
        }
    };
    for w in gtk4::Window::list_toplevels() {
        if &w != f.w.win.upcast_ref::<gtk4::Widget>() {
            if let Ok(w) = w.downcast::<gtk4::Window>() {
                w.destroy();
            }
        }
    }
    pump_for(400);
    keys.focus_window_at(300, 300).expect("focus the window");
    let r = f.w.s.review.clone();
    let win = f.w.win.clone().upcast::<gtk4::Window>();
    let focus = || gtk4::prelude::GtkWindowExt::focus(&win);
    let named = |w: &Option<gtk4::Widget>| match w {
        Some(w) => format!("{} {:?}", w.type_().name(), w.tooltip_text()),
        None => "nothing".to_owned(),
    };
    r.view_buttons["pair"].grab_focus();
    pump_for(300);
    let mut stops: Vec<gtk4::Widget> = focus().into_iter().collect();
    let footer: gtk4::Widget = r.next_btn.button.clone().upcast();
    let toolbar: gtk4::Widget = r.show_in_queue_btn.clone().upcast();
    for press in 1..=30 {
        keys.tap(Key::Tab, &[]).expect("inject");
        pump_for(250);
        let now = focus();
        assert!(
            now.as_ref() != stops.last(),
            "Tab {press} stays on {}; the walk so far: {:?}",
            named(&now),
            stops.iter().map(|w| w.tooltip_text()).collect::<Vec<_>>()
        );
        let Some(now) = now else {
            panic!("Tab {press} leaves focus on nothing");
        };
        stops.push(now.clone());
        if now == footer {
            break;
        }
    }
    let at = |w: &gtk4::Widget| stops.iter().position(|s| s == w);
    assert!(
        at(&toolbar).is_some(),
        "Tab never reached Show in Queue: {:?}",
        stops.iter().map(|w| w.tooltip_text()).collect::<Vec<_>>()
    );
    assert!(
        at(&footer) > at(&toolbar),
        "Tab never got from Show in Queue to the Next button: {:?}",
        stops.iter().map(|w| w.tooltip_text()).collect::<Vec<_>>()
    );
    f.close();
}

/// The window's outer size once it is told to be as small as it can be.
fn smallest_window(f: &Fx) -> (i32, i32) {
    f.w.win.set_default_size(1, 1);
    pump_for(300);
    (f.w.win.width(), f.w.win.height())
}

/// DAN-2219 (design read): a Not found row's frames are 372 px tall against 305 for a match, and the
/// window had to grow to 812 px at the stated minimum.
fn review_with_a_not_found_row_selected_keeps_the_stated_minimum(app: &adw::Application) {
    use hatate_gtk::window::{MIN_HEIGHT, MIN_WIDTH};
    let dir = tempfile::tempdir().unwrap();
    let f = review_fx(app, dir.path());
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(1);
    pump_for(300);
    let (w, h) = smallest_window(&f);
    println!("OUTER review-not-found window={w}x{h}");
    assert!(
        w <= MIN_WIDTH && h <= MIN_HEIGHT,
        "a Not found row makes the window {w}x{h}; the stated minimum is {MIN_WIDTH}x{MIN_HEIGHT}: {}",
        tallest_chain(f.w.win.upcast_ref(), w)
    );
    f.close();
}

/// DAN-2219 (design read): the Add tags dialog (a 140 px text field and its prompt) pushed the
/// window to 812 px at the stated minimum.
fn review_with_add_tags_open_keeps_the_stated_minimum(app: &adw::Application) {
    use hatate_gtk::dialogs::{AdwPrompter, Prompter};
    use hatate_gtk::window::{MIN_HEIGHT, MIN_WIDTH};
    let dir = tempfile::tempdir().unwrap();
    let f = review_fx(app, dir.path());
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    assert!(pump_until(5_000, || f.w.s.review.local.shows_picture()));
    AdwPrompter::new(&f.w.win).add_tags(
        "Add tags to the selected image. One tag per line, or separated by commas.",
        Box::new(|_| {}),
    );
    pump_for(300);
    let (w, h) = smallest_window(&f);
    println!("OUTER review-add-tags window={w}x{h}");
    assert!(
        w <= MIN_WIDTH && h <= MIN_HEIGHT,
        "Add tags makes the window {w}x{h}; the stated minimum is {MIN_WIDTH}x{MIN_HEIGHT}: {}",
        tallest_chain(f.w.win.upcast_ref(), w)
    );
    f.close();
}

/// DAN-2219: the readout moves between the header and the row under it as the column narrows. The
/// row was shown only if `readout_root.is_visible()`, which also asks the row, so a row hidden
/// while the readout sat in the header stayed hidden once the readout dropped into it: a match was
/// selected and the readout was nowhere. (Hosted `ci` caught it: its fonts need more width.)
fn the_readout_shows_in_the_row_under_the_header_once_it_has_dropped_there(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let f = review_fx(app, dir.path());
    f.w.set_mode("review");
    f.w.win.set_default_size(1500, 900);
    pump_for(200);
    f.w.s.queue_view.select_row(0);
    assert!(pump_until(5_000, || f.w.s.review.local.shows_picture()));
    pump_for(200);
    let r = f.w.s.review.clone();
    assert!(r.readout_root.is_visible());
    assert!(
        r.readout_in_header(),
        "at 1500 px the readout sits beside the tools"
    );
    // Beside the tools the words are not ellipsized: GTK allocated a tracked label its natural width
    // and Pango cut it there, so "01 // Compare" read "01 // COMPAR…" and "SIMILARITY" "SIMILARIT…"
    // with room to spare (the 1440 x 900 shot, host and hosted `ci` toolkits alike).
    for l in [&r.title, &r.readout_label, &r.readout_diff] {
        assert!(
            !l.layout().is_ellipsized(),
            "'{}' is drawn cut beside the tools ({} px given)",
            l.text(),
            l.width()
        );
    }
    // Squeeze the compare column until the readout no longer fits beside the tools.
    f.w.win.set_default_size(1, 1);
    assert!(pump_until(5_000, || !r.readout_in_header()));
    pump_for(200);
    assert!(
        r.readout_root.is_visible(),
        "the readout is on the line under the header"
    );
    f.close();
}

/// A box's minimum is its children's: a hint bar that has not yet been given a width must not ask
/// for all of its groups, or a window that opens at the stated minimum is sized 1028 px wide.
fn a_fresh_hint_bar_asks_for_its_first_group_only(_app: &adw::Application) {
    use hatate_gtk::widgets::{KeyHintBar, Piece};
    let groups: Vec<Vec<Piece>> = (0..4)
        .map(|i| {
            vec![
                Piece::Strong(format!("{i} of 9")),
                Piece::Text("a fairly long group of words".to_owned()),
            ]
        })
        .collect();
    let bar = KeyHintBar::new(&groups);
    let (first, _, _, _) = bar.root.measure(gtk4::Orientation::Horizontal, -1);
    let one = KeyHintBar::new(&groups[..1]);
    let (alone, _, _, _) = one.root.measure(gtk4::Orientation::Horizontal, -1);
    assert_eq!(
        first, alone,
        "a four-group bar asked for {first} px before its first tick; its first group alone is {alone}"
    );
}

/// `review_shortcuts`: the keys on the Review page move, mark and skip, and a text field keeps
/// its characters.
fn review_keys_step_mark_skip_and_open_the_match(app: &adw::Application) {
    use common::xtest::{Key, Xtest};
    let dir = tempfile::tempdir().unwrap();
    let opened = Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let o = opened.clone();
    let f = Fx::new_with(
        app,
        Opts {
            seed: review_seed(dir.path()),
            tweak: Some(offline()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
        move |env| env.open_url = Rc::new(move |u| o.borrow_mut().push(u.to_owned())),
    );
    already_fetched(&f);
    let r = f.w.s.review.clone();
    let selected = |f: &Fx| -> Vec<u32> {
        f.w.s
            .queue_view
            .selected_ids()
            .iter()
            .filter_map(|id| f.w.s.queue_view.row_of(*id))
            .collect()
    };

    // The handlers, by action id: J / K stop at the ends, N skips what is decided.
    f.w.run_review_action("next_row");
    assert_eq!(selected(&f), [0], "from nothing, next lands on the first");
    f.w.run_review_action("next_row");
    f.w.run_review_action("next_row");
    f.w.run_review_action("next_row");
    assert_eq!(selected(&f), [2], "stops at the end rather than wrapping");
    f.w.run_review_action("prev_row");
    assert_eq!(selected(&f), [1]);
    f.w.s.queue_view.select_row(0);
    f.w.run_review_action("toggle_reviewed");
    assert_eq!(r.mark_btn.text(), "Unmark reviewed");
    assert_eq!(
        f.say(),
        "Marked 1 image(s) as reviewed \u{2014} 2 still to review"
    );
    f.w.run_review_action("next_unreviewed");
    assert_eq!(selected(&f), [1], "the next row still wanting a decision");
    f.w.run_review_action("next_unreviewed");
    assert_eq!(selected(&f), [2]);
    f.w.run_review_action("next_unreviewed");
    assert_eq!(f.say(), "Nothing left to review after this one");
    assert_eq!(selected(&f), [2], "and it stays put");
    f.w.run_review_action("prev_unreviewed");
    assert_eq!(selected(&f), [1]);

    // Open match: a status line with no match, a warning for a bad address, the browser for a good one.
    f.w.run_review_action("open_match");
    assert_eq!(f.say(), "Open match: this image has no match");
    f.w.s.queue_view.select_row(2);
    f.w.run_review_action("open_match");
    assert_eq!(*opened.borrow(), ["https://danbooru.donmai.us/posts/3"]);

    // The same handlers from the keys, on the Review page.
    f.w.set_mode("review");
    f.w.win.set_default_size(1100, 800);
    let mut keys = match Xtest::connect() {
        Ok(x) => x,
        Err(why) => {
            assert!(
                std::env::var_os("CI").is_none(),
                "CI is set and the X server cannot inject keys: {why}: failing closed"
            );
            println!("review_keys: the key half is skipped ({why})");
            f.close();
            return;
        }
    };
    for w in gtk4::Window::list_toplevels() {
        if &w != f.w.win.upcast_ref::<gtk4::Widget>() {
            if let Ok(w) = w.downcast::<gtk4::Window>() {
                w.destroy();
            }
        }
    }
    pump_for(400);
    keys.focus_window_at(300, 300).expect("focus the window");
    f.w.s.review.root.grab_focus();
    pump_for(200);
    let mut press = |key: Key| {
        keys.tap(key, &[]).expect("inject");
        pump_for(300);
    };
    f.w.s.queue_view.select_row(0);
    pump_for(100);
    press(Key::Char('j'));
    assert_eq!(
        selected(&f),
        [1],
        "J on the Review page steps to the next image"
    );
    press(Key::Char('k'));
    assert_eq!(selected(&f), [0], "K steps back");
    press(Key::Char(' '));
    assert!(
        !f.w.s.queue.borrow().entries()[0].reviewed,
        "Space unmarks the marked row"
    );
    f.close();
}

/// A flat grey picture, with a bright patch when `patch` is set.
fn grey_png(w: u32, h: u32, patch: bool) -> Vec<u8> {
    let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([90, 90, 90, 255]));
    if patch {
        for y in h / 4..h / 2 {
            for x in w / 4..w / 2 {
                img.put_pixel(x, y, image::Rgba([240, 240, 240, 255]));
            }
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// The match's file as a `data:` URI, so the fetch needs no network at all.
fn data_uri(bytes: &[u8]) -> String {
    let mut uri = String::from("data:image/png,");
    for b in bytes {
        uri.push_str(&format!("%{b:02X}"));
    }
    uri
}

/// The review seed with real local pictures (160 x 120) and, for the rows with a match, a full file
/// at another size (200 x 150, a patch added) that is already named, so no site is asked.
fn wipe_seed(dir: &std::path::Path) -> Vec<hatate_core::model::Entry> {
    let mut seed = review_seed(dir);
    for e in seed.iter_mut() {
        std::fs::write(&e.path, grey_png(160, 120, false)).unwrap();
        for c in e.candidates.iter_mut() {
            c.direct_file_url = Some(data_uri(&grey_png(200, 150, true)));
        }
    }
    seed
}

fn wipe_fx(app: &adw::Application, seed: Vec<hatate_core::model::Entry>) -> Fx {
    let f = Fx::new(
        app,
        Opts {
            seed,
            tweak: Some(offline()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    already_fetched(&f);
    f
}

/// `_on_review_view_changed`, `_ensure_comparison_loaded`, `_apply_wipe_mode`, `_show_differences`,
/// `refresh_review` (DAN-2340): Wipe fetches the full file once, Differences builds the overlay
/// off the main loop, and a new selection lets the pair go.
fn review_wipe_and_differences_load_off_the_main_loop_and_follow_the_selection(
    app: &adw::Application,
) {
    let dir = tempfile::tempdir().unwrap();
    let f = wipe_fx(app, wipe_seed(dir.path()));
    let r = f.w.s.review.clone();
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    assert!(pump_until(5_000, || r.local.shows_picture()));
    assert_eq!(r.stack.visible_child_name().as_deref(), Some("pair"));
    assert!(!r.slider_row.is_visible() && !r.zoom_tools.is_visible());
    assert!(r.view_buttons["wipe"].is_sensitive() && r.view_buttons["diff"].is_sensitive());

    // Wipe: the page switches at once and says it is loading; the pair arrives off the main loop.
    r.view_buttons["wipe"].set_active(true);
    assert_eq!(r.stack.visible_child_name().as_deref(), Some("comparison"));
    assert!(r.slider_row.is_visible() && r.zoom_tools.is_visible() && r.note.is_visible());
    assert!(
        pump_until(10_000, || r.wipe.has_pictures() == (true, true)),
        "both pictures arrive"
    );
    assert_eq!(r.note.text(), hatate_gtk::review_view::WIPE_HINT);
    assert!(!r.wipe.showing_differences());
    pump_for(50);
    assert!(
        r.zoom_label.text().ends_with('%') && r.wipe.zoom_percent() > 0.0,
        "the zoom is reported: {:?}",
        r.zoom_label.text()
    );

    // The slider moves the wipe.
    r.wipe_slider.set_value(25.0);
    assert!((r.wipe.position() - 0.25).abs() < 1e-9);

    // Zoom in, then fit.
    let fit = r.wipe.zoom_percent();
    r.wipe.zoom_in();
    assert!((r.wipe.zoom_percent() / fit - 1.25).abs() < 1e-9);
    r.wipe.zoom_to_fit();
    assert!((r.wipe.zoom_percent() - fit).abs() < 1e-9);

    // Differences: the overlay is built on a worker; the note carries the verdict.
    r.view_buttons["diff"].set_active(true);
    assert!(pump_until(10_000, || r.wipe.showing_differences()));
    let note = r.note.text().to_string();
    assert!(
        note.contains("drag the wipe to sweep the highlights across both"),
        "{note}"
    );
    assert!(
        note.starts_with(|c: char| c.is_ascii_digit()),
        "the verdict leads: {note}"
    );
    assert!(
        (r.wipe.position() - 0.25).abs() < 1e-9,
        "the slider's place is kept"
    );

    // Back to Wipe: the highlights go, the how-to comes back.
    r.view_buttons["wipe"].set_active(true);
    assert!(!r.wipe.showing_differences());
    assert_eq!(r.note.text(), hatate_gtk::review_view::WIPE_HINT);

    // A new selection lets the pair go and, with the comparison showing, fetches the new one.
    f.w.s.queue_view.select_row(2);
    assert!(pump_until(10_000, || {
        f.w.current_review_key() == f.w.s.comparison.borrow().loaded_for.clone()
    }));
    assert_eq!(r.wipe.has_pictures(), (true, true));

    // A row with no match: Wipe has only the local picture, Differences is off.
    f.w.s.queue_view.select_row(1);
    assert!(pump_until(10_000, || {
        f.w.current_review_key() == f.w.s.comparison.borrow().loaded_for.clone()
    }));
    assert_eq!(r.wipe.has_pictures(), (true, false));
    assert!(!r.view_buttons["diff"].is_sensitive());
    assert!(r.note.text().contains("No match image could be fetched."));

    // Side by side again hides the controls.
    r.view_buttons["pair"].set_active(true);
    assert_eq!(r.stack.visible_child_name().as_deref(), Some("pair"));
    assert!(!r.slider_row.is_visible());
    f.close();
}

/// Design gate 2.8: a local file that cannot be opened says what to do in plain words, keeps the OS
/// text in the detail line, and the slider goes insensitive while the note stays at its ordinary
/// colour (DAN-2385: it is the one sentence that says what to do, not a control).
fn review_wipe_with_an_unreadable_local_file_shows_the_error_state(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let seed = wipe_seed(dir.path());
    let missing = seed[0].path.clone();
    std::fs::remove_file(&missing).unwrap();
    let f = wipe_fx(app, seed);
    let r = f.w.s.review.clone();
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    r.view_buttons["wipe"].set_active(true);
    assert!(pump_until(10_000, || r.error_detail.is_visible()));
    let note = r.note.text().to_string();
    assert!(note.contains("could not be opened"), "{note}");
    assert!(
        !note.contains(&missing) && !note.contains("os error"),
        "the message leaks neither the path nor the OS text: {note}"
    );
    assert!(
        r.error_detail.text().contains(&missing) && r.error_detail.text().contains("os error"),
        "the detail line has them: {}",
        r.error_detail.text()
    );
    assert!(!r.wipe_slider.is_sensitive());
    assert!(
        r.note.is_sensitive(),
        "the error note is not painted at the disabled colour"
    );
    assert!(!r.view_buttons["diff"].is_sensitive());
    assert_eq!(r.wipe.has_pictures(), (false, true));

    // Selecting a row with a readable file leaves the error state.
    f.w.s.queue_view.select_row(2);
    assert!(pump_until(10_000, || !r.error_detail.is_visible()
        && r.wipe.has_pictures() == (true, true)));
    assert!(r.wipe_slider.is_sensitive() && r.note.is_sensitive());
    f.close();
}

/// `_apply_wipe_mode` runs `clear_difference_view()` -> `zoom_to_fit()` for every non-diff view, so
/// Python goes back to fit on re-entering Wipe even when the local file cannot be opened (DAN-2380,
/// seed 6 compare-021). The error state must not skip it.
fn review_wipe_error_state_returns_to_fit_on_re_entering_wipe(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let seed = wipe_seed(dir.path());
    std::fs::remove_file(&seed[0].path).unwrap();
    let f = wipe_fx(app, seed);
    let r = f.w.s.review.clone();
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    r.view_buttons["wipe"].set_active(true);
    assert!(pump_until(10_000, || r.error_detail.is_visible()
        && r.wipe.zoom_percent() > 0.0));
    assert_eq!(r.wipe.zoom_factor(), 1.0);

    r.wipe.zoom_out();
    assert!(r.wipe.zoom_factor() < 1.0, "zoomed out in the error state");
    r.view_buttons["pair"].set_active(true);
    r.view_buttons["wipe"].set_active(true);
    assert_eq!(r.wipe.zoom_factor(), 1.0, "back at fit on re-entering Wipe");
    // Still the error state: the note, the detail line and the insensitive slider stay.
    assert!(r.error_detail.is_visible());
    assert!(
        r.note.text().contains("could not be opened"),
        "{}",
        r.note.text()
    );
    assert!(!r.wipe_slider.is_sensitive() && r.note.is_sensitive());
    f.close();
}

/// `TestWipeChrome` (DAN-1393) at the parts the Python tests name: the slider's ends, the zoom row's
/// four buttons, the one-line hint and the warning that replaces it when only a preview came down,
/// and the chrome staying out of Side by side.
fn review_wipe_chrome_names_its_ends_and_zoom_buttons_and_a_fallback_warns(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let mut seed = wipe_seed(dir.path());
    // Row 0's match has no full file, only a preview: the comparison falls back and says so.
    let preview = seed[0].candidates[0].direct_file_url.take();
    seed[0].candidates[0].preview_url = preview;
    let f = wipe_fx(app, seed);
    let r = f.w.s.review.clone();
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    r.view_buttons["wipe"].set_active(true);
    assert!(pump_until(10_000, || r.wipe.has_pictures() == (true, true)));

    let mut ends = Vec::new();
    let mut c = r.slider_row.first_child();
    while let Some(w) = c {
        if let Some(l) = w.downcast_ref::<gtk4::Label>() {
            ends.push(l.text().to_string());
        }
        c = w.next_sibling();
    }
    assert_eq!(ends, ["Local", "Match"]);
    let mut buttons = Vec::new();
    let mut c = r.zoom_tools.first_child();
    while let Some(w) = c {
        if let Some(b) = w.downcast_ref::<gtk4::Button>() {
            buttons.push(b.label().unwrap_or_default().to_string());
        }
        c = w.next_sibling();
    }
    assert_eq!(buttons, ["\u{2212}", "+", "Fit", "100%"]);
    assert!(
        r.note.text().starts_with("Showing the downscaled preview"),
        "{}",
        r.note.text()
    );

    // Side by side again: the slider row, the note and the zoom row go.
    r.view_buttons["pair"].set_active(true);
    assert!(!r.slider_row.is_visible() && !r.note.is_visible() && !r.zoom_tools.is_visible());
    f.close();
}

/// D-C (docs/gtk-6f2.md): Python prints `0%` in the review zoom readout for an empty frame; here it
/// is blank. The readout is visible only in the "Loading the match..." moment, which is when the
/// frame is empty, so this is where the difference could be seen.
fn review_zoom_readout_is_blank_while_nothing_is_loaded(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let f = wipe_fx(app, wipe_seed(dir.path()));
    let r = f.w.s.review.clone();
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    r.view_buttons["wipe"].set_active(true);
    assert!(pump_until(10_000, || r.wipe.has_pictures() == (true, true)));
    pump_for(50);
    assert!(
        r.zoom_label.text().ends_with('%'),
        "{:?}",
        r.zoom_label.text()
    );

    // A new selection lets the pair go and fetches the next: until it arrives the frame is empty,
    // the readout is on screen, and it says nothing.
    f.w.s.queue_view.select_row(2);
    assert_eq!(r.wipe.has_pictures(), (false, false));
    assert!(r.zoom_tools.is_visible());
    assert_eq!(r.zoom_label.text(), "");
    assert!(pump_until(10_000, || r.wipe.has_pictures() == (true, true)));
    f.close();
}

/// D-E (docs/gtk-6f2.md): after an unopenable row, selecting a readable one on Side by side makes
/// the Differences button live again at once. Python leaves it disabled until the next comparison
/// load, so its button is dead until Wipe has been clicked once.
fn review_differences_button_is_live_again_after_an_unopenable_row(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let seed = wipe_seed(dir.path());
    std::fs::remove_file(&seed[0].path).unwrap();
    let f = wipe_fx(app, seed);
    let r = f.w.s.review.clone();
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    r.view_buttons["wipe"].set_active(true);
    assert!(pump_until(10_000, || r.error_detail.is_visible()));
    assert!(!r.view_buttons["diff"].is_sensitive());

    // Back to Side by side and on to a readable row, with no Wipe click.
    r.view_buttons["pair"].set_active(true);
    f.w.s.queue_view.select_row(2);
    assert!(pump_until(5_000, || r.local.shows_picture()));
    assert_eq!(r.stack.visible_child_name().as_deref(), Some("pair"));
    assert!(r.view_buttons["diff"].is_sensitive());
    f.close();
}

/// Design gate 2.5 at the Wipe, Differences, no-match and error states (DAN-2340, DAN-2356): the
/// window opened at the stated minimum reads exactly 1000 x 780 inside its shadow (the shot's PNG
/// header), not 788 and not 854. The fit is measured on the shot path: the state is prepared first,
/// then the window is given the smallest size, with no later resize.
fn review_comparison_states_keep_the_stated_minimum(app: &adw::Application) {
    use hatate_gtk::window::{MIN_HEIGHT, MIN_WIDTH};
    for state in ["wipe", "differences", "error", "no-match"] {
        let dir = tempfile::tempdir().unwrap();
        let seed = wipe_seed(dir.path());
        let gone = seed[0].path.clone();
        let f = wipe_fx(app, seed);
        let r = f.w.s.review.clone();
        f.w.set_mode("review");
        // The window is given the smallest size first, as a shot opens at `min`; the state is
        // prepared after, and a state that asks for more makes the window grow.
        smallest_window(&f);
        f.w.s
            .queue_view
            .select_row(if state == "no-match" { 1 } else { 0 });
        if state == "error" {
            std::fs::remove_file(&gone).unwrap();
        }
        assert!(pump_until(5_000, || r.local.shows_picture() || state == "error"));
        r.view_buttons[if state == "differences" {
            "diff"
        } else {
            "wipe"
        }]
        .set_active(true);
        assert!(
            pump_until(10_000, || match state {
                "error" => r.error_detail.is_visible(),
                "no-match" => r.wipe.has_pictures() == (true, false),
                "differences" => r.wipe.showing_differences(),
                _ => r.wipe.has_pictures() == (true, true),
            }),
            "{state}: the page settles"
        );
        pump_for(300);
        let (w, h) = (f.w.win.width(), f.w.win.height());
        println!("OUTER review-{state} window={w}x{h}");
        assert!(
            w == MIN_WIDTH - 10 && h == MIN_HEIGHT - 10,
            "{state}: the window at the stated minimum is {w}x{h}, not {}x{}: {}",
            MIN_WIDTH - 10,
            MIN_HEIGHT - 10,
            tallest_chain(f.w.win.upcast_ref(), w)
        );
        f.close();
    }
}

/// A site that accepts the connection and never answers: the window keeps handling actions while
/// the full-resolution fetch waits, and a new selection lets the stale fetch go.
fn review_wipe_waits_on_a_stalled_site_without_blocking_the_window(app: &adw::Application) {
    use std::io::Read;
    use std::net::TcpListener;
    let dir = tempfile::tempdir().unwrap();
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();
    let held = std::thread::spawn(move || {
        let (mut conn, _) = server.accept().unwrap();
        let mut buf = [0u8; 1024];
        let _ = conn.read(&mut buf);
        std::thread::sleep(std::time::Duration::from_millis(900));
    });
    let mut seed = wipe_seed(dir.path());
    seed[0].candidates[0].direct_file_url = Some("http://www.deviantart.com/full-1.png".to_owned());
    let f = Fx::new_with(
        app,
        Opts {
            seed,
            tweak: Some(offline()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
        |env| {
            let mut net = hatate_core::net::NetConfig::default();
            net.host_overrides
                .push(("www.deviantart.com".to_owned(), addr));
            let paths = env.paths.clone();
            let (backend, _) = common::search::Scripted::with_net(&paths, net);
            env.search = Some(backend);
        },
    );
    already_fetched(&f);
    let r = f.w.s.review.clone();
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    let ticks = Rc::new(Cell::new(0u32));
    let t = ticks.clone();
    let tick = gtk4::glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        t.set(t.get() + 1);
        gtk4::glib::ControlFlow::Continue
    });
    r.view_buttons["wipe"].set_active(true);
    assert_eq!(
        r.note.text(),
        "Loading the match at full resolution\u{2026}"
    );
    assert!(f.w.s.comparison_worker.is_running());
    // The first frame lays the fonts out; the property is that the loop turns while the site waits.
    pump_for(250);
    let before = ticks.get();
    f.w.run_review_action("toggle_reviewed");
    assert!(f.w.s.queue.borrow().entries()[0].reviewed);
    pump_for(200);
    assert!(
        ticks.get() >= before + 4,
        "the main loop kept turning: {before} -> {}",
        ticks.get()
    );
    assert!(
        f.w.s.comparison_worker.is_running(),
        "still waiting on the site"
    );
    // The site lets go; the fetch ends with no picture for the match, and the note says so.
    assert!(pump_until(10_000, || r
        .note
        .text()
        .contains("No match image could be fetched.")));
    assert_eq!(r.wipe.has_pictures(), (true, false));
    tick.remove();
    held.join().unwrap();
    f.close();
}

/// `CompareDialog`: opens on "Loading…", then the summary, the pair and the four modes
/// (`_apply_mode`), with Differences built once and off the main loop.
fn compare_dialog_loads_the_pair_and_switches_modes(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let f = wipe_fx(app, wipe_seed(dir.path()));
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    let id = f.w.current_id().unwrap();
    f.w.open_compare_dialog(id);
    let dialog =
        f.w.s
            .compare_open
            .borrow()
            .clone()
            .expect("the dialog is open");
    assert_eq!(dialog.window.title().as_deref(), Some("Compare - a.png"));
    assert!(pump_until(10_000, || dialog.view.has_pictures() == (true, true)));
    let lines = dialog.summary_text();
    assert!(lines[0].starts_with("Local: 160\u{d7}120"), "{lines:?}");
    assert!(lines[1].starts_with("Match: 200\u{d7}150"), "{lines:?}");
    assert!(
        lines.iter().any(|l| l.starts_with("The match is ")),
        "the size verdict: {lines:?}"
    );
    assert!(dialog.diff_radio.is_sensitive() && dialog.wipe_radio.is_active());
    assert_eq!(dialog.slider_right_label.text(), "Match");
    assert!(!dialog.diff_note.is_visible());

    // Local only and Match only park the wipe at an end and lock the slider.
    dialog.local_radio.set_active(true);
    assert!(!dialog.slider.is_sensitive() && (dialog.view.position() - 1.0).abs() < 1e-9);
    dialog.matched_radio.set_active(true);
    assert!(!dialog.slider.is_sensitive() && dialog.view.position().abs() < 1e-9);
    dialog.wipe_radio.set_active(true);
    dialog.slider.set_value(30.0);
    assert!(dialog.slider.is_sensitive() && (dialog.view.position() - 0.3).abs() < 1e-9);

    // Differences: the slider's right-hand name changes with it; the overlay is built on a worker.
    dialog.diff_radio.set_active(true);
    assert_eq!(dialog.slider_right_label.text(), "Differences");
    assert!(pump_until(10_000, || dialog.view.showing_differences()));
    assert!(dialog.diff_note.is_visible());
    assert!(
        dialog
            .diff_note
            .text()
            .contains("drag the wipe to sweep the highlights across both")
    );
    dialog.wipe_radio.set_active(true);
    assert!(!dialog.view.showing_differences() && !dialog.diff_note.is_visible());

    dialog.window.close();
    assert!(
        f.w.s.compare_open.borrow().is_none(),
        "closing lets go of it"
    );
    f.close();
}

/// `_open_compare_dialog`'s two guards: no match, and a local file that is gone.
fn compare_dialog_answers_in_a_box_when_there_is_nothing_to_compare(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let seed = wipe_seed(dir.path());
    let gone = seed[2].path.clone();
    std::fs::remove_file(&gone).unwrap();
    let f = wipe_fx(app, seed);
    f.w.set_mode("review");

    // Row 1 has no match.
    f.w.s.queue_view.select_row(1);
    let b = f.w.current_id().unwrap();
    f.w.open_compare_dialog(b);
    assert!(pump_until(5_000, || f.script.count_shown() == 1));
    let shown = f.script.last_shown();
    assert_eq!(
        (shown.title.as_str(), shown.text.as_str()),
        ("Compare", "This image has no match to compare against yet.")
    );
    assert!(f.w.s.compare_open.borrow().is_none());

    // Row 2's file is gone.
    f.w.s.queue_view.select_row(2);
    let c = f.w.current_id().unwrap();
    f.w.open_compare_dialog(c);
    assert!(pump_until(5_000, || f.script.count_shown() == 2));
    assert_eq!(
        f.script.last_shown().text,
        "The local file is no longer on disk, so there's nothing to compare the match against."
    );
    assert!(f.w.s.compare_open.borrow().is_none());

    // The Compare key on the Review page asks the same way.
    f.w.run_review_action("compare");
    assert!(pump_until(5_000, || f.script.count_shown() == 3));
    f.close();
}

/// The largest spread between a pixel's red, green and blue over a rendered pane (cairo's B, G, R, A).
fn max_channel_spread(bgra: &[u8]) -> u8 {
    bgra.chunks_exact(4)
        .map(|p| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2]))
        .max()
        .unwrap_or(0)
}

/// Design gate 2.2: the Differences pane is an R=G=B ramp, whatever colour the pictures are. The
/// Wipe pane shows them in colour, which is the control that proves the sampler can see chroma.
fn the_differences_pane_has_zero_chroma_and_the_wipe_pane_shows_the_pictures_colours(
    app: &adw::Application,
) {
    let dir = tempfile::tempdir().unwrap();
    let mut seed = wipe_seed(dir.path());
    // Coloured pictures: a red local file, a blue-ish match with a bright patch.
    let tint = |w: u32, h: u32, rgb: [u8; 3], patch: bool| {
        let mut img =
            image::RgbaImage::from_pixel(w, h, image::Rgba([rgb[0], rgb[1], rgb[2], 255]));
        if patch {
            for y in h / 4..h / 2 {
                for x in w / 4..w / 2 {
                    img.put_pixel(x, y, image::Rgba([250, 250, 40, 255]));
                }
            }
        }
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    };
    std::fs::write(&seed[0].path, tint(160, 120, [200, 40, 40], false)).unwrap();
    seed[0].candidates[0].direct_file_url = Some(data_uri(&tint(200, 150, [40, 60, 200], true)));
    let f = wipe_fx(app, seed);
    let r = f.w.s.review.clone();
    f.w.set_mode("review");
    f.w.s.queue_view.select_row(0);
    r.view_buttons["wipe"].set_active(true);
    assert!(pump_until(10_000, || r.wipe.has_pictures() == (true, true)));
    assert!(pump_until(5_000, || r.wipe.area.width() > 0
        && r.wipe.render().is_some()));
    let (wipe_px, _, _) = r.wipe.render().unwrap();
    assert!(
        max_channel_spread(&wipe_px) > 100,
        "the control: the Wipe pane carries the pictures' colour ({})",
        max_channel_spread(&wipe_px)
    );

    r.view_buttons["diff"].set_active(true);
    assert!(pump_until(10_000, || r.wipe.showing_differences()));
    let (px, w, h) = r.wipe.render().unwrap();
    assert_eq!(
        max_channel_spread(&px),
        0,
        "no pixel of the {w} x {h} Differences pane has chroma"
    );
    // And it is not blank: the red local file's luma (87) sits at 26 + 87 * 171 / 255 = 84 on the
    // ramp, and the overlay lifts the changed patch toward the light end.
    let on_ramp = px
        .chunks_exact(4)
        .filter(|p| p[3] == 255 && p[0] == 84)
        .count();
    assert!(
        on_ramp > 100,
        "the local picture is on the ramp: {on_ramp} px"
    );
    assert!(
        px.chunks_exact(4).any(|p| p[3] == 255 && p[0] > 150),
        "the highlighted patch is lifted toward the ramp's light end"
    );
    f.close();
}

// ---------------------------------------------------------------------------------------------
// Hydrus (6.g, DAN-2220). Every answer comes from a scripted local server; every key is a
// placeholder.
// ---------------------------------------------------------------------------------------------

const NEW_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HELD_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Settings that point the window at the scripted Hydrus (`hydrus.api_url`, as the user's own
/// setting does) and keep it off every other host.
fn hydrus_tweak(url: &str, key: bool) -> Tweak {
    let url = url.to_owned();
    Box::new(move |s: &mut hatate_core::config::Settings| {
        s.retrieve_tags_from_booru = false;
        s.drop_dead_matches = false;
        s.hydrus.api_url = url.clone();
        if key {
            s.hydrus.access_key = hatate_core::config::Secret::new(common::hydrus::KEY);
        }
        s.hydrus.timeout = 5.0;
        // The scripted https server's CA is throw-away: the user's own `verify_ssl` setting.
        s.hydrus.verify_ssl = false;
        s.remove_after_import = false;
    })
}

fn hydrus_fx(
    app: &adw::Application,
    url: &str,
    key: bool,
    seed: Vec<hatate_core::model::Entry>,
) -> Fx {
    Fx::new(
        app,
        Opts {
            seed,
            tweak: Some(hydrus_tweak(url, key)),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    )
}

fn row_ids(f: &Fx) -> Vec<hatate_core::queue::EntryId> {
    let q = f.w.s.queue.borrow();
    (0..q.len() as i64).filter_map(|r| q.id_at(r)).collect()
}

/// A Hydrus that takes every upload and answers `file_metadata` from `state`.
fn accepting_hydrus(
    state: &'static str,
) -> impl Fn(&common::hydrus::Recorded) -> common::hydrus::Reply + Send + Sync + 'static {
    use common::hydrus::json_reply;
    use serde_json::json;
    move |q| match q.target.split('?').next().unwrap_or("") {
        "/add_files/add_file" => json_reply(json!({"status": 1, "hash": NEW_HASH, "note": ""})),
        "/get_files/file_metadata" => json_reply(json!({"metadata": [
            {"hash": HELD_HASH, "file_id": 1, "is_local": state == "present",
             "is_trashed": state == "trashed", "is_deleted": state == "deleted"}]})),
        "/manage_pages/new_page" => json_reply(json!({"page_key": "k"})),
        _ => json_reply(json!({})),
    }
}

/// `action_send_to_hydrus`: the selected rows go up with their URL and tags, the Sent column
/// follows, and the status line counts them.
fn hydrus_send_uploads_the_selected_rows_and_marks_them_sent(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(accepting_hydrus("present"));
    let f = hydrus_fx(app, &server.url, true, review_seed(dir.path()));
    let ids = row_ids(&f);
    f.w.action_send_to_hydrus(&[ids[0], ids[2]]);
    assert!(pump_until(10_000, || f.say().starts_with("Sent ")));
    assert_eq!(f.say(), "Sent 2 file(s) to Hydrus");
    {
        let q = f.w.s.queue.borrow();
        for i in [0, 2] {
            let e = &q.entries()[i];
            assert!(e.sent_to_hydrus && e.hydrus_import_confirmed, "row {i}");
            assert_eq!(e.hydrus_hash.as_deref(), Some(NEW_HASH));
        }
        assert!(
            !q.entries()[1].sent_to_hydrus,
            "the unselected row is untouched"
        );
    }
    let calls = server.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|c| *c == "POST /add_files/add_file")
            .count(),
        2
    );
    assert!(calls.iter().any(|c| c == "POST /add_urls/associate_url"));
    assert!(calls.iter().any(|c| c == "POST /add_tags/add_tags"));
    assert_eq!(f.script.count_shown(), 0, "nothing to warn about");
    f.close();
}

/// The same send over https: the scheme the shipped client uses against a TLS Hydrus.
fn hydrus_send_works_over_https_with_the_users_verify_setting(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::https(accepting_hydrus("present"));
    assert!(server.url.starts_with("https://"));
    let f = hydrus_fx(app, &server.url, true, review_seed(dir.path()));
    let ids = row_ids(&f);
    f.w.action_send_to_hydrus(&[ids[0]]);
    assert!(pump_until(10_000, || f.say().starts_with("Sent ")));
    assert_eq!(f.say(), "Sent 1 file(s) to Hydrus");
    assert!(f.w.s.queue.borrow().entries()[0].sent_to_hydrus);
    assert!(
        server
            .calls()
            .contains(&"POST /add_files/add_file".to_owned()),
        "the upload reached the https server"
    );
    f.close();
}

/// A refusal is one box for the batch, with the file and Hydrus's own words in the details, and
/// the rows stay unsent.
fn hydrus_send_reports_what_failed_in_one_box_and_leaves_the_rows_unsent(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(|_| common::hydrus::Reply::new(500, "boom"));
    let f = hydrus_fx(app, &server.url, true, review_seed(dir.path()));
    let ids = row_ids(&f);
    f.w.action_send_to_hydrus(&[ids[0], ids[1]]);
    assert!(pump_until(10_000, || f.script.count_shown() > 0));
    let d = f.script.last_shown();
    assert_eq!(
        (d.kind, d.title.as_str()),
        (Kind::Warning, "Send to Hydrus")
    );
    assert_eq!(d.text, "2 file(s) failed to import.");
    assert!(d.detail.starts_with("a.png: "), "{}", d.detail);
    assert!(d.detail.contains("\nb.png: "), "{}", d.detail);
    assert_eq!(f.say(), "Sent 0 file(s) to Hydrus, 2 failed");
    let q = f.w.s.queue.borrow();
    assert!(q.entries().iter().all(|e| !e.sent_to_hydrus));
    assert!(q.entries()[0].error_message.is_some(), "the row says why");
    drop(q);
    f.close();
}

/// The checks every Hydrus action makes first, in Python's order: a selection, then a key. No
/// request leaves the window for either.
fn hydrus_actions_ask_for_a_selection_and_a_key_before_anything_goes_out(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(accepting_hydrus("present"));
    let f = hydrus_fx(app, &server.url, false, review_seed(dir.path()));
    let ids = row_ids(&f);
    f.w.action_send_to_hydrus(&[]);
    let d = f.script.last_shown();
    assert_eq!(
        (d.kind, d.title.as_str(), d.text.as_str()),
        (
            Kind::Information,
            "Send to Hydrus",
            "Select at least one row."
        )
    );
    f.w.action_send_to_hydrus(&ids[..1]);
    let d = f.script.last_shown();
    assert_eq!(
        (d.kind, d.title.as_str(), d.text.as_str()),
        (
            Kind::Warning,
            "Send to Hydrus",
            "Set your Hydrus access key in Settings first."
        )
    );
    f.w.action_import_url_to_hydrus(&ids[..1]);
    assert_eq!(f.script.last_shown().title, "Send URL to Hydrus's Importer");
    f.w.action_download_and_send_to_hydrus(&ids[..1]);
    assert_eq!(f.script.last_shown().title, "Download + Send to Hydrus");
    f.w.show_rows_in_hydrus(&ids[..1]);
    assert_eq!(f.script.last_shown().title, "Show in Hydrus");
    f.w.delete_rows_from_hydrus(&ids[..1]);
    assert_eq!(f.script.last_shown().title, "Delete from Hydrus");
    assert_eq!(f.script.count_shown(), 6);
    assert!(
        server.calls().is_empty(),
        "nothing was sent: {:?}",
        server.calls()
    );
    assert!(!f.w.hydrus_configured());
    f.close();
}

/// `_delete_rows_from_hydrus`: Hydrus is asked first, the confirmation's default is Cancel, and
/// only a Delete answer reaches the delete endpoint and takes the rows out of the list.
fn hydrus_delete_defaults_to_cancel_and_only_deletes_after_the_answer(app: &adw::Application) {
    use common::hydrus::json_reply;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let deleted = Arc::new(AtomicBool::new(false));
    let d2 = deleted.clone();
    let server =
        common::hydrus::Hydrus::http(move |q| match q.target.split('?').next().unwrap_or("") {
            "/add_files/delete_files" => {
                d2.store(true, Ordering::SeqCst);
                json_reply(json!({}))
            }
            "/get_files/file_metadata" => {
                let gone = d2.load(Ordering::SeqCst);
                json_reply(json!({"metadata": [
                    {"hash": HELD_HASH, "file_id": 1, "is_local": !gone,
                     "is_trashed": gone, "is_deleted": false}]}))
            }
            _ => json_reply(json!({})),
        });
    let mut seed = review_seed(dir.path());
    seed[0].hydrus_hash = Some(HELD_HASH.to_owned());
    let f = hydrus_fx(app, &server.url, true, seed);
    let ids = row_ids(&f);

    // Unscripted: the box is answered by its default button, as Enter would.
    f.w.delete_rows_from_hydrus(&[ids[0], ids[1]]);
    assert!(pump_until(10_000, || f.script.count_shown() > 0));
    let d = f.script.last_shown();
    assert_eq!(
        (d.kind, d.title.as_str()),
        (Kind::Warning, "Delete from Hydrus")
    );
    assert_eq!(d.buttons, Buttons::DeleteCancel);
    assert_eq!(
        d.default,
        Response::Cancel,
        "destructive: Cancel is the default"
    );
    assert!(
        d.text.starts_with(
            "Tell Hydrus to delete 1 file(s)?\n\n1 of the selected file(s) aren't in Hydrus"
        ),
        "{}",
        d.text
    );
    assert_eq!(d.detail, "a.png");
    assert!(pump_until(2_000, || f.say() == "Delete cancelled"));
    assert!(!deleted.load(Ordering::SeqCst));
    assert_eq!(f.len(), 3);

    f.script.reset();
    f.script.push(Answer::Msg(Response::Delete));
    f.w.delete_rows_from_hydrus(&[ids[0]]);
    assert!(pump_until(10_000, || f.len() == 2));
    assert!(deleted.load(Ordering::SeqCst));
    assert!(
        server
            .calls()
            .contains(&"POST /add_files/delete_files".to_owned())
    );
    let shown = f.script.shown.borrow().clone();
    assert_eq!(shown.last().unwrap().title, "Delete from Hydrus");
    assert_eq!(
        shown.last().unwrap().text,
        "Hydrus deleted 1 file(s) (now in its trash)."
    );
    assert_eq!(f.say(), "Hydrus deleted 1 file(s)");
    f.close();
}

/// `_show_rows_in_hydrus` and `_explain_show_in_hydrus_failure`.
fn hydrus_show_opens_a_page_and_explains_a_refusal(app: &adw::Application) {
    use common::hydrus::{Reply, json_reply};
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(accepting_hydrus("present"));
    let mut seed = review_seed(dir.path());
    seed[0].hydrus_hash = Some(HELD_HASH.to_owned());
    let f = hydrus_fx(app, &server.url, true, seed.clone());
    let ids = row_ids(&f);
    // A row Hydrus does not hold (the window may have hashed it by now): nothing to show.
    f.w.show_rows_in_hydrus(&[ids[1]]);
    assert!(pump_until(10_000, || f.script.count_shown() > 0));
    let text = f.script.last_shown().text;
    assert!(
        text.starts_with("None of the selected files have a hash yet")
            || text.starts_with("Hydrus doesn't have any of the selected files"),
        "{text}"
    );
    f.script.reset();
    f.w.show_rows_in_hydrus(&[ids[0]]);
    assert!(pump_until(10_000, || f.say().starts_with("Showing ")));
    assert_eq!(f.say(), "Showing 1 file(s) in Hydrus");
    assert!(
        server
            .calls()
            .contains(&"POST /manage_pages/new_page".to_owned())
    );
    f.close();

    let refusing =
        common::hydrus::Hydrus::http(|q| match q.target.split('?').next().unwrap_or("") {
            "/get_files/file_metadata" => json_reply(json!({"metadata": [
                {"hash": HELD_HASH, "file_id": 1, "is_local": true,
                 "is_trashed": false, "is_deleted": false}]})),
            "/manage_pages/new_page" => Reply::new(403, "no"),
            _ => json_reply(json!({})),
        });
    let f = hydrus_fx(app, &refusing.url, true, seed.clone());
    let ids = row_ids(&f);
    f.w.show_rows_in_hydrus(&[ids[0]]);
    assert!(pump_until(10_000, || f.script.count_shown() > 0));
    let d = f.script.last_shown();
    assert_eq!(
        (d.kind, d.title.as_str()),
        (Kind::Warning, "Show in Hydrus")
    );
    assert!(
        d.text.starts_with(
            "Hydrus refused this because the access key lacks the \"manage pages\" permission."
        ),
        "{}",
        d.text
    );
    f.close();

    // Any other failure (D-B): the headline is the prefix and `user_message()`, which says what
    // to do and carries none of what the server wrote; Python wrote `str(exc)` there.
    let failing =
        common::hydrus::Hydrus::http(|q| match q.target.split('?').next().unwrap_or("") {
            "/get_files/file_metadata" => json_reply(json!({"metadata": [
                {"hash": HELD_HASH, "file_id": 1, "is_local": true,
                 "is_trashed": false, "is_deleted": false}]})),
            "/manage_pages/new_page" => Reply::new(500, "scripted-boom-text"),
            _ => json_reply(json!({})),
        });
    let f = hydrus_fx(app, &failing.url, true, seed);
    let ids = row_ids(&f);
    f.w.show_rows_in_hydrus(&[ids[0]]);
    assert!(pump_until(10_000, || f.script.count_shown() > 0));
    let d = f.script.last_shown();
    assert!(
        d.text.starts_with(
            "Hydrus could not open the page: Hydrus answered with an error (HTTP 500)."
        ),
        "{}",
        d.text
    );
    assert!(
        !d.text.contains("scripted-boom-text"),
        "the headline carries what the server wrote: {}",
        d.text
    );
    f.close();
}

/// `action_reconcile_with_hydrus` and the status it leaves.
fn hydrus_reconcile_confirms_queued_rows_and_says_when_nothing_waits(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(accepting_hydrus("present"));
    let mut seed = review_seed(dir.path());
    seed[0].sent_to_hydrus = true;
    seed[0].hydrus_hash = Some(HELD_HASH.to_owned());
    let f = hydrus_fx(app, &server.url, true, seed);
    f.w.action_reconcile_with_hydrus();
    assert!(pump_until(10_000, || f.say() == "confirmed 1 of 1"));
    assert!(f.w.s.queue.borrow().entries()[0].hydrus_import_confirmed);
    f.w.action_reconcile_with_hydrus();
    assert_eq!(
        f.say(),
        "Nothing is waiting on Hydrus - every sent file is confirmed"
    );
    f.close();
}

/// Nothing blocks the main loop: the window keeps turning (and handling actions) while Hydrus
/// holds an upload.
fn hydrus_requests_leave_the_main_loop_running_while_the_server_stalls(app: &adw::Application) {
    use common::hydrus::json_reply;
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(|q| {
        let r = json_reply(json!({"status": 1, "hash": NEW_HASH}));
        if q.target.starts_with("/add_files/add_file") {
            r.delayed(std::time::Duration::from_millis(900))
        } else {
            r
        }
    });
    let f = hydrus_fx(app, &server.url, true, review_seed(dir.path()));
    let ids = row_ids(&f);
    let ticks = Rc::new(Cell::new(0u32));
    let t = ticks.clone();
    let tick = gtk4::glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        t.set(t.get() + 1);
        gtk4::glib::ControlFlow::Continue
    });
    f.w.action_send_to_hydrus(&[ids[0]]);
    assert!(f.w.s.hydrus_batch_worker.is_running());
    pump_for(250);
    let before = ticks.get();
    // The window still answers an action while the upload waits.
    f.w.run_review_action("toggle_reviewed");
    pump_for(200);
    assert!(
        ticks.get() >= before + 4,
        "the main loop kept turning: {before} -> {}",
        ticks.get()
    );
    assert!(
        f.w.s.hydrus_batch_worker.is_running(),
        "still waiting on Hydrus"
    );
    // A second send is refused politely rather than racing the first.
    f.w.action_send_to_hydrus(&[ids[2]]);
    assert!(
        f.script
            .last_shown()
            .text
            .starts_with("A Hydrus request is already under way.")
    );
    assert!(pump_until(10_000, || f.say().starts_with("Sent ")));
    tick.remove();
    f.close();
}

/// `HydrusQueryDialog`: the search lists the files with their facts, marks the ones already in
/// the list, and Import selected downloads only the rest, with Hydrus's tags on them.
fn hydrus_query_searches_lists_and_imports_what_the_list_does_not_hold(app: &adw::Application) {
    use common::hydrus::{Reply, json_reply};
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(|q| match q.target.split('?').next().unwrap_or("") {
        "/get_files/search_files" => json_reply(json!({"file_ids": [1, 2]})),
        "/get_files/file_metadata" => json_reply(json!({"metadata": [
            {"hash": NEW_HASH, "file_id": 1, "mime": "image/png", "width": 4, "height": 3,
             "tags": {"svc": {"display_tags": {"0": ["series:foo", "solo"]}}}},
            {"hash": HELD_HASH, "file_id": 2, "mime": "image/png", "width": 4, "height": 3,
             "tags": {}},
        ]})),
        "/get_files/file" => Reply::ok(common::search::PNG.to_vec()),
        _ => json_reply(json!({})),
    });
    let mut seed = review_seed(dir.path());
    seed[1].hydrus_hash = Some(HELD_HASH.to_owned());
    let f = hydrus_fx(app, &server.url, true, seed);
    f.w.action_query_hydrus();
    let d =
        f.w.s
            .hydrus_query_dialog
            .borrow()
            .clone()
            .expect("the dialog opened");
    d.set_tags_text("  solo \n\n series:foo\n");
    d.search();
    assert!(pump_until(10_000, || d.labels().len() == 2));
    let labels = d.labels();
    assert_eq!(
        labels[0],
        "aaaaaaaaaaaa\u{2026}  (image/png, 4x3, 2 tag(s))"
    );
    assert_eq!(
        labels[1],
        "bbbbbbbbbbbb\u{2026}  (image/png, 4x3, 0 tag(s)) [already in list]"
    );
    // Nothing selected: a box, not an import.
    d.import_selected();
    assert_eq!(
        f.script.last_shown().text,
        "Select at least one file to import."
    );
    d.select_results(&[0, 1]);
    d.import_selected();
    assert!(pump_until(10_000, || f.len() == 4));
    assert_eq!(
        f.say(),
        "Imported 1 file(s) from Hydrus (1 duplicate(s) already in the list, skipped)"
    );
    let calls = server.calls();
    assert_eq!(
        calls.iter().filter(|c| *c == "GET /get_files/file").count(),
        1
    );
    let q = f.w.s.queue.borrow();
    let imported = q.entries().last().unwrap();
    assert_eq!(imported.hydrus_hash.as_deref(), Some(NEW_HASH));
    assert!(imported.path.ends_with(&format!("{NEW_HASH}.png")));
    assert!(std::path::Path::new(&imported.path).exists());
    let mut tags: Vec<(Option<String>, String, hatate_core::model::TagSource)> = imported
        .tags
        .iter()
        .map(|t| (t.namespace.clone(), t.name.clone(), t.source))
        .collect();
    tags.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    assert_eq!(
        tags,
        vec![
            (
                None,
                "solo".to_owned(),
                hatate_core::model::TagSource::Hydrus
            ),
            (
                Some("series".to_owned()),
                "foo".to_owned(),
                hatate_core::model::TagSource::Hydrus
            ),
        ]
    );
    drop(q);
    f.close();
}

/// The dialog's own checks: a tag, a key, and the empty answer.
fn hydrus_query_asks_for_a_tag_a_key_and_says_when_nothing_matched(app: &adw::Application) {
    use common::hydrus::json_reply;
    use serde_json::json;
    let server = common::hydrus::Hydrus::http(|_| json_reply(json!({"file_ids": []})));
    let f = hydrus_fx(app, &server.url, false, Vec::new());
    f.w.action_query_hydrus();
    let d = f.script.last_shown();
    assert_eq!(
        (d.kind, d.title.as_str(), d.text.as_str()),
        (
            Kind::Warning,
            "Query Hydrus",
            "Set your Hydrus access key in Settings first."
        )
    );
    assert!(f.w.s.hydrus_query_dialog.borrow().is_none());
    f.close();

    let f = hydrus_fx(app, &server.url, true, Vec::new());
    f.w.action_query_hydrus();
    let d = f.w.s.hydrus_query_dialog.borrow().clone().unwrap();
    d.search();
    assert_eq!(f.script.last_shown().text, "Enter at least one tag.");
    d.set_tags_text("nothing");
    d.search();
    assert!(pump_until(10_000, || f.script.count_shown() == 2));
    assert_eq!(f.script.last_shown().text, "No files matched those tags.");
    assert_eq!(server.calls(), ["GET /get_files/search_files"]);
    d.window.close();
    f.close();
}

// ---------------------------------------------------------------------------------------------
// Pawchive Index (6.g, DAN-2220): a scripted Pawchive over https with a throw-away CA, handed to
// the window through its constructor.
// ---------------------------------------------------------------------------------------------

/// What a window's content asks for: its least width, and its height at `width` (the stated
/// minimum width), read from the child so the window's own size request does not hide an overflow.
fn content_need(win: &gtk4::Window, width: i32) -> (i32, i32) {
    let child = win.child().expect("a window with content");
    let w = child.measure(gtk4::Orientation::Horizontal, -1).0;
    let h = child.measure(gtk4::Orientation::Vertical, width.max(w)).0;
    (w, h)
}

/// DAN-2220: each 6.g window, told to be as small as it can, does not grow past the minimum its
/// code states (the Query window with its long result rows wrapped, the Pawchive window with its
/// two note paragraphs and its button rows). On the shot path (hosted `ci`, GTK 4.14) the
/// `hydrus-query-results` and `pawchive` shots at `min` carry the same numbers in their headers.
fn pawchive_and_query_windows_keep_their_stated_minimum(app: &adw::Application) {
    use common::hydrus::{Reply, json_reply};
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(|q| match q.target.split('?').next().unwrap_or("") {
        "/get_files/search_files" => json_reply(json!({"file_ids": [1, 2]})),
        "/get_files/file_metadata" => json_reply(json!({"metadata": [
            {"hash": NEW_HASH, "file_id": 1, "mime": "image/png", "width": 4, "height": 3,
             "tags": {}},
            {"hash": HELD_HASH, "file_id": 2, "mime": "image/png", "width": 4, "height": 3,
             "tags": {}},
        ]})),
        "/get_files/file" => Reply::ok(common::search::PNG.to_vec()),
        _ => json_reply(json!({})),
    });
    let mut seed = review_seed(dir.path());
    seed[1].hydrus_hash = Some(HELD_HASH.to_owned());
    let f = hydrus_fx(app, &server.url, true, seed);
    f.w.action_query_hydrus();
    let d =
        f.w.s
            .hydrus_query_dialog
            .borrow()
            .clone()
            .expect("the dialog opened");
    d.set_tags_text("solo");
    d.search();
    assert!(pump_until(10_000, || d.labels().len() == 2));
    pump_for(300);
    let (w, h) = content_need(d.window.upcast_ref(), hatate_gtk::hydrus_query::MIN_WIDTH);
    println!("OUTER query-hydrus content={w}x{h}");
    use hatate_gtk::hydrus_query::{MIN_HEIGHT, MIN_WIDTH};
    assert!(
        w <= MIN_WIDTH && h <= MIN_HEIGHT,
        "the Query Hydrus window is {w}x{h}; its stated minimum is {MIN_WIDTH}x{MIN_HEIGHT}"
    );
    f.close();

    let server = common::hydrus::Hydrus::https(scripted_pawchive(std::time::Duration::ZERO));
    let f = pawchive_fx(app, &server, Vec::new());
    f.w.action_pawchive_index();
    let d =
        f.w.s
            .pawchive_dialog
            .borrow()
            .clone()
            .expect("the window opened");
    pump_for(300);
    let (w, h) = content_need(
        d.window.upcast_ref(),
        hatate_gtk::pawchive_dialog::MIN_WIDTH,
    );
    println!("OUTER pawchive content={w}x{h}");
    use hatate_gtk::pawchive_dialog::{MIN_HEIGHT as PH, MIN_WIDTH as PW};
    assert!(
        w <= PW && h <= PH,
        "the Pawchive Index window is {w}x{h}; its stated minimum is {PW}x{PH}"
    );
    f.close();
}

/// DAN-2395 (F1, F2, F5, Tab order): the two windows carry the pick-window class that the theme's
/// well and border rules key on, Import selected acts only on a selection, an empty list and the
/// status label are not Tab stops, and the Pawchive entry takes the page's `field` look.
fn query_and_pawchive_windows_follow_the_design_read(app: &adw::Application) {
    use common::hydrus::{Reply, json_reply};
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(|q| match q.target.split('?').next().unwrap_or("") {
        "/get_files/search_files" => json_reply(json!({"file_ids": [1]})),
        "/get_files/file_metadata" => json_reply(json!({"metadata": [
            {"hash": NEW_HASH, "file_id": 1, "mime": "image/png", "width": 4, "height": 3,
             "tags": {}},
        ]})),
        "/get_files/file" => Reply::ok(common::search::PNG.to_vec()),
        _ => json_reply(json!({})),
    });
    let f = hydrus_fx(app, &server.url, true, review_seed(dir.path()));
    f.w.action_query_hydrus();
    let d =
        f.w.s
            .hydrus_query_dialog
            .borrow()
            .clone()
            .expect("the dialog opened");
    assert!(d.window.has_css_class("pick-window"));
    assert!(
        !d.import_button.is_sensitive(),
        "Import selected promises nothing while there is nothing to import"
    );
    assert!(
        !d.results.is_focusable(),
        "an empty results list is not a Tab stop"
    );
    assert!(
        !d.results
            .ancestor(gtk4::ScrolledWindow::static_type())
            .unwrap()
            .is_focusable()
    );
    d.set_tags_text("solo");
    d.search();
    assert!(pump_until(10_000, || d.labels().len() == 1));
    pump_for(100);
    assert!(
        !d.import_button.is_sensitive(),
        "results with no row selected do not enable Import"
    );
    assert!(d.results.is_focusable(), "a list with rows takes focus");
    d.select_results(&[0]);
    assert!(d.import_button.is_sensitive());
    d.select_results(&[]);
    assert!(!d.import_button.is_sensitive());
    f.close();

    let server = common::hydrus::Hydrus::https(scripted_pawchive(std::time::Duration::ZERO));
    let f = pawchive_fx(app, &server, Vec::new());
    f.w.action_pawchive_index();
    let d =
        f.w.s
            .pawchive_dialog
            .borrow()
            .clone()
            .expect("the window opened");
    assert!(d.window.has_css_class("pick-window"));
    assert!(
        d.query.has_css_class("field"),
        "the entry takes the page's field look, whose placeholder is 4.5:1"
    );
    assert!(
        !d.status.is_focusable(),
        "the status label is not a Tab stop"
    );
    assert!(
        !d.table.is_focusable(),
        "the empty artist table is not a Tab stop"
    );
    assert!(
        !d.table
            .ancestor(gtk4::ScrolledWindow::static_type())
            .unwrap()
            .is_focusable()
    );
    assert_eq!(
        d.none_yet.text(),
        "No artists indexed yet. Add one below.",
        "an empty index says so inside the list"
    );
    assert!(d.none_yet.parent().is_some(), "and the line is in the list");
    d.query
        .set_text("https://pawchive.pw/fanbox/user/42/post/9");
    d.find_or_add();
    assert!(d.table.is_focusable(), "an index with artists takes focus");
    assert!(
        d.none_yet.parent().is_some(),
        "the placeholder survives a refresh of the rows"
    );
    f.close();
}

fn pawchive_fx(
    app: &adw::Application,
    server: &common::hydrus::Hydrus,
    seed: Vec<hatate_core::model::Entry>,
) -> Fx {
    let ca = server.ca_der.clone().expect("an https server");
    let base = server.url.clone();
    Fx::new_with(
        app,
        Opts {
            seed,
            tweak: Some(offline()),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
        move |env| {
            let net = hatate_core::net::NetClient::new(
                hatate_core::net::NetConfig::default().with_root_certificate_der(ca),
            )
            .unwrap();
            env.pawchive = Some(Arc::new(
                hatate_core::pawchive_index::PawchiveCrawler::with_hosts(
                    net,
                    &format!("{base}/api/v1"),
                    &format!("{base}/t"),
                ),
            ));
        },
    )
}

fn scripted_pawchive(
    delay: std::time::Duration,
) -> impl Fn(&common::hydrus::Recorded) -> common::hydrus::Reply + Send + Sync + 'static {
    use common::hydrus::json_reply;
    use serde_json::json;
    move |q| {
        let path = q.target.split('?').next().unwrap_or("");
        if path.ends_with("/creators") {
            return json_reply(json!([
                {"id": "1", "name": "TEKU", "service": "patreon", "favorited": 50, "public_id": "t"},
                {"id": "2", "name": "Tekuho", "service": "fanbox", "favorited": 1900, "public_id": "u"},
                {"id": "3", "name": "Someone", "service": "fanbox", "favorited": 10, "public_id": "x"},
            ]))
            .delayed(delay);
        }
        if path.ends_with("/posts") {
            return json_reply(json!([]));
        }
        json_reply(json!({}))
    }
}

/// The window's whole path: a link adds an artist at once, a name is searched, the ticked ones are
/// added, Index selected runs the crawl, Remove asks (Cancel by default) and then removes.
/// DAN-2398 N1: the artist list is a table. Each column has one width and one alignment for the
/// header cell and every row, so the right edge of a column is the same in all of them (a drifting
/// row put `fanbox` under POSTS). Measured on allocations, in window coordinates, to within half a pixel (the design read asks for 1).
fn pawchive_artist_columns_share_one_edge_in_the_header_and_every_row(app: &adw::Application) {
    use gtk4::prelude::*;
    let server = common::hydrus::Hydrus::https(scripted_pawchive(std::time::Duration::ZERO));
    let f = pawchive_fx(app, &server, Vec::new());
    let index = hatate_core::pawchive_index::PawchiveIndex::new(f.w.s.paths.pawchive_index_db());
    for (service, id, name, images, posts) in [
        ("patreon", "1001", "Synthetic Artist", 1284usize, 96usize),
        ("fanbox", "2002", "Another Synthetic", 37, 5),
        ("fanbox", "3003", "Third Synthetic", 0, 0),
    ] {
        index.add_creator(service, id, Some(name)).expect("artist");
        let rows: Vec<_> = (0..images)
            .map(|i| hatate_core::pawchive_index::ImageRow {
                service: service.to_owned(),
                user_id: id.to_owned(),
                post_id: format!("p{i}"),
                page_index: 0,
                sha256: format!("{i:064x}"),
                path: format!("/synthetic/{id}/{i}.png"),
                title: None,
                dhash8: Some(format!("{i:016x}")),
                dhash16: Some(format!("{i:064x}")),
            })
            .collect();
        index.add_images(&rows).expect("images");
        if posts > 0 {
            index
                .finish_creator(service, id, Some(name), posts)
                .expect("stamp");
        }
    }
    f.w.action_pawchive_index();
    let d = f.w.s.pawchive_dialog.borrow().clone().expect("opened");
    d.window.present();
    pump_for(400);
    let cells = |w: &gtk4::Widget| -> Vec<gtk4::Widget> {
        let mut v = Vec::new();
        let mut c = w.first_child();
        while let Some(x) = c {
            c = x.next_sibling();
            v.push(x);
        }
        v
    };
    let edges = |row: &gtk4::Widget| -> Vec<(f32, f32)> {
        cells(row)
            .iter()
            .map(|c| {
                let b = c.compute_bounds(&d.window).expect("bounds");
                (b.x(), b.x() + b.width())
            })
            .collect()
    };
    let head_row = d.head.row_at_index(0).unwrap().child().unwrap();
    let head = edges(&head_row);
    assert_eq!(head.len(), 5, "{head:?}");
    let mut n = 0;
    while let Some(r) = d.table.row_at_index(n) {
        let row = edges(&r.child().unwrap());
        assert_eq!(row.len(), 5);
        for (i, ((hl, hr), (rl, rr))) in head.iter().zip(&row).enumerate() {
            assert!(
                (hr - rr).abs() < 0.5 && (hl - rl).abs() < 0.5,
                "row {n} column {i}: header {hl}..{hr} vs row {rl}..{rr}"
            );
        }
        n += 1;
    }
    assert_eq!(n, 3, "all three artists are rows");
    // the numeric columns read against their right edge, the others against their left
    for (i, w) in cells(&head_row).iter().enumerate() {
        let label = w.downcast_ref::<gtk4::Label>().unwrap();
        let want = if hatate_gtk::pawchive_dialog::RIGHT_ALIGNED[i] {
            1.0
        } else {
            0.0
        };
        assert_eq!(label.xalign(), want, "header column {i}");
    }
    f.close();
}

/// DAN-2398 P3-1: the file-name box of the Delete from Hydrus question starts at its top-left and
/// is indented like the tag box; centred it left a blank first line.
fn message_detail_box_starts_at_its_top_left_and_is_indented(_app: &adw::Application) {
    use gtk4::prelude::*;
    let (scroller, lines) = hatate_gtk::dialogs::detail_box("a.png\nb.png");
    assert_eq!(
        lines.yalign(),
        0.0,
        "the first line sits at the top of the box"
    );
    assert_eq!(lines.xalign(), 0.0);
    assert!(
        lines.margin_start() >= 8 && lines.margin_end() >= 8,
        "indented like the tag box"
    );
    assert!(lines.is_selectable() && scroller.has_css_class("field"));
}

fn pawchive_index_window_adds_by_link_searches_by_name_indexes_and_removes(app: &adw::Application) {
    let server = common::hydrus::Hydrus::https(scripted_pawchive(std::time::Duration::ZERO));
    let f = pawchive_fx(app, &server, Vec::new());
    f.w.action_pawchive_index();
    let d =
        f.w.s
            .pawchive_dialog
            .borrow()
            .clone()
            .expect("the window opened");
    assert_eq!(
        d.window.title().unwrap(),
        "Pawchive Index - 0 artist(s), 0 image(s) searchable"
    );
    d.query
        .set_text("https://pawchive.pw/fanbox/user/42/post/9");
    d.find_or_add();
    assert_eq!(
        d.status_text(),
        "Added 42 (fanbox) - click Index selected to index them."
    );
    assert_eq!(d.row_texts()[0], ["42", "fanbox", "0", "", "not yet"]);
    assert_eq!(
        d.selected_creators(),
        [("fanbox".to_owned(), "42".to_owned())]
    );
    d.query.set_text("https://pawchive.pw/fanbox/user/42");
    d.find_or_add();
    assert_eq!(d.status_text(), "That artist is already in the list.");

    d.query.set_text("tek");
    d.find_or_add();
    assert!(!d.find_button.is_sensitive(), "the search is under way");
    assert!(pump_until(10_000, || d.find_button.is_sensitive()));
    assert_eq!(
        d.found_labels(),
        [
            "Tekuho  (fanbox)  -  1,900 favourites",
            "TEKU  (patreon)  -  50 favourites"
        ]
    );
    assert_eq!(d.status_text(), "2 artist(s) found - tick the ones to add.");
    d.tick_found(&[0]);
    d.add_checked();
    assert_eq!(
        d.status_text(),
        "Added 1 artist(s) - click Index selected to index them."
    );
    assert_eq!(d.row_texts().len(), 2);
    assert_eq!(
        d.selected_creators(),
        [("fanbox".to_owned(), "2".to_owned())]
    );

    d.start_indexing(true);
    assert!(d.stop_button.is_sensitive() && !d.index_all_button.is_sensitive());
    assert!(pump_until(20_000, || !d.stop_button.is_sensitive()));
    assert!(d.index_all_button.is_sensitive());
    assert!(
        d.status_text().starts_with("2: 0 image(s), 0 new"),
        "{}",
        d.status_text()
    );
    assert!(
        server
            .calls()
            .iter()
            .any(|c| c.starts_with("GET /api/v1/fanbox/user/2/posts")),
        "{:?}",
        server.calls()
    );

    // Remove: Cancel is the default, so an unscripted box removes nothing.
    d.select_rows(&[0]);
    d.remove_selected();
    let shown = f.script.last_shown();
    assert_eq!(shown.title, "Remove artists");
    assert_eq!(shown.default, Response::Cancel);
    assert_eq!(d.row_texts().len(), 2);
    f.script.push(Answer::Msg(Response::Remove));
    d.remove_selected();
    assert_eq!(d.row_texts().len(), 1);
    assert_eq!(d.status_text(), "Removed 1 artist(s).");
    d.window.close();
    assert!(f.w.s.pawchive_dialog.borrow().is_none());
    f.close();
}

/// One window at a time, and the artist search (a slow download) leaves the loop turning.
fn pawchive_index_window_is_one_window_and_the_search_does_not_block_the_loop(
    app: &adw::Application,
) {
    let server =
        common::hydrus::Hydrus::https(scripted_pawchive(std::time::Duration::from_millis(900)));
    let f = pawchive_fx(app, &server, Vec::new());
    f.w.action_pawchive_index();
    let first = f.w.s.pawchive_dialog.borrow().clone().unwrap();
    f.w.action_pawchive_index();
    assert!(Rc::ptr_eq(
        &first,
        f.w.s.pawchive_dialog.borrow().as_ref().unwrap()
    ));
    let ticks = Rc::new(Cell::new(0u32));
    let t = ticks.clone();
    let tick = gtk4::glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        t.set(t.get() + 1);
        gtk4::glib::ControlFlow::Continue
    });
    first.query.set_text("tek");
    first.find_or_add();
    pump_for(250);
    let before = ticks.get();
    pump_for(200);
    assert!(
        ticks.get() >= before + 4,
        "the main loop kept turning: {before} -> {}",
        ticks.get()
    );
    assert!(
        !first.find_button.is_sensitive(),
        "still waiting on pawchive"
    );
    assert!(pump_until(10_000, || first.find_button.is_sensitive()));
    tick.remove();
    // The main window closing takes the Index window with it.
    f.w.close_pawchive_dialog();
    assert!(f.w.s.pawchive_dialog.borrow().is_none());
    f.close();
}

// ---------------------------------------------------------------------------------------------
// Hydrus thumbnails (6.g, DAN-2220, DAN-2116 item 3)
// ---------------------------------------------------------------------------------------------

/// With a key and `thumbnail_source = "hydrus"`, a file Hydrus holds is drawn from Hydrus's own
/// thumbnail (its file on disk is unreadable here, so a picture proves where it came from), a file
/// it does not hold from disk, and a thumbnail that will not decode falls back to disk.
fn hydrus_thumbnails_come_from_hydrus_for_files_it_holds_and_from_disk_for_the_rest(
    app: &adw::Application,
) {
    use common::hydrus::{Reply, json_reply};
    use hatate_core::thumb::ThumbState;
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, Ordering};
    let dir = tempfile::tempdir().unwrap();
    let broken_thumbnail = Arc::new(AtomicBool::new(false));
    let broken = broken_thumbnail.clone();
    let server =
        common::hydrus::Hydrus::http(move |q| match q.target.split('?').next().unwrap_or("") {
            "/get_files/file_metadata" => json_reply(json!({"metadata": [
                {"hash": HELD_HASH, "file_id": 1, "is_local": true,
                 "is_trashed": false, "is_deleted": false}]})),
            "/get_files/thumbnail" if broken.load(Ordering::SeqCst) => {
                Reply::ok(b"not a picture".to_vec())
            }
            "/get_files/thumbnail" => Reply::ok(common::search::PNG.to_vec()),
            _ => json_reply(json!({})),
        });
    let mut seed = review_seed(dir.path());
    // Row 0: held by Hydrus, and its own file cannot be read.
    std::fs::write(&seed[0].path, b"garbage").unwrap();
    seed[0].hydrus_hash = Some(HELD_HASH.to_owned());
    // Row 1: Hydrus does not hold it; it has a hash all the same.
    seed[1].hydrus_hash = Some(NEW_HASH.to_owned());
    let f = Fx::new(
        app,
        Opts {
            seed,
            tweak: Some(Box::new({
                let inner = hydrus_tweak(&server.url, true);
                move |s| {
                    inner(s);
                    s.thumbnail_source = "hydrus".to_owned();
                }
            })),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let ids = row_ids(&f);
    f.w.start_thumbnail_generation(&ids, true);
    assert!(pump_until(10_000, || f.w.s.thumbs.in_flight() == 0));
    let view = &f.w.s.queue_view;
    assert_eq!(view.thumb_state(ids[0]), ThumbState::Loaded, "from Hydrus");
    assert_eq!(view.thumb_state(ids[1]), ThumbState::Loaded, "from disk");
    assert_eq!(view.thumb_state(ids[2]), ThumbState::Loaded, "from disk");
    let calls = server.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|c| *c == "GET /get_files/thumbnail")
            .count(),
        1,
        "only the held file is fetched: {calls:?}"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|c| *c == "GET /get_files/file_metadata")
            .count(),
        1,
        "one check for the whole pass"
    );
    f.close();

    // A thumbnail that does not decode falls back to the file, which here is unreadable too.
    broken_thumbnail.store(true, Ordering::SeqCst);
    let mut seed = review_seed(dir.path());
    std::fs::write(&seed[0].path, b"garbage").unwrap();
    seed[0].hydrus_hash = Some(HELD_HASH.to_owned());
    let f = hydrus_fx(app, &server.url, true, seed);
    let ids = row_ids(&f);
    f.w.start_thumbnail_generation(&ids[..1], true);
    assert!(pump_until(10_000, || f.w.s.thumbs.in_flight() == 0));
    assert_eq!(f.w.s.queue_view.thumb_state(ids[0]), ThumbState::Failed);
    f.close();
}

/// `TestHydrusThumbnailPath` and `test_hydrus_source_without_an_access_key_falls_back_to_local`:
/// whatever Hydrus cannot supply, the row's own file is drawn. With no access key Hydrus is not
/// asked at all; an empty thumbnail and one that does not decode are asked for, then the file is
/// read (its picture is readable here, so `Loaded` is the file's).
fn hydrus_thumbnails_fall_back_to_the_file_when_hydrus_cannot_supply_one(app: &adw::Application) {
    use common::hydrus::{Reply, json_reply};
    use hatate_core::thumb::ThumbState;
    use serde_json::json;
    use std::sync::atomic::{AtomicU8, Ordering};
    let dir = tempfile::tempdir().unwrap();
    // 0: an empty thumbnail; 1: bytes that are not a picture.
    let mode = Arc::new(AtomicU8::new(0));
    let served = mode.clone();
    let server =
        common::hydrus::Hydrus::http(move |q| match q.target.split('?').next().unwrap_or("") {
            "/get_files/file_metadata" => json_reply(json!({"metadata": [
                {"hash": HELD_HASH, "file_id": 1, "is_local": true,
                 "is_trashed": false, "is_deleted": false}]})),
            "/get_files/thumbnail" if served.load(Ordering::SeqCst) == 0 => Reply::ok(Vec::new()),
            "/get_files/thumbnail" => Reply::ok(b"not an image".to_vec()),
            _ => json_reply(json!({})),
        });
    let pass = |key: bool| {
        let mut seed = review_seed(dir.path());
        seed[0].hydrus_hash = Some(HELD_HASH.to_owned());
        let f = Fx::new(
            app,
            Opts {
                seed,
                tweak: Some(Box::new({
                    let inner = hydrus_tweak(&server.url, key);
                    move |s| {
                        inner(s);
                        s.thumbnail_source = "hydrus".to_owned();
                    }
                })),
                missing: Some(Arc::new(|_, _| HashSet::new())),
                ..Opts::default()
            },
        );
        let ids = row_ids(&f);
        f.w.start_thumbnail_generation(&ids[..1], true);
        assert!(pump_until(10_000, || f.w.s.thumbs.in_flight() == 0));
        let state = f.w.s.queue_view.thumb_state(ids[0]);
        f.close();
        state
    };
    // No access key: the source is the disk, and Hydrus hears nothing.
    assert_eq!(pass(false), ThumbState::Loaded);
    assert!(server.calls().is_empty(), "{:?}", server.calls());
    // An empty thumbnail, then one that does not decode: each was asked for, each fell back.
    for (m, what) in [(0, "empty"), (1, "undecodable")] {
        mode.store(m, Ordering::SeqCst);
        let before = server
            .calls()
            .iter()
            .filter(|c| *c == "GET /get_files/thumbnail")
            .count();
        assert_eq!(pass(true), ThumbState::Loaded, "{what} thumbnail");
        let after = server
            .calls()
            .iter()
            .filter(|c| *c == "GET /get_files/thumbnail")
            .count();
        assert_eq!(after, before + 1, "{what}: Hydrus was asked first");
    }
}

/// `TestHydrusMenu.test_*_triggers_the_real_handler` (the two Send entries and Re-check, through
/// the menu's own actions) and `TestShowInHydrus.test_h_*` (the `H` key's handler, through the
/// guard the key goes through).
fn hydrus_menu_entries_and_the_h_key_reach_their_handlers(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(accepting_hydrus("present"));
    let mut seed = review_seed(dir.path());
    seed[0].hydrus_hash = Some(HELD_HASH.to_owned());
    let f = hydrus_fx(app, &server.url, true, seed);
    f.w.s.queue_view.select_ids(&[]);
    f.activate("send-to-hydrus");
    let d = f.script.last_shown();
    assert_eq!(
        (d.kind, d.title.as_str(), d.text.as_str()),
        (
            Kind::Information,
            "Send to Hydrus",
            "Select at least one row."
        )
    );
    f.activate("import-url-to-hydrus");
    let d = f.script.last_shown();
    assert_eq!(
        (d.kind, d.title.as_str(), d.text.as_str()),
        (
            Kind::Information,
            "Send URL to Hydrus's Importer",
            "Select at least one row."
        )
    );
    f.activate("reconcile");
    assert_eq!(
        f.say(),
        "Nothing is waiting on Hydrus - every sent file is confirmed"
    );
    // `H` with nothing selected says so and asks Hydrus nothing.
    f.w.guarded_review_action("show_in_hydrus");
    assert_eq!(f.say(), "Show in Hydrus: select an image first");
    assert!(server.calls().is_empty(), "{:?}", server.calls());
    // `H` on a row shows that row's file.
    f.w.s.queue_view.select_row(0);
    f.w.guarded_review_action("show_in_hydrus");
    assert!(pump_until(10_000, || f.say() == "Showing 1 file(s) in Hydrus"));
    let page = server
        .recorded()
        .into_iter()
        .find(|q| q.target.starts_with("/manage_pages/new_page"))
        .expect("a page was opened");
    let body = String::from_utf8_lossy(&page.body).into_owned();
    assert!(body.contains(HELD_HASH), "the row's own file: {body}");
    f.close();
}

/// `test_sending_the_current_image_away_keeps_your_place` and
/// `test_sending_the_last_row_away_lands_on_the_new_last_row`, through the Send itself with
/// Remove after import on: the selection lands on the row that followed, or the new last row.
fn sending_the_row_you_are_on_away_keeps_your_place(app: &adw::Application) {
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(accepting_hydrus("present"));
    let f = Fx::new(
        app,
        Opts {
            seed: review_seed(dir.path()),
            tweak: Some(Box::new({
                let inner = hydrus_tweak(&server.url, true);
                move |s| {
                    inner(s);
                    s.remove_after_import = true;
                }
            })),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let ids = row_ids(&f);
    let view = &f.w.s.queue_view;
    view.select_row(1);
    f.w.action_send_to_hydrus(&ids[1..2]);
    assert!(pump_until(10_000, || f.len() == ids.len() - 1));
    assert_eq!(view.selected_ids(), vec![ids[2]], "the row that followed");
    // The last row goes: the new last row is selected.
    view.select_row(1);
    f.w.action_send_to_hydrus(&ids[2..3]);
    assert!(pump_until(10_000, || f.len() == ids.len() - 2));
    assert_eq!(view.selected_ids(), vec![ids[0]], "the new last row");
    f.close();
}

/// Real Tab presses from the window's first stop until the button labelled `last` has focus.
/// Every press has to move focus: a container that is not itself a stop but swallows the key
/// leaves it on the widget before (the Search Hydrus and Remove selected traps of 8872a2f), which
/// `is_focusable()` pins cannot see. Returns the stops in order, `None` off an X server that can
/// not inject keys (a failure under `CI`).
fn tab_to_button(
    keys: &mut common::xtest::Xtest,
    win: &gtk4::Window,
    last: &str,
) -> Vec<gtk4::Widget> {
    use common::xtest::Key;
    let focus = || gtk4::prelude::GtkWindowExt::focus(win);
    let named = |w: &Option<gtk4::Widget>| match w {
        Some(w) => format!("{} {:?}", w.type_().name(), w.tooltip_text()),
        None => "nothing".to_owned(),
    };
    let mut stops: Vec<gtk4::Widget> = focus().into_iter().collect();
    for press in 1..=30 {
        keys.tap(Key::Tab, &[]).expect("inject");
        pump_for(250);
        let now = focus();
        assert!(
            now.is_some() && now.as_ref() != stops.last(),
            "Tab {press} stays on {}; the walk so far: {:?}",
            named(&now),
            stops
                .iter()
                .map(|w| named(&Some(w.clone())))
                .collect::<Vec<_>>()
        );
        let now = now.unwrap();
        stops.push(now.clone());
        if now
            .downcast_ref::<gtk4::Button>()
            .and_then(|b| b.label())
            .is_some_and(|l| l == last)
        {
            return stops;
        }
    }
    panic!(
        "Tab never reached {last}: {:?}",
        stops
            .iter()
            .map(|w| named(&Some(w.clone())))
            .collect::<Vec<_>>()
    );
}

/// Close every other window and put the pointer and keyboard focus on `win`.
fn only(keys: &mut common::xtest::Xtest, win: &gtk4::Window) {
    for w in gtk4::Window::list_toplevels() {
        if w != *win.upcast_ref::<gtk4::Widget>() {
            if let Ok(w) = w.downcast::<gtk4::Window>() {
                w.destroy();
            }
        }
    }
    pump_for(400);
    keys.focus_window_at(4, 4).expect("focus the dialog");
    pump_for(300);
}

/// The labels of the buttons among `stops`, in the order Tab met them.
fn button_stops(stops: &[gtk4::Widget]) -> Vec<String> {
    stops
        .iter()
        .filter_map(|w| w.downcast_ref::<gtk4::Button>()?.label())
        .map(|l| l.to_string())
        .collect()
}

/// Query Hydrus and the Pawchive index window, walked with real Tab presses (XTEST) in the states
/// that regressed in 8872a2f: an empty results list or index (a non-focusable container with a
/// non-focusable child used to swallow Tab, so Cancel / Close were out of reach), and the same
/// windows holding rows.
fn tab_walks_the_query_hydrus_and_pawchive_windows_to_their_footers(app: &adw::Application) {
    use common::hydrus::{Reply, json_reply};
    use common::xtest::Xtest;
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let mut keys = match Xtest::connect() {
        Ok(x) => x,
        Err(why) => {
            assert!(
                std::env::var_os("CI").is_none(),
                "CI is set and the X server cannot inject keys: {why}: failing closed"
            );
            println!("tab_walk: skipped ({why})");
            return;
        }
    };
    let server = common::hydrus::Hydrus::http(|q| match q.target.split('?').next().unwrap_or("") {
        "/get_files/search_files" => json_reply(json!({"file_ids": [1, 2]})),
        "/get_files/file_metadata" => json_reply(json!({"metadata": [
            {"hash": NEW_HASH, "file_id": 1, "mime": "image/png", "width": 4, "height": 3, "tags": {}},
            {"hash": "bb".repeat(32), "file_id": 2, "mime": "image/png", "width": 4, "height": 3, "tags": {}},
        ]})),
        "/get_files/file" => Reply::ok(common::search::PNG.to_vec()),
        _ => json_reply(json!({})),
    });
    let f = hydrus_fx(app, &server.url, true, review_seed(dir.path()));
    f.w.action_query_hydrus();
    let d = f.w.s.hydrus_query_dialog.borrow().clone().unwrap();
    d.window.present();
    pump_for(300);
    only(&mut keys, &d.window);
    // the tag box has focus when the window opens
    let first = gtk4::prelude::GtkWindowExt::focus(&d.window);
    assert!(
        first
            .as_ref()
            .is_some_and(|w| w.is_ancestor(&d.tags) || *w == d.tags),
        "the tag box is the first stop: {first:?}"
    );
    // no search run: Search Hydrus, then Cancel, with nothing in between to hold the key
    let stops = tab_to_button(&mut keys, &d.window, "Cancel");
    assert_eq!(
        button_stops(&stops),
        ["Search Hydrus", "Cancel"],
        "{stops:?}"
    );
    d.set_tags_text("solo");
    d.search();
    assert!(pump_until(10_000, || d.labels().len() == 2));
    d.select_results(&[0]);
    pump_for(200);
    only(&mut keys, &d.window);
    gtk4::prelude::GtkWindowExt::set_focus(&d.window, Some(&d.tags));
    pump_for(200);
    // results: the rows are stops, then Cancel, then Import selected
    let stops = tab_to_button(&mut keys, &d.window, "Import selected");
    assert_eq!(
        button_stops(&stops),
        ["Search Hydrus", "Cancel", "Import selected"],
        "{stops:?}"
    );
    assert!(
        stops.iter().any(|w| w.css_name() == "row"),
        "Tab reaches a result row: {stops:?}"
    );
    f.close();

    let server = common::hydrus::Hydrus::https(scripted_pawchive(std::time::Duration::ZERO));
    let f = pawchive_fx(app, &server, Vec::new());
    f.w.action_pawchive_index();
    let d = f.w.s.pawchive_dialog.borrow().clone().unwrap();
    d.window.present();
    pump_for(300);
    only(&mut keys, &d.window);
    // an empty index opens on the name/link entry, and Tab goes on to Close
    let first = gtk4::prelude::GtkWindowExt::focus(&d.window);
    assert!(
        first
            .as_ref()
            .is_some_and(|w| w.is_ancestor(&d.query) || *w == d.query),
        "the name/link entry is the first stop: {first:?}"
    );
    let stops = tab_to_button(&mut keys, &d.window, "Close");
    let buttons = button_stops(&stops);
    assert_eq!(
        buttons.last().map(String::as_str),
        Some("Close"),
        "{buttons:?}"
    );
    // nothing to act on: Index selected, Index / refresh all and Remove selected are insensitive
    // (DAN-2398 P3-3), so they are not Tab stops (this was the walk's assertion that Remove
    // selected was reached; changed on purpose with the sensitivity)
    assert_eq!(
        buttons,
        ["Find / Add", "Add artists from my matches", "Close"],
        "{buttons:?}"
    );
    assert!(!d.index_selected_button.is_sensitive());
    assert!(!d.index_all_button.is_sensitive());
    assert!(!d.remove_button.is_sensitive());
    // and Close wraps to the entry rather than to nothing
    use common::xtest::Key;
    keys.tap(Key::Tab, &[]).expect("inject");
    pump_for(250);
    let wrapped = gtk4::prelude::GtkWindowExt::focus(&d.window);
    assert!(
        wrapped
            .as_ref()
            .is_some_and(|w| w.is_ancestor(&d.query) || *w == d.query),
        "Tab from Close wraps to the entry: {wrapped:?}"
    );
    d.query
        .set_text("https://pawchive.pw/fanbox/user/42/post/9");
    d.find_or_add();
    pump_for(500);
    only(&mut keys, &d.window);
    gtk4::prelude::GtkWindowExt::set_focus(&d.window, Some(&d.query));
    pump_for(200);
    // one artist, selected: the three list buttons are live, the table row joins the walk and
    // Close is still reachable
    assert!(d.index_selected_button.is_sensitive());
    assert!(d.index_all_button.is_sensitive());
    assert!(d.remove_button.is_sensitive());
    let stops = tab_to_button(&mut keys, &d.window, "Close");
    assert!(
        button_stops(&stops).contains(&"Remove selected".to_owned()),
        "Tab reaches Remove selected: {stops:?}"
    );
    keys.tap(Key::Tab, &[]).expect("inject");
    pump_for(250);
    let wrapped = gtk4::prelude::GtkWindowExt::focus(&d.window);
    assert!(
        wrapped.as_ref().is_some_and(|w| w.css_name() == "row"),
        "Tab from Close wraps to the artist row: {wrapped:?}"
    );
    // reopened with an artist in the index, the window still opens on the entry, not the table
    d.window.close();
    pump_for(300);
    f.w.action_pawchive_index();
    let d = f.w.s.pawchive_dialog.borrow().clone().unwrap();
    d.window.present();
    pump_for(300);
    only(&mut keys, &d.window);
    assert!(d.table.row_at_index(0).is_some(), "the artist is listed");
    let first = gtk4::prelude::GtkWindowExt::focus(&d.window);
    assert!(
        first
            .as_ref()
            .is_some_and(|w| w.is_ancestor(&d.query) || *w == d.query),
        "the name/link entry is the first stop with an artist indexed: {first:?}"
    );
    f.close();
}

// -- Settings (6.h) ----------------------------------------------------------------------------

fn settings_dialog_opens_on_the_stored_values_and_ok_saves_what_python_would(
    app: &adw::Application,
) {
    use hatate_core::config::Settings;
    use hatate_gtk::settings_values::{self, Value};
    let f = fx(app);
    {
        let mut s = f.w.s.settings.lock().unwrap();
        s.delay_min_seconds = 12.5;
        s.hash_source = "nonsense".into();
        s.tag_blacklist = vec!["bad_*".into(), "meta:*".into()];
        s.hydrus.api_url = "http://example.test:1234/".into();
        s.enabled_tag_sources = vec!["User".into(), "Booru".into(), "Gone".into()];
    }
    let before = f.w.s.settings.lock().unwrap().clone();
    f.w.activate_action("preferences");
    let d =
        f.w.s
            .settings_dialog
            .borrow()
            .clone()
            .expect("the dialog is open");
    assert_eq!(
        d.tab_names(),
        [
            "General",
            "Engine",
            "Import",
            "SauceNAO",
            "Hydrus",
            "Tag Namespaces",
            "Site Logins",
            "Shortcuts",
            "MCP"
        ]
    );
    assert_eq!(
        d.values(),
        settings_values::initial(&before),
        "the controls open on the stored values"
    );
    // Cancel changes nothing and stores nothing
    let cfg = f.paths.config_file();
    let existed = cfg.exists();
    d.click_cancel();
    assert!(f.w.s.settings_dialog.borrow().is_none());
    assert_eq!(f.w.s.settings.lock().unwrap().clone(), before);
    assert_eq!(cfg.exists(), existed);

    // OK after one edit: what Python's apply_to_settings would write, normalisation included
    f.w.activate_action("preferences");
    let d = f.w.s.settings_dialog.borrow().clone().unwrap();
    d.set_value("delay_max", &Value::Num(10.0));
    d.click_ok();
    let mut want = before.clone();
    let mut v = settings_values::initial(&before);
    v.set("delay_max", Value::Num(10.0));
    settings_values::apply(&v, before.deviantart_cookies.expose(), &mut want);
    assert_eq!(
        want.delay_max_seconds, 12.5,
        "the max never sits under the min"
    );
    assert_eq!(
        want.hash_source, "hydrus",
        "an unknown choice saves as the first item"
    );
    assert_eq!(want.hydrus.api_url, "http://example.test:1234");
    assert_eq!(want.enabled_tag_sources, ["User", "Booru"]);
    assert_eq!(f.w.s.settings.lock().unwrap().clone(), want);
    assert_eq!(Settings::load(&f.paths).settings, want, "and it is on disk");
    f.close();
}

fn settings_dialog_widgets_hand_back_the_values_they_were_given(app: &adw::Application) {
    use hatate_gtk::settings_values;
    let f = fx(app);
    {
        let mut s = f.w.s.settings.lock().unwrap();
        // outside the ranges and finer than two decimals: the spin boxes round, then bound
        s.delay_min_seconds = 3.0;
        s.delay_max_seconds = 12.3456;
        s.search_timeout = 1000.0;
        s.hash_workers = 99;
        s.autosave_interval_seconds = 1;
        s.borrow_tags_similarity_slack = 0.005;
        s.url_import_confirm_interval = 0.25;
        s.engine_delays.0 = vec![
            ("iqdb".into(), vec![30.0, 20.0]),
            ("yandex".into(), vec![0.0, 40.0]),
        ];
        s.tag_namespace_remap.0 = vec![
            ("artist".into(), "creator".into()),
            ("x".into(), String::new()),
        ];
    }
    let settings = f.w.s.settings.lock().unwrap().clone();
    f.w.activate_action("preferences");
    let d = f.w.s.settings_dialog.borrow().clone().unwrap();
    assert_eq!(d.values(), settings_values::initial(&settings));
    d.click_cancel();
    f.close();
}

/// DAN-2221: the Settings window, told to be as small as it can, does not need more than the
/// minimum its code states, on every page and with the Find results list showing. The fit test
/// measures the window's child at the stated width, so the window's own size request cannot hide
/// an overflow. On the shot path (hosted `ci`, GTK 4.14) the `settings-*-min` shots carry the same
/// numbers in their PNG headers.
fn settings_window_keeps_its_stated_minimum(app: &adw::Application) {
    use hatate_gtk::settings_dialog::{MIN_HEIGHT, MIN_WIDTH};
    let f = fx(app);
    f.w.activate_action("preferences");
    let d = f.w.s.settings_dialog.borrow().clone().unwrap();
    for (i, name) in d.tab_names().iter().enumerate() {
        d.tabs.set_current_page(Some(i as u32));
        pump_for(50);
        for query in ["", "delay"] {
            d.find.set_text(query);
            pump_for(50);
            let (w, h) = content_need(d.window.upcast_ref(), MIN_WIDTH);
            println!("OUTER settings {name} find={query:?} content={w}x{h}");
            assert!(
                w <= MIN_WIDTH && h <= MIN_HEIGHT,
                "the Settings window ({name}, find {query:?}) is {w}x{h}; its stated minimum is {MIN_WIDTH}x{MIN_HEIGHT}"
            );
        }
    }
    d.click_cancel();
    f.close();
}

/// DAN-2221: the Settings window is modal, so the Lens warning and the Browse chooser must be
/// asked over it (Python: `QMessageBox.warning(self, ...)` from inside the dialog), not over the
/// main window beneath it, where they could not be reached. The production prompter is built over
/// the main window and must answer `over` with one bound to the Settings window; the shot
/// `settings-lens-*` (production prompter) shows the box inside the Settings window.
fn settings_window_asks_its_questions_over_itself(app: &adw::Application) {
    use hatate_gtk::dialogs::{AdwPrompter, Prompter};
    let f = fx(app);
    f.w.activate_action("preferences");
    let d = f.w.s.settings_dialog.borrow().clone().unwrap();
    let production = AdwPrompter::new(&f.w.win);
    let main_win: gtk4::Window = f.w.win.clone().upcast();
    assert_eq!(production.parent_window().as_ref(), Some(&main_win));
    let over = production
        .over(&d.window)
        .expect("the production prompter has windows");
    assert_eq!(
        over.parent_window().as_ref(),
        Some(&d.window),
        "the box would open over the main window, under the modal Settings window"
    );
    // the scripted prompter has no windows: the dialog uses it as it is
    assert!(d.prompter.parent_window().is_none());
    d.click_cancel();
    f.close();
}

/// `TestHighlightReentrancy`: a double click on a result marks the same widget twice before the
/// first revert has run. The first revert must not take the mark off the second call's window,
/// and the mark must be gone once the second window has passed (`HIGHLIGHT_MS`, 2 s).
fn settings_highlight_second_call_inside_the_window_still_restores(app: &adw::Application) {
    let f = fx(app);
    f.w.activate_action("preferences");
    let d = f.w.s.settings_dialog.borrow().clone().unwrap();
    let w: gtk4::Widget = d.ok_button.clone().upcast();
    assert!(!w.has_css_class("found"));
    d.highlight(&w);
    assert!(w.has_css_class("found"));
    pump_for(1200);
    d.highlight(&w);
    pump_for(1100); // 2.3 s after the first call: its revert has run, the second's is pending
    assert!(
        w.has_css_class("found"),
        "the first call's revert took the second call's mark off"
    );
    pump_for(1200); // 2.3 s after the second call
    assert!(!w.has_css_class("found"), "the mark is restored");
    d.click_cancel();
    f.close();
}

fn settings_find_box_lists_and_jumps(app: &adw::Application) {
    let f = fx(app);
    f.w.activate_action("preferences");
    let d = f.w.s.settings_dialog.borrow().clone().unwrap();
    assert!(!d.results_visible());
    d.find.set_text("quota pause");
    assert_eq!(
        d.result_texts(),
        ["SauceNAO  \u{203a}  Pause searching when the daily quota runs out"]
    );
    assert!(d.results_visible());
    d.find.set_text("zzzzzz");
    assert_eq!(d.result_texts(), ["Nothing matches that."]);
    assert!(d.results_visible() && !d.result_is_activatable(0));
    d.find.set_text("   ");
    assert!(d.result_texts().is_empty() && !d.results_visible());

    d.find.set_text("hydrus key");
    assert_eq!(d.result_texts().len(), 1);
    assert_eq!(d.current_tab(), 0);
    d.jump_to_setting(0);
    assert_eq!(d.current_tab(), 4, "the Hydrus tab");
    d.click_cancel();
    f.close();
}

fn settings_engine_line_lens_warning_and_browse_follow_python(app: &adw::Application) {
    use hatate_core::lens_browser::deps::BrowserEnv;
    use hatate_gtk::settings_values::Value;
    let f = fx(app);
    f.w.activate_action("preferences");
    let d = f.w.s.settings_dialog.borrow().clone().unwrap();
    let line = |d: &hatate_gtk::settings_dialog::SettingsDialog| d.live_text("engine_pipeline");
    let before = line(&d);
    assert!(!before.is_empty());
    d.set_value("enable_tracemoe", &Value::Bool(true));
    let after = line(&d);
    assert_ne!(after, before, "the line restates when an engine is ticked");
    d.set_value("enable_tracemoe", &Value::Bool(false));
    assert_eq!(line(&d), before);

    // Lens ticked with no browser: a warning, and the box stays ticked
    let empty = tempfile::tempdir().unwrap();
    *d.lens_env.borrow_mut() = BrowserEnv {
        home: Some(empty.path().to_path_buf()),
        path: Some(empty.path().into()),
        ..BrowserEnv::default()
    };
    f.script.reset();
    d.set_value("enable_google_lens", &Value::Bool(true));
    let shown = f.script.last_shown();
    assert_eq!(shown.title, "Google Lens is not installed yet");
    assert!(
        shown
            .text
            .starts_with("Google Lens is on, but it cannot run yet: ")
    );
    assert!(
        shown
            .text
            .ends_with("Lens will be skipped until this is fixed.")
    );
    assert!(!shown.detail.is_empty());
    assert_eq!(d.values().0["enable_google_lens"], Value::Bool(true));
    // switching it off, or on again with nothing changed, asks nothing more of the first
    f.script.reset();
    d.set_value("enable_google_lens", &Value::Bool(false));
    assert_eq!(f.script.count_shown(), 0);
    // a working install says nothing (`test_a_working_install_says_nothing`): a browser on PATH
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = tempfile::tempdir().unwrap();
        let exe = bin.path().join("chromium");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        *d.lens_env.borrow_mut() = BrowserEnv {
            home: Some(empty.path().to_path_buf()),
            path: Some(bin.path().into()),
            ..BrowserEnv::default()
        };
        f.script.reset();
        d.set_value("enable_google_lens", &Value::Bool(true));
        assert_eq!(
            f.script.count_shown(),
            0,
            "a browser is found: nothing to say"
        );
        d.set_value("enable_google_lens", &Value::Bool(false));
    }

    // Browse: the chooser opens on the current path, and the answer fills the field
    let chosen = f.dir.path().join("log.txt");
    f.script.push(Answer::Path(Some(chosen.clone())));
    d.press("button.browse");
    assert_eq!(d.text_of("log_path"), chosen.to_string_lossy());
    f.script.push(Answer::Path(None));
    d.press("button.browse");
    assert_eq!(
        d.text_of("log_path"),
        chosen.to_string_lossy(),
        "Cancel leaves it"
    );
    d.click_cancel();
    f.close();
}

// ---- Phase 6.i: the MCP server in the window --------------------------------------------------

/// A window with the server enabled on `port` and the synthetic token.
fn mcp_fx(
    app: &adw::Application,
    port: u16,
    token: &'static str,
    seed: Vec<hatate_core::model::Entry>,
) -> Fx {
    Fx::new(
        app,
        Opts {
            seed,
            tweak: Some(Box::new(move |s| {
                s.mcp.enabled = true;
                s.mcp.port = i64::from(port);
                s.mcp.token = token.into();
            })),
            ..Opts::default()
        },
    )
}

/// `MainWindow.__init__` starts the server when Settings > MCP has it on, the tools answer over
/// plain http through the controller's own constructor, and `closeEvent` stops it first.
fn mcp_server_starts_with_the_window_answers_over_http_and_stops_on_close(app: &adw::Application) {
    let port = mcp::free_port();
    let f = mcp_fx(app, port, mcp::TOKEN, seeded(3, MatchStatus::Good));
    assert!(
        f.w.mcp_report().running,
        "the server did not start with the window"
    );
    let mut client = mcp::Client::new(port);
    // a wrong token is refused before any tool runs
    let refused = client.post_with(
        "nope",
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
    );
    assert_eq!(refused.status, 401);

    let listed = client.payload("list_queue", serde_json::json!({}));
    assert_eq!(listed["total_in_queue"], 3, "{listed}");
    let audit = std::fs::read_to_string(f.paths.mcp_audit_log()).expect("the call is audited");
    assert!(audit.contains("\"tool\": \"list_queue\""), "{audit}");
    assert!(
        !audit.contains(mcp::TOKEN),
        "the token reached the audit log"
    );

    f.close();
    assert!(
        pump_until(5_000, || !client.listening()),
        "the server kept listening after the window closed"
    );
}

/// Off stays off; a held port and a blank token leave the window up and the card saying why,
/// with words that carry no token character.
fn mcp_server_stays_off_when_disabled_and_reports_a_held_port_and_a_blank_token(
    app: &adw::Application,
) {
    use hatate_gtk::mcp_audit_view::{Connection, connection};

    // disabled: nothing is built, nothing listens
    let port = mcp::free_port();
    let f = Fx::new(app, Opts::default());
    assert!(!f.w.mcp_report().running);
    assert!(!f.w.s.mcp.running());
    assert!(!mcp::Client::new(port).listening());
    f.close();

    // a port already held
    let held = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let busy_port = held.local_addr().unwrap().port();
    let f = mcp_fx(app, busy_port, mcp::TOKEN, Vec::new());
    let report = f.w.mcp_report();
    assert!(!report.running);
    let card = connection(true, i64::from(busy_port), Some(&report));
    assert_eq!(card.state, Connection::PortBusy);
    assert_eq!(
        card.primary,
        format!("Can't start \u{2014} port {busy_port} is already in use.")
    );
    let user = report.user_message.expect("a message for the user");
    assert!(
        !user.contains(mcp::TOKEN) && !user.contains("127.0.0.1"),
        "{user}"
    );
    // freeing the port and restarting with the typed settings brings it up (the Restart button)
    drop(held);
    let mut typed = f.w.s.settings.lock().unwrap().mcp.clone();
    typed.enabled = true;
    assert!(f.w.restart_mcp(&typed));
    assert!(f.w.mcp_report().running);
    assert!(mcp::Client::new(busy_port).listening());
    f.close();

    // enabled with no token
    let f = mcp_fx(app, mcp::free_port(), "", Vec::new());
    let report = f.w.mcp_report();
    assert!(!report.running);
    let card = connection(true, 8787, Some(&report));
    assert_eq!(card.state, Connection::NoToken);
    f.close();
}

/// The tools reach the list through the window's own queue: a change a tool makes is on the row
/// the table shows, and the audit log says what was done.
fn mcp_tool_calls_change_the_list_and_the_table_follows(app: &adw::Application) {
    let port = mcp::free_port();
    let f = mcp_fx(app, port, mcp::TOKEN, seeded(2, MatchStatus::Good));
    let mut client = mcp::Client::new(port);
    assert!(!f.w.s.queue.borrow().entry_at(1).unwrap().reviewed);
    let answered = client.payload(
        "toggle_reviewed",
        serde_json::json!({"row": 1, "reviewed": true}),
    );
    assert_eq!(answered["reviewed"], true, "{answered}");
    assert!(f.w.s.queue.borrow().entry_at(1).unwrap().reviewed);
    // `_set_rows_reviewed` ends on the status line, as it does for a click
    assert_eq!(
        f.say(),
        "Marked 1 image(s) as reviewed \u{2014} 1 still to review"
    );
    let id = f.w.s.queue.borrow().id_at(1).unwrap();
    let row = f.w.s.queue_view.row_of(id).expect("the row is shown");
    let reviewed_cell = hatate_gtk::queue_view::cell_view(
        f.w.s.queue.borrow().entry_at(1).unwrap(),
        hatate_core::table_cells::Column::Reviewed,
    );
    assert_eq!(
        f.w.s
            .queue_view
            .cell(row, hatate_core::table_cells::Column::Reviewed)
            .map(|c| c.text),
        Some(reviewed_cell.text),
        "the table still shows the old value"
    );

    // a destructive tool is refused while its switch is off, and the refusal is audited
    let refused = client.payload("remove_row", serde_json::json!({"row": 0}));
    assert_eq!(refused["allowed"], false, "{refused}");
    assert_eq!(f.len(), 2);
    let audit = std::fs::read_to_string(f.paths.mcp_audit_log()).unwrap();
    assert!(audit.contains("\"tool\": \"remove_row\"") && audit.contains("\"allowed\": false"));

    // with it on, the row leaves the list and the table
    f.w.s.settings.lock().unwrap().mcp.allow_destructive = true;
    let removed = client.payload("remove_row", serde_json::json!({"row": 0}));
    assert_eq!(removed["row"], 0, "{removed}");
    assert_eq!(f.len(), 1);
    // `_remove_rows` ends on the status line
    assert_eq!(f.say(), "Removed 1 file(s) from the list");
    assert_eq!(f.w.s.queue_view.total_count(), 1);
    f.close();
}

fn open_settings(f: &Fx) -> Rc<hatate_gtk::settings_dialog::SettingsDialog> {
    f.w.activate_action("preferences");
    f.w.s
        .settings_dialog
        .borrow()
        .clone()
        .expect("the dialog is open")
}

/// DAN-2319 (6.h F3, 6.i riders): a paragraph on a Settings page wraps to the page, so at the 560 px
/// minimum it is read, not scrolled sideways (`general:bik` was cut mid-sentence); and every page's
/// scroll bar is persistent, because an overlay one is invisible at rest (the MCP page at its
/// default size gave no sign that more lay below the fold).
fn settings_prose_wraps_to_the_page_and_every_page_keeps_its_bar_at_rest(app: &adw::Application) {
    let f = fx(app);
    let d = open_settings(&f);
    let mut unwrapped = Vec::new();
    for (i, name) in d.tab_names().iter().enumerate() {
        d.tabs.set_current_page(Some(i as u32));
        pump_for(50);
        let page = d.tabs.nth_page(Some(i as u32)).expect("page");
        let scroll = page
            .downcast_ref::<gtk4::ScrolledWindow>()
            .unwrap_or_else(|| panic!("{name}: a page is a scrolled window"));
        assert!(
            !scroll.is_overlay_scrolling(),
            "{name}: the page's bar is an overlay"
        );
        let mut labels = Vec::new();
        all_labels(&page, &mut labels);
        for l in labels {
            let t = l.text();
            // A tick's own words, a list's rows, a drop-down's entries and a card's heading are a
            // control row (they keep the sideways scroll), and a path caption is cut in the middle
            // on purpose.
            let in_tick = l.ancestor(gtk4::CheckButton::static_type()).is_some()
                || l.ancestor(gtk4::ListBoxRow::static_type()).is_some()
                || l.ancestor(gtk4::DropDown::static_type()).is_some()
                || l.has_css_class("heading");
            let cut = l.ellipsize() != gtk4::pango::EllipsizeMode::None;
            if (t.contains('\n') || t.chars().count() > 48) && !l.wraps() && !in_tick && !cut {
                unwrapped.push(format!("{name}: {t:?}"));
            }
        }
    }
    assert!(
        unwrapped.is_empty(),
        "paragraphs that do not wrap: {unwrapped:#?}"
    );
    d.click_cancel();
    f.close();
}

/// The Connection card says what the server is doing, the Dry run sentence flips with its box, the
/// Hydrus marker is the sent glyph, and the destructive hint is drawn at the top of the ink ramp.
fn settings_mcp_page_states_the_server_and_follows_the_boxes(app: &adw::Application) {
    use hatate_gtk::settings_values::Value;
    // off: Stopped, no Restart button
    let f = fx(app);
    let d = open_settings(&f);
    assert_eq!(d.live_text("mcp_connection_primary"), "Stopped.");
    assert!(!d.control_visible("mcp_restart"));
    assert_eq!(d.live_text("mcp_live_marker"), "\u{25cf}");
    assert_eq!(d.live_text("mcp_token_hint"), "");
    assert!(d.live_has_class("mcp_destructive_hint", "ink-100"));
    assert!(
        d.live_text("mcp_destructive_hint")
            .starts_with("Throws away work")
    );
    // Dry run: the sentence and its weight flip with the box
    assert!(
        d.live_text("mcp_dry_run_hint")
            .starts_with("Dry run is off.")
    );
    assert!(d.live_has_class("mcp_dry_run_hint", "ink-100"));
    d.set_value("mcp_dry_run", &Value::Bool(true));
    assert!(
        d.live_text("mcp_dry_run_hint")
            .starts_with("Dry run is on.")
    );
    assert!(d.live_has_class("mcp_dry_run_hint", "ink-65"));
    assert!(!d.live_has_class("mcp_dry_run_hint", "ink-100"));
    // ticking the server box states the server as it is: enabled, but nothing has started it
    d.set_value("mcp_enabled", &Value::Bool(true));
    assert!(d.control_visible("mcp_restart"));
    f.w.s.settings_dialog.borrow_mut().take();
    d.click_cancel();
    f.close();

    // running: Listening on the port
    let port = mcp::free_port();
    let f = mcp_fx(app, port, mcp::TOKEN, Vec::new());
    let d = open_settings(&f);
    assert_eq!(
        d.live_text("mcp_connection_primary"),
        format!("Listening on 127.0.0.1:{port}.")
    );
    assert_eq!(
        d.live_text("mcp_connection_remedy"),
        "A client with the token below can connect now."
    );
    assert!(d.live_has_class("mcp_connection_primary", "ink-65"));
    // the token field is always masked
    assert!(!d.entry_visible("mcp_token"));
    assert_eq!(d.text_of("mcp_token"), mcp::TOKEN);
    d.click_cancel();
    f.close();
}

/// The enable summary: not on opening a saved enabled state, once for each real change to on, and
/// it lists exactly the tiers ticked in the dialog.
fn settings_mcp_enable_summary_fires_on_a_real_change_only(app: &adw::Application) {
    use hatate_gtk::settings_values::Value;
    let port = mcp::free_port();
    let f = mcp_fx(app, port, mcp::TOKEN, Vec::new());
    f.script.reset();
    let d = open_settings(&f);
    assert_eq!(
        f.script.count_shown(),
        0,
        "opening on a saved enabled state shows nothing"
    );
    // off then on: once
    d.set_value("mcp_enabled", &Value::Bool(false));
    assert_eq!(f.script.count_shown(), 0);
    d.set_value("mcp_enabled", &Value::Bool(true));
    assert_eq!(f.script.count_shown(), 1);
    let shown = f.script.last_shown();
    assert_eq!(shown.kind, Kind::Information);
    assert_eq!(shown.title, "Enabling the MCP server");
    assert_eq!(shown.buttons, Buttons::GotIt);
    assert_eq!(shown.default, Response::Ok);
    // Hydrus writes are on by default: that bullet is listed, the other two are not
    assert!(
        shown
            .text
            .contains("Send files and tags to your live Hydrus library")
    );
    assert!(!shown.text.contains("Re-search a stuck entry"));
    assert!(!shown.text.contains("Remove rows and reset results"));
    assert!(
        shown
            .text
            .starts_with("Starting now, a connected AI with the token below can:\n\n")
    );
    assert!(
        shown
            .text
            .ends_with("this off in the groups above, any time.")
    );
    // every combination of the three boxes lists exactly the ticked ones
    for (h, r, x) in [
        (false, false, false),
        (true, true, true),
        (false, true, false),
        (false, false, true),
    ] {
        d.set_value("mcp_enabled", &Value::Bool(false));
        d.set_value("mcp_allow_hydrus_writes", &Value::Bool(h));
        d.set_value("mcp_allow_research", &Value::Bool(r));
        d.set_value("mcp_allow_destructive", &Value::Bool(x));
        f.script.reset();
        d.set_value("mcp_enabled", &Value::Bool(true));
        let text = f.script.last_shown().text;
        assert_eq!(text.contains("live Hydrus library"), h, "{text}");
        assert_eq!(text.contains("Re-search a stuck entry"), r, "{text}");
        assert_eq!(text.contains("Remove rows and reset results"), x, "{text}");
    }
    d.click_cancel();
    f.close();
}

/// Generate puts a fresh 43-character token in the masked field and on the clipboard, with a note
/// that a hand edit clears; Restart re-binds with what is typed and does not save it; OK saves the
/// page byte-for-byte as Python would and re-binds the server only when enabled, port or token
/// changed.
fn settings_mcp_token_buttons_restart_and_ok_rebind_the_server(app: &adw::Application) {
    use hatate_gtk::settings_values::Value;
    let port = mcp::free_port();
    let f = mcp_fx(app, port, mcp::TOKEN, Vec::new());
    let d = open_settings(&f);

    d.press("mcp_generate_token");
    let token = d.text_of("mcp_token");
    assert_eq!(token.len(), 43);
    assert_ne!(token, mcp::TOKEN);
    assert_eq!(d.copied.borrow().as_deref(), Some(token.as_str()));
    assert_eq!(
        d.live_text("mcp_token_hint"),
        "New token generated and copied to clipboard."
    );
    d.press("mcp_copy_token");
    assert_eq!(d.copied.borrow().as_deref(), Some(token.as_str()));
    assert!(!d.entry_visible("mcp_token"), "still masked after generate");
    // a hand edit clears the note
    d.type_into("mcp_token", &format!("{token}x"));
    assert_eq!(d.live_text("mcp_token_hint"), "");

    // Restart binds the typed port, and leaves the saved settings alone
    let other = mcp::free_port();
    d.set_value("mcp_port", &Value::Num(f64::from(other)));
    d.type_into("mcp_token", "another-synthetic-token");
    d.press("mcp_restart");
    assert!(f.w.mcp_report().running);
    assert_eq!(
        d.live_text("mcp_connection_primary"),
        format!("Listening on 127.0.0.1:{other}.")
    );
    assert!(
        !mcp::Client::new(port).listening(),
        "the old port was released"
    );
    assert!(mcp::Client::new(other).listening());
    assert_eq!(f.w.s.settings.lock().unwrap().mcp.port, i64::from(port));
    assert_eq!(
        f.w.s.settings.lock().unwrap().mcp.token.expose(),
        mcp::TOKEN
    );

    // OK: saves the page and, the port and token having changed, re-binds on the saved values
    d.set_value("mcp_dry_run", &Value::Bool(true));
    d.set_value("mcp_allow_research", &Value::Bool(true));
    d.click_ok();
    let saved = f.w.s.settings.lock().unwrap().mcp.clone();
    assert_eq!(saved.port, i64::from(other));
    assert_eq!(saved.token.expose(), "another-synthetic-token");
    assert!(saved.dry_run && saved.allow_research && saved.enabled);
    assert!(saved.allow_hydrus_writes && !saved.allow_destructive);
    assert_eq!(saved.audit_log_max_bytes, 5_000_000);
    let on_disk = std::fs::read_to_string(f.paths.config_file()).unwrap();
    let json: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
    assert_eq!(json["mcp"]["port"], i64::from(other));
    assert_eq!(json["mcp"]["dry_run"], true);
    let client = mcp::Client::new(other);
    assert!(client.listening());
    // the new token is the one the server wants
    let ok = client.post_with(
        "another-synthetic-token",
        &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
    );
    assert_eq!(ok.status, 200);

    // a save that changes none of enabled, port, token does not drop a connected client
    let sessions_before = client
        .post_with(
            "another-synthetic-token",
            &serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .status;
    let d = open_settings(&f);
    d.set_value("mcp_allow_destructive", &Value::Bool(true));
    d.click_ok();
    assert!(f.w.s.settings.lock().unwrap().mcp.allow_destructive);
    assert_eq!(
        sessions_before, 400,
        "no session id yet, so the server refuses (and is up)"
    );
    assert!(client.listening());

    // switching it off stops the server
    let d = open_settings(&f);
    d.set_value("mcp_enabled", &Value::Bool(false));
    d.click_ok();
    assert!(pump_until(5_000, || !mcp::Client::new(other).listening()));
    f.close();
}

/// The Recent tool calls table: newest first, Actions hides reads, the audit log of a real call
/// shows up, and a log mid-write is not an error.
fn settings_mcp_audit_table_follows_the_log_newest_first(app: &adw::Application) {
    let port = mcp::free_port();
    let f = mcp_fx(app, port, mcp::TOKEN, seeded(3, MatchStatus::Good));
    let mut client = mcp::Client::new(port);
    client.payload("list_queue", serde_json::json!({}));
    client.payload(
        "toggle_reviewed",
        serde_json::json!({"row": 2, "reviewed": true}),
    );
    client.payload(
        "select_candidate",
        serde_json::json!({"row": 1, "candidate_index": 0}),
    );
    let d = open_settings(&f);
    d.audit.refresh();
    let rows = d.audit.rows();
    // Actions (the default) hides the read
    let tools: Vec<&str> = rows.iter().map(|r| r.tool.as_str()).collect();
    assert_eq!(tools, ["select_candidate", "toggle_reviewed"], "{rows:?}");
    assert_eq!(rows[1].outcome, "Allowed \u{2014} marked reviewed");
    assert_eq!(rows[1].target, "#2");
    assert_eq!(rows[1].tier, "Reversible");
    d.audit.all.set_active(true);
    let tools: Vec<String> = d.audit.rows().iter().map(|r| r.tool.clone()).collect();
    assert_eq!(tools, ["select_candidate", "toggle_reviewed", "list_queue"]);
    // the log being written while the table reads it
    let log = f.paths.mcp_audit_log();
    let mut text = std::fs::read_to_string(&log).unwrap();
    text.push_str("{\"timestamp\": \"2020-01-02T03:04:05Z\", \"tool\": \"send_up");
    std::fs::write(&log, text).unwrap();
    d.audit.refresh();
    assert_eq!(d.audit.rows().len(), 3);
    // the timer only runs while the MCP tab is on show and the follow button is down
    assert!(!d.audit.following());
    d.show_tab(hatate_gtk::settings_dialog::SettingsDialog::mcp_tab_index());
    assert!(d.audit.following());
    d.audit.follow.set_active(false);
    assert!(!d.audit.following());
    d.audit.follow.set_active(true);
    assert!(d.audit.following());
    d.show_tab(0);
    assert!(!d.audit.following());
    // the whole page fits the stated minimum window, so the Outcome column is never cut off the
    // side: the token box, the path caption and the table's columns give way before the page does
    let page = d
        .tabs
        .nth_page(Some(
            hatate_gtk::settings_dialog::SettingsDialog::mcp_tab_index() as u32,
        ))
        .expect("the MCP page");
    let content = page
        .downcast_ref::<gtk4::ScrolledWindow>()
        .and_then(|sw| sw.child())
        .and_then(|v| v.first_child())
        .expect("the page's content");
    let (min_width, _, _, _) = content.measure(gtk4::Orientation::Horizontal, -1);
    let room = hatate_gtk::settings_dialog::MIN_WIDTH - 40;
    assert!(
        min_width <= room,
        "the MCP page needs {min_width}px, the minimum window gives {room}"
    );
    d.click_cancel();
    f.close();
}

/// The Shortcuts tab: the registry's keys in Qt's spelling, a click records the next key, Clear
/// leaves an action unbound, a duplicate is named and OK refuses it, and what OK saves is only the
/// keys that differ from their defaults.
fn settings_shortcuts_page_records_clears_resets_and_refuses_a_conflict(app: &adw::Application) {
    use gtk4::gdk;
    use hatate_core::shortcuts::REVIEW_ACTIONS;
    use hatate_gtk::settings_values::Value;
    let f = fx(app);
    f.w.s.settings.lock().unwrap().review_shortcuts =
        hatate_core::config::OrderedMap(vec![("next_row".to_owned(), "x".to_owned())]);
    let d = open_settings(&f);
    // a stored lower-case x shows as the sequence Qt writes
    let page = &d.shortcuts;
    assert_eq!(page.edit("next_row").unwrap().value(), "X");
    assert_eq!(page.edit("prev_row").unwrap().value(), "K");
    assert_eq!(page.edit("prev_unreviewed").unwrap().value(), "Shift+N");
    assert_eq!(page.edits.len(), REVIEW_ACTIONS.len());
    assert_eq!(page.conflict.text(), "");

    // a click records the next key; a modifier on its own does not end it
    let edit = page.edit("prev_row").unwrap().clone();
    edit.start_recording();
    assert!(edit.is_recording());
    assert_eq!(edit.button.label().as_deref(), Some("Press a key\u{2026}"));
    edit.press(gdk::Key::Shift_L, gdk::ModifierType::empty());
    assert!(edit.is_recording());
    edit.press(gdk::Key::q, gdk::ModifierType::SHIFT_MASK);
    assert!(!edit.is_recording());
    assert_eq!(edit.value(), "Shift+Q");
    edit.start_recording();
    edit.press(gdk::Key::Delete, gdk::ModifierType::empty());
    assert_eq!(edit.value(), "Del");

    // a duplicate is named, and OK keeps the dialog open with the warning
    page.edit("prev_row").unwrap().set("x");
    assert_eq!(
        page.conflict.text(),
        "Same key on more than one action - Qt fires neither, so these would both stop working:\nX is bound to Next image and Previous image"
    );
    f.script.reset();
    d.click_ok();
    assert!(
        f.w.s.settings_dialog.borrow().is_some(),
        "the dialog stays open on a conflict"
    );
    let shown = f.script.last_shown();
    assert_eq!(shown.kind, Kind::Warning);
    assert_eq!(shown.title, "Shortcut conflict");
    assert!(
        shown
            .text
            .ends_with("\n\nGive one of them a different key, or clear it."),
        "{}",
        shown.text
    );
    assert_eq!(
        f.w.s.settings.lock().unwrap().review_shortcuts.0,
        vec![("next_row".to_owned(), "x".to_owned())],
        "nothing was saved"
    );

    // Clear leaves the action with no key at all, and that is not a conflict
    page.clears[1].emit_clicked();
    assert_eq!(page.edit("prev_row").unwrap().value(), "");
    assert_eq!(page.conflict.text(), "");
    // Reset puts every default back
    d.set_value("shortcut.next_row", &Value::Text("shift+ctrl+a".into()));
    assert_eq!(page.edit("next_row").unwrap().value(), "Ctrl+Shift+A");
    page.reset.emit_clicked();
    for a in REVIEW_ACTIONS {
        assert_eq!(page.edit(a.id).unwrap().value(), a.default, "{}", a.id);
    }

    // OK saves only what differs from the default
    d.set_value("shortcut.next_row", &Value::Text("F9".into()));
    d.set_value("shortcut.prev_row", &Value::Text("".into()));
    d.click_ok();
    assert!(f.w.s.settings_dialog.borrow().is_none());
    let saved = f.w.s.settings.lock().unwrap().review_shortcuts.0.clone();
    assert_eq!(
        saved,
        vec![
            ("next_row".to_owned(), "F9".to_owned()),
            ("prev_row".to_owned(), "".to_owned())
        ]
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(f.paths.config_file()).unwrap()).unwrap();
    assert_eq!(json["review_shortcuts"]["next_row"], "F9");
    assert_eq!(json["review_shortcuts"]["prev_row"], "");
    f.close();
}

/// The `research` tool starts a search of the window's own (the strip, the button and the rows
/// follow it), refuses while one is running, and answers when it ends. The server's thread waits
/// for it; the main loop does not.
fn mcp_research_runs_the_windows_own_search_and_the_tool_goes_on_when_it_ends(
    app: &adw::Application,
) {
    let pics = tempfile::tempdir().unwrap();
    let port = mcp::free_port();
    let f = Fx::new(
        app,
        Opts {
            seed: unsearched(pics.path(), 2),
            tweak: Some(Box::new(move |s| {
                (search_tweak())(s);
                s.mcp.enabled = true;
                s.mcp.port = i64::from(port);
                s.mcp.token = mcp::TOKEN.into();
                s.mcp.allow_research = true;
            })),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    for _ in 0..2 {
        f.search
            .engines
            .iqdb
            .push(common::search::scripted::Step::Return(Ok(Vec::new())));
    }
    let mut client = mcp::Client::new(port);
    let answer = client.payload("research", serde_json::json!({"rows": [0, 1]}));
    assert_eq!(answer["research_started"], true, "{answer}");
    assert_eq!(answer["completed"], true, "{answer}");
    // the window's own run did it: both rows were searched, the strip is idle again
    assert_eq!(statuses(&f), vec![MatchStatus::NotFound; 2]);
    assert!(!f.w.s.run_active.get());
    assert!(!f.w.s.queue.borrow().search_running());
    assert_eq!(f.search.engines.calls(), vec!["iqdb"; 2]);
    assert_eq!(f.say(), "Search finished");
    assert_eq!(start_button(&f), "\u{25B6} Start Search");
    // and the audit says what happened
    let audit = std::fs::read_to_string(f.paths.mcp_audit_log()).unwrap();
    assert!(audit.contains("\"tool\": \"research\"") && audit.contains("\"allowed\": true"));
    f.close();
}

/// A `send_upload` tool call against a Hydrus that stalls: the server's thread waits, the main
/// loop keeps turning, and the row is marked sent when the answer comes.
fn mcp_send_leaves_the_main_loop_running_while_hydrus_stalls(app: &adw::Application) {
    use common::hydrus::json_reply;
    use serde_json::json;
    let dir = tempfile::tempdir().unwrap();
    let server = common::hydrus::Hydrus::http(|q| {
        let r = json_reply(json!({"status": 1, "hash": NEW_HASH, "note": ""}));
        if q.target.starts_with("/add_files/add_file") {
            r.delayed(std::time::Duration::from_millis(900))
        } else {
            r
        }
    });
    let port = mcp::free_port();
    let url = server.url.clone();
    let f = Fx::new(
        app,
        Opts {
            seed: review_seed(dir.path()),
            tweak: Some(Box::new(move |s| {
                (hydrus_tweak(&url, true))(s);
                s.mcp.enabled = true;
                s.mcp.port = i64::from(port);
                s.mcp.token = mcp::TOKEN.into();
            })),
            missing: Some(Arc::new(|_, _| HashSet::new())),
            ..Opts::default()
        },
    );
    let ticks = Rc::new(Cell::new(0u32));
    let t = ticks.clone();
    let tick = gtk4::glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
        t.set(t.get() + 1);
        gtk4::glib::ControlFlow::Continue
    });
    let mut client = mcp::Client::new(port);
    let answer = client.payload("send_upload", json!({"row": 0}));
    assert_eq!(answer["success"], true, "{answer}");
    assert!(
        ticks.get() >= 10,
        "the main loop kept turning while the call waited: {} ticks",
        ticks.get()
    );
    assert!(f.w.s.queue.borrow().entry_at(0).unwrap().sent_to_hydrus);
    tick.remove();
    f.close();
}

/// `mcp-settings-spec.md` section 9: the tab name, the checkbox labels, the four group titles and
/// the two informational lines are all findable through the Find box.
fn settings_find_box_reaches_every_string_the_mcp_spec_lists(app: &adw::Application) {
    use hatate_gtk::settings_mcp as m;
    let f = fx(app);
    let d = open_settings(&f);
    let wanted = [
        ("mcp", "MCP  \u{203a}  MCP".to_owned()),
        (
            "Enable the MCP server",
            format!("MCP  \u{203a}  {}", m::ENABLE),
        ),
        ("practice mode", format!("MCP  \u{203a}  {}", m::DRY_RUN)),
        (
            "re-search a stuck",
            format!("MCP  \u{203a}  {}", m::RESEARCH_LABEL),
        ),
        (
            "send files and tags",
            format!("MCP  \u{203a}  {}", m::HYDRUS_LABEL),
        ),
        (
            "remove rows and reset",
            format!("MCP  \u{203a}  {}", m::DESTRUCTIVE_LABEL),
        ),
        (
            "Always available",
            format!("MCP  \u{203a}  {}", m::ALWAYS_TITLE),
        ),
        (
            "sends to your live",
            format!("MCP  \u{203a}  {}", m::HYDRUS_TITLE),
        ),
        (
            "spends a limited",
            format!("MCP  \u{203a}  {}", m::RESEARCH_TITLE),
        ),
        (
            "hard to undo",
            format!("MCP  \u{203a}  {}", m::DESTRUCTIVE_TITLE),
        ),
        (
            "Recent tool calls",
            "MCP  \u{203a}  Recent tool calls".to_owned(),
        ),
        ("Token", "MCP  \u{203a}  Token".to_owned()),
        ("Port", "MCP  \u{203a}  Port".to_owned()),
    ];
    for (query, row) in wanted {
        d.find.set_text(query);
        let found = d.result_texts();
        assert!(
            found.contains(&row),
            "{query:?} did not reach {row:?}: {found:?}"
        );
    }
    d.find.set_text("");
    d.click_cancel();
    f.close();
}
