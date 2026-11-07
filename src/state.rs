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

    pub fn set_mode(&mut self, mode: EditingMode) {
        self.view.mode = mode;
    }

    /// Activate the outermost rendered block containing this global UTF-8 byte offset.
    /// Invalid byte offsets and positions in non-rendered regions leave the editor untouched.
    pub fn activate_block_at(&mut self, offset: usize) -> bool {
        self.sync_document();
        if !self.editor.text.is_char_boundary(offset) {
            return false;
        }
        let Some(range) = self
            .blocks()
            .iter()
            .find(|block| block.source_range.contains(&offset))
            .map(|block| block.source_range.clone())
        else {
            return false;
        };
        *self.view.active_source_range.borrow_mut() = Some(range);
        self.editor.anchor = offset;
        self.editor.focus = offset;
        self.editor.focused = true;
        true
    }

    pub fn sync_document(&self) -> bool {
        if self.parsed.borrow().is_current(&self.editor.text) {
            return false;
        }
        self.parsed.borrow_mut().refresh(&self.editor.text);
        // Ranges belong to a source revision. Invalidate on edits until the bounded input
        // implementation can rebase and retain an edited region across block splits/merges.
        *self.view.active_source_range.borrow_mut() = None;
        true
    }
}
