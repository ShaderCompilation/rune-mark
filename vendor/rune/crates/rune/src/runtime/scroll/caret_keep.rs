//! `Tx::keep_caret_at_press` and `Tx::keep_caret_on_screen`: hold the focused field's caret
//! at a height in the window across a commit that reshapes the content around it, such as an
//! editor that swaps rendered blocks for their source.
use super::*;

/// Where a commit's caret should land vertically.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaretKeep {
    /// Where the press that caused the commit started.
    Press,
    /// Where the last frame showed the caret, if its viewport showed it.
    Caret,
}

impl<S: 'static> Runtime<S> {
    /// Before a commit: the window height the caret line's middle should keep.
    pub(super) fn capture_caret_keep(&self, tx: &mut Tx) -> Option<f32> {
        match tx.keep_caret.take()? {
            CaretKeep::Press => Some(self.press_at.1),
            CaretKeep::Caret => {
                let i = self.focused_index()?;
                let r = self.caret_rect(&self.nodes, i)?;
                let v = self.viewport_of(i)?;
                let [_, top, _, bottom] = self.nodes[v].target;
                let mid = r[1] + r[3] * 0.5;
                (top..=bottom).contains(&mid).then_some(mid)
            }
        }
    }

    /// After the commit: scroll the focused field's nearest vertical viewport so its caret line's
    /// middle sits at `y`, clamped to the content. Instant; any momentum or glide stops.
    pub(super) fn apply_caret_keep(&mut self, y: Option<f32>) {
        let Some(y) = y else { return };
        self.step_scroll(0.0);
        let Some(i) = self.focused_index() else { return };
        let Some(r) = self.caret_rect(&self.nodes, i) else { return };
        let Some(v) = self.viewport_of(i) else { return };
        let delta = f64::from(r[1] + r[3] * 0.5 - y);
        let stick = self.nodes[v].scroll.is_some_and(|s| s.stick_bottom);
        let Some(st) = self.scrolls.get_mut(&self.nodes[v].id) else { return };
        let to = (st.offset + delta).clamp(0.0, st.max);
        if (to - st.offset).abs() < 1e-5 { return; }
        st.offset = to;
        st.v = 0.0;
        st.glide = None;
        st.glide_to = None;
        st.pinned = stick && to >= st.max - 0.5;
        st.direct = true;
        if self.virtual_window_changed() {
            self.commit(&Tx { instant: true, ..Tx::default() });
            self.step_scroll(0.0);
        }
    }
}
