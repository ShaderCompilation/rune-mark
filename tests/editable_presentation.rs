//! The styled editable presentation: an active region keeps its rendered look while syntax the
//! caret has not reached stays hidden through Rune's display projection. Exercises the shared
//! input through the same document view as the window.
use rune::accesskit::Role;
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

fn runtime_at(source: &str, caret: Option<usize>, width: f32) -> Runtime<State> {
    let mut state = State::new(source);
    state.set_mode(EditingMode::LivePreview);
    match caret {
        Some(offset) => assert!(state.activate_block_at(offset)),
        None => state.editor.focused = false,
    }
    let mut rt = Runtime::new(state, view, (width, 900.0), Env::new(false, 1.0));
    rt.frame(0.0);
    rt
}

fn runtime(source: &str, caret: Option<usize>) -> Runtime<State> {
    runtime_at(source, caret, 520.0)
}

fn press(rt: &mut Runtime<State>, key: Key) {
    let event = KeyEvent {
        key,
        mods: Mods::default(),
    };
    rt.key(event.clone());
    rt.key_up(event);
    rt.frame(0.0);
}

fn input(rt: &Runtime<State>) -> ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("markdown-input") && !e.exiting)
        .expect("source input")
}

fn displayed(rt: &Runtime<State>) -> String {
    input(rt).text.unwrap()
}

fn rendered(rt: &Runtime<State>, value: &str) -> ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.text.as_deref() == Some(value) && !e.exiting)
        .unwrap_or_else(|| panic!("missing rendered text {value:?}"))
}

fn caret_to(rt: &mut Runtime<State>, at: usize) {
    rt.update(|s, _| s.editor.set_caret(at));
    rt.frame(0.0);
}

fn click(rt: &mut Runtime<State>, x: f32, y: f32) {
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
}

#[test]
fn a_focused_heading_keeps_its_typography_and_reveals_only_its_markers() {
    let source = "intro\n\n## Heading with *em*\n\nafter";
    let mut rt = runtime(source, None);
    let heading = rendered(&rt, "Heading with em");
    let start = source.find("Heading").unwrap();
    rt.update(|s, _| assert!(s.activate_block_at(start)));
    rt.frame(0.0);
    // Heading markers reveal while editing the heading; the emphasis is not touched.
    assert_eq!(displayed(&rt), "## Heading with em");
    let field = input(&rt);
    assert!(
        (field.rect[3] - heading.rect[3]).abs() < 0.5,
        "the input keeps the heading's line height ({} vs {})",
        field.rect[3],
        heading.rect[3]
    );
    assert!(
        (field.rect[1] - heading.rect[1]).abs() < 0.5,
        "and its place"
    );
    let caret = rt.ime_area.unwrap();
    assert!(
        (caret[3] - heading.rect[3]).abs() < 2.0,
        "caret spans the heading line"
    );

    // Moving into the emphasis reveals its delimiters before the caret enters them.
    let em = source.find('*').unwrap();
    caret_to(&mut rt, em);
    assert_eq!(displayed(&rt), "## Heading with *em*");
    assert_eq!(rt.state.editor.text, source, "reveal changes no source");
}

#[test]
fn arrows_reach_hidden_delimiters_through_reveal_without_changing_history() {
    let source = "Some **bold** text";
    let mut rt = runtime(source, Some(0));
    assert_eq!(displayed(&rt), "Some bold text");
    for _ in 0..4 {
        press(&mut rt, Key::Right);
    }
    assert_eq!(rt.state.editor.focus, 4);
    assert_eq!(displayed(&rt), "Some bold text");
    // The next stop is just before `**`: touching it reveals it.
    press(&mut rt, Key::Right);
    assert_eq!(rt.state.editor.focus, 5);
    assert_eq!(displayed(&rt), "Some **bold** text");
    press(&mut rt, Key::Right);
    assert_eq!(
        rt.state.editor.focus, 6,
        "revealed syntax is an ordinary caret stop"
    );
    // Leaving the construct hides it again; Left comes back to its far edge.
    caret_to(&mut rt, source.len());
    assert_eq!(displayed(&rt), "Some bold text");
    for _ in 0..5 {
        press(&mut rt, Key::Left);
    }
    assert_eq!(rt.state.editor.focus, source.find(" text").unwrap());
    assert_eq!(displayed(&rt), "Some **bold** text");
    assert_eq!(rt.state.editor.text, source);
    // Reveal and hide recorded nothing: undo has nothing to restore.
    press_command(&mut rt, 'z');
    assert_eq!(rt.state.editor.text, source);
}

