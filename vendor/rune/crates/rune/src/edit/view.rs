//! A bounded view of the shared model. All stored selection and history offsets stay global.
use super::*;

impl TextEdit {
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
