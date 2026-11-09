use std::cell::{Cell, Ref, RefCell};
use std::ops::Range;

use rune::{Key, KeyEvent, SourceRangeExit, TextEdit};
use rune_kit::markdown::SourceClick;

use crate::markdown::{BlockMetadata, ParsedDocument};
use crate::zoom::Zoom;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditingMode {
    #[default]
    LivePreview,
    Source,
}

/// How Source mode lays out the document: the source alone, or the source on the left with a
/// rendered preview on the right. Live Preview returns to the last one chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceLayout {
    #[default]
    Full,
    Split,
}

/// Presentation state derived from, but never stored in, the editor. Interior mutability lets
/// a render normalize it against the current buffer and selection.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ViewState {
    mode: Cell<EditingMode>,
    source_layout: Cell<SourceLayout>,
    active_source_range: RefCell<Option<Range<usize>>>,
    /// An undo or redo key is being applied; its resulting selection decides the mode.
    history_step: Cell<bool>,
    /// A rendered-text click that arrived during a composition, applied once it ends.
    deferred_click: RefCell<Option<SourceClick>>,
}

impl ViewState {
    pub fn mode(&self) -> EditingMode {
        self.mode.get()
    }

    pub fn source_layout(&self) -> SourceLayout {
        self.source_layout.get()
    }

    pub fn active_source_range(&self) -> Option<Range<usize>> {
        self.active_source_range.borrow().clone()
    }
}

/// Whether a selection belongs to an active range. A range's final newline ends it: the caret
/// after it starts the next region.
fn holds(text: &str, range: &Range<usize>, (a, b): (usize, usize)) -> bool {
    range.start <= a
        && (b < range.end
            || (b == range.end && (range.is_empty() || !text[..range.end].ends_with('\n'))))
}

