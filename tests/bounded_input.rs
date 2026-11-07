use rune::accesskit::{Action, ActionData, Role, TextPosition, TextSelection};
use rune::{Cx, El, Env, Ime, Key, KeyEvent, Mods, Runtime, TextEdit, col};
use rune_kit::Kit;
use rune_markdown_prototype::editor::source_input;
use rune_markdown_prototype::state::{EditingMode, State};

fn view(state: &State, _: &mut Cx<State>) -> El {
    col()
        .w_full()
        .child(source_input(state, &Kit::neutral(false)))
}

fn runtime(source: &str, offset: usize, width: f32) -> Runtime<State> {
    let mut state = State::new(source);
    state.set_mode(EditingMode::LivePreview);
    assert!(state.activate_block_at(offset));
    let mut rt = Runtime::new(state, view, (width, 400.0), Env::new(false, 1.0));
    rt.frame(0.0);
    rt
}

fn key(rt: &mut Runtime<State>, key: Key, mods: Mods) {
    let event = KeyEvent { key, mods };
    rt.key(event.clone());
    rt.key_up(event);
    rt.frame(0.0);
}

fn command(rt: &mut Runtime<State>, c: char) {
    key(
        rt,
        Key::Command(c),
        Mods {
            cmd: true,
            ..Mods::default()
        },
    );
}

fn displayed(rt: &Runtime<State>) -> String {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("markdown-input"))
        .unwrap()
        .text
        .unwrap()
}

