//! The settings model's own cases: defaults, clamping, and the blob's shape.

use super::*;
use crate::appearance::BaseMode;

#[test]
fn settings_round_trip() {
    let mut s = Settings::default();
    s.appearance.base = BaseMode::Dark;
    s.appearance.tint_hue = 200;
    s.appearance.tint_strength = 40;
    s.default_zoom = 1.25;
    s.last_path = Some("/tmp/a.pdf".to_string());
    let json = serde_json::to_string(&s).unwrap();
    let back: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
}

#[test]
fn applying_a_preset_sets_both_look_and_selection() {
    let mut s = Settings::default();
    s.apply_preset("green");
    assert_eq!(s.active_preset.as_deref(), Some("green"));
    assert_eq!(s.appearance.tint_hue, 104);
}

#[test]
fn editing_a_slider_detaches_from_the_preset() {
    let mut s = Settings::default();
    s.apply_preset("sepia");
    s.appearance.tint_hue = 210;
    s.touch_appearance();
    assert_eq!(s.active_preset, None, "an edited preset is no longer that preset");
}

#[test]
fn editing_back_onto_a_preset_reselects_it() {
    // If you dial the sliders to exactly Green, the menu should say
    // Green — anything else is a lying UI.
    let mut s = Settings::default();
    s.apply_preset("sepia");
    s.appearance = builtin_presets().into_iter().find(|p| p.id == "green").unwrap().appearance;
    s.touch_appearance();
    assert_eq!(s.active_preset.as_deref(), Some("green"));
}

#[test]
fn user_presets_cannot_shadow_builtins_or_be_nameless() {
    let mut s = Settings {
        user_presets: vec![
            Preset { id: "sepia".into(), name: "Mine".into(), group: String::new(), appearance: Appearance::default() },
            Preset { id: "ok".into(), name: "  ".into(), group: String::new(), appearance: Appearance::default() },
            Preset { id: "good".into(), name: "Good".into(), group: "G".into(), appearance: Appearance::default() },
        ],
        ..Settings::default()
    };
    sanitize(&mut s);
    let ids: Vec<String> = s.user_presets.iter().map(|p| p.id.clone()).collect();
    assert_eq!(ids, vec!["good".to_string()]);
}

#[test]
fn a_stale_plain_base_selection_is_dropped_not_dangled() {
    // Settings persisted while Light/Dark/Dim were presets carry their
    // ids as `active_preset`; the sanitizer clears the selection (the look
    // itself lives in `appearance` and survives) rather than highlight a
    // swatch that no longer exists.
    let mut s = Settings {
        active_preset: Some("light".to_string()),
        ..Settings::default()
    };
    sanitize(&mut s);
    assert_eq!(s.active_preset, None);
}

#[test]
fn a_deleted_active_preset_does_not_dangle() {
    let mut s = Settings {
        active_preset: Some("gone".to_string()),
        ..Settings::default()
    };
    sanitize(&mut s);
    assert_eq!(s.active_preset, None);
}

#[test]
fn missing_fields_default() {
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert_eq!(s.appearance, Appearance::default());
    assert!(s.user_presets.is_empty());
}

#[test]
fn an_old_blob_halves_its_tint_strengths_once() {
    // A blob written before the doubled tint curve: no gate field, and
    // strengths calibrated against the old /100 slope — Sepia's 45, and
    // a user preset saved at 40.
    let mut s: Settings = serde_json::from_str(
        r#"{"appearance": {"tint_hue": 34, "tint_strength": 45},
            "user_presets": [
                {"id": "mine", "name": "Mine", "appearance": {"tint_strength": 40}}
            ]}"#,
    )
    .unwrap();
    assert!(!s.tint_strength_halved, "a blob without the gate loads un-migrated");
    sanitize(&mut s);
    assert_eq!(
        s.appearance.tint_strength, 23,
        "45 on the old slope is the Sepia preset's own 23 on the new one"
    );
    assert_eq!(s.user_presets[0].appearance.tint_strength, 20);
    assert!(s.tint_strength_halved);
    // Once means once: the app sanitizes on load AND on every write, so
    // a second pass must not halve again.
    sanitize(&mut s);
    assert_eq!(s.appearance.tint_strength, 23);
}

