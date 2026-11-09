//! Keys that would leave a bounded input (`El::source_range`), offered to the app first.
use super::*;

/// A pending vertical move out of a projected field: the window x the caret left from, where
/// it lands on the next field's line once that field is laid out.
#[derive(Debug, Default)]
pub(super) struct SourceRangeMove {
    pub(super) goal: Option<f32>,
    /// The last goal and the caret it produced: the next vertical move from that same caret
    /// keeps the goal, so passing through a blank line or a short line keeps the column.
    pub(super) sticky: Option<(f32, usize)>,
}

/// How a key a bounded field offered to `on_source_range_exit` applies.
pub(super) enum ExitKey {
    /// Within the range, as if no hook were installed.
    Bounded,
    /// To the whole buffer.
    Whole,
    /// Up or Down out of a projected field: the caret moves to this position just beyond the
    /// range (the end of the line before it, or the start of the line after it), then lands at
    /// its horizontal place on that line once the field showing it is laid out
    /// (`apply_caret_goal`). A projected field's styled lines don't match a plain layout of the
    /// whole text, so that layout can't say where the line beyond is.
    Line(usize),
}

impl<S: 'static> Runtime<S> {
    /// Offers a bounded field's key that would act beyond its range to `on_source_range_exit`,
    /// before it applies, and says how it applies after the app's answer.
    pub(super) fn offer_source_range_exit(&mut self, ev: &KeyEvent, layout: Option<&Layout<Ink>>, tx: &mut Tx) -> ExitKey {
        if !matches!(ev.key, crate::edit::Key::Up | crate::edit::Key::Down) && !matches!(ev.control_binding(), Some('n' | 'p')) {
            self.source_range.sticky = None;
        }
        let Some((hook, access, range, single_line, projection)) = self.focused_input().and_then(|i| {
            Some((i.range_exit.clone()?, i.access.clone()?, i.edit.input_source_range()?, i.single_line, i.edit.projection()))
        }) else {
            return ExitKey::Bounded;
        };
        let projected = projection.is_some();
        let exit = {
            // The classifier copies the model only for keys that can leave the range.
            let edit = access(states!(self));
            let previous = (edit.input_source_range(), edit.projection());
            edit.set_single_line(single_line);
            edit.set_input_range(Some(range.clone()));
            edit.set_projection(projection);
            let exit = edit.source_range_exit(ev, layout);
            edit.set_input_range(previous.0);
            edit.set_projection(previous.1);
            exit
        };
        let Some(exit) = exit else { return ExitKey::Bounded };
        log_event!(self, "source range exit {exit:?}");
        if !hook(states!(self), exit, tx) {
            return ExitKey::Bounded;
        }
        let vertical = match ev.control_binding() {
            Some(c) => matches!(c, 'n' | 'p').then_some(c == 'n'),
            None if !ev.mods.cmd && !ev.mods.alt => matches!(ev.key, crate::edit::Key::Up | crate::edit::Key::Down).then_some(ev.key == crate::edit::Key::Down),
            None => None,
        };
        let Some(down) = vertical.filter(|_| projected && exit == (crate::edit::SourceRangeExit::Navigate { extend: false })) else {
            return ExitKey::Whole;
        };
        let Some(access) = self.focused_input().and_then(|i| i.access.clone()) else { return ExitKey::Whole };
        let focus = access(states!(self)).focus;
        let goal = match self.source_range.sticky {
            Some((x, at)) if at == focus => Some(x),
            _ => self.focused_index().and_then(|i| self.caret_rect(&self.nodes, i)).map(|r| r[0]),
        };
        let text = &access(states!(self)).text;
        let to = if down {
            text[range.end..].find('\n').map_or(text.len(), |i| range.end + i + 1)
        } else {
            text[..range.start].rfind('\n').unwrap_or(0)
        };
        self.source_range.goal = goal;
        ExitKey::Line(to)
    }

    /// The focused field's layout for the whole model text at its current width, for a key a
    /// bounded field applies unbounded: vertical movement then follows the full document.
    pub(super) fn focused_unbounded_layout(&mut self) -> Option<Rc<Layout<Ink>>> {
        let path = self.focused.clone()?;
        let access = self.focused_input()?.access.clone()?;
        let mut edit = access(states!(self)).clone();
        edit.set_input_range(None);
        edit.set_projection(None);
        let s = self.env.scale;
        let n = self.node(&path)?;
        let (mut spec, w) = (n.text.clone()?, n.target[2] - n.target[0] - n.inset[0] - n.inset[2]);
        spec.replace_content(edit.display().0);
        // The bounded field's styling is indexed by its range; plain text keeps line metrics.
        spec.spans.clear();
        spec.strikes.clear();
        spec.blocks.clear();
        spec.soft_breaks.clear();
        spec.line_boxes.clear();
        Some(self.text.layout_for_spec(&spec, s, spec.wrap.then_some(w * s), line_alignment(&spec)))
    }
}
