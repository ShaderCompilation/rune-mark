//! Source input presentation over the one document editor.
use rune::{El, TextStyle, text_input};
use rune_kit::Kit;

use crate::state::{EditingMode, State};

pub fn source_input(state: &State, kit: &Kit) -> El {
    let mut input = text_input(&state.editor, TextStyle::new(14.0, 400.0).mono().lh(22.0))
        .key("markdown-input")
        .label("Markdown source")
        .w_full()
        .min_h(64.0)
        .wrap_anywhere()
        .color(kit.c.text_base)
        .caret(kit.c.text_base, kit.selection())
        .on_edit(|state: &mut State| &mut state.editor)
        .on_input_change(|state: &mut State, _| {
            state.sync_document();
        })
        .on_submit(|state: &mut State, _| {
            state.editor.insert("\n");
            state.sync_document();
        });
    if state.view().mode() == EditingMode::LivePreview
        && let Some(range) = state.view().active_source_range()
    {
        input = input.source_range(range);
    }
    input
}
