//! A bounded view of the shared model. All stored selection and history offsets stay global.
use parley::Layout;

use super::*;
use crate::text::Ink;

/// A key that a bounded input ([`El::source_range`](crate::El::source_range)) would apply
/// beyond its range, offered to [`El::on_source_range_exit`](crate::El::on_source_range_exit)
/// before it applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SourceRangeExit {
    /// Caret movement leaving the range: an arrow past its first or last line or character, a
    /// word or document jump. `extend` when Shift extends the selection.
    Navigate { extend: bool },
    /// Select All, which the full input applies to the whole document.
    SelectAll,
    /// A deletion that would remove text outside the range, such as Backspace at its start.
    Delete,
}

impl TextEdit {
    /// How `ev` would act beyond the input range, judged on copies before it applies (history is
    /// copy-on-write, so a probe never touches the model's). `layout` is the bounded layout.
    /// Nothing exits during a composition, in a secure field, or without a range.
    pub(crate) fn source_range_exit(&self, ev: &KeyEvent, layout: Option<&Layout<Ink>>) -> Option<SourceRangeExit> {
        let bounds = self.input_range.clone()?;
        if self.secure || self.preedit.is_some() {
            return None;
        }
        let control = ev.control_binding();
        let navigates = matches!(control, Some('a' | 'b' | 'e' | 'f' | 'n' | 'p'))
            || (control.is_none()
                && !ev.is_command_chord()
                && matches!(ev.key, Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End));
        let deletes = matches!(control, Some('d' | 'h' | 'k'))
            || (control.is_none() && !ev.is_command_chord() && matches!(ev.key, Key::Backspace | Key::Delete));
        if control.is_none() && ev.key == Key::Command('a') && !ev.mods.shift {
            return (bounds != (0..self.text.len())).then_some(SourceRangeExit::SelectAll);
        }
        let outside = |i: usize| i < bounds.start || i > bounds.end;
        if navigates {
            let mut probe = self.clone();
            probe.key(ev, layout);
            // Up on a field's first line goes to its start and Down on its last to its end; at a
            // range's edge they leave it instead, for the line beyond.
            let vertical = match control {
                Some(c) => matches!(c, 'n' | 'p').then_some(c == 'n'),
                None if !ev.mods.cmd => matches!(ev.key, Key::Up | Key::Down).then_some(ev.key == Key::Down),
                None => None,
            };
            let at_edge = vertical.zip(layout).is_some_and(|(down, l)| {
                !self.is_hidden_multiline() && crate::text::on_edge_line(l, self.display_index(self.focus), down)
            });
            // A rich field's Right or Down can add a paragraph after a trailing block: not movement.
            let moved_out = probe.text == self.text && (at_edge || outside(probe.focus) || outside(probe.anchor));
            return moved_out.then_some(SourceRangeExit::Navigate { extend: ev.mods.shift });
        }
        if deletes {
            let mut probe = self.clone();
            probe.input_range = None;
            probe.key(ev, None);
            // A plain deletion leaves the caret where the removed text started.
            let removed = self.text.len().checked_sub(probe.text.len()).filter(|&n| n > 0)?;
            let start = probe.anchor.min(probe.focus);
            return (outside(start) || outside(start + removed)).then_some(SourceRangeExit::Delete);
        }
        None
    }

    /// The bounded source range while an input event or its callbacks are being dispatched.
    /// It is rebased by each replacement, including undo/redo, and is `None` outside dispatch
    /// on the app-owned model. An `on_input_change` callback can retain this exact range in view
    /// state, avoiding ambiguity when the document contains repeated text.
    pub fn input_source_range(&self) -> Option<Range<usize>> {
        self.input_range.clone()
    }

    pub(crate) fn set_input_range(&mut self, range: Option<Range<usize>>) {
        self.input_range = range.map(|r| {
            let start = Self::clamp_char_boundary(&self.text, r.start);
            start..Self::clamp_char_boundary(&self.text, r.end.max(start))
        });
    }

    /// Show the input range through `projection` (see [`SourceProjection`]). Ignored unless
    /// it was built for the current range and text.
    pub(crate) fn set_projection(&mut self, projection: Option<Arc<SourceProjection>>) {
        self.projection = projection;
    }

    pub(crate) fn projection(&self) -> Option<Arc<SourceProjection>> {
        self.projection.clone()
    }