fn press_command(rt: &mut Runtime<State>, c: char) {
    let event = KeyEvent {
        key: Key::Command(c),
        mods: Mods {
            cmd: true,
            ..Mods::default()
        },
    };
    rt.key(event.clone());
    rt.key_up(event);
    rt.frame(0.0);
}

#[test]
fn pointer_hits_map_through_hidden_syntax_to_source() {
    let source = "A **bold** move and `code` here";
    let mut rt = runtime(source, Some(0));
    // Hidden backticks leave the rendered chip's generated narrow padding.
    let pad = "\u{202f}\u{202f}";
    assert_eq!(
        displayed(&rt),
        format!("A bold move and {pad}code{pad} here")
    );
    // Neither position touches a construct, so both see the same layout.
    for word in ["move", "and", "here"] {
        let at = source.find(word).unwrap();
        caret_to(&mut rt, at);
        let [x, y, _, h] = rt.ime_area.unwrap();
        caret_to(&mut rt, 0);
        click(&mut rt, x + 0.1, y + h * 0.5);
        assert_eq!(rt.state.editor.focus, at, "click before {word:?}");
    }
    // A click inside the hidden-delimiter bold word lands inside its source and reveals it.
    // Its glyphs lie between "A" and "move" in the layout without delimiters.
    caret_to(&mut rt, 1);
    let [left, y, _, h] = rt.ime_area.unwrap();
    caret_to(&mut rt, source.find("move").unwrap());
    let right = rt.ime_area.unwrap()[0];
    caret_to(&mut rt, source.find("here").unwrap());
    click(&mut rt, (left + right) * 0.5, y + h * 0.5);
    let bold = source.find("bold").unwrap();
    let at = rt.state.editor.focus;
    assert!(bold < at && at < bold + 4, "{at} inside the bold text");
    assert_eq!(
        displayed(&rt),
        format!("A **bold** move and {pad}code{pad} here")
    );
}

#[test]
fn wrapped_projected_text_moves_by_visual_lines() {
    let source = "Lead **strong** words keep wrapping across several lines in this narrow field";
    let start = source.find("field").unwrap() + 2;
    let mut rt = runtime_at(source, Some(start), 160.0);
    let shown = displayed(&rt);
    assert!(!shown.contains("**"));
    let bottom = rt.ime_area.unwrap()[1];
    press(&mut rt, Key::Up);
    let up = rt.ime_area.unwrap()[1];
    assert!(up < bottom, "Up follows the projected wrapping");
    assert!(rt.state.editor.focus < start);
    assert!(rt.state.editor.text.is_char_boundary(rt.state.editor.focus));
    press(&mut rt, Key::Down);
    assert!((rt.ime_area.unwrap()[1] - bottom).abs() < 0.5);
    assert_eq!(rt.state.editor.focus, start, "and back to the same column");
}

#[test]
fn composition_inside_revealed_syntax_stays_visible_until_commit() {
    let source = "x **bold** y";
    let inside = source.find("bold").unwrap() + 2;
    let mut rt = runtime(source, Some(inside));
    assert_eq!(displayed(&rt), "x **bold** y");
    rt.ime(Ime::Preedit(
        "日本".into(),
        Some(("日本".len(), "日本".len())),
    ));
    rt.frame(0.0);
    assert_eq!(rt.state.editor.text, source);
    assert_eq!(displayed(&rt), "x **bo日本ld** y");
    let tree = rt.accessibility_tree();
    let field = tree
        .nodes
        .iter()
        .find(|(_, n)| n.author_id() == Some("markdown-input"))
        .unwrap();
    assert_eq!(
        field.1.text_selection().unwrap().focus.character_index,
        "x **bo日本".chars().count()
    );
    rt.ime(Ime::Commit("日本".into()));
    rt.frame(0.0);
    assert_eq!(rt.state.editor.text, "x **bo日本ld** y");
    assert_eq!(rt.state.editor.focus, inside + "日本".len());
    assert_eq!(displayed(&rt), "x **bo日本ld** y");
}

