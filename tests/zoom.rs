//! Window-wide zoom through the app's view and window key handler: shortcuts, the toolbar
//! control, and editing and clicking while zoomed.
use rune::runtime::ElementInfo;
use rune::{Env, Key, KeyEvent, Mods, Runtime};
use rune_markdown_prototype::app::{view, window_key};
use rune_markdown_prototype::state::{EditingMode, State};

const SIZE: (f32, f32) = (640.0, 600.0);

fn start(state: State) -> Runtime<State> {
    let mut rt = Runtime::new(state, view, SIZE, Env::new(false, 1.0));
    rt.on_key = Some(window_key);
    rt.frame(0.0);
    rt
}

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

fn command(rt: &mut Runtime<State>, c: char, shift: bool) {
    let mods = Mods {
        cmd: true,
        shift,
        ..Mods::default()
    };
    press(rt, Key::Command(c), mods);
}

fn element(rt: &Runtime<State>, key: &str) -> Option<ElementInfo> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
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

fn factor(rt: &Runtime<State>) -> f32 {
    rt.state.zoom().factor()
}

fn caret_mid(rt: &Runtime<State>) -> f32 {
    let caret = element(rt, "markdown-input")
        .expect("source input")
        .caret
        .expect("a focused caret");
    caret[1] + caret[3] * 0.5
}

fn active(rt: &Runtime<State>) -> Option<&str> {
    let range = rt.state.view().active_source_range()?;
    Some(&rt.state.editor.text[range])
}

fn long_document() -> String {
    let mut s = String::from("# Notes\n\n");
    for i in 1..=40 {
        s += &format!("\nParagraph {i} has **bold** and *emphasis* in it.\n");
    }
    s
}

/// The window's content fills the window at any zoom, and draws `k` times its layout size.
fn assert_fills_window(rt: &Runtime<State>) {
    let k = factor(rt);
    let root = element(rt, "scratchpad").expect("window content");
    for (got, want) in root.rect.iter().zip([0.0, 0.0, SIZE.0, SIZE.1]) {
        assert!((got - want).abs() < 1.0, "content drawn at {:?}", root.rect);
    }
    assert!(
        (root.scale - k).abs() < 1e-3,
        "scale {} at zoom {k}",
        root.scale
    );
}

#[test]
fn shortcuts_zoom_with_the_editor_focused_in_every_mode() {
    let source = "# Title\n\nalpha";
    let mut rt = live(source, Some(source.find("alpha").unwrap()));
    assert_eq!(factor(&rt), 1.0);
    let title = element(&rt, "toolbar").unwrap().rect[3];

    command(&mut rt, '=', false);
    assert_eq!(factor(&rt), 1.1);
    command(&mut rt, '+', true);
    assert_eq!(factor(&rt), 1.25);
    assert_fills_window(&rt);
    let zoomed = element(&rt, "toolbar").unwrap().rect[3];
    assert!(
        (zoomed - title * 1.25).abs() < 1.0,
        "toolbar {title} → {zoomed}"
    );
    // The editor keeps its text, caret and focus; nothing was typed into it.
    assert_eq!(rt.state.editor.text, source);
    assert!(rt.state.editor.focused);

    command(&mut rt, 'e', false);
    assert_eq!(rt.state.view().mode(), EditingMode::Source);
    command(&mut rt, '-', false);
    assert_eq!(factor(&rt), 1.1);
    command(&mut rt, '0', false);
    assert_eq!(factor(&rt), 1.0);
    assert_fills_window(&rt);
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn the_toolbar_control_steps_and_resets() {
    let mut rt = live("body", None);
    click_key(&mut rt, "zoom-in");
    click_key(&mut rt, "zoom-in");
    assert_eq!(factor(&rt), 1.25);
    assert!(rt.accessibility_json().to_string().contains("125%"));
    click_key(&mut rt, "zoom-out");
    assert_eq!(factor(&rt), 1.1);
    click_key(&mut rt, "zoom-reset");
    assert_eq!(factor(&rt), 1.0);
    for _ in 0..20 {
        click_key(&mut rt, "zoom-out");
    }
    assert_eq!(factor(&rt), 0.5);
    assert_fills_window(&rt);
}

#[test]
fn the_window_follows_a_resize_while_zoomed() {
    let mut rt = live("body", None);
    command(&mut rt, '=', false);
    command(&mut rt, '=', false);
    rt.set_viewport((900.0, 500.0), 1.0);
    rt.frame(0.0);
    let root = element(&rt, "scratchpad").unwrap();
    assert!((root.rect[2] - 900.0).abs() < 1.0 && (root.rect[3] - 500.0).abs() < 1.0);
}

#[test]
fn clicking_and_typing_work_while_zoomed() {
    let source = long_document();
    let mut rt = live(&source, None);
    for _ in 0..4 {
        command(&mut rt, '=', false);
    }
    assert_eq!(factor(&rt), 1.75);
    rt.wheel(300.0, 300.0, -400.0);
    rt.frame(0.0);

    let target = rt
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
    let n: String = target.text.as_deref().unwrap()["Paragraph ".len()..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    let paragraph = format!("Paragraph {n} has **bold** and *emphasis* in it.\n");
    let (x, y) = (target.rect[0] + 2.0, target.rect[1] + target.rect[3] * 0.5);
    click(&mut rt, x, y);
    assert_eq!(active(&rt), Some(paragraph.as_str()));
    assert!(
        (caret_mid(&rt) - y).abs() < 2.0,
        "caret at {} for a click at {y}",
        caret_mid(&rt)
    );

    press(&mut rt, Key::Text("!".into()), Mods::default());
    let at = source.find(&paragraph).unwrap();
    assert_eq!(&rt.state.editor.text[at..at + 1], "!");
}
