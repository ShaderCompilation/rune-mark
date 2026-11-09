//! The window's view toggle, its shortcut, Select All and scroll stability, through the same
//! view and window key handler as the app.
use rune::runtime::ElementInfo;
use rune::{Env, Key, KeyEvent, Mods, Runtime};
use rune_markdown_prototype::app::{view, window_key};
use rune_markdown_prototype::state::{EditingMode, SourceLayout, State};

const SIZE: (f32, f32) = (640.0, 600.0);

fn start(state: State) -> Runtime<State> {
    let mut rt = Runtime::new(state, view, SIZE, Env::new(false, 1.0));
    rt.on_key = Some(window_key);
    rt.frame(0.0);
    rt
}

/// Live Preview with the caret at `caret`, or nothing open when `None`.
fn live(source: &str, caret: Option<usize>) -> Runtime<State> {
    let mut state = State::new(source);
    match caret {
        Some(at) => assert!(state.activate_block_at(at)),
        None => state.editor.focused = false,
    }
    start(state)
}

fn press(rt: &mut Runtime<State>, key: Key, mods: Mods) {
    let event = KeyEvent { key, mods };
    rt.key(event.clone());
    rt.key_up(event);
    rt.frame(0.0);
}

fn command(rt: &mut Runtime<State>, c: char) {
    let mods = Mods {
        cmd: true,
        ..Mods::default()
    };
    press(rt, Key::Command(c), mods);
}

fn element(rt: &Runtime<State>, key: &str) -> Option<ElementInfo> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
}

fn input(rt: &Runtime<State>) -> ElementInfo {
    element(rt, "markdown-input").expect("source input")
}

fn text_element(rt: &Runtime<State>, value: &str) -> ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.text.as_deref() == Some(value) && !e.exiting)
        .unwrap_or_else(|| panic!("missing rendered text {value:?}"))
}

fn click(rt: &mut Runtime<State>, x: f32, y: f32) {
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
}

fn click_key(rt: &mut Runtime<State>, key: &str) {
    let e = element(rt, key).unwrap_or_else(|| panic!("missing {key}"));
    click(rt, e.rect[0] + e.rect[2] * 0.5, e.rect[1] + e.rect[3] * 0.5);
}

fn selection(rt: &Runtime<State>) -> (usize, usize) {
    (rt.state.editor.anchor, rt.state.editor.focus)
}

fn active(rt: &Runtime<State>) -> Option<&str> {
    let range = rt.state.view().active_source_range()?;
    Some(&rt.state.editor.text[range])
}

fn caret_mid(rt: &Runtime<State>) -> f32 {
    let caret = input(rt).caret.expect("a focused caret");
    caret[1] + caret[3] * 0.5
}

