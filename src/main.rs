use rune::{AppSpec, Cx, El, ScrollSpec, TextStyle, col, el, row, text, text_input};
use rune_kit::{Kit, Markdown};
use rune_markdown_prototype::state::State;

const SAMPLE: &str = include_str!("../sample.md");
const WINDOW_SIZE: (f32, f32) = (1200.0, 800.0);

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
                .h(44.0)
                .padding(12.0)
                .center_y()
                .fill(kit.c.bg_base)
                .child(
                    text(title)
                        .font(TextStyle::new(13.0, 600.0))
                        .color(kit.c.text_muted),
                ),
        )
        .child(content)
}

fn view(state: &State, cx: &mut Cx<State>) -> El {
    state.sync_document();
    let kit = Kit::neutral(cx.system_dark());

    let preview = col()
        .key("preview-scroll")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col().w_full().padding(24.0).child(
                Markdown::new(&state.editor.text)
                    .selectable(true)
                    .scroll_wide(true)
                    .build(&kit)
                    .key("rendered-markdown"),
            ),
        );

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
            col().w_full().padding(24.0).child(
                text_input(&state.editor, TextStyle::new(14.0, 400.0).mono().lh(22.0))
                    .key("markdown-input")
                    .label("Markdown source")
                    .w_full()
                    .min_h(64.0)
                    .wrap_anywhere()
                    .color(kit.c.text_base)
                    .caret(kit.c.text_base, kit.selection())
                    .on_edit(|state: &mut State| &mut state.editor)
                    // Rune submits on plain Enter; a source editor inserts a newline.
                    .on_submit(|state: &mut State, _| state.editor.insert("\n")),
            ),
        );

    row()
        .key("scratchpad")
        .w_full()
        .h_full()
        .fill(kit.c.bg_base)
        .child(pane("preview-pane", "Preview", preview, &kit))
        .child(el().key("divider").w(1.0).h_full().fill(kit.c.border_base))
        .child(pane("editor-pane", "Markdown", editor, &kit))
}

fn main() {
    // AppSpec also provides Rune's --snap, --tree, --theme and --size tools.
    rune::run(AppSpec::new(
        "Rune Markdown · Scratchpad",
        WINDOW_SIZE,
        || State::new(SAMPLE),
        view,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{ClipboardContent, Env, Key, KeyEvent, Mods, Runtime};

    fn runtime(source: &str, dark: bool) -> Runtime<State> {
        let mut rt = Runtime::new(
            State::new(source),
            view,
            WINDOW_SIZE,
            Env {
                system_dark: dark,
                scale: 1.0,
            },
        );
        rt.frame(0.0);
        rt
    }

    #[test]
    fn enter_splits_a_line_updates_preview_and_can_be_undone() {
        let mut rt = runtime("# Բարևtail", false);
        let caret = "# Բարև".len();
        rt.update(|state, _| {
            state.editor.anchor = caret;
            state.editor.focus = caret;
        });
        rt.frame(1.0 / 60.0);
        let enter = KeyEvent {
            key: Key::Enter,
            mods: Mods::default(),
        };
        rt.key(enter.clone());
        rt.key_up(enter);
        rt.frame(1.0 / 60.0);

        assert_eq!(rt.state.editor.text, "# Բարև\ntail");
        assert_eq!(rt.state.blocks().len(), 2);
        assert_eq!(rt.state.editor.anchor, caret + 1);
        assert_eq!(rt.state.editor.focus, caret + 1);
        assert!(rt.state.editor.focused);
        assert!(rt.accessibility_json().to_string().contains("\"Բարև\""));

        let undo = KeyEvent {
            key: Key::Command('z'),
            mods: Mods {
                cmd: true,
                ..Mods::default()
            },
        };
        rt.key(undo.clone());
        rt.key_up(undo);
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.editor.text, "# Բարևtail");
        assert_eq!(rt.state.blocks().len(), 1);
        assert!(rt.state.editor.focused);
    }

    #[test]
    fn paste_updates_the_preview_and_keeps_editor_focus() {
        let mut rt = runtime("", false);
        rt.paste_content(ClipboardContent::new(
            "# Updated preview\n\n**Live** Markdown.",
        ));
        rt.frame(1.0 / 60.0);
        assert_eq!(
            rt.state.editor.text,
            "# Updated preview\n\n**Live** Markdown."
        );
        assert!(rt.state.editor.focused);
        // The raw editor value includes '#'; this exact name comes from the rendered heading.
        assert_eq!(rt.state.blocks().len(), 2);
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("\"Updated preview\"")
        );
    }

    #[test]
    fn clearing_source_removes_previous_preview_content() {
        let mut rt = runtime("# A heading to remove", true);
        assert!(
            rt.accessibility_json()
                .to_string()
                .contains("\"A heading to remove\"")
        );
        rt.update(|state, _| state.editor.clear());
        rt.frame(1.0 / 60.0);
        assert!(rt.state.editor.text.is_empty());
        assert!(rt.state.blocks().is_empty());
        assert!(
            !rt.accessibility_json()
                .to_string()
                .contains("A heading to remove")
        );
        assert!(rt.state.editor.focused);
    }

    #[test]
    fn unicode_and_incomplete_markdown_can_be_edited() {
        let mut rt = runtime("", true);
        let source = "# Բարև\n\nمرحبا · café\n\n**unfinished\n\n```rust\nfn main() {";
        rt.paste_content(ClipboardContent::new(source));
        rt.frame(1.0 / 60.0);
        assert_eq!(rt.state.editor.text, source);
        assert!(rt.state.editor.focused);
        assert!(rt.accessibility_json().to_string().contains("Բարև"));
    }
}
