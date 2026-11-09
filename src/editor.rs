//! Source input, Live Preview presentation and the view toggle over the one document editor.
use rune::{El, Key, KeyEvent, SourceRangeExit, TextStyle, Tx, row, text, text_input};
use rune_kit::markdown::{MarkdownStyle, editable};
use rune_kit::{Kit, Markdown};

use crate::state::{EditingMode, SourceLayout, State};

/// The toggle's choices, in order: Live Preview, Source alone, and Source beside a preview.
pub const VIEWS: [&str; 3] = ["Live Preview", "Source", "Split"];

/// The shortcut hint shown beside the toggle.
pub const TOGGLE_SHORTCUT: &str = if cfg!(target_os = "macos") {
    "⌘E"
} else {
    "Ctrl+E"
};

/// Cmd+E on macOS, Ctrl+E elsewhere: switch between Live Preview and Source.
pub fn is_toggle_shortcut(ev: &KeyEvent) -> bool {
    ev.key == Key::Command('e') && !ev.mods.shift && !ev.mods.alt
}

fn is_select_all(ev: &KeyEvent) -> bool {
    ev.key == Key::Command('a') && !ev.mods.shift && !ev.mods.alt
}

/// Every view switch lands at once and keeps the caret where it was drawn, so the document
/// redraws around it rather than jumping.
fn settle_switch(tx: &mut Tx) {
    tx.instant();
    tx.keep_caret_on_screen();
}

/// Ctrl/Cmd+E from anywhere: switch modes, keeping the caret, focus and history.
pub fn toggle_mode(state: &mut State, tx: &mut Tx) -> bool {
    if !state.toggle_mode() {
        return false;
    }
    settle_switch(tx);
    true
}

/// The index of the current view in [`VIEWS`].
pub fn selected_view(state: &State) -> usize {
    match (state.view().mode(), state.view().source_layout()) {
        (EditingMode::LivePreview, _) => 0,
        (EditingMode::Source, SourceLayout::Full) => 1,
        (EditingMode::Source, SourceLayout::Split) => 2,
    }
}

/// Choose a view from [`VIEWS`].
pub fn pick_view(state: &mut State, view: usize, tx: &mut Tx) {
    let mode = state.view().mode();
    match view {
        0 if mode == EditingMode::Source => {
            toggle_mode(state, tx);
        }
        1 | 2 => {
            let layout = if view == 1 {
                SourceLayout::Full
            } else {
                SourceLayout::Split
            };
            let changed = layout != state.view().source_layout();
            state.set_source_layout(layout);
            if mode == EditingMode::LivePreview {
                toggle_mode(state, tx);
            } else if changed {
                settle_switch(tx);
            }
        }
        _ => {}
    }
}

/// The window's keys that no focused control took: the mode and zoom shortcuts, and Select All
/// in Live Preview while no block is open, which selects the whole document in Source mode.
pub fn window_key(state: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if crate::zoom::zoom_key(state, ev, tx) {
        return true;
    }
    if is_toggle_shortcut(ev) {
        return toggle_mode(state, tx);
    }
    if is_select_all(ev) && state.view().mode() == EditingMode::LivePreview {
        if state.select_all() {
            tx.instant();
        }
        return true;
    }
    false
}

