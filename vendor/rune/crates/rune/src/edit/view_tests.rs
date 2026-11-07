use super::*;
use crate::{Span, TextStyle, text_input};

#[test]
fn local_display_and_global_model_roundtrip_with_clipping() {
    let mut e = TextEdit::new("before\nԲարև café 👋🏽\nafter");
    let start = e.text.find("Բարև").unwrap();
    let end = e.text.find("\nafter").unwrap();
    e.set_input_range(Some(start..end));
    let shown = e.display().0;
    assert_eq!(shown, "Բարև café 👋🏽");
    for (i, _) in shown.char_indices().chain([(shown.len(), '\0')]) {
        assert_eq!(e.model_index(i), start + i);
        assert_eq!(e.display_index(start + i), i);
    }
    assert_eq!(e.model_index(0), start);
    assert_eq!(e.model_index(usize::MAX), end);
    assert_eq!(e.display_selection_range(0, e.text.len()), (0, end - start));
    assert_eq!(e.map_range_to_display(0, e.text.len()), [(0, (end - start) as u32)]);
    assert!(e.map_range_to_display(0, start).is_empty());
    assert!(e.map_range_to_display(end, e.text.len()).is_empty());
}

#[test]
fn composition_maps_marks_on_both_sides_and_leaves_preedit_uncommitted() {
    let mut e = TextEdit::new("before\ncafé tail\nafter");
    let start = e.text.find("café").unwrap();
    let end = e.text.find("\nafter").unwrap();
    e.set_input_range(Some(start..end));
    e.set_selection(start, start + "café".len());
    e.ime(&Ime::Preedit("日本\n語".into(), Some((0, 3))));
    assert_eq!(e.display(), ("日本\n語 tail".into(), Some((0, "日本\n語".len()))));
    assert_eq!(e.model_index(1), start);
    assert_eq!(e.model_index("日本\n語".len()), start + "café".len());
    assert_eq!(e.display_index(end), "日本\n語 tail".len());
    assert_eq!(e.map_range_to_display(start, end), [("日本\n語".len() as u32, "日本\n語 tail".len() as u32)]);
    e.ime(&Ime::Preedit(String::new(), None));
    assert_eq!(e.display().0, "café tail");
    assert_eq!(e.text, "before\ncafé tail\nafter");
}

#[test]
fn rich_blocks_decorations_pills_and_soft_breaks_use_local_layout_offsets() {
    let mut e = TextEdit::from_parts(
        "head\ncode\nchip\nend",
        vec![
            MarkRange { start: 0, end: 4, mark: Mark::Bold },
            MarkRange { start: 10, end: 14, mark: Mark::Pill("id".into()) },
        ],
        vec![Block::Paragraph, Block::Code, Block::Paragraph, Block::Quote],
    )
    .with_soft_breaks(vec![2]);
    let style = TextStyle::new(14.0, 400.0);
    let field = text_input(&e, style).source_range(5..14).spans(vec![Span {
        start: 5,
        end: 9,
        color: Some(Color::WHITE),
        ..Span::default()
    }]);
    let spec = field.text.unwrap();
    assert_eq!(spec.content, "code\nchip");
    assert_eq!(spec.blocks, [Block::Code, Block::Paragraph]);
    assert_eq!(spec.soft_breaks, [1]);
    assert_eq!((spec.pills[0].start, spec.pills[0].end), (5, 9));
    assert!(spec.spans.iter().any(|s| s.start == 0 && s.end == 4 && s.mono && s.color == Some(Color::WHITE)));
    e.set_input_range(Some(5..14));
    e.set_caret(7);
    e.ime(&Ime::Preedit("x\ny".into(), None));
    let field = text_input(&e, style).source_range(5..14);
    let spec = field.text.unwrap();
    assert_eq!(spec.content, "cox\nyde\nchip");
    assert_eq!(spec.blocks, [Block::Code, Block::Code, Block::Paragraph]);
    assert_eq!(spec.soft_breaks, [2]);
    assert_eq!((spec.pills[0].start, spec.pills[0].end), (8, 12));
}