#[test]
fn fresh_settings_are_born_on_the_new_curve() {
    // The in-memory default must not look like an old blob: a fresh
    // install's dialled strength is already a new-curve number, and a
    // migration that ran on it would halve a look the reader chose on
    // purpose.
    let mut s = Settings::default();
    s.appearance.tint_strength = 45;
    sanitize(&mut s);
    assert_eq!(s.appearance.tint_strength, 45, "nothing to migrate on a fresh blob");
    assert!(s.tint_strength_halved);
}

#[test]
fn layout_settings_default() {
    let s = LayoutSettings::default();
    assert_eq!(s.page_margin, 0.0);
    assert!(s.auto_scale);
    assert!(s.auto_resize);
    assert!(s.page_shadow);
    assert!(!s.sidebar_overlay);
    assert!(!s.blend_mode);
    assert_eq!(s.blend_area, PaperArea::WholePage);
    assert!(!s.floating_label_persist);
    assert_eq!(s.floating_label_max_pct, 100.0);
    // Startup fit defaults to Fit Page.
    assert_eq!(s.default_fit, crate::zoom_math::FitMode::Page);

    // A blob saved BEFORE `auto_resize` existed is exactly this shape, so
    // these assertions are also the promise that an existing install keeps
    // the behaviour it had.
    let s: LayoutSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(s.page_margin, 0.0);
    assert!(s.auto_scale);
    assert!(s.auto_resize);
    assert!(s.page_shadow);
    assert!(!s.sidebar_overlay);
    assert!(!s.blend_mode);
    assert_eq!(s.blend_area, PaperArea::WholePage);
    assert_eq!(s.default_fit, crate::zoom_math::FitMode::Page);
    assert!(!s.floating_label_persist);
    assert_eq!(s.floating_label_max_pct, 100.0);
}

#[test]
fn a_startup_fit_of_none_is_reset_to_page() {
    let mut s = Settings::default();
    s.layout.default_fit = crate::zoom_math::FitMode::None;
    sanitize(&mut s);
    assert_eq!(s.layout.default_fit, crate::zoom_math::FitMode::Page);
}

#[test]
fn the_detection_area_round_trips() {
    let s = LayoutSettings {
        blend_area: PaperArea::Edges,
        ..LayoutSettings::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("\"blend_area\":\"edges\""), "{json}");
    let back: LayoutSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(back.blend_area, PaperArea::Edges);
}

#[test]
fn a_blob_from_the_fixed_mode_era_still_loads() {
    // Older builds persisted a paper mode and a scan budget alongside the
    // switch. Both are gone; a blob that still carries them must load
    // cleanly with the switch and area it named.
    let s: LayoutSettings = serde_json::from_str(
        r#"{"blend_mode":true,"blend_scope":"fixed","blend_area":"edges","blend_scan_pages":100}"#,
    )
    .unwrap();
    assert!(s.blend_mode);
    assert_eq!(s.blend_area, PaperArea::Edges);
}

#[test]
fn every_animation_is_on_until_told_otherwise() {
    let a = AnimationSettings::default();
    assert!(a.enabled);
    assert!(a.sidebar_slide && a.canvas_resize);
    assert!(a.zoom && a.scroll_jumps);

    // A blob saved before this group existed deserialises like `{}`: the
    // reader must keep animating across an update rather than freeze.
    let s: Settings = serde_json::from_str("{}").unwrap();
    assert!(s.animations.enabled && s.animations.zoom);
    // A half-written group defaults the fields it does not carry, one by
    // one — a stored master-off must not silently turn the details on.
    let a: AnimationSettings = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
    assert!(!a.enabled);
    assert!(a.zoom && a.sidebar_slide && a.canvas_resize);
}

#[test]
fn label_width_limit_is_clamped() {
    let mut s = Settings::default();
    s.layout.floating_label_max_pct = 420.0;
    sanitize(&mut s);
    assert_eq!(s.layout.floating_label_max_pct, 100.0);

    s.layout.floating_label_max_pct = 0.0;
    sanitize(&mut s);
    assert_eq!(s.layout.floating_label_max_pct, 10.0);
}

#[test]
fn the_column_width_dial_is_clamped() {
    let mut s = Settings::default();
    assert_eq!(s.layout.column_width_pct, DEFAULT_COLUMN_WIDTH_PCT);

    s.layout.column_width_pct = 400.0;
    sanitize(&mut s);
    assert_eq!(s.layout.column_width_pct, MAX_COLUMN_WIDTH_PCT);

    s.layout.column_width_pct = 5.0;
    sanitize(&mut s);
    assert_eq!(s.layout.column_width_pct, MIN_COLUMN_WIDTH_PCT);
}