/// A Live Preview / Source / Split switch. Its segments leave the editor focused when pressed,
/// so an open block stays open and the caret keeps its place.
pub fn view_toggle(state: &State, kit: &Kit) -> El {
    let c = &kit.c;
    let selected = selected_view(state);
    let segments = VIEWS.iter().enumerate().map(|(i, label)| {
        let on = i == selected;
        let mut segment = row()
            .key(format!("view-{}", label.to_lowercase().replace(' ', "-")))
            .role("radio")
            .label(*label)
            .checked(on)
            .position_in_set(i + 1, VIEWS.len())
            .preserve_focus_on_press(true)
            .h(26.0)
            .px(12.0)
            .center()
            .radius(5.0)
            .child(
                text(*label)
                    .font(TextStyle::new(12.5, if on { 600.0 } else { 500.0 }))
                    .color(if on { c.text_base } else { c.text_muted })
                    .a11y_hidden(),
            )
            .on_press_tx(move |state: &mut State, tx| pick_view(state, i, tx));
        if on {
            segment = segment.fill(c.bg_base).box_shadow(rune::Shadow {
                spread: 0.5,
                color: c.border_strong,
                ..Default::default()
            });
        }
        segment
    });
    row()
        .key("view-toggle")
        .role("radiogroup")
        .label("View")
        .help(format!("Switch Live Preview / Source ({TOGGLE_SHORTCUT})"))
        .preserve_focus_on_press(true)
        .gap(2.0)
        .padding(1.0)
        .center_y()
        .radius(6.0)
        .fill(c.layer_01)
        .box_shadow(rune::Shadow {
            spread: 0.5,
            color: c.border_base,
            ..Default::default()
        })
        .children(segments)
}

/// The shared input: the whole document in Source mode, or the active region's Markdown in
/// Live Preview. Both bind the same `State::editor`, so text, selection and history are shared.
/// In Live Preview the region keeps its rendered look: syntax the caret has not revealed is
/// hidden through a display projection, and styles come from the Markdown renderer's rules.
pub fn source_input(state: &State, kit: &Kit) -> El {
    let input = text_input(&state.editor, TextStyle::new(14.0, 400.0).mono().lh(22.0))
        .key("markdown-input")
        .label("Markdown source")
        .w_full()
        .wrap_anywhere()
        .color(kit.c.text_base)
        .caret(kit.c.text_base, kit.selection())
        .on_edit(|state: &mut State| &mut state.editor)
        .on_input_key(|state: &mut State, ev, tx| {
            if is_toggle_shortcut(ev) {
                // Taken even during a composition, which the switch waits for.
                toggle_mode(state, tx);
                return true;
            }
            state.input_key(ev)
        })
        .on_source_range_exit(|state: &mut State, exit, tx| {
            let leaves = state.source_range_exit(exit);
            if leaves {
                // The block left behind redraws rendered and the next opens as source; hold
                // the caret's height so the text around it stays put.
                tx.instant();
                if exit == (SourceRangeExit::Navigate { extend: false }) {
                    tx.keep_caret_on_screen();
                }
            }
            leaves
        })
        .on_composition_end(|state: &mut State, _| state.composition_ended())
        .on_input_change(|state: &mut State, _| {
            state.sync_document();
        })
        .on_submit(|state: &mut State, _| {
            // Return in a plain paragraph submits; in this editor it always inserts a newline.
            state.editor.insert("\n");
            state.sync_document();
        });
    match state.input_range() {
        Some(range) => {
            let reveal = state.reveal();
            let style = MarkdownStyle::from_kit(kit);
            editable(&state.editor.text, range.clone(), &reveal.ranges, &style)
                .apply(input.source_range(range))
        }
        None => input.min_h(64.0),
    }
}

/// The document in the current mode. Live Preview renders every block from the full parse
/// except the active region, which shows its Markdown in the shared input. Clicking rendered
/// text, links, task markers or gaps activates that region at the mapped source position and
/// keeps the caret under the pointer.
pub fn document(state: &State, kit: &Kit) -> El {
    if state.view().mode() == EditingMode::Source {
        return source_input(state, kit);
    }
    let mut markdown = Markdown::new(&state.editor.text)
        .scroll_wide(true)
        .on_source_click(|state: &mut State, click, tx| {
            if state.activate_click(click) {
                tx.keep_caret_at_press();
            }
        });
    if let Some(range) = state.view().active_source_range() {
        markdown = markdown.replace_source_range(range, source_input(state, kit));
    }
    markdown.build(kit).key("rendered-markdown")
}
