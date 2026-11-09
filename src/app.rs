//! The window: a toolbar with the view toggle above Live Preview, the source alone, or the
//! source beside a rendered preview.
use rune::{Cx, El, ScrollSpec, TextStyle, col, el, row, text};
use rune_kit::{Kit, Markdown};

use crate::editor::{TOGGLE_SHORTCUT, document, source_input, view_toggle};
use crate::state::{EditingMode, SourceLayout, State};

pub use crate::editor::window_key;

fn pane(key: &'static str, title: &'static str, content: El, kit: &Kit) -> El {
    col()
        .key(key)
        .grow(1.0)
        .shrink()
        .min_h(0.0)
        .h_full()
        .child(
            row()
                .w_full()
                .h(36.0)
                .px(24.0)
                .center_y()
                .fill(kit.c.bg_base)
                .child(
                    text(title)
                        .font(TextStyle::new(12.0, 600.0))
                        .color(kit.c.text_muted),
                ),
        )
        .child(content)
}

/// The whole window for the current mode and layout.
pub fn view(state: &State, cx: &mut Cx<State>) -> El {
    state.sync_document();
    let kit = Kit::neutral(cx.system_dark());
    let body = match (state.view().mode(), state.view().source_layout()) {
        (EditingMode::Source, SourceLayout::Split) => split(state, &kit),
        _ => single(state, &kit),
    };
    let window = col()
        .key("scratchpad")
        .fill(kit.c.bg_base)
        .child(toolbar(state, &kit))
        .child(
            el().key("toolbar-rule")
                .w_full()
                .h(1.0)
                .fill(kit.c.border_base),
        )
        .child(col().key("body").w_full().grow(1.0).min_h(0.0).child(body));
    window.w_full().h_full()
}

fn toolbar(state: &State, kit: &Kit) -> El {
    row()
        .key("toolbar")
        .w_full()
        .h(44.0)
        .px(16.0)
        .gap(10.0)
        .center_y()
        .fill(kit.c.bg_base)
        .child(view_toggle(state, kit))
        .child(
            text(TOGGLE_SHORTCUT)
                .font(TextStyle::new(12.0, 400.0))
                .color(kit.c.text_muted),
        )
}

/// Live Preview, or the source alone: one scrolling document. Both keep the same scroll
/// container, so switching keeps its offset as well as the caret's place on screen.
fn single(state: &State, kit: &Kit) -> El {
    let mut scroll = col()
        .key("document-scroll")
        .w_full()
        .h_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    if state.view().mode() == EditingMode::Source {
        // Clicking blank space below the source should also return focus to the editor.
        scroll = scroll.role("textbox");
    }
    scroll.child(col().w_full().padding(24.0).child(document(state, kit)))
}

/// The source on the left and a selectable rendered preview on the right, each scrolling on
/// its own.
fn split(state: &State, kit: &Kit) -> El {
    let editor = col()
        .key("editor-scroll")
        // Clicking blank space in the pane should also return focus to the editor.
        .role("textbox")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col()
                .w_full()
                .px(24.0)
                .pb(24.0)
                .child(source_input(state, kit)),
        );

    let preview = col()
        .key("preview-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col().w_full().px(24.0).pb(24.0).child(
                Markdown::new(&state.editor.text)
                    .selectable(true)
                    .scroll_wide(true)
                    .build(kit)
                    .key("rendered-markdown"),
            ),
        );

    row()
        .key("split")
        .w_full()
        .h_full()
        .child(pane("editor-pane", "Markdown", editor, kit))
        .child(el().key("divider").w(1.0).h_full().fill(kit.c.border_base))
        .child(pane("preview-pane", "Preview", preview, kit))
}
