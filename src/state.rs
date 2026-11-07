use std::cell::{Ref, RefCell};
use std::ops::Range;

use rune::TextEdit;

use crate::markdown::{BlockMetadata, ParsedDocument};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditingMode {
    LivePreview,
    // The current split layout still uses the full source editor.
    #[default]
    Source,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ViewState {
    mode: EditingMode,
    active_source_range: RefCell<Option<Range<usize>>>,
}

impl ViewState {
    pub fn mode(&self) -> EditingMode {
        self.mode
    }

    pub fn active_source_range(&self) -> Option<Range<usize>> {
        self.active_source_range.borrow().clone()
    }
}

pub struct State {
    /// The only authoritative document, caret, selection, composition, and undo/redo history.
    pub editor: TextEdit,
    view: ViewState,
    parsed: RefCell<ParsedDocument>,
}

impl State {
    pub fn new(source: &str) -> Self {
        let mut editor = TextEdit::new(source);
        // Request focus once, at the top, rather than resetting it on every render.
        editor.anchor = 0;
        editor.focus = 0;
        editor.focused = true;
        Self {
            editor,
            view: ViewState::default(),
            parsed: RefCell::new(ParsedDocument::new(source)),
        }
    }

    pub fn view(&self) -> &ViewState {
        self.sync_document();
        &self.view
    }

    /// Read-only metadata for the current buffer, including Rune edits in the current frame.
    /// Interior mutability is limited to derived metadata and its source-revision validity.
    pub fn blocks(&self) -> Ref<'_, [BlockMetadata]> {
        self.sync_document();
        Ref::map(self.parsed.borrow(), ParsedDocument::blocks)
    }

    /// Derived editable regions, including source lines that have no rendered block.
    pub fn regions(&self) -> Ref<'_, [rune_kit::markdown::SourceRegion]> {
        self.sync_document();
        Ref::map(self.parsed.borrow(), ParsedDocument::regions)
    }

    pub fn set_mode(&mut self, mode: EditingMode) {
        self.view.mode = mode;
    }

    /// Activate the source region at this global UTF-8 byte boundary, including blank lines,
    /// hidden source and EOF. Invalid byte offsets leave the editor untouched.
    pub fn activate_block_at(&mut self, offset: usize) -> bool {
        self.sync_document();
        if !self.editor.text.is_char_boundary(offset) {
            return false;
        }
        let Some(range) = self
            .regions()
            .iter()
            .find(|region| {
                region.source_range.contains(&offset)
                    || (offset == self.editor.text.len() && region.source_range.is_empty())
            })
            .map(|region| region.source_range.clone())
        else {
            return false;
        };
        *self.view.active_source_range.borrow_mut() = Some(range);
        self.editor.anchor = offset;
        self.editor.focus = offset;
        self.editor.focused = true;
        true
    }

    /// Explicitly leave a source region without changing document bytes or history.
    pub fn deactivate_block(&mut self) {
        *self.view.active_source_range.borrow_mut() = None;
    }

    fn close_departed_region(&self) {
        if self.editor.preedit.is_some() {
            return;
        }
        let mut active = self.view.active_source_range.borrow_mut();
        if active.as_ref().is_some_and(|r| {
            !self.editor.focused
                || self.editor.anchor < r.start
                || self.editor.anchor > r.end
                || self.editor.focus < r.start
                || self.editor.focus > r.end
        }) {
            *active = None;
        }
    }

    pub fn sync_document(&self) -> bool {
        if self.parsed.borrow().is_current(&self.editor.text) {
            self.close_departed_region();
            return false;
        }
        let active = self.view.active_source_range.borrow().clone();
        let rebased = active.map(|range| {
            self.editor
                .input_source_range()
                .unwrap_or_else(|| self.parsed.borrow().rebase_range(range, &self.editor.text))
        });
        self.parsed.borrow_mut().refresh(&self.editor.text);
        if let Some(mut range) = rebased {
            // Keep all of a region after splits, and absorb any parsed container that now
            // overlaps it. Do not shrink to the block containing the caret on every edit.
            for region in self.parsed.borrow().regions() {
                let r = &region.source_range;
                if (r.start < range.end && range.start < r.end)
                    || (range.is_empty() && r.contains(&range.start))
                {
                    range.start = range.start.min(r.start);
                    range.end = range.end.max(r.end);
                }
            }
            *self.view.active_source_range.borrow_mut() = Some(range);
        }
        self.close_departed_region();
        true
    }
}
