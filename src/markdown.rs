//! Read-only metadata derived from the shared editor buffer.

use std::ops::Range;
use std::rc::Rc;

use rune_kit::markdown::{
    Block, SourceRegion, SyntaxNode, block_source_ranges, block_syntax, parse_cached,
    source_regions,
};

use crate::reveal::{Focus, Reveal, reveal};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Paragraph,
    Heading(u8),
    Code,
    List,
    Quote,
    Rule,
    Table,
    Diff,
    Mermaid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockMetadata {
    pub kind: BlockKind,
    /// UTF-8 byte offsets into the complete Markdown source, including block markers.
    /// Lists and quotes include all their nested content.
    pub source_range: Range<usize>,
}

pub struct ParsedDocument {
    // A read-only cache key, never an editing buffer or a history snapshot.
    source: String,
    blocks: Vec<BlockMetadata>,
    regions: Vec<SourceRegion>,
    /// Parallel to `blocks`, shared with Rune Kit's parse cache.
    syntax: Rc<[SyntaxNode]>,
}

impl ParsedDocument {
    pub fn new(source: &str) -> Self {
        let parsed = parse_cached(source);
        let ranges = block_source_ranges(source);
        let blocks = parsed
            .iter()
            .zip(ranges.iter())
            .map(|(block, range)| BlockMetadata {
                kind: match block {
                    Block::Para(_) => BlockKind::Paragraph,
                    Block::Heading(level, _) => BlockKind::Heading(*level),
                    Block::Code(_) => BlockKind::Code,
                    Block::List { .. } => BlockKind::List,
                    Block::Quote(_) => BlockKind::Quote,
                    Block::Rule => BlockKind::Rule,
                    Block::Table(_) => BlockKind::Table,
                    Block::Diff(_) => BlockKind::Diff,
                    Block::Mermaid(_) => BlockKind::Mermaid,
                },
                source_range: range.clone(),
            })
            .collect();
        Self {
            source: source.to_owned(),
            blocks,
            regions: source_regions(source),
            syntax: block_syntax(source),
        }
    }

    /// Reparse only when the source changes; selection and mode changes reuse metadata.
    pub fn refresh(&mut self, source: &str) -> bool {
        if self.is_current(source) {
            return false;
        }
        *self = Self::new(source);
        true
    }

    pub fn blocks(&self) -> &[BlockMetadata] {
        &self.blocks
    }

    pub fn regions(&self) -> &[SourceRegion] {
        &self.regions
    }

    /// Editable syntax for each block: marker, fence, prefix and delimiter ranges.
    pub fn syntax(&self) -> &[SyntaxNode] {
        &self.syntax
    }

    /// The syntax Live Preview reveals for `focus` (`None` when the editor is unfocused).
    /// Uses the cached syntax: a caret or selection change never reparses.
    pub fn reveal(&self, focus: Option<Focus>) -> Reveal {
        reveal(&self.syntax, focus)
    }

    pub(crate) fn rebase_range(&self, range: Range<usize>, source: &str) -> Range<usize> {
        rune::TextEdit::rebase_source_range(range, &self.source, source)
    }

    pub(crate) fn is_current(&self, source: &str) -> bool {
        self.source == source
    }
}
