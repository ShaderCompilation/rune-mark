//! Live Preview interactions through the same document view and editor binding as the window.
use rune::runtime::ElementInfo;
use rune::{Cx, El, Env, Ime, Key, KeyEvent, Mods, Runtime, col};
use rune_kit::Kit;
use rune_markdown_prototype::editor::document;
use rune_markdown_prototype::state::{EditingMode, State};

fn view(state: &State, _: &mut Cx<State>) -> El {
    col()
        .w_full()
        .padding(24.0)
        .child(document(state, &Kit::neutral(false)))
}

/// A Live Preview runtime with the caret at `caret`, or nothing active when `None`.
fn runtime(source: &str, caret: Option<usize>) -> Runtime<State> {
    let mut state = State::new(source);
    state.set_mode(EditingMode::LivePreview);
    match caret {
        Some(offset) => assert!(state.activate_block_at(offset)),
        None => state.editor.focused = false,
    }
    let mut rt = Runtime::new(state, view, (520.0, 900.0), Env::new(false, 1.0));
    rt.frame(0.0);
    rt
}

fn press(rt: &mut Runtime<State>, key: Key, mods: Mods) {
    let event = KeyEvent { key, mods };
    rt.key(event.clone());
    rt.key_up(event);
    rt.frame(0.0);
}

fn plain(rt: &mut Runtime<State>, key: Key) {
    press(rt, key, Mods::default());
}

fn shift(rt: &mut Runtime<State>, key: Key) {
    press(
        rt,
        key,
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
}

fn command(rt: &mut Runtime<State>, c: char, shift: bool) {
    press(
        rt,
        Key::Command(c),
        Mods {
            cmd: true,
            shift,
            ..Mods::default()
        },
    );
}

fn input(rt: &Runtime<State>) -> Option<ElementInfo> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("markdown-input") && !e.exiting)
}

fn displayed(rt: &Runtime<State>) -> String {
    input(rt).expect("source input").text.unwrap()
}

fn text_element(rt: &Runtime<State>, value: &str) -> ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.text.as_deref() == Some(value) && !e.exiting)
        .unwrap_or_else(|| panic!("missing rendered text {value:?}"))
}

fn has_text(rt: &Runtime<State>, value: &str) -> bool {
    rt.elements()
        .iter()
        .any(|e| e.text.as_deref() == Some(value) && !e.exiting)
}

fn click(rt: &mut Runtime<State>, x: f32, y: f32) {
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
}

fn selection(rt: &Runtime<State>) -> (usize, usize) {
    (rt.state.editor.anchor, rt.state.editor.focus)
}

fn active(rt: &Runtime<State>) -> Option<&str> {
    let range = rt.state.view().active_source_range()?;
    Some(&rt.state.editor.text[range])
}

#[test]
fn clicking_rendered_text_reveals_its_markdown_and_focuses_the_mapped_position() {
    let source = "# Title\n\nSome **bold** text\n\nlast";
    let mut rt = runtime(source, None);
    assert!(input(&rt).is_none(), "nothing is active before a click");

    let paragraph = text_element(&rt, "Some bold text");
    click(&mut rt, paragraph.rect[0] + 0.5, paragraph.rect[1] + 8.0);
    let start = source.find("Some").unwrap();
    assert_eq!(active(&rt), Some("Some **bold** text\n"));
    assert_eq!(selection(&rt), (start, start));
    // The caret does not touch the bold construct, so its delimiters stay hidden.
    assert_eq!(displayed(&rt), "Some bold text");
    assert!(input(&rt).unwrap().focused);
    assert!(rt.state.editor.focused);
    assert!(has_text(&rt, "Title") && has_text(&rt, "last"));
    assert!(
        !rt.elements()
            .into_iter()
            .any(|e| e.text.as_deref() == Some("Some bold text")
                && e.key.as_deref() != Some("markdown-input")
                && !e.exiting),
        "the rendered paragraph is replaced by the input"
    );

    // The heading's first rendered character maps past its marker.
    let heading = text_element(&rt, "Title");
    click(&mut rt, heading.rect[0] + 0.5, heading.rect[1] + 8.0);
    assert_eq!(active(&rt), Some("# Title\n"));
    assert_eq!(selection(&rt), (2, 2));
    assert_eq!(displayed(&rt), "# Title");
    assert!(input(&rt).unwrap().focused);
    assert!(has_text(&rt, "Some bold text"));
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn plain_clicks_on_links_and_task_markers_activate_editing() {
    let source = "[site](https://example.com) here\n\n- [ ] task\n";
    let mut rt = runtime(source, None);
    let paragraph = text_element(&rt, "site here");
    click(&mut rt, paragraph.rect[0] + 4.0, paragraph.rect[1] + 8.0);
    assert_eq!(active(&rt), Some("[site](https://example.com) here\n"));
    assert!(
        (1..=5).contains(&rt.state.editor.focus),
        "inside the link text"
    );
    assert!(input(&rt).unwrap().focused);

    let control = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("control"))
        .unwrap();
    click(
        &mut rt,
        control.rect[0] + control.rect[2] * 0.5,
        control.rect[1] + control.rect[3] * 0.5,
    );
    let list = source.find("- [ ]").unwrap();
    assert_eq!(active(&rt), Some("- [ ] task\n"));
    assert_eq!(selection(&rt), (list, list));
    assert_eq!(rt.state.editor.text, source, "the task is not toggled");
}