    /// The projection, while it matches the range and text it was built for. Secure fields and
    /// unbounded inputs never project.
    pub(crate) fn active_projection(&self) -> Option<&SourceProjection> {
        let p = self.projection.as_deref()?;
        let range = self.input_range.as_ref()?;
        (!self.secure && p.applies(range, self.text.len())).then_some(p)
    }

    /// The display span the selection a composition replaces occupies, before the preedit.
    pub(super) fn preedit_display_span(&self, p: &SourceProjection) -> (usize, usize) {
        let (a, b) = self.range();
        (p.to_display(a), p.to_display(b))
    }

    /// The caret movement Left or Right makes through a projection, or `None` without one.
    /// Stops come from the displayed text; past its last stop the caret leaves the range.
    pub(super) fn projected_step(&self, forward: bool) -> Option<usize> {
        let p = self.active_projection().filter(|_| self.preedit.is_none())?;
        let r = self.input_bounds();
        let stop = if forward { p.stop_after(self.focus) } else { p.stop_before(self.focus) };
        Some(stop.unwrap_or_else(|| {
            if forward {
                if r.end < self.text.len() { next_char(&self.text, r.end) } else { r.end }
            } else if r.start > 0 {
                prev_char(&self.text, r.start)
            } else {
                r.start
            }
        }))
    }

    pub(crate) fn input_bounds(&self) -> Range<usize> {
        self.input_range.clone().map_or(0..self.text.len(), |r| {
            let start = Self::clamp_char_boundary(&self.text, r.start);
            start..Self::clamp_char_boundary(&self.text, r.end.max(start))
        })
    }

    /// A pill's visible part strictly containing this global byte boundary.
    pub(super) fn pill_around(&self, i: usize) -> Option<(usize, usize)> {
        let bounds = self.input_bounds();
        self.marks.iter().filter(|m| m.mark.is_pill()).find_map(|m| {
            let (start, end) = (m.start.max(bounds.start), m.end.min(bounds.end));
            (start < i && i < end).then_some((start, end))
        })
    }

    /// Rebase a view range after a buffer revision, including undo/redo or direct assignment.
    /// The changed interval is found from the common UTF-8 prefix and suffix. Insertions at
    /// either edge belong to the open view; replacements before it shift both endpoints.
    /// This changes no text, selection, composition or history.
    pub fn rebase_source_range(range: Range<usize>, before: &str, after: &str) -> Range<usize> {
        let start_bound = before.floor_char_boundary(range.start);
        let range = start_bound..before.floor_char_boundary(range.end.max(start_bound));
        let mut start = before.bytes().zip(after.bytes()).take_while(|(a, b)| a == b).count();
        start = before.floor_char_boundary(start);
        let suffix =
            before[start..].bytes().rev().zip(after[start..].bytes().rev()).take_while(|(a, b)| a == b).count();
        let end = before.ceil_char_boundary(before.len() - suffix);
        let inserted = after.len() - (before.len() - end) - start;
        Self::rebase_replacement(range, start..end, inserted)
    }

    pub(super) fn rebase_replacement(range: Range<usize>, edit: Range<usize>, inserted: usize) -> Range<usize> {
        let shift = |i: usize| i - edit.len() + inserted;
        let start = if range.start <= edit.start {
            range.start
        } else if range.start >= edit.end {
            shift(range.start)
        } else {
            edit.start
        };
        let end = if range.end < edit.start {
            range.end
        } else if range.end >= edit.end {
            shift(range.end)
        } else {
            edit.start + inserted
        };
        start..end.max(start)
    }

    pub(super) fn rebase_input_range(&mut self, edit: Range<usize>, inserted: usize) {
        if let Some(range) = self.input_range.take() {
            self.input_range = Some(Self::rebase_replacement(range, edit, inserted));
        }
    }

    /// Map global model bytes to local display bytes, including masking and composition.
    pub(crate) fn display_index(&self, byte: usize) -> usize {
        if let Some(p) = self.active_projection() {
            let d = p.to_display(byte);
            let Some((pre, _)) = &self.preedit else { return d };
            let (da, db) = self.preedit_display_span(p);
            return if d <= da { d } else if d < db { da } else { d - (db - da) + pre.len() };
        }
        let r = self.input_bounds();
        let byte = Self::clamp_char_boundary(&self.text, byte.clamp(r.start, r.end));
        if self.is_hidden_multiline() {
            if r.is_empty() { 0 } else { MULTILINE_SECURE_MASK.len() }
        } else if self.secure && !self.revealed {
            self.text[r.start..byte].chars().count() * '•'.len_utf8()
        } else if !self.secure
            && let Some((p, _)) = &self.preedit
        {
            let (a, b) = self.range();
            if byte <= a {
                byte - r.start
            } else if byte < b {
                a - r.start
            } else {
                byte - r.start - (b - a) + p.len()
            }
        } else {
            byte - r.start
        }
    }

