use rune::AppSpec;
use rune_markdown_prototype::app::{view, window_key};
use rune_markdown_prototype::state::State;

const SAMPLE: &str = include_str!("../sample.md");
const WINDOW_SIZE: (f32, f32) = (1200.0, 800.0);

fn main() {
    // AppSpec also provides Rune's --snap, --tree, --theme and --size tools.
    rune::run(AppSpec {
        on_key: Some(window_key),
        ..AppSpec::new(
            "Rune Markdown · Scratchpad",
            WINDOW_SIZE,
            || State::new(SAMPLE),
            view,
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{ClipboardContent, Env, Key, KeyEvent, Mods, Runtime};
    use rune_markdown_prototype::state::{EditingMode, SourceLayout};

    /// The window in its split layout, where the preview re-renders the whole buffer as the
    /// source is edited.
    fn runtime(source: &str, dark: bool) -> Runtime<State> {
        let mut state = State::new(source);
        state.set_mode(EditingMode::Source);
        state.set_source_layout(SourceLayout::Split);
        let mut rt = Runtime::new(
            state,
            view,
            WINDOW_SIZE,
            Env {
                system_dark: dark,
                scale: 1.0,
            },
        );
        rt.on_key = Some(window_key);
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
