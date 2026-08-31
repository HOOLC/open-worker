use gpui::{point, px, AssetSource};
use zork_gui::assets::EmbeddedAssets;
use zork_gui::design::{codex_ui_spec, ComposerActionShape, TranscriptTreatment};
use zork_gui::window_chrome::native_titlebar_options;

#[test]
fn native_window_uses_codex_full_size_transparent_chrome() {
    let titlebar = native_titlebar_options();

    assert!(titlebar.title.is_none());
    assert!(titlebar.appears_transparent);
    assert_eq!(
        titlebar.traffic_light_position,
        Some(point(px(16.0), px(16.0)))
    );
}

#[test]
fn codex_shell_uses_the_reference_palette_and_geometry() {
    let spec = codex_ui_spec();

    assert!(spec.force_light_window_chrome);
    assert_eq!(spec.palette.canvas, 0xFFFFFF);
    assert_eq!(spec.palette.sidebar, 0xFBFBFB);
    assert_eq!(spec.palette.sidebar_hover, 0xF2F2F2);
    assert_eq!(spec.palette.selected, 0xF2F2F2);
    assert_eq!(spec.palette.text, 0x1A1C1F);
    assert_eq!(spec.palette.muted, 0x737373);
    assert_eq!(spec.layout.sidebar_width, 275.0);
    assert_eq!(spec.layout.header_height, 46.0);
    assert_eq!(spec.layout.transcript_max_width, 736.0);
    assert_eq!(spec.layout.composer_width, 736.0);
    assert_eq!(spec.layout.composer_height, 141.0);
    assert_eq!(spec.layout.composer_bottom_inset, 16.0);
    assert!(!spec.layout.has_global_status_bar);
}

#[test]
fn live_cdp_shell_metrics_drive_sidebar_home_and_thread() {
    let spec = codex_ui_spec();

    assert_eq!(spec.sidebar.toolbar_height, 46.0);
    assert_eq!(spec.sidebar.footer_height, 46.0);
    assert_eq!(spec.sidebar.inline_inset, 8.0);
    assert_eq!(spec.sidebar.row_height, 30.0);
    assert_eq!(spec.sidebar.row_radius, 12.5);
    assert_eq!(spec.sidebar.item_font_size, 14.0);
    assert_eq!(spec.sidebar.item_line_height, 21.0);
    assert_eq!(spec.sidebar.section_label_font_size, 14.0);
    assert_eq!(spec.sidebar.section_label_line_height, 21.0);
    assert_eq!(spec.sidebar.section_label_weight, 500);
    assert_eq!(spec.sidebar.selected_fill, 0xF2F2F2);
    assert!(!spec.sidebar.has_hard_divider);

    assert_eq!(spec.home.heading_font_size, 28.0);
    assert_eq!(spec.home.heading_line_height, 33.6);
    assert_eq!(spec.home.heading_weight, 400);
    assert_eq!(spec.home.suggestion_grid_width, 710.0);
    assert_eq!(spec.home.suggestion_grid_height, 104.0);
    assert_eq!(spec.home.suggestion_gap, 12.0);
    assert_eq!(spec.home.suggestion_count, 4);
    assert_eq!(spec.home.card_radius, 20.0);
    assert_eq!(spec.home.card_padding_x, 16.0);
    assert_eq!(spec.home.card_padding_y, 12.0);
    assert_eq!(spec.home.card_label_font_size, 13.0);
    assert_eq!(spec.home.card_label_line_height, 20.0);
    assert_eq!(spec.home.card_label_weight, 500);
    assert!(spec.home.suggestions_fill_composer);
    assert!(spec.home.uses_real_icon_assets);
    assert!(!spec.home.counterfeits_codex_mark);

    assert_eq!(spec.thread.header_height, 46.0);
    assert_eq!(spec.thread.title_font_size, 14.0);
    assert_eq!(spec.thread.title_line_height, 24.0);
    assert_eq!(spec.thread.title_weight, 500);
    assert_eq!(spec.thread.assistant_font_size, 14.0);
    assert_eq!(spec.thread.assistant_line_height, 22.0);
    assert_eq!(spec.thread.content_left_inset, 76.0);
    assert!(spec.thread.aligns_followup_composer);
    assert_eq!(spec.thread.user_font_size, 16.0);
    assert_eq!(spec.thread.user_line_height, 24.0);
    assert_eq!(spec.thread.user_max_width_ratio, 0.77);
    assert_eq!(spec.thread.user_padding_x, 12.0);
    assert_eq!(spec.thread.user_padding_y, 8.0);
    assert_eq!(spec.thread.user_radius, 20.0);
    assert_eq!(spec.thread.user_fill, 0xF2F2F2);
}