#[test]
fn typing_enter_paste_and_deletion_update_the_shared_buffer_immediately() {
    let source = "# Title\n\nalpha\n\nomega";
    let end = source.find("alpha").unwrap() + "alpha".len();
    let mut rt = runtime(source, Some(end));
    plain(&mut rt, Key::Text("!".into()));
    assert_eq!(rt.state.editor.text, "# Title\n\nalpha!\n\nomega");
    plain(&mut rt, Key::Enter);
    assert_eq!(rt.state.editor.text, "# Title\n\nalpha!\n\n\nomega");
    // The new line belongs to the next region; the edited block renders again.
    assert_eq!(active(&rt), Some("\n"));
    assert!(has_text(&rt, "alpha!"));
    plain(&mut rt, Key::Text("beta".into()));
    assert_eq!(rt.state.editor.text, "# Title\n\nalpha!\nbeta\n\nomega");
    assert_eq!(displayed(&rt), "alpha!\nbeta");
    rt.paste("\n\n## Pasted");
    rt.frame(0.0);
    assert_eq!(
        rt.state.editor.text,
        "# Title\n\nalpha!\nbeta\n\n## Pasted\n\nomega"
    );
    assert_eq!(displayed(&rt), "alpha!\nbeta\n\n## Pasted");
    plain(&mut rt, Key::Backspace);
    assert_eq!(
        rt.state.editor.text,
        "# Title\n\nalpha!\nbeta\n\n## Paste\n\nomega"
    );
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    assert!(has_text(&rt, "Title") && has_text(&rt, "omega"));
    for _ in 0..8 {
        command(&mut rt, 'z', false);
    }
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn arrows_past_the_active_block_activate_the_adjacent_block_and_keep_the_column() {
    let source = "# Heading\nparagraph text\n\n- item";
    let paragraph = source.find("paragraph").unwrap();
    let mut rt = runtime(source, Some(paragraph + 4));
    assert_eq!(displayed(&rt), "paragraph text");

    plain(&mut rt, Key::Up);
    assert_eq!(active(&rt), Some("# Heading\n"));
    assert_eq!(selection(&rt), (4, 4));
    assert_eq!(displayed(&rt), "# Heading");
    assert!(input(&rt).unwrap().focused);

    plain(&mut rt, Key::Down);
    assert_eq!(active(&rt), Some("paragraph text\n"));
    assert_eq!(selection(&rt), (paragraph + 4, paragraph + 4));

    plain(&mut rt, Key::End);
    plain(&mut rt, Key::Right);
    let gap = source.find("\n\n").unwrap() + 1;
    assert_eq!(selection(&rt), (gap, gap));
    assert_eq!(active(&rt), Some("\n"));
    assert_eq!(displayed(&rt), "");
    plain(&mut rt, Key::Down);
    assert_eq!(active(&rt), Some("- item"));
    plain(&mut rt, Key::Left);
    plain(&mut rt, Key::Left);
    assert_eq!(active(&rt), Some("paragraph text\n"));
    assert_eq!(rt.state.editor.focus, gap - 1);

    // Selection inside the block stays in Live Preview.
    shift(&mut rt, Key::Left);
    assert_eq!(selection(&rt), (gap - 1, gap - 2));
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn shift_navigation_beyond_the_block_switches_to_source_before_extending() {
    let source = "# Heading\nparagraph text\n\n- item";
    let paragraph = source.find("paragraph").unwrap();
    let mut rt = runtime(source, Some(paragraph + 4));
    shift(&mut rt, Key::Up);
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert_eq!(rt.state.view().active_source_range(), None);
    // The extension follows the whole document's layout, as in Source mode.
    assert_eq!(selection(&rt), (paragraph + 4, 4));
    assert_eq!(displayed(&rt), source);
    assert!(input(&rt).unwrap().focused);
    shift(&mut rt, Key::Down);
    assert_eq!(selection(&rt), (paragraph + 4, paragraph + 4));
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn select_all_switches_to_source_and_selects_the_document() {
    let source = "# Heading\n\nbody\n\ntail";
    let mut rt = runtime(source, Some(source.find("body").unwrap()));
    command(&mut rt, 'a', false);
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert_eq!(selection(&rt), (0, source.len()));
    assert_eq!(displayed(&rt), source);
    assert!(input(&rt).unwrap().focused);
    plain(&mut rt, Key::Backspace);
    assert!(rt.state.editor.text.is_empty());
    command(&mut rt, 'z', false);
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn deletions_across_a_block_boundary_switch_to_source_and_apply_once() {
    let source = "alpha\n\nomega";
    let omega = source.find("omega").unwrap();
    let mut rt = runtime(source, Some(omega));
    plain(&mut rt, Key::Backspace);
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert_eq!(rt.state.editor.text, "alpha\nomega");
    assert_eq!(selection(&rt), (omega - 1, omega - 1));
    assert!(input(&rt).unwrap().focused);
    command(&mut rt, 'z', false);
    assert_eq!(rt.state.editor.text, source);

    // Delete at the end of a block's text would remove its line break.
    let mut rt = runtime(source, Some("alpha".len()));
    plain(&mut rt, Key::Delete);
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert_eq!(rt.state.editor.text, "alpha\nomega");
    command(&mut rt, 'z', false);
    assert_eq!(rt.state.editor.text, source);

    // Word deletion inside the block stays in Live Preview.
    let mut rt = runtime(source, Some(omega + "omega".len()));
    press(
        &mut rt,
        Key::Backspace,
        Mods {
            alt: true,
            ..Mods::default()
        },
    );
    assert_eq!(rt.state.editor.text, "alpha\n\n");
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
}

#[test]
fn undo_and_redo_are_document_wide_and_leave_live_preview_only_outside_the_block() {
    let source = "first\n\nsecond";
    let second = source.find("second").unwrap();
    let mut rt = runtime(source, Some("first".len()));
    rt.paste("X");
    rt.frame(0.0);
    // "X" shifted the second block by one byte; put the caret after its "s".
    rt.update(|s, _| {
        s.activate_block_at(second + 2);
    });
    rt.frame(0.0);
    rt.paste("Y");
    rt.frame(0.0);
    assert_eq!(rt.state.editor.text, "firstX\n\nsYecond");

    command(&mut rt, 'z', false);
    assert_eq!(rt.state.editor.text, "firstX\n\nsecond");
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    assert_eq!(active(&rt), Some("second"));

    command(&mut rt, 'z', false);
    assert_eq!(rt.state.editor.text, source);
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert!(input(&rt).unwrap().focused);

    command(&mut rt, 'z', true);
    command(&mut rt, 'z', true);
    assert_eq!(rt.state.editor.text, "firstX\n\nsYecond");
}

#[test]
fn composition_defers_block_and_mode_changes_until_commit_or_cancel() {
    let source = "first\n\nsecond";
    let mut rt = runtime(source, Some(0));
    rt.ime(Ime::Preedit("に".into(), Some(("に".len(), "に".len()))));
    rt.frame(0.0);
    let second = text_element(&rt, "second");
    click(&mut rt, second.rect[0] + 0.5, second.rect[1] + 8.0);
    assert_eq!(active(&rt), Some("first\n"));
    assert_eq!(selection(&rt), (0, 0));
    // Keys that would leave the block stay bounded while composing.
    plain(&mut rt, Key::Up);
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);

    rt.ime(Ime::Commit("に".into()));
    rt.frame(0.0);
    assert_eq!(rt.state.editor.text, "にfirst\n\nsecond");
    let start = rt.state.editor.text.find("second").unwrap();
    assert_eq!(active(&rt), Some("second"));
    assert_eq!(selection(&rt), (start, start));

    // A cancelled composition applies the deferred click too.
    rt.ime(Ime::Preedit("か".into(), None));
    rt.frame(0.0);
    let first = text_element(&rt, "にfirst");
    click(&mut rt, first.rect[0] + 0.5, first.rect[1] + 8.0);
    assert_eq!(active(&rt), Some("second"));
    rt.ime(Ime::Preedit(String::new(), None));
    rt.frame(0.0);
    assert_eq!(active(&rt), Some("にfirst\n"));
    assert_eq!(selection(&rt), (0, 0));
    assert_eq!(rt.state.editor.text, "にfirst\n\nsecond");
}