#[test]
fn bounded_layout_caret_and_hit_testing_use_global_unicode_offsets() {
    let source = "hidden before\n\nԲարև café 👋🏽 tail\n\nhidden after";
    let start = source.find("Բարև").unwrap();
    let mut rt = runtime(source, start, 240.0);
    let range = rt.state.view().active_source_range().unwrap();
    assert_eq!(displayed(&rt), source[range]);
    assert!(rt.ime_area.unwrap()[0] < 1.0);
    for ch in ["Բ", "ա", "ր", "և", " ", "c", "a", "f", "é", " ", "👋🏽"] {
        let before = rt.state.editor.focus;
        key(&mut rt, Key::Right, Mods::default());
        assert_eq!(rt.state.editor.focus, before + ch.len());
        let [x, y, _, h] = rt.ime_area.unwrap();
        rt.pointer_down(x + 0.1, y + h * 0.5);
        rt.pointer_up(x + 0.1, y + h * 0.5);
        rt.frame(0.0);
        assert_eq!(rt.state.editor.focus, before + ch.len());
    }
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn wrapped_vertical_navigation_uses_the_bounded_layout() {
    let source =
        "prefix\n\none two three four five six seven eight nine ten eleven twelve\n\nsuffix";
    let start = source.find("one").unwrap();
    let mut rt = runtime(source, start, 100.0);
    let y = rt.ime_area.unwrap()[1];
    key(&mut rt, Key::Down, Mods::default());
    assert!(rt.state.editor.focus > start);
    assert!(rt.ime_area.unwrap()[1] > y);
    key(&mut rt, Key::Up, Mods::default());
    assert_eq!(rt.state.editor.focus, start);
}

#[test]
fn copy_cut_paste_and_undo_share_one_document_history() {
    let source = "before\n\ncafé and Բարև\n\nafter";
    let start = source.find("café").unwrap();
    let mut rt = runtime(source, start, 300.0);
    let range = rt.state.view().active_source_range().unwrap();
    command(&mut rt, 'a');
    assert_eq!(
        (rt.state.editor.anchor, rt.state.editor.focus),
        (range.start, range.end)
    );
    command(&mut rt, 'c');
    assert_eq!(
        rt.copy_request.take().as_deref(),
        Some(&source[range.clone()])
    );
    command(&mut rt, 'x');
    assert_eq!(
        rt.copy_request.take().as_deref(),
        Some(&source[range.clone()])
    );
    assert_eq!(
        rt.state.editor.text,
        format!("{}{}", &source[..range.start], &source[range.end..])
    );
    rt.paste("new\n\nsecond");
    rt.frame(0.0);
    assert_eq!(rt.state.editor.text, "before\n\nnew\n\nsecond\nafter");
    assert!(displayed(&rt).contains("new\n\nsecond"));
    command(&mut rt, 'z');
    command(&mut rt, 'z');
    assert_eq!(rt.state.editor.text, source);
    key(
        &mut rt,
        Key::Command('z'),
        Mods {
            cmd: true,
            shift: true,
            ..Mods::default()
        },
    );
    assert_eq!(
        rt.state.editor.text,
        format!("{}{}", &source[..range.start], &source[range.end..])
    );
    // Returning to the full source preserves the document history and global selection.
    rt.update(|s, _| s.set_mode(EditingMode::Source));
    rt.frame(0.0);
    assert_eq!(displayed(&rt), rt.state.editor.text);
    command(&mut rt, 'z');
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn composition_replaces_the_global_selection_and_reports_local_caret_geometry() {
    let source = "prefix\n\ncafé tail\n\nsuffix";
    let start = source.find("café").unwrap();
    let mut rt = runtime(source, start, 300.0);
    rt.update(|s, _| s.editor.set_selection(start, start + "café".len()));
    rt.frame(0.0);
    rt.ime(Ime::Preedit("日本".into(), Some(("日".len(), "日".len()))));
    rt.frame(0.0);
    assert_eq!(rt.state.editor.text, source);
    assert_eq!(displayed(&rt), "日本 tail\n");
    let first_caret = rt.ime_area.unwrap();
    rt.ime(Ime::Preedit(
        "日本".into(),
        Some(("日本".len(), "日本".len())),
    ));
    rt.frame(0.0);
    assert!(rt.ime_area.unwrap()[0] > first_caret[0]);
    let tree = rt.accessibility_tree();
    let field = tree
        .nodes
        .iter()
        .find(|(_, n)| n.author_id() == Some("markdown-input"))
        .unwrap();
    assert_eq!(field.1.value(), Some("café tail\n"));
    assert_eq!(field.1.text_selection().unwrap().focus.character_index, 2);
    rt.ime(Ime::Commit("日本".into()));
    rt.frame(0.0);
    assert_eq!(rt.state.editor.text, "prefix\n\n日本 tail\n\nsuffix");
    assert_eq!(rt.state.editor.focus, start + "日本".len());
    command(&mut rt, 'z');
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn accessibility_reads_and_edits_only_the_view_with_global_model_selection() {
    let source = "private prefix\n\nԲարև café\n\nprivate suffix";
    let start = source.find("Բարև").unwrap();
    let mut rt = runtime(source, start, 300.0);
    let tree = rt.accessibility_tree();
    let field = tree
        .nodes
        .iter()
        .find(|(_, n)| n.author_id() == Some("markdown-input"))
        .unwrap();
    assert_eq!(field.1.value(), Some("Բարև café\n"));
    let run = tree
        .nodes
        .iter()
        .find(|(_, n)| n.role() == Role::TextRun)
        .unwrap();
    let selection = TextSelection {
        anchor: TextPosition {
            node: run.0,
            character_index: 0,
        },
        focus: TextPosition {
            node: run.0,
            character_index: 4,
        },
    };
    assert!(rt.accessibility_action(
        "markdown-input",
        Action::SetTextSelection,
        Some(ActionData::SetTextSelection(selection))
    ));
    rt.frame(0.0);
    assert_eq!(
        (rt.state.editor.anchor, rt.state.editor.focus),
        (start, start + "Բարև".len())
    );
    assert!(rt.accessibility_action(
        "markdown-input",
        Action::ReplaceSelectedText,
        Some(ActionData::Value("hello".into()))
    ));
    rt.frame(0.0);
    assert_eq!(
        rt.state.editor.text,
        "private prefix\n\nhello café\n\nprivate suffix"
    );
    assert!(rt.accessibility_action(
        "markdown-input",
        Action::SetValue,
        Some(ActionData::Value("new".into()))
    ));
    rt.frame(0.0);
    assert_eq!(
        rt.state.editor.text,
        "private prefix\n\nnew\nprivate suffix"
    );
    command(&mut rt, 'z');
    command(&mut rt, 'z');
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn enter_and_paste_keep_the_entire_edited_region_open_across_block_splits() {
    let source = "# before\n\nalpha beta\n\n# after";
    let start = source.find("alpha").unwrap();
    let mut rt = runtime(source, start + "alpha".len(), 300.0);
    key(&mut rt, Key::Enter, Mods::default());
    assert_eq!(rt.state.editor.text, "# before\n\nalpha\n beta\n\n# after");
    rt.paste("\n\n# inserted\n\n");
    rt.frame(0.0);
    let range = rt.state.view().active_source_range().unwrap();
    assert_eq!(range.start, start);
    assert!(rt.state.editor.text[range.clone()].contains("alpha\n\n\n# inserted\n\n beta"));
    assert_eq!(displayed(&rt), rt.state.editor.text[range]);
    command(&mut rt, 'z');
    command(&mut rt, 'z');
    assert_eq!(rt.state.editor.text, source);
    assert!(rt.state.view().active_source_range().is_some());
}

#[test]
fn parser_merges_expand_the_open_range_in_both_directions() {
    for source in ["before\n\nbody\n\nafter", "before\n\nbody\n\n- next"] {
        let start = source.find("body").unwrap();
        let mut state = State::new(source);
        state.activate_block_at(start);
        // An edit can come through the full editor or an undo group outside a frame.
        state.editor.set_selection(start - 1, start);
        state.editor.insert("");
        state.sync_document();
        let range = state.view().active_source_range().unwrap();
        assert_eq!(range.start, 0);
        assert!(state.editor.text[range].starts_with("before\nbody"));
    }
    let source = "before\n\nbody\n\nafter";
    let start = source.find("body").unwrap();
    let mut state = State::new(source);
    state.activate_block_at(start);
    let end = state.view().active_source_range().unwrap().end;
    state.editor.set_selection(end, end + 1);
    state.editor.insert("");
    state.sync_document();
    let range = state.view().active_source_range().unwrap();
    assert_eq!(range, start..state.editor.text.len());
    assert_eq!(&state.editor.text[range], "body\nafter");
}

#[test]
fn leaving_or_blurring_closes_the_region_but_composition_keeps_it_open() {
    let source = "before\n\nbody\n\nafter";
    let start = source.find("body").unwrap();
    let mut rt = runtime(source, start, 300.0);
    rt.update(|s, _| s.editor.preedit = Some(("に".into(), None)));
    rt.frame(0.0);
    rt.update(|s, _| s.editor.set_caret(0));
    rt.frame(0.0);
    assert!(rt.state.view().active_source_range().is_some());
    rt.ime(Ime::Preedit(String::new(), None));
    rt.frame(0.0);
    assert_eq!(rt.state.view().active_source_range(), None);
    rt.update(|s, _| {
        s.activate_block_at(start);
    });
    rt.frame(0.0);
    key(&mut rt, Key::Left, Mods::default());
    assert_eq!(rt.state.editor.focus, start - 1);
    assert_eq!(rt.state.view().active_source_range(), None);
    rt.update(|s, _| {
        s.activate_block_at(start);
    });
    rt.frame(0.0);
    key(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.editor.focused);
    assert_eq!(rt.state.view().active_source_range(), None);
}

#[test]
fn blank_hidden_and_eof_ranges_remain_editable() {
    for source in ["", "\n\n", "[id]: /target\n", "before\n\n"] {
        let offset = source.len();
        let mut rt = runtime(source, offset, 300.0);
        assert_eq!(displayed(&rt), "");
        rt.paste("Բարև");
        rt.frame(0.0);
        assert_eq!(rt.state.editor.text, format!("{source}Բարև"));
        assert!(displayed(&rt).contains("Բարև"));
        command(&mut rt, 'z');
        assert_eq!(rt.state.editor.text, source);
    }
}

#[test]
fn rebase_handles_edits_before_inside_at_edges_and_over_the_whole_range() {
    for (before, after, range, expected) in [
        ("abcDEFghi", "xabcDEFghi", 3..6, 4..7),
        ("abcDEFghi", "abxDEFghi", 3..6, 3..6),
        ("abcDEFghi", "abcXDEFghi", 3..6, 3..7),
        ("abcDEFghi", "abcDEFXghi", 3..6, 3..7),
        ("abcDEFghi", "abcDXFghi", 3..6, 3..6),
        ("abcDEFghi", "abcghi", 3..6, 3..3),
        ("abcDEFghi", "aXi", 3..6, 1..2),
        ("café", "cafê", 3..5, 3..5),
        ("Բարև", "աԲարև", 0..0, 0..2),
        ("", "日本", 0..0, 0..6),
        ("abc", "abc", 1..2, 1..2),
    ] {
        let rebased = TextEdit::rebase_source_range(range, before, after);
        assert_eq!(rebased, expected, "{before:?} -> {after:?}");
        assert!(after.get(rebased).is_some());
    }
}

#[test]
fn repeated_text_is_rebased_from_the_actual_edit_and_undo_positions() {
    let source = "same\n\nsame\n\nsame\n";
    let mut rt = runtime(source, 0, 300.0);
    rt.paste("same\n\n");
    rt.frame(0.0);
    assert_eq!(rt.state.view().active_source_range(), Some(0..11));
    assert_eq!(displayed(&rt), "same\n\nsame\n");
    command(&mut rt, 'z');
    assert_eq!(rt.state.editor.text, source);
    assert_eq!(rt.state.view().active_source_range(), Some(0..5));
    key(
        &mut rt,
        Key::Command('z'),
        Mods {
            cmd: true,
            shift: true,
            ..Mods::default()
        },
    );
    assert_eq!(rt.state.view().active_source_range(), Some(0..11));
}

#[test]
fn pointer_drag_double_and_triple_click_stay_within_the_view() {
    let source = "before\n\nalpha beta gamma\n\nafter";
    let start = source.find("alpha").unwrap();
    let mut rt = runtime(source, start, 300.0);
    let style = rune::TextStyle::new(14.0, 400.0).mono().lh(22.0);
    let x = rune::text::advance("alpha ", style);
    rt.pointer_down(0.1, 11.0);
    rt.frame(0.0);
    rt.pointer_move(x, 11.0);
    rt.frame(0.0);
    rt.pointer_up(x, 11.0);
    rt.frame(0.0);
    assert_eq!(
        (rt.state.editor.anchor, rt.state.editor.focus),
        (start, start + 6)
    );
    // Leave the double-click interval, then start a word selection in "beta".
    rt.frame(1.0);
    for _ in 0..2 {
        rt.pointer_down(x + 3.0, 11.0);
        rt.pointer_up(x + 3.0, 11.0);
        rt.frame(0.0);
    }
    assert_eq!(rt.state.editor.selected(), "beta");
    rt.pointer_down(x + 3.0, 11.0);
    rt.pointer_up(x + 3.0, 11.0);
    rt.frame(0.0);
    assert_eq!(rt.state.editor.selected(), "alpha beta gamma\n");
    assert_eq!(
        rt.state.view().active_source_range(),
        Some(start..start + 17)
    );
}

#[test]
fn boundary_deletions_do_not_modify_hidden_content() {
    let source = "before\n\nbody\n\nafter";
    let start = source.find("body").unwrap();
    let mut rt = runtime(source, start, 300.0);
    for mods in [
        Mods::default(),
        Mods {
            alt: true,
            ..Mods::default()
        },
        Mods {
            cmd: true,
            ..Mods::default()
        },
    ] {
        key(&mut rt, Key::Backspace, mods);
        assert_eq!(rt.state.editor.text, source);
    }
    key(&mut rt, Key::End, Mods::default());
    let end = rt.state.editor.focus;
    key(&mut rt, Key::Delete, Mods::default());
    assert_eq!(rt.state.editor.text, source);
    assert_eq!(rt.state.editor.focus, end);
    command(&mut rt, 'z');
    assert_eq!(
        rt.state.editor.text, source,
        "boundary no-ops add no undo entries"
    );
}

#[test]
fn block_replacement_uses_the_bounded_shared_input_and_keeps_document_context() {
    fn inline_view(state: &State, _: &mut Cx<State>) -> El {
        let kit = Kit::neutral(false);
        let range = state.view().active_source_range().unwrap();
        rune_kit::Markdown::new(&state.editor.text)
            .replace_source_range(range, source_input(state, &kit))
            .build(&kit)
            .w_full()
    }
    let source = "# before\n\nbody\n\n[link][id]\n\n[id]: /target\n";
    let start = source.find("body").unwrap();
    let mut state = State::new(source);
    state.set_mode(EditingMode::LivePreview);
    state.activate_block_at(start);
    let mut rt = Runtime::new(state, inline_view, (400.0, 400.0), Env::new(false, 1.0));
    rt.frame(0.0);
    assert_eq!(displayed(&rt), "body\n");
    let field = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("markdown-input"))
        .unwrap();
    assert!(field.path.ends_with("source-editor/markdown-input"));
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("link"))
    );
    rt.paste("new\n\n");
    rt.frame(0.0);
    assert_eq!(
        rt.state.editor.text,
        "# before\n\nnew\n\nbody\n\n[link][id]\n\n[id]: /target\n"
    );
    assert_eq!(displayed(&rt), "new\n\nbody\n");
    command(&mut rt, 'z');
    assert_eq!(rt.state.editor.text, source);
    assert_eq!(displayed(&rt), "body\n");
}

#[test]
fn shift_drag_clamps_an_existing_global_anchor_to_the_view() {
    fn bounded(edit: &TextEdit, _: &mut Cx<TextEdit>) -> El {
        rune::text_input(edit, rune::TextStyle::new(14.0, 400.0).mono().lh(22.0))
            .key("bounded")
            .source_range(8..24)
            .on_edit(|edit: &mut TextEdit| edit)
    }
    let mut edit = TextEdit::new("before\n\nalpha beta gamma\n\nafter");
    edit.set_caret(0);
    edit.focused = true;
    let mut rt = Runtime::new(edit, bounded, (300.0, 100.0), Env::default());
    rt.frame(0.0);
    rt.physical_input(rune::PhysicalInput::Modifiers(Mods {
        shift: true,
        ..Mods::default()
    }));
    rt.pointer_down(40.0, 11.0);
    rt.frame(0.0);
    assert_eq!(rt.state.anchor, 8);
    rt.pointer_move(80.0, 11.0);
    rt.frame(0.0);
    assert_eq!(rt.state.anchor, 8);
    assert!((8..=24).contains(&rt.state.focus));
    assert_eq!(rt.state.input_source_range(), None);
    rt.pointer_up(80.0, 11.0);
    rt.physical_input(rune::PhysicalInput::Modifiers(Mods::default()));
}

#[test]
fn pointer_selection_at_a_clipped_pill_stays_in_the_view() {
    fn bounded(edit: &TextEdit, _: &mut Cx<TextEdit>) -> El {
        rune::text_input(edit, rune::TextStyle::new(14.0, 400.0).mono().lh(22.0))
            .source_range(5..9)
            .on_edit(|edit: &mut TextEdit| edit)
    }
    let mut edit = TextEdit::from_parts(
        "prefix chip suffix",
        vec![rune::MarkRange {
            start: 0,
            end: 11,
            mark: rune::Mark::Pill("clip".into()),
        }],
        vec![rune::Block::Paragraph],
    );
    edit.focused = true;
    let mut rt = Runtime::new(edit, bounded, (300.0, 100.0), Env::default());
    rt.frame(0.0);
    rt.pointer_down(0.1, 11.0);
    rt.frame(0.0);
    assert!((5..=9).contains(&rt.state.focus));
    assert!((5..=9).contains(&rt.state.anchor));
    rt.pointer_up(0.1, 11.0);
    rt.paste("X");
    rt.frame(0.0);
    assert_eq!(rt.state.text, "prefiXx chip suffix");
    let undo = KeyEvent {
        key: Key::Command('z'),
        mods: Mods {
            cmd: true,
            ..Mods::default()
        },
    };
    rt.key(undo.clone());
    rt.key_up(undo);
    rt.frame(0.0);
    assert_eq!(rt.state.text, "prefix chip suffix");
    assert_eq!((rt.state.anchor, rt.state.focus), (5, 5));
    rt.pointer_down(0.1, 11.0);
    rt.frame(0.0);
    rt.pointer_move(200.0, 11.0);
    rt.frame(0.0);
    assert!((5..=9).contains(&rt.state.focus));
    assert!((5..=9).contains(&rt.state.anchor));
}

#[test]
fn formatting_undo_in_a_callback_preserves_the_input_range_and_global_selection() {
    type Model = (TextEdit, Vec<Option<std::ops::Range<usize>>>);
    fn bounded(state: &Model, _: &mut Cx<Model>) -> El {
        rune::text_input(&state.0, rune::TextStyle::new(14.0, 400.0))
            .source_range(7..11)
            .on_edit(|state: &mut Model| &mut state.0)
            .on_submit(|state: &mut Model, _| {
                let e = &mut state.0;
                e.set_selection(0, e.text.len());
                e.toggle_mark(rune::Mark::Bold);
                state.1.push(e.input_source_range());
                e.undo();
                state.1.push(e.input_source_range());
                e.redo();
                state.1.push(e.input_source_range());
            })
    }
    let mut e = TextEdit::new("prefix body suffix").rich();
    e.set_caret(7);
    e.focused = true;
    let mut rt = Runtime::new((e, Vec::new()), bounded, (300.0, 100.0), Env::default());
    rt.frame(0.0);
    let enter = KeyEvent {
        key: Key::Enter,
        mods: Mods::default(),
    };
    rt.key(enter.clone());
    rt.key_up(enter);
    rt.frame(0.0);
    assert_eq!(rt.state.1, vec![Some(7..11); 3]);
    assert_eq!(rt.state.0.text, "prefix body suffix");
    assert_eq!(
        (rt.state.0.anchor, rt.state.0.focus),
        (0, rt.state.0.text.len())
    );
    assert_eq!(rt.state.0.input_source_range(), None);
}