#[test]
fn transcript_uses_codex_content_treatments() {
    let spec = codex_ui_spec();

    assert_eq!(spec.transcript.user, TranscriptTreatment::PromptPill);
    assert_eq!(spec.transcript.assistant, TranscriptTreatment::PlainProse);
    assert_eq!(
        spec.transcript.activity,
        TranscriptTreatment::InlineActivity
    );
}

#[test]
fn composer_and_task_rows_express_the_codex_hierarchy() {
    let spec = codex_ui_spec();

    assert!(spec.composer.fixed_to_bottom);
    assert!(spec.composer.multiline);
    assert!(spec.composer.controls_inside_surface);
    assert!(spec.composer.send_becomes_stop);
    assert!(spec.composer.visible_keyboard_focus);
    assert_eq!(spec.composer.workspace_tray_height, 61.0);
    assert_eq!(spec.composer.input_surface_height, 98.0);
    assert_eq!(spec.composer.tray_overlap, 18.0);
    assert_eq!(spec.composer.workspace_tray_inline_inset, 13.0);
    assert_eq!(spec.composer.workspace_tray_top_inset, 4.0);
    assert_eq!(spec.composer.surface_radius, 25.0);
    assert_eq!(spec.composer.editor_height, 44.0);
    assert_eq!(spec.composer.editor_horizontal_inset, 12.0);
    assert_eq!(spec.composer.control_height, 28.0);
    assert_eq!(spec.composer.action_size, 28.0);
    assert_eq!(spec.composer.placeholder_font_size, 14.0);
    assert_eq!(spec.composer.control_font_size, 13.0);
    assert_eq!(spec.composer.project_surface_color, 0xF6F6F6);
    assert_eq!(spec.composer.primary_text_color, 0x1A1C1F);
    assert!(spec.composer.surface_has_prominent_shadow);
    assert!(!spec.composer.has_hard_divider);
    assert!(spec.composer.compact_split_controls);
    assert!(spec.composer.shared_new_and_followup_structure);
    assert_eq!(spec.composer.action_shape, ComposerActionShape::Circle);
    assert!(spec.composer.action_uses_icon_asset);
    assert!(spec.composer.model_selector_uses_icon_asset);
    assert!(spec.task_rows.group_by_workspace);
    assert!(spec.task_rows.selected_uses_fill);
    assert!(!spec.task_rows.draw_card_borders);
    assert!(!spec.task_rows.show_model_suffix);
    assert!(!spec.task_rows.show_leading_status_dot);
    assert!(spec.task_rows.workspace_uses_icon_asset);
}

#[test]
fn primary_regions_fit_the_default_window_without_overlap() {
    let geometry = codex_ui_spec()
        .layout
        .resolve(1280.0, 800.0)
        .expect("the supported default window must resolve");

    assert_eq!(geometry.sidebar.right, geometry.main.left);
    assert!(geometry.transcript.right <= geometry.main.right);
    assert!(geometry.composer.left >= geometry.main.left);
    assert!(geometry.composer.right <= geometry.main.right);
    assert!(geometry.composer.top > geometry.header.bottom);
    assert!(geometry.transcript.bottom <= geometry.composer.top);
}

#[test]
fn composer_icons_are_real_embedded_library_assets() {
    let assets = EmbeddedAssets;

    for path in [
        "icons/phosphor-arrow-up.svg",
        "icons/phosphor-caret-down.svg",
        "icons/phosphor-brain.svg",
        "icons/phosphor-cube.svg",
        "icons/phosphor-folder-simple.svg",
        "icons/phosphor-stop-fill.svg",
        "icons/phosphor-terminal-window.svg",
    ] {
        assert!(assets.load(path).expect("asset load succeeds").is_some());
    }
}
