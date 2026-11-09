//! Named Live Preview states over the reveal fixture, for headless snapshots:
//! `rune-markdown-prototype --scene fixture-code --snap code.png`. Each opens the fixture with
//! the caret at one construct, so the snapshot shows that construct's revealed syntax beside
//! rendered neighbours. Scenes only set the caret; they never touch history.
use crate::state::State;

/// The fixture the reveal tests and scenes share.
pub const FIXTURE: &str = include_str!("../tests/fixtures/live_preview.md");

fn open_at(state: &mut State, needle: &str, offset: usize) {
    *state = State::new(FIXTURE);
    let at = FIXTURE.find(needle).expect("fixture text") + offset;
    state.activate_block_at(at);
}

/// Every scene: the fixture unfocused, then the caret on each construct.
pub const SCENES: &[(&str, fn(&mut State))] = &[
    ("fixture", |s| {
        *s = State::new(FIXTURE);
        s.editor.focused = false;
    }),
    ("fixture-heading", |s| open_at(s, "Heading with", 3)),
    ("fixture-inline", |s| open_at(s, "**bold**,", 3)),
    ("fixture-link", |s| open_at(s, "inline link]", 2)),
    ("fixture-code", |s| open_at(s, "println!", 0)),
    ("fixture-quote", |s| open_at(s, "A quote line", 0)),
    ("fixture-nested-quote", |s| open_at(s, "A nested quote", 0)),
    ("fixture-list", |s| open_at(s, "Second bullet", 0)),
    ("fixture-ordered", |s| open_at(s, "Seven,", 0)),
    ("fixture-task", |s| open_at(s, "Nested done task", 0)),
    ("fixture-list-code", |s| open_at(s, "const inList", 0)),
];