    pub(crate) fn model_index(&self, byte: usize) -> usize {
        if let Some(p) = self.active_projection() {
            let Some((pre, _)) = &self.preedit else { return p.to_source(byte) };
            let (a, b) = self.range();
            let (da, db) = self.preedit_display_span(p);
            return if byte < da {
                p.to_source(byte)
            } else if byte < da + pre.len() {
                a
            } else if byte == da + pre.len() {
                b
            } else {
                p.to_source(byte - pre.len() + (db - da))
            };
        }
        let r = self.input_bounds();
        if self.is_hidden_multiline() {
            return r.end;
        }
        if self.secure && !self.revealed {
            return self.text[r.clone()].char_indices().nth(byte / '•'.len_utf8()).map_or(r.end, |(i, _)| r.start + i);
        }
        let model = if !self.secure
            && let Some((p, _)) = &self.preedit
        {
            let (a, b) = self.range();
            let start = a - r.start;
            if byte <= start {
                r.start + byte
            } else if byte < start + p.len() {
                a
            } else {
                r.start + byte - p.len() + (b - a)
            }
        } else {
            r.start.saturating_add(byte)
        };
        Self::clamp_char_boundary(&self.text, model.min(r.end))
    }

    /// [`Self::model_index`] choosing `side` where hidden bytes collapse, regardless of the
    /// projection's own preference (a line's first character is after its hidden prefix).
    pub(crate) fn model_index_with(&self, byte: usize, side: CollapseSide) -> usize {
        match self.active_projection() {
            Some(p) if self.preedit.is_none() => p.to_source_side(byte, side),
            _ => self.model_index(byte),
        }
    }

    pub(crate) fn display_selection_range(&self, a: usize, b: usize) -> (usize, usize) {
        if self.is_hidden_multiline() {
            if a == b || self.input_bounds().is_empty() { (0, 0) } else { (0, MULTILINE_SECURE_MASK.len()) }
        } else {
            (self.display_index(a), self.display_index(b))
        }
    }

    /// The selection as ordered byte offsets, clamped to character boundaries of `text` (an app
    /// may have replaced `text` without moving the caret).
    pub fn range(&self) -> (usize, usize) {
        let r = self.input_bounds();
        let clamp = |i: usize| Self::clamp_char_boundary(&self.text, i.clamp(r.start, r.end));
        (clamp(self.anchor.min(self.focus)), clamp(self.anchor.max(self.focus)))
    }

    /// Move the caret to `byte` (clamped to a character boundary of `self.text`) and collapse the
    /// selection, clearing any pending typing style.
    pub fn set_caret(&mut self, byte: usize) {
        let b = Self::clamp_char_boundary(&self.text, byte);
        self.anchor = b;
        self.focus = b;
        self.snap_to_pills();
        self.pending = None;
    }

    /// Set the selection `anchor` and `focus` byte indices (clamped to character boundaries of
    /// `self.text`), clearing any pending typing style. A selection that ends inside a
    /// [`Mark::Pill`] grows to cover it.
    pub fn set_selection(&mut self, anchor: usize, focus: usize) {
        self.anchor = Self::clamp_char_boundary(&self.text, anchor);
        self.focus = Self::clamp_char_boundary(&self.text, focus);
        self.snap_to_pills();
        self.pending = None;
    }

    /// Pointer and accessibility selections cannot grow into a pill outside the view.
    /// Navigation and restored history still retain their original global endpoints.
    pub(crate) fn set_view_selection(&mut self, anchor: usize, focus: usize, bounds: Range<usize>) {
        self.set_selection(anchor.clamp(bounds.start, bounds.end), focus.clamp(bounds.start, bounds.end));
        self.anchor = self.anchor.clamp(bounds.start, bounds.end);
        self.focus = self.focus.clamp(bounds.start, bounds.end);
    }
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