#[test]
fn endpoints_clamp_to_valid_utf8_and_empty_views_accept_edits() {
    let mut e = TextEdit::new("aԲc");
    e.set_input_range(Some(2..usize::MAX));
    assert_eq!(e.input_bounds(), 1..4);
    assert_eq!(e.display().0, "Բc");
    e.set_input_range(Some(3..1));
    assert_eq!(e.display().0, "");
    e.set_caret(3);
    e.paste("日本");
    assert_eq!(e.text, "aԲ日本c");
    assert_eq!(e.input_bounds(), 3..9);
    assert_eq!(e.display().0, "日本");
    assert_eq!(e.focus, 9);
    e.undo();
    assert_eq!(e.input_bounds(), 3..3);
    assert_eq!(e.text, "aԲc");
}

#[test]
fn browser_ime_mapping_stays_bounded_with_global_positions() {
    let mut e = TextEdit::new("prefix\nbody\nsuffix");
    e.set_input_range(Some(7..11));
    e.set_selection(0, e.text.len());
    e.paste("日本");
    assert_eq!(e.text, "prefix\n日本\nsuffix");
    assert_eq!(e.focus, 7 + "日本".len());
    assert_eq!(e.input_bounds(), 7..13);
    assert_eq!(e.display().0, "日本");
    e.undo();
    assert_eq!(e.text, "prefix\nbody\nsuffix");
    assert_eq!((e.anchor, e.focus), (0, e.text.len()), "undo keeps global snapshot selection");
}

#[test]
fn bounded_secure_edits_keep_masking_and_do_not_touch_adjoining_content() {
    for revealed in [false, true] {
        let mut e = TextEdit::new("before\nsecret\nafter").secure().revealed(revealed);
        e.set_single_line(true);
        e.set_input_range(Some(7..13));
        e.set_caret(7);
        e.key(&KeyEvent { key: Key::Backspace, mods: Mods::default() }, None);
        assert_eq!(e.text, "before\nsecret\nafter");
        e.paste("x");
        assert_eq!(e.text, "before\nxsecret\nafter");
        assert_eq!(e.input_bounds(), 7..14);
        assert_eq!(e.display().0, if revealed { "xsecret" } else { "•••••••" });
        e.undo();
        assert_eq!(e.text, "before\nxsecret\nafter", "secure fields still have no undo");
    }
}

#[test]
fn formatting_undo_does_not_rebase_an_unchanged_buffer() {
    let mut e = TextEdit::new("prefix body suffix").rich();
    e.set_input_range(Some(7..11));
    e.set_selection(0, e.text.len());
    e.toggle_mark(Mark::Bold);
    assert_eq!(e.input_bounds(), 7..11);
    e.undo();
    assert_eq!(e.input_bounds(), 7..11);
    assert_eq!((e.anchor, e.focus), (0, e.text.len()));
    e.redo();
    assert_eq!(e.input_bounds(), 7..11);
    assert_eq!(e.text, "prefix body suffix");
}

#[test]
fn editing_a_clipped_pill_preserves_the_global_caret_in_history() {
    let mut e = TextEdit::from_parts(
        "prefix chip suffix",
        vec![MarkRange { start: 0, end: 11, mark: Mark::Pill("clip".into()) }],
        vec![Block::Paragraph],
    );
    e.set_input_range(Some(5..9));
    e.set_view_selection(5, 5, e.input_bounds());
    e.key(&KeyEvent { key: Key::Text("X".into()), mods: Mods::default() }, None);
    assert_eq!(e.text, "prefiXx chip suffix");
    assert_eq!(e.input_bounds(), 5..10);
    e.undo();
    assert_eq!(e.text, "prefix chip suffix");
    assert_eq!(e.input_bounds(), 5..9);
    assert_eq!((e.anchor, e.focus), (5, 5));
}