pub struct State {
    /// The only authoritative document, caret, selection, composition, and undo/redo history.
    pub editor: TextEdit,
    view: ViewState,
    parsed: RefCell<ParsedDocument>,
    /// Window-wide zoom; it never touches the document or view state.
    zoom: Zoom,
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
            zoom: Zoom::default(),
        }
    }

    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    pub fn zoom_mut(&mut self) -> &mut Zoom {
        &mut self.zoom
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

    /// Change mode without touching the editor. Source mode has no active region; returning to
    /// Live Preview activates the region at the caret on the next sync.
    pub fn set_mode(&mut self, mode: EditingMode) {
        self.view.mode.set(mode);
        if mode == EditingMode::Source {
            self.deactivate_block();
        }
    }

    /// Choose how Source mode lays out the document. Neither the editor nor the mode changes.
    pub fn set_source_layout(&mut self, layout: SourceLayout) {
        self.view.source_layout.set(layout);
    }

    /// Switch between Live Preview and Source, keeping the document, caret, focus and history.
    /// A selection that spans regions collapses to its focus end, the caret Live Preview shows.
    /// Waits for an open composition: returns false and changes nothing.
    pub fn toggle_mode(&mut self) -> bool {
        if self.editor.preedit.is_some() {
            return false;
        }
        self.sync_document();
        match self.view.mode.get() {
            EditingMode::LivePreview => self.set_mode(EditingMode::Source),
            EditingMode::Source => {
                let (a, b) = (
                    self.editor.anchor.min(self.editor.focus),
                    self.editor.anchor.max(self.editor.focus),
                );
                let fits = self
                    .region_for_caret(a)
                    .is_some_and(|r| holds(&self.editor.text, &r, (a, b)));
                if !fits {
                    self.editor.anchor = self.editor.focus;
                }
                self.set_mode(EditingMode::LivePreview);
                self.sync_document();
            }
        }
        true
    }

    /// Select the whole document in Source mode and focus it. Changes no text or history.
    pub fn select_all(&mut self) -> bool {
        if self.editor.preedit.is_some() {
            return false;
        }
        self.set_mode(EditingMode::Source);
        self.editor.anchor = 0;
        self.editor.focus = self.editor.text.len();
        self.editor.focused = true;
        true
    }

    /// Activate the source region at this global UTF-8 byte boundary, including blank lines,
    /// hidden source and EOF. Invalid byte offsets leave the editor untouched.
    pub fn activate_block_at(&mut self, offset: usize) -> bool {
        self.sync_document();
        let Some(range) = self.region_for_caret(offset) else {
            return false;
        };
        self.activate(range, offset);
        true
    }

    /// A click on rendered Live Preview text, a link, a task marker or a gap: activate its
    /// region and focus the mapped source position. During a composition the click waits
    /// until the input method commits or cancels.
    pub fn activate_click(&mut self, click: SourceClick) -> bool {
        if self.editor.preedit.is_some() {
            *self.view.deferred_click.borrow_mut() = Some(click);
            return false;
        }
        self.sync_document();
        let (range, at) = (click.source_range, click.position);
        let current = self
            .parsed
            .borrow()
            .regions()
            .iter()
            .any(|r| r.source_range == range);
        if current && range.start <= at && at <= range.end && self.editor.text.is_char_boundary(at)
        {
            self.activate(range, at);
            return true;
        }
        self.activate_block_at(at)
    }

    /// The part of the active region the Live Preview input shows: its Markdown without trailing
    /// line breaks, which would otherwise draw as an empty last line, unless the selection is on
    /// one (a loose list's separating blank line, say). Vertical movement past the last shown
    /// line then reaches the next block.
    pub fn input_range(&self) -> Option<Range<usize>> {
        let view = self.view();
        if view.mode() != EditingMode::LivePreview {
            return None;
        }
        let range = view.active_source_range()?;
        let text = &self.editor.text;
        let shown = range.start + text[range.clone()].trim_end_matches(['\n', '\r']).len();
        let b = self.editor.anchor.max(self.editor.focus);
        let end = if (range.start..range.end).contains(&b) {
            text.ceil_char_boundary(shown.max(b))
        } else {
            shown
        };
        Some(range.start..end)
    }

    /// Explicitly leave a source region without changing document bytes or history.
    pub fn deactivate_block(&mut self) {
        *self.view.active_source_range.borrow_mut() = None;
    }

    /// The bounded input's key filter, before the editor applies the key.
    pub fn input_key(&mut self, ev: &KeyEvent) -> bool {
        self.view
            .history_step
            .set(matches!(ev.key, Key::Command('z')));
        false
    }

    /// A key would leave the active region. Plain navigation moves into the adjacent region,
    /// which the next sync activates. Selection beyond a block, Select All and deletions across
    /// its boundary switch to Source mode first. Either way the key then applies to the whole
    /// buffer, exactly as in Source mode.
    pub fn source_range_exit(&mut self, exit: SourceRangeExit) -> bool {
        if self.editor.preedit.is_some() {
            return false;
        }
        if exit != (SourceRangeExit::Navigate { extend: false }) {
            self.set_mode(EditingMode::Source);
        }
        true
    }

    /// Apply view changes that waited for a composition to commit or cancel.
    pub fn composition_ended(&mut self) {
        let click = self.view.deferred_click.borrow_mut().take();
        if let Some(click) = click {
            self.activate_click(click);
        }
        self.sync_document();
    }

    fn activate(&mut self, range: Range<usize>, caret: usize) {
        *self.view.active_source_range.borrow_mut() = Some(range);
        self.editor.anchor = caret;
        self.editor.focus = caret;
        self.editor.focused = true;
    }

    /// The editable region for a caret: the region containing it, else a final region it ends
    /// (text without a trailing newline), else the EOF insertion region.
    fn region_for_caret(&self, offset: usize) -> Option<Range<usize>> {
        let text = &self.editor.text;
        if !text.is_char_boundary(offset) {
            return None;
        }
        let parsed = self.parsed.borrow();
        let regions = parsed.regions();
        regions
            .iter()
            .find(|r| r.source_range.contains(&offset))
            .or_else(|| {
                regions.iter().find(|r| {
                    !r.source_range.is_empty()
                        && r.source_range.end == offset
                        && !text[..offset].ends_with('\n')
                })
            })
            .or_else(|| {
                regions
                    .iter()
                    .find(|r| offset == text.len() && r.source_range.is_empty())
            })
            .map(|r| r.source_range.clone())
    }

    /// Keep the view consistent with the editor after any change, including navigation, undo
    /// and focus loss. Deferred while a composition is open.
    fn normalize_view(&self) {
        if self.editor.preedit.is_some() {
            return;
        }
        let history = self.view.history_step.replace(false);
        if self.view.mode.get() != EditingMode::LivePreview {
            return;
        }
        if !self.editor.focused {
            *self.view.active_source_range.borrow_mut() = None;
            return;
        }
        let (a, b) = (
            self.editor.anchor.min(self.editor.focus),
            self.editor.anchor.max(self.editor.focus),
        );
        let inside = |r: &Range<usize>| holds(&self.editor.text, r, (a, b));
        if self
            .view
            .active_source_range
            .borrow()
            .as_ref()
            .is_some_and(inside)
        {
            return;
        }
        // Undo and redo restore selections anywhere; Live Preview cannot show one outside the
        // active region. Otherwise a caret that moved on activates the region it reached.
        let next = (!history)
            .then(|| self.region_for_caret(a))
            .flatten()
            .filter(inside);
        if next.is_none() {
            self.view.mode.set(EditingMode::Source);
        }
        *self.view.active_source_range.borrow_mut() = next;
    }

    pub fn sync_document(&self) -> bool {
        if self.parsed.borrow().is_current(&self.editor.text) {
            self.normalize_view();
            return false;
        }
        let active = self.view.active_source_range.borrow().clone();
        let rebased = active.map(|range| {
            self.editor
                .input_source_range()
                .unwrap_or_else(|| self.parsed.borrow().rebase_range(range, &self.editor.text))
        });
        if let Some(click) = self.view.deferred_click.borrow_mut().as_mut() {
            let parsed = self.parsed.borrow();
            let at = click.position;
            click.position = parsed.rebase_range(at..at, &self.editor.text).start;
            click.source_range = parsed.rebase_range(click.source_range.clone(), &self.editor.text);
        }
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
        self.normalize_view();
        true
    }
}