#[test]
fn accessibility_reads_the_displayed_text_and_selects_source() {
    let source = "Plain **bold** end";
    let mut rt = runtime(source, Some(source.len()));
    let tree = rt.accessibility_tree();
    let field = tree
        .nodes
        .iter()
        .find(|(_, n)| n.author_id() == Some("markdown-input"))
        .unwrap();
    // The value is the authoritative source; runs are what is drawn.
    assert_eq!(field.1.value(), Some(source));
    let run = tree
        .nodes
        .iter()
        .find(|(_, n)| n.role() == Role::TextRun)
        .unwrap();
    assert_eq!(run.1.value(), Some("Plain bold end"));
    let at = |c| rune::accesskit::TextPosition {
        node: run.0,
        character_index: c,
    };
    // Select the displayed word "bold": characters 6..10.
    assert!(rt.accessibility_action(
        "markdown-input",
        rune::accesskit::Action::SetTextSelection,
        Some(rune::accesskit::ActionData::SetTextSelection(
            rune::accesskit::TextSelection {
                anchor: at(6),
                focus: at(10),
            }
        )),
    ));
    rt.frame(0.0);
    let bold = source.find("bold").unwrap();
    let (a, f) = (rt.state.editor.anchor, rt.state.editor.focus);
    assert!(
        (bold - 2..=bold).contains(&a) && (bold + 4..=bold + 6).contains(&f),
        "selection {a}..{f} covers the source of the displayed word"
    );
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn entities_and_escapes_show_decoded_until_touched() {
    let source = "Tom &amp; Jerry \\*not em\\* end";
    let mut rt = runtime(source, Some(source.len()));
    assert_eq!(displayed(&rt), "Tom & Jerry *not em* end");
    let amp = source.find('&').unwrap();
    caret_to(&mut rt, amp);
    assert_eq!(displayed(&rt), "Tom &amp; Jerry *not em* end");
    // Right from before the entity steps over it whole.
    press(&mut rt, Key::Right);
    assert_eq!(
        rt.state.editor.focus,
        amp + 1,
        "revealed, it is ordinary text"
    );
    caret_to(&mut rt, amp - 1);
    assert_eq!(displayed(&rt), "Tom & Jerry *not em* end");
    press(&mut rt, Key::Right);
    assert_eq!(rt.state.editor.focus, amp, "Right stops before the entity");
}

#[test]
fn fenced_code_reveals_its_fences_inside_the_frame_and_keeps_highlighting() {
    let source = "intro\n\n```rust\nfn main() {}\n```\n\nafter";
    let code = source.find("fn main").unwrap();
    let mut rt = runtime(source, None);
    let frame = rt
        .elements()
        .into_iter()
        .find(|e| e.role == "code" && !e.exiting)
        .unwrap();
    rt.update(|s, _| assert!(s.activate_block_at(code)));
    rt.frame(0.0);
    assert_eq!(displayed(&rt), "```rust\nfn main() {}\n```");
    let edited = rt
        .elements()
        .into_iter()
        .find(|e| e.role == "code" && !e.exiting)
        .expect("the input is drawn in the code frame");
    assert!((edited.rect[1] - frame.rect[1]).abs() < 0.5);
    let field = input(&rt);
    assert!(field.rect[0] > edited.rect[0] && field.rect[1] > edited.rect[1]);
}

#[test]
fn list_markers_hide_until_the_caret_reaches_them() {
    let source = "- one\n- two **b**\n- [ ] task\n\nafter";
    let two = source.find("two").unwrap();
    let mut rt = runtime(source, Some(two + 2));
    assert_eq!(displayed(&rt), "one\ntwo b\ntask");
    // Content start touches the item's marker: only that marker shows.
    caret_to(&mut rt, two);
    assert_eq!(displayed(&rt), "one\n- two b\ntask");
    let task = source.find("[ ]").unwrap();
    caret_to(&mut rt, task + "[ ] ".len());
    assert_eq!(displayed(&rt), "one\ntwo b\n- [ ] task");
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn quote_prefixes_hide_and_wrapped_lines_keep_the_quote_indent() {
    let source = "> quoted line that wraps across lines here\n> second\n\nafter";
    let second = source.find("second").unwrap();
    let mut rt = runtime_at(source, Some(second + 3), 200.0);
    assert_eq!(
        displayed(&rt),
        "quoted line that wraps across lines here\nsecond"
    );
    let field = input(&rt);
    // The caret sits right of the quote rule and its padding.
    assert!(rt.ime_area.unwrap()[0] > field.rect[0] + 10.0);
    caret_to(&mut rt, second);
    assert_eq!(
        displayed(&rt),
        "quoted line that wraps across lines here\n> second"
    );
}
