//! The type scale's own cases.

use super::*;

#[test]
fn defaults_are_the_documented_middle() {
    let s = TextSettings::default();
    assert!(!s.book_layout);
    assert_eq!(s.paragraph_margin, 1.0);
    assert_eq!(s.line_height, 1.7);
    assert_eq!(s.word_spacing, 0.0);
    assert_eq!(s.letter_spacing, 0.0);
    assert_eq!(s.text_indent, 0.0);
    assert!(!s.justify);
    assert!(!s.hyphenation);
    assert_eq!(s.font_size, 17.0);
    assert_eq!(s.font_weight, 400);
    assert_eq!(s.default_font, FontChoice::Default);
    assert_eq!(s.column_align, TextColumnAlign::Center);
    assert_eq!(s.ink_contrast, 100.0);
}

#[test]
fn an_empty_blob_loads_as_the_defaults() {
    let s: TextSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(s, TextSettings::default());
}

#[test]
fn font_choices_round_trip_through_storage() {
    for choice in [
        FontChoice::Default,
        FontChoice::System(SystemFont::Georgia),
        FontChoice::System(SystemFont::TrebuchetMs),
        FontChoice::BuiltIn("reader-serif".into()),
    ] {
        let stored = choice.to_storage();
        assert_eq!(FontChoice::from_storage(&stored), choice, "{stored}");
    }
    // Unknown ids fall back to Default rather than failing the blob.
    assert_eq!(FontChoice::from_storage("system:nope"), FontChoice::Default);
    assert_eq!(FontChoice::from_storage("builtin:"), FontChoice::Default);
    assert_eq!(FontChoice::from_storage("junk"), FontChoice::Default);
}

#[test]
fn settings_round_trip_with_fonts() {
    let s = TextSettings {
        default_font: FontChoice::System(SystemFont::Baskerville),
        mono_font: FontChoice::System(SystemFont::Consolas),
        serif_font: FontChoice::BuiltIn("future".into()),
        ..Default::default()
    };
    let json = serde_json::to_string(&s).unwrap();
    let back: TextSettings = serde_json::from_str(&json).unwrap();
    assert_eq!(s, back);
    assert!(json.contains("\"default_font\":\"system:baskerville\""), "{json}");
    assert!(json.contains("\"mono_font\":\"system:consolas\""), "{json}");
    assert!(json.contains("\"serif_font\":\"builtin:future\""), "{json}");
}

#[test]
fn sanitize_clamps_everything_into_range() {
    let mut s = TextSettings {
        paragraph_margin: 9.0,
        line_height: 0.2,
        word_spacing: 99.0,
        letter_spacing: -5.0,
        text_indent: 40.0,
        font_size: 2.0,
        font_weight: 9999,
        ink_contrast: 500.0,
        ..TextSettings::default()
    };
    sanitize(&mut s);
    assert_eq!(s.paragraph_margin, 3.0);
    assert_eq!(s.line_height, 1.0);
    assert_eq!(s.word_spacing, 10.0);
    assert_eq!(s.letter_spacing, -0.02);
    assert_eq!(s.text_indent, 4.0);
    assert_eq!(s.font_size, 10.0);
    assert_eq!(s.font_weight, 900);
    assert_eq!(s.ink_contrast, 100.0);
}

#[test]
fn column_align_carries_labels_and_classes() {
    assert_eq!(TextColumnAlign::default(), TextColumnAlign::Center);
    let all = [TextColumnAlign::Left, TextColumnAlign::Center, TextColumnAlign::Right];
    // Every choice names itself, and positions with its own class.
    for (i, a) in all.iter().enumerate() {
        assert_eq!(a.label(), ["Left", "Center", "Right"][i]);
        for b in &all[i + 1..] {
            assert_ne!(a.container_class(), b.container_class());
        }
    }
}

#[test]
fn system_fonts_carry_consistent_metadata() {
    for f in SystemFont::all() {
        assert!(!f.id().is_empty());
        assert!(!f.label().is_empty());
        // Every stack ends in its family's generic keyword.
        assert!(
            f.stack().ends_with(f.family().generic()),
            "{:?}: {}",
            f,
            f.stack()
        );
        let w = f.avg_char_width();
        assert!(w > 0.4 && w < 0.7, "{w}");
    }
    // Ids are unique — the storage key depends on it.
    let all = SystemFont::all();
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            assert_ne!(a.id(), b.id());
        }
    }
}
