//! Caret-sensitive reveal in the window: navigation over styled lines (Home/End, Up/Down within
//! and between regions), caret holds while reveal reshapes a region, pointer and drag selection,
//! source clipboard, deletion and history, and parse/highlight reuse on caret moves.
use std::rc::Rc;

use rune::runtime::ElementInfo;
use rune::{Env, Key, KeyEvent, Mods, Runtime};
use rune_kit::markdown::block_syntax;
use rune_markdown_prototype::app::{view, window_key};
use rune_markdown_prototype::state::{EditingMode, State};

const SIZE: (f32, f32) = (640.0, 600.0);

fn live_at(source: &str, caret: Option<usize>, size: (f32, f32)) -> Runtime<State> {
    let mut state = State::new(source);
    match caret {
        Some(at) => assert!(state.activate_block_at(at)),
        None => state.editor.focused = false,
    }
    let mut rt = Runtime::new(state, view, size, Env::new(false, 1.0));
    rt.on_key = Some(window_key);
    rt.frame(0.0);
    rt
}

fn live(source: &str, caret: Option<usize>) -> Runtime<State> {
    live_at(source, caret, SIZE)
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
    let mods = Mods {
        shift: true,
        ..Mods::default()
    };
    press(rt, key, mods);
}

fn command(rt: &mut Runtime<State>, c: char, shift: bool) {
    let mods = Mods {
        cmd: true,
        shift,
        ..Mods::default()
    };
    press(rt, Key::Command(c), mods);
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

fn caret(rt: &Runtime<State>) -> [f32; 4] {
    input(rt).caret.expect("a focused caret")
}

fn caret_mid(rt: &Runtime<State>) -> f32 {
    let c = caret(rt);
    c[1] + c[3] * 0.5
}

fn selection(rt: &Runtime<State>) -> (usize, usize) {
    (rt.state.editor.anchor, rt.state.editor.focus)
}

fn caret_at(rt: &Runtime<State>) -> usize {
    let (a, b) = selection(rt);
    assert_eq!(a, b, "a collapsed caret");
    b
}

fn active(rt: &Runtime<State>) -> Option<&str> {
    let range = rt.state.view().active_source_range()?;
    Some(&rt.state.editor.text[range])
}

fn set_caret(rt: &mut Runtime<State>, at: usize) {
    rt.update(|s, _| s.editor.set_caret(at));
    rt.frame(0.0);
}

fn history_len(rt: &Runtime<State>) -> usize {
    // Undo everything on a copy: reveal must never add steps.
    let mut copy = rt.state.editor.clone();
    let mut n = 0;
    let mut last = copy.text.clone();
    loop {
        copy.undo();
        if copy.text == last {
            return n;
        }
        last = copy.text.clone();
        n += 1;
    }
}

/// Filler paragraphs, so the document scrolls and caret holds have room to act.
fn filler(n: usize) -> String {
    (1..=n)
        .map(|i| format!("\nFiller paragraph {i} keeps the document long enough to scroll.\n"))
        .collect()
}

#[test]
fn home_and_end_go_to_the_edges_of_displayed_lines() {
    let source = "Some **bold** words in a paragraph that is long enough to wrap onto a second \
                  visual line in this narrow window, and then keeps going for a third.";
    let mut rt = live_at(source, Some(0), (360.0, 600.0));
    let first = caret_mid(&rt);
    let far = source.find("third").unwrap();
    set_caret(&mut rt, far);
    let line = caret_mid(&rt);
    assert!(line > first + 20.0, "the caret is on a later visual line");

    plain(&mut rt, Key::Home);
    let home = caret_at(&rt);
    assert!(
        home > 0 && home < far,
        "Home stays on the displayed line: {home}"
    );
    assert!((caret_mid(&rt) - line).abs() < 1.0);
    let left = caret(&rt)[0];
    assert!(left < input(&rt).rect[0] + 2.0, "at the line's left edge");

    plain(&mut rt, Key::End);
    let end = caret_at(&rt);
    assert!(end >= far && end <= source.len(), "End: {end}");
    assert!((caret_mid(&rt) - line).abs() < 1.0);

    // Home on the first line lands before the hidden `**` only once they are revealed: the
    // line starts with visible text, so it is simply the region's start.
    set_caret(&mut rt, 2);
    plain(&mut rt, Key::Home);
    assert_eq!(caret_at(&rt), 0);
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn home_on_a_list_item_reaches_its_content_start_and_reveals_the_marker() {
    let source = "- first item\n- second item\n";
    let second = source.find("second").unwrap();
    let mut rt = live(source, Some(second + 4));
    assert_eq!(displayed(&rt), "first item\nsecond item");
    plain(&mut rt, Key::Home);
    assert_eq!(
        caret_at(&rt),
        second,
        "the content start, just after the marker"
    );
    assert_eq!(displayed(&rt), "first item\n- second item");
    // Home again goes to the revealed marker's start.
    plain(&mut rt, Key::Home);
    assert_eq!(caret_at(&rt), second - 2);
    plain(&mut rt, Key::End);
    assert_eq!(caret_at(&rt), second + "second item".len());
    assert_eq!(rt.state.editor.text, source);
}

#[test]
fn up_and_down_between_regions_keep_the_screen_column() {
    let source =
        "## Heading text here\nbody text that continues for a while\n\nnext paragraph with words\n";
    let body = source.find("body").unwrap();
    let mut rt = live(source, Some(body + 10));
    let x = caret(&rt)[0];

    plain(&mut rt, Key::Up);
    assert_eq!(active(&rt), Some("## Heading text here\n"));
    let up = caret_at(&rt);
    // Heading type is larger: the same screen x is fewer characters in.
    assert!(up < 3 + 10, "landed at {up}");
    assert!((caret(&rt)[0] - x).abs() < 12.0, "{} vs {x}", caret(&rt)[0]);

    plain(&mut rt, Key::Down);
    assert_eq!(active(&rt), Some("body text that continues for a while\n"));
    assert!((caret(&rt)[0] - x).abs() < 9.0);
    set_caret(&mut rt, body + 10);

    // Down through the blank line keeps the column for the paragraph beyond it.
    plain(&mut rt, Key::Down);
    assert_eq!(active(&rt), Some("\n"));
    plain(&mut rt, Key::Down);
    assert_eq!(active(&rt), Some("next paragraph with words\n"));
    let next = source.find("next").unwrap();
    let at = caret_at(&rt);
    assert!((next + 8..=next + 12).contains(&at), "{at}");
    assert!(
        (caret(&rt)[0] - x).abs() < 6.0,
        "the column survives the blank line"
    );
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    assert_eq!(history_len(&rt), 0);
}

#[test]
fn down_into_a_region_with_hidden_syntax_lands_by_its_displayed_text() {
    // Plain text above a paragraph whose first word is bold: in source, `**` would shift the
    // column by two characters; the styled line puts the caret under the same screen x.
    let source = "plain words above\n\n**Bold** words below\n";
    let mut rt = live(source, Some(10));
    let x = caret(&rt)[0];
    plain(&mut rt, Key::Down);
    plain(&mut rt, Key::Down);
    assert_eq!(active(&rt), Some("**Bold** words below\n"));
    let at = caret_at(&rt);
    let below = source.find("**Bold**").unwrap();
    assert!(at > below + 8, "past the bold word: {at}");
    assert!((caret(&rt)[0] - x).abs() < 9.0, "{} vs {x}", caret(&rt)[0]);
}

#[test]
fn a_fence_revealed_above_the_caret_keeps_the_caret_on_screen() {
    let list = "- item\n\n  ```rust\n  let x = 1;\n  ```\n- next\n";
    let source = format!("# Top\n{}{list}{}", filler(6), filler(30));
    let item = source.find("item").unwrap();
    let mut rt = live(&source, Some(item));
    // Scroll so the list sits mid-window, leaving room to scroll either way.
    rt.wheel(300.0, 300.0, -200.0);
    rt.frame(0.0);
    assert!(
        !displayed(&rt).contains("```"),
        "fences hidden: {:?}",
        displayed(&rt)
    );
    plain(&mut rt, Key::Down);
    assert!(
        !displayed(&rt).contains("```"),
        "the blank line in the item: fences still hidden"
    );
    let before = caret_mid(&rt);

    plain(&mut rt, Key::Down);
    let code = source.find("let x").unwrap();
    let at = caret_at(&rt);
    assert!(at >= code - 2 && at <= code + 6, "in the code line: {at}");
    assert!(
        displayed(&rt).contains("```rust"),
        "fences show inside the block"
    );
    // The opening fence took a line above the caret; the caret stays where the old layout put
    // the code line (one line below the item), not two.
    let moved = caret_mid(&rt) - before;
    assert!(moved > 10.0 && moved < 34.0, "caret moved {moved}");
    assert_eq!(history_len(&rt), 0);
}

#[test]
fn clicking_inside_an_open_region_keeps_the_caret_under_the_pointer_through_reveal() {
    let list = "- item\n\n  ```rust\n  let x = 1;\n  ```\n- next\n";
    let source = format!("# Top\n{}{list}{}", filler(6), filler(30));
    let item = source.find("item").unwrap();
    let mut rt = live(&source, Some(item + 2));
    rt.wheel(300.0, 300.0, -200.0);
    rt.frame(0.0);
    let field = input(&rt);
    // The code line is the field's third displayed line ("item", the blank, "let x").
    let lines = displayed(&rt);
    assert!(lines.starts_with("item\n\n"), "{lines:?}");
    let first = caret_mid(&rt);
    let y = first + 2.0 * 22.0 + 2.0;
    rt.pointer_down(field.rect[0] + 40.0, y);
    rt.pointer_up(field.rect[0] + 40.0, y);
    rt.frame(0.0);
    let code = source.find("let x").unwrap();
    let at = caret_at(&rt);
    assert!(at >= code && at <= code + "let x = 1;".len(), "{at}");
    assert!(displayed(&rt).contains("```rust"));
    assert!(
        (caret_mid(&rt) - y).abs() < 8.0,
        "caret at {} for a press at {y}",
        caret_mid(&rt)
    );
}

#[test]
fn drag_selection_reveals_what_it_touches_and_copies_source_bytes() {
    let source = "Some **bold** text\n\nafter";
    let mut rt = live(source, Some(0));
    let field = input(&rt);
    let y = field.rect[1] + field.rect[3] * 0.5;
    rt.pointer_down(field.rect[0] + 1.0, y);
    rt.frame(0.0);
    assert_eq!(selection(&rt), (0, 0));
    // Drag to the far right of the line: the selection reaches the end of the source line.
    rt.pointer_move(field.rect[0] + field.rect[2] - 1.0, y);
    rt.frame(0.0);
    rt.pointer_up(field.rect[0] + field.rect[2] - 1.0, y);
    rt.frame(0.0);
    let (a, b) = selection(&rt);
    assert_eq!(a, 0, "the drag keeps its source anchor");
    assert_eq!(b, "Some **bold** text".len());
    assert_eq!(
        displayed(&rt),
        "Some **bold** text",
        "the selection reveals the delimiters"
    );
    command(&mut rt, 'c', false);
    assert_eq!(
        rt.copy_request.take().as_deref(),
        Some("Some **bold** text"),
        "copy takes source bytes, Markdown included"
    );
    assert_eq!(history_len(&rt), 0);

    // Shift-selection inside the bold word reveals its delimiters too; collapsing hides them.
    set_caret(&mut rt, 7);
    shift(&mut rt, Key::Right);
    assert_eq!(selection(&rt), (7, 8));
    assert_eq!(displayed(&rt), "Some **bold** text");
    set_caret(&mut rt, 1);
    assert_eq!(displayed(&rt), "Some bold text");
}

#[test]
fn deleting_a_revealed_delimiter_leaves_editable_source_and_undo_restores_it() {
    let source = "# Title\n\nSome **bold** text\n\n- item";
    let close = source.find("** text").unwrap() + 2;
    let mut rt = live(source, Some(close));
    assert_eq!(displayed(&rt), "Some **bold** text");
    plain(&mut rt, Key::Backspace);
    assert_eq!(
        rt.state.editor.text,
        "# Title\n\nSome **bold* text\n\n- item"
    );
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    // The broken construct shows as local source; neighbours keep their rendered look.
    assert!(displayed(&rt).contains("bold*"), "{:?}", displayed(&rt));
    rendered(&rt, "Title");
    rendered(&rt, "item");
    plain(&mut rt, Key::Backspace);
    plain(&mut rt, Key::Backspace);
    assert_eq!(rt.state.editor.text, "# Title\n\nSome **bol text\n\n- item");
    assert!(
        displayed(&rt).contains("**bol"),
        "unfinished syntax stays visible"
    );

    // Deletions group into undo steps by Rune's usual rules; undo walks back to the source.
    for _ in 0..3 {
        if rt.state.editor.text != source {
            command(&mut rt, 'z', false);
        }
    }
    assert_eq!(rt.state.editor.text, source);
    assert_eq!(rt.state.view().mode(), EditingMode::LivePreview);
    assert_eq!(caret_at(&rt), close);
    assert_eq!(displayed(&rt), "Some **bold** text");
    for _ in 0..3 {
        command(&mut rt, 'z', true);
    }
    assert_eq!(rt.state.editor.text, "# Title\n\nSome **bol text\n\n- item");
}

#[test]
fn every_reveal_rule_applies_at_runtime() {
    let source = "7. seven\n8. eight\n\n- [ ] task\n\nA `code` and [link](https://x.y) and ~~gone~~.\n\n> quoted\n";
    let mut rt = live(source, Some(source.find("seven").unwrap()));
    // Ordered marker: the authored number shows only at its boundary.
    assert_eq!(displayed(&rt), "7. seven\neight");
    set_caret(&mut rt, source.find("seven").unwrap() + 2);
    assert_eq!(displayed(&rt), "seven\neight");
    set_caret(&mut rt, source.find("eight").unwrap());
    assert_eq!(displayed(&rt), "seven\n8. eight");

    rt.update(|s, _| assert!(s.activate_block_at(source.find("task").unwrap())));
    rt.frame(0.0);
    assert_eq!(displayed(&rt), "- [ ] task");

    let para = source.find("A `code`").unwrap();
    rt.update(|s, _| assert!(s.activate_block_at(para)));
    rt.frame(0.0);
    // Hidden backticks leave the chip's narrow generated padding.
    let shown = displayed(&rt);
    assert_eq!(shown.replace('\u{202f}', ""), "A code and link and gone.");
    assert!(shown.contains("\u{202f}code\u{202f}"));
    set_caret(&mut rt, source.find("code").unwrap());
    assert!(displayed(&rt).starts_with("A `code` and link"));
    set_caret(&mut rt, source.find("link").unwrap());
    assert!(displayed(&rt).contains("[link](https://x.y)"));
    assert!(
        !displayed(&rt).contains('`'),
        "unrelated constructs stay rendered"
    );
    set_caret(&mut rt, source.find("gone").unwrap());
    assert!(displayed(&rt).contains("~~gone~~"));

    let quote = source.find("> quoted").unwrap();
    rt.update(|s, _| assert!(s.activate_block_at(quote + 2)));
    rt.frame(0.0);
    assert_eq!(displayed(&rt), "> quoted");
    set_caret(&mut rt, quote + 5);
    assert_eq!(displayed(&rt), "quoted");
    assert_eq!(rt.state.editor.text, source);
    assert_eq!(history_len(&rt), 0);
}

#[test]
fn caret_moves_reuse_the_parse_and_the_highlighting() {
    let source = "# Title\n\n```rust\nfn main() {}\n```\n\nSome **bold** text\n";
    let mut rt = live(source, Some(source.find("fn main").unwrap()));
    let syntax = block_syntax(source);
    let code = "fn main() {}\n";
    let highlight = rune_kit::syntax::highlight(Some("rust"), code);
    for key in [
        Key::Right,
        Key::Right,
        Key::Down,
        Key::Up,
        Key::Left,
        Key::Home,
        Key::End,
    ] {
        plain(&mut rt, key);
    }
    set_caret(&mut rt, source.find("bold").unwrap());
    assert!(Rc::ptr_eq(&syntax, &block_syntax(source)), "no reparse");
    assert!(
        Rc::ptr_eq(&highlight, &rune_kit::syntax::highlight(Some("rust"), code)),
        "no rehighlight"
    );
    assert_eq!(rt.state.editor.text, source);
}