/// A rendered paragraph drawn around the middle of the window, and its source.
fn middle_paragraph(rt: &Runtime<State>) -> (ElementInfo, String) {
    let e = rt
        .elements()
        .into_iter()
        .filter(|e| {
            !e.exiting
                && e.text
                    .as_deref()
                    .is_some_and(|t| t.starts_with("Paragraph "))
        })
        .min_by(|a, b| {
            let d = |e: &ElementInfo| (e.rect[1] - SIZE.1 * 0.5).abs();
            d(a).total_cmp(&d(b))
        })
        .expect("a paragraph on screen");
    let n: String = e.text.as_deref().unwrap()["Paragraph ".len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let source = format!("Paragraph {n} has **bold** and *emphasis* in it.\n");
    (e, source)
}

fn no_exits(rt: &Runtime<State>) -> bool {
    rt.elements().iter().all(|e| !e.exiting)
}

/// Paragraphs long enough to scroll, with a tall list near the top.
fn long_document() -> String {
    let mut s = String::from("# Notes\n\n");
    s += &(1..=8)
        .map(|i| format!("- list item {i}\n"))
        .collect::<String>();
    for i in 1..=40 {
        s += &format!("\nParagraph {i} has **bold** and *emphasis* in it.\n");
    }
    s
}

#[test]
fn opens_in_live_preview_with_a_visible_toggle() {
    let rt = live("# Title\n\nbody", None);
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    for key in ["view-live-preview", "view-source", "view-split"] {
        assert!(element(&rt, key).is_some(), "missing {key}");
    }
    assert!(element(&rt, "document-scroll").is_some());
    assert!(element(&rt, "split").is_none());
    let a11y = rt.accessibility_json().to_string();
    assert!(a11y.contains("Live Preview") && a11y.contains("radio"));
    assert!(text_element(&rt, "Title").rect[3] > 0.0);
}

#[test]
fn the_shortcut_toggles_modes_and_keeps_text_caret_focus_and_history() {
    let source = "# Title\n\nalpha\n\nomega";
    let end = source.find("alpha").unwrap() + "alpha".len();
    let mut rt = live(source, Some(end));
    press(&mut rt, Key::Text("!".into()), Mods::default());
    let edited = "# Title\n\nalpha!\n\nomega";
    assert_eq!(rt.state.editor.text, edited);

    command(&mut rt, 'e');
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert_eq!(rt.state.editor.text, edited);
    assert_eq!(selection(&rt), (end + 1, end + 1));
    assert_eq!(input(&rt).text.as_deref(), Some(edited));
    assert!(input(&rt).focused && rt.state.editor.focused);
    assert!(no_exits(&rt), "the switch lands without transitions");

    command(&mut rt, 'e');
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    assert_eq!(active(&rt), Some("alpha!\n"));
    assert_eq!(selection(&rt), (end + 1, end + 1));
    assert!(input(&rt).focused);
    assert!(no_exits(&rt));

    // History survives both switches; typing still goes to the shared editor.
    command(&mut rt, 'z');
    assert_eq!(rt.state.editor.text, source);
    press(
        &mut rt,
        Key::Command('z'),
        Mods {
            cmd: true,
            shift: true,
            ..Mods::default()
        },
    );
    assert_eq!(rt.state.editor.text, edited);
    press(&mut rt, Key::Text("?".into()), Mods::default());
    assert_eq!(rt.state.editor.text, "# Title\n\nalpha!?\n\nomega");
}

#[test]
fn the_shortcut_works_while_nothing_is_focused() {
    let mut rt = live("# Title\n\nbody", None);
    command(&mut rt, 'e');
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert!(!rt.state.editor.focused, "the switch doesn't take focus");
    command(&mut rt, 'e');
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    assert_eq!(active(&rt), None);
}

#[test]
fn returning_to_live_preview_collapses_a_selection_across_blocks_to_its_focus() {
    let source = "first\n\nsecond\n\nthird";
    let mut state = State::new(source);
    state.set_mode(EditingMode::Source);
    let focus = source.find("ird").unwrap();
    state.editor.anchor = 1;
    state.editor.focus = focus;
    let mut rt = start(state);
    command(&mut rt, 'e');
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    assert_eq!(selection(&rt), (focus, focus));
    assert_eq!(active(&rt), Some("third"));

    // A selection inside one block stays as it is.
    command(&mut rt, 'e');
    rt.update(|s, _| {
        s.editor.anchor = 7;
        s.editor.focus = 10;
    });
    rt.frame(0.0);
    command(&mut rt, 'e');
    assert_eq!(selection(&rt), (7, 10));
    assert_eq!(active(&rt), Some("second\n"));
}

#[test]
fn select_all_without_an_open_block_selects_the_document_in_source() {
    let source = "# Title\n\nbody";
    let mut rt = live(source, None);
    command(&mut rt, 'a');
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert_eq!(selection(&rt), (0, source.len()));
    assert!(input(&rt).focused);
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn toggle_segments_switch_views_and_keep_the_editor_focused() {
    let source = "# Title\n\nalpha\n\nomega";
    let at = source.find("pha").unwrap();
    let mut rt = live(source, Some(at));

    click_key(&mut rt, "view-split");
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert_eq!(rt.state.view().source_layout(), SourceLayout::Split);
    let field = input(&rt);
    let preview = element(&rt, "rendered-markdown").expect("preview");
    assert!(
        field.rect[0] + field.rect[2] <= preview.rect[0],
        "source on the left"
    );
    assert!(field.focused);
    assert_eq!(selection(&rt), (at, at));

    // The shortcut returns to Live Preview and back to the layout last chosen.
    command(&mut rt, 'e');
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    command(&mut rt, 'e');
    assert_eq!(rt.state.view().source_layout(), SourceLayout::Split);
    assert!(element(&rt, "split").is_some());

    click_key(&mut rt, "view-source");
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert_eq!(rt.state.view().source_layout(), SourceLayout::Full);
    assert!(element(&rt, "split").is_none());
    assert!(element(&rt, "rendered-markdown").is_none());
    assert!(input(&rt).focused);

    click_key(&mut rt, "view-live-preview");
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    assert_eq!(active(&rt), Some("alpha\n"));
    assert_eq!(selection(&rt), (at, at));
    assert!(input(&rt).focused);
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn clicking_a_block_keeps_the_caret_under_the_pointer() {
    let source = long_document();
    // Open the tall list, then scroll it off the top so its collapse happens above the view.
    let mut rt = live(&source, Some(source.find("list item 3").unwrap()));
    rt.wheel(300.0, 300.0, -500.0);
    rt.frame(0.0);
    let (target, paragraph) = middle_paragraph(&rt);
    let (x, y) = (target.rect[0] + 2.0, target.rect[1] + target.rect[3] * 0.5);
    click(&mut rt, x, y);
    assert_eq!(active(&rt), Some(paragraph.as_str()));
    assert!(
        (caret_mid(&rt) - y).abs() < 4.0,
        "caret at {} for a click at {y}",
        caret_mid(&rt)
    );
    assert!(no_exits(&rt));
}

#[test]
fn switching_modes_and_moving_between_blocks_keep_the_caret_in_place() {
    let source = long_document();
    let mut rt = live(&source, None);
    rt.wheel(300.0, 300.0, -400.0);
    rt.frame(0.0);
    let (target, paragraph) = middle_paragraph(&rt);
    click(&mut rt, target.rect[0] + 2.0, target.rect[1] + 8.0);
    assert_eq!(active(&rt), Some(paragraph.as_str()));
    let before = caret_mid(&rt);

    command(&mut rt, 'e');
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    assert!((caret_mid(&rt) - before).abs() < 1.0);
    command(&mut rt, 'e');
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    assert!((caret_mid(&rt) - before).abs() < 1.0);

    // Down leaves the paragraph through its blank line: the block behind renders again and
    // the caret holds its height as the next region opens.
    press(&mut rt, Key::Down, Mods::default());
    assert_ne!(active(&rt), Some(paragraph.as_str()));
    assert!((caret_mid(&rt) - before).abs() < 1.0);
    assert!(no_exits(&rt));
}
