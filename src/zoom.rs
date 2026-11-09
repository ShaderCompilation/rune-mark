//! Window-wide zoom: Ctrl/Cmd + and - step through browser-like levels, Ctrl/Cmd+0 resets, and
//! a toolbar control shows the level. The whole window, toolbar included, draws scaled from its
//! top-left corner and lays out in `window / zoom` points, so text reflows rather than cropping.
use rune::{Cx, El, Key, KeyEvent, TextStyle, Tx, col, row, text};
use rune_kit::Kit;

use crate::state::State;

/// The zoom levels, as browsers offer them.
pub const LEVELS: [f32; 13] = [
    0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];

/// The shortcut modifier as the hints spell it.
const MOD: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl+"
};

/// The index of 100% in [`LEVELS`].
const DEFAULT: usize = 5;

/// The current zoom level, as an index into [`LEVELS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Zoom(usize);

impl Default for Zoom {
    fn default() -> Self {
        Self(DEFAULT)
    }
}

impl Zoom {
    pub fn factor(self) -> f32 {
        LEVELS[self.0]
    }

    /// The level as a percentage label, such as "125%".
    pub fn label(self) -> String {
        format!("{}%", (self.factor() * 100.0).round())
    }

    pub fn can_zoom_in(self) -> bool {
        self.0 + 1 < LEVELS.len()
    }

    pub fn can_zoom_out(self) -> bool {
        self.0 > 0
    }

    pub fn is_default(self) -> bool {
        self.0 == DEFAULT
    }

    /// One level in. Returns whether the level changed.
    pub fn zoom_in(&mut self) -> bool {
        let changed = self.can_zoom_in();
        if changed {
            self.0 += 1;
        }
        changed
    }

    /// One level out. Returns whether the level changed.
    pub fn zoom_out(&mut self) -> bool {
        let changed = self.can_zoom_out();
        if changed {
            self.0 -= 1;
        }
        changed
    }

    /// Back to 100%. Returns whether the level changed.
    pub fn reset(&mut self) -> bool {
        let changed = !self.is_default();
        self.0 = DEFAULT;
        changed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    In,
    Out,
    Reset,
}

/// Ctrl/Cmd with `=` or `+` zooms in, with `-` zooms out, and with `0` resets. Shift is allowed
/// because `+` needs it on many layouts.
fn step_for(ev: &KeyEvent) -> Option<Step> {
    if ev.mods.alt {
        return None;
    }
    match ev.key {
        Key::Command('=' | '+') => Some(Step::In),
        Key::Command('-' | '_') => Some(Step::Out),
        Key::Command('0') if !ev.mods.shift => Some(Step::Reset),
        _ => None,
    }
}

fn apply(state: &mut State, step: Step, tx: &mut Tx) {
    let zoom = state.zoom_mut();
    let changed = match step {
        Step::In => zoom.zoom_in(),
        Step::Out => zoom.zoom_out(),
        Step::Reset => zoom.reset(),
    };
    if changed {
        // Land at once: text under a moving scale draws blurred until it settles.
        tx.instant();
    }
}

/// The zoom shortcuts. Returns whether `ev` was one, even at the first or last level.
pub fn zoom_key(state: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(step) = step_for(ev) else {
        return false;
    };
    apply(state, step, tx);
    true
}

/// The window's content at the current zoom. `content` lays out in `window / zoom` points and
/// draws `zoom` times its size from the top-left corner, so it fills the window exactly.
pub fn zoomed(state: &State, cx: &Cx<State>, background: rune::Color, content: El) -> El {
    let k = state.zoom().factor();
    let content = match cx.layout_size("window") {
        Some([w, h]) if k != 1.0 => content.w(w / k).h(h / k).scale(k).origin(0.0, 0.0),
        _ => content.w_full().h_full(),
    };
    col()
        .key("window")
        .w_full()
        .h_full()
        .fill(background)
        .child(content)
}

/// The toolbar's zoom control: zoom out, the level (press to reset), zoom in.
pub fn zoom_control(state: &State, kit: &Kit) -> El {
    let c = &kit.c;
    let zoom = state.zoom();
    let button =
        |key: &'static str, label: String, help: String, width: f32, enabled: bool, step| {
            let mut b = row()
                .key(key)
                .role("button")
                .label(help.clone())
                .help(help)
                .preserve_focus_on_press(true)
                .h(26.0)
                .min_w(width)
                .px(8.0)
                .center()
                .radius(5.0)
                .child(
                    text(label)
                        .font(TextStyle::new(12.5, 500.0))
                        .color(if enabled { c.text_base } else { c.text_muted })
                        .a11y_hidden(),
                );
            if enabled {
                b = kit
                    .interactive(b)
                    .on_press_tx(move |state: &mut State, tx| apply(state, step, tx));
            } else {
                b = b.disabled(true);
            }
            b
        };
    row()
        .key("zoom-control")
        .role("group")
        .label("Zoom")
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
        .child(button(
            "zoom-out",
            "−".into(),
            format!("Zoom out ({MOD}−)"),
            26.0,
            zoom.can_zoom_out(),
            Step::Out,
        ))
        .child(button(
            "zoom-reset",
            zoom.label(),
            format!("Zoom {}, reset ({MOD}0)", zoom.label()),
            52.0,
            !zoom.is_default(),
            Step::Reset,
        ))
        .child(button(
            "zoom-in",
            "+".into(),
            format!("Zoom in ({MOD}+)"),
            26.0,
            zoom.can_zoom_in(),
            Step::In,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::Mods;

    fn cmd(c: char, shift: bool) -> KeyEvent {
        KeyEvent {
            key: Key::Command(c),
            mods: Mods {
                cmd: true,
                shift,
                ..Mods::default()
            },
        }
    }

    #[test]
    fn levels_step_and_stop_at_the_ends() {
        let mut z = Zoom::default();
        assert_eq!(z.label(), "100%");
        assert!(z.zoom_in());
        assert_eq!(z.label(), "110%");
        while z.zoom_in() {}
        assert_eq!(z.factor(), 3.0);
        assert!(!z.zoom_in());
        while z.zoom_out() {}
        assert_eq!(z.factor(), 0.5);
        assert!(z.reset());
        assert!(z.is_default());
        assert!(!z.reset());
    }

    #[test]
    fn shortcuts_cover_plus_with_and_without_shift() {
        assert_eq!(step_for(&cmd('=', false)), Some(Step::In));
        assert_eq!(step_for(&cmd('+', true)), Some(Step::In));
        assert_eq!(step_for(&cmd('-', false)), Some(Step::Out));
        assert_eq!(step_for(&cmd('_', true)), Some(Step::Out));
        assert_eq!(step_for(&cmd('0', false)), Some(Step::Reset));
        assert_eq!(step_for(&cmd('e', false)), None);
    }
}
