//! Editable syntax structure: the exact source ranges of Markdown markers and delimiters, from
//! the same pulldown-cmark walk (and the same numbered-list interruptions) that builds [`Block`].
//! Kept beside the blocks so `Block` keeps its shape.
//!
//! All positions are global UTF-8 byte offsets into the complete source. Ranges point at the
//! authored spelling, so marker characters, numbering, fence lengths and whitespace are read
//! back from the source rather than normalized.

use super::*;

mod build;
mod lines;

pub(super) use build::SyntaxBuilder;

/// The editable syntax of one parsed block. [`block_syntax`] returns one per top-level
/// [`Block`], in the same order; containers hold the syntax of their nested blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SyntaxNode {
    Paragraph(ParagraphSyntax),
    Heading(HeadingSyntax),
    /// A code block, including diff and mermaid fences.
    Code(CodeSyntax),
    List(ListSyntax),
    Quote(QuoteSyntax),
    /// A GFM table: its whole source range. Cell syntax is not retained.
    Table(Range<usize>),
    /// A thematic break: its whole source range.
    Rule(Range<usize>),
}

/// A paragraph, or the text of a tight list item.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ParagraphSyntax {
    pub range: Range<usize>,
    /// From the first to the last inline source byte (escape backslashes included).
    pub content: Range<usize>,
    pub inline: Vec<InlineSyntax>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct HeadingSyntax {
    /// Includes indentation, markers and the terminating newline (a setext heading includes
    /// its underline).
    pub range: Range<usize>,
    /// The authored level, 1 to 6 (the rendered block clamps deeper levels).
    pub level: u8,
    pub setext: bool,
    /// Indentation, `#` markers and the whitespace before the content. Empty for setext.
    pub open: Range<usize>,
    /// Trailing whitespace and an optional closing `#` sequence, or a setext underline.
    pub close: Range<usize>,
    pub content: Range<usize>,
    pub inline: Vec<InlineSyntax>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct CodeSyntax {
    /// A fenced block runs from its opening fence to the end of its closing fence (no newline).
    pub range: Range<usize>,
    /// The code text as written, including container prefixes on its lines. Empty (at the
    /// start of the second line) when the block has no text.
    pub content: Range<usize>,
    /// `None` for indented code.
    pub fence: Option<FenceSyntax>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FenceSyntax {
    /// The opening line from its fence, without the newline.
    pub opening: Range<usize>,
    /// The run of backticks or tildes, at its authored length.
    pub marker: Range<usize>,
    /// The trimmed info string, when there is one.
    pub info: Option<Range<usize>>,
    /// The closing line from its fence, without the newline. `None` while unclosed.
    pub closing: Option<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ListSyntax {
    pub range: Range<usize>,
    /// The first item's number for an ordered list.
    pub start: Option<u64>,
    pub items: Vec<ListItemSyntax>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ListItemSyntax {
    pub range: Range<usize>,
    /// The bullet (`-`, `*`, `+`) or the number with its delimiter (`7)`), as written.
    pub marker: Range<usize>,
    /// The authored number of an ordered item, which may differ from the displayed sequence.
    pub number: Option<u64>,
    pub task: Option<TaskSyntax>,
    /// Where the item's content starts on its first line (after a task marker and its
    /// space), or the end of that line when content starts below it.
    pub content_start: usize,
    /// One per source line of the item. The first line's prefix runs from the end of any
    /// enclosing prefixes through the marker and its spacing (excluding a task marker);
    /// later lines' prefixes are the indentation the item consumes.
    pub lines: Vec<ContainerLine>,
    pub children: Vec<SyntaxNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TaskSyntax {
    /// `[ ]` or `[x]`.
    pub range: Range<usize>,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct QuoteSyntax {
    /// From this quote's `>` (after enclosing prefixes), including nested content.
    pub range: Range<usize>,
    /// One per source line. A prefix is this quote's `>` and one following space or tab.
    pub lines: Vec<ContainerLine>,
    pub children: Vec<SyntaxNode>,
}

/// One source line of a list item or quote.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ContainerLine {
    /// From the start of the source line to its end, without the newline.
    pub line: Range<usize>,
    /// The bytes this container consumes on the line, after any enclosing prefixes. `None`
    /// for a lazy continuation line, which has no prefix of its own.
    pub prefix: Option<Range<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InlineSyntaxKind {
    Emphasis,
    Strong,
    Strikethrough,
    Code,
    Link,
    Image,
    /// `<https://…>` or `<user@host>`.
    Autolink,
    /// A backslash escape: `open` is the backslash.
    Escape,
    /// A decoded entity or numeric character reference: `open` is its whole spelling.
    Entity,
    /// Two or more spaces, or a backslash, before a newline: `open` is that spelling.
    HardBreak,
}

/// One inline construct. Ranges nest; `parent` indexes the enclosing construct in the same
/// list, which is ordered by start (an enclosing construct comes before its contents).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct InlineSyntax {
    pub kind: InlineSyntaxKind,
    pub range: Range<usize>,
    /// The opening delimiter as written (`**`, `` ` ``, `[`, `![`, `<`).
    pub open: Range<usize>,
    /// The closing delimiter as written, including a link's whole `](…)` or `][…]` tail.
    pub close: Range<usize>,
    pub parent: Option<usize>,
    pub link: Option<LinkSyntax>,
}

impl InlineSyntax {
    /// Between the delimiters. Code span content keeps its authored padding and line breaks.
    pub fn content(&self) -> Range<usize> {
        self.open.end..self.close.start.max(self.open.end)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LinkSyntaxKind {
    /// `[label](destination "title")`
    Inline,
    /// `[label][reference]`
    Reference,
    /// `[label][]`
    Collapsed,
    /// `[label]`
    Shortcut,
    Autolink,
    Email,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LinkSyntax {
    pub kind: LinkSyntaxKind,
    /// As written, including angle brackets. The destination of a reference link lives in its
    /// definition, which is not part of the link.
    pub destination: Option<Range<usize>>,
    /// As written, including its quotes or parentheses.
    pub title: Option<Range<usize>>,
    /// The label inside a full reference link's second brackets.
    pub reference: Option<Range<usize>>,
}

/// A flattened piece of syntax a projection can hide or reveal (see [`SyntaxNode::marks`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SyntaxMarkKind {
    HeadingMarker,
    /// A whole opening or closing fence line, with its info string.
    Fence,
    QuotePrefix,
    /// A list item's first-line prefix and task marker, up to its content.
    ListMarker,
    /// Indentation a list item consumes on a continuation line.
    ItemIndent,
    /// An inline construct's opening or closing delimiter.
    Delimiter(InlineSyntaxKind),
    /// An escape, entity or hard break, whose rendering differs from its spelling.
    Transformed(InlineSyntaxKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxMark {
    pub kind: SyntaxMarkKind,
    pub range: Range<usize>,
}

impl SyntaxNode {
    pub fn range(&self) -> Range<usize> {
        match self {
            SyntaxNode::Paragraph(p) => p.range.clone(),
            SyntaxNode::Heading(h) => h.range.clone(),
            SyntaxNode::Code(c) => c.range.clone(),
            SyntaxNode::List(l) => l.range.clone(),
            SyntaxNode::Quote(q) => q.range.clone(),
            SyntaxNode::Table(r) | SyntaxNode::Rule(r) => r.clone(),
        }
    }

    /// The nonempty syntax ranges of this node and everything nested in it, in document order.
    /// Link destinations, titles and references are inside their link's closing delimiter.
    pub fn marks(&self) -> Vec<SyntaxMark> {
        let mut out = Vec::new();
        self.collect_marks(&mut out);
        out.retain(|m| !m.range.is_empty());
        out.sort_by_key(|m| (m.range.start, m.range.end));
        out
    }

    fn collect_marks(&self, out: &mut Vec<SyntaxMark>) {
        let mut mark = |kind, range: &Range<usize>| out.push(SyntaxMark { kind, range: range.clone() });
        match self {
            SyntaxNode::Paragraph(p) => inline_marks(&p.inline, out),
            SyntaxNode::Heading(h) => {
                mark(SyntaxMarkKind::HeadingMarker, &h.open);
                mark(SyntaxMarkKind::HeadingMarker, &h.close);
                inline_marks(&h.inline, out);
            }
            SyntaxNode::Code(c) => {
                if let Some(f) = &c.fence {
                    mark(SyntaxMarkKind::Fence, &f.opening);
                    if let Some(closing) = &f.closing {
                        mark(SyntaxMarkKind::Fence, closing);
                    }
                }
            }
            SyntaxNode::List(l) => {
                for item in &l.items {
                    for (i, line) in item.lines.iter().enumerate() {
                        let Some(prefix) = &line.prefix else { continue };
                        if i == 0 {
                            out.push(SyntaxMark {
                                kind: SyntaxMarkKind::ListMarker,
                                range: prefix.start..item.content_start.max(prefix.end),
                            });
                        } else {
                            out.push(SyntaxMark { kind: SyntaxMarkKind::ItemIndent, range: prefix.clone() });
                        }
                    }
                    for child in &item.children {
                        child.collect_marks(out);
                    }
                }
            }
            SyntaxNode::Quote(q) => {
                for prefix in q.lines.iter().filter_map(|l| l.prefix.as_ref()) {
                    out.push(SyntaxMark { kind: SyntaxMarkKind::QuotePrefix, range: prefix.clone() });
                }
                for child in &q.children {
                    child.collect_marks(out);
                }
            }
            SyntaxNode::Table(_) | SyntaxNode::Rule(_) => {}
        }
    }

    pub(super) fn shift(&mut self, d: usize) {
        match self {
            SyntaxNode::Paragraph(p) => {
                shift_all(d, [&mut p.range, &mut p.content]);
                p.inline.iter_mut().for_each(|i| i.shift(d));
            }
            SyntaxNode::Heading(h) => {
                shift_all(d, [&mut h.range, &mut h.open, &mut h.close, &mut h.content]);
                h.inline.iter_mut().for_each(|i| i.shift(d));
            }
            SyntaxNode::Code(c) => {
                shift_all(d, [&mut c.range, &mut c.content]);
                if let Some(f) = &mut c.fence {
                    shift_all(d, [&mut f.opening, &mut f.marker]);
                    shift_all(d, f.info.iter_mut().chain(f.closing.iter_mut()));
                }
            }
            SyntaxNode::List(l) => {
                shift_all(d, [&mut l.range]);
                for item in &mut l.items {
                    shift_all(d, [&mut item.range, &mut item.marker]);
                    item.content_start += d;
                    if let Some(t) = &mut item.task {
                        shift_all(d, [&mut t.range]);
                    }
                    item.lines.iter_mut().for_each(|l| l.shift(d));
                    item.children.iter_mut().for_each(|c| c.shift(d));
                }
            }
            SyntaxNode::Quote(q) => {
                shift_all(d, [&mut q.range]);
                q.lines.iter_mut().for_each(|l| l.shift(d));
                q.children.iter_mut().for_each(|c| c.shift(d));
            }
            SyntaxNode::Table(r) | SyntaxNode::Rule(r) => shift_all(d, [r]),
        }
    }
}

fn inline_marks(inline: &[InlineSyntax], out: &mut Vec<SyntaxMark>) {
    for i in inline {
        match i.kind {
            InlineSyntaxKind::Escape | InlineSyntaxKind::Entity | InlineSyntaxKind::HardBreak => {
                out.push(SyntaxMark { kind: SyntaxMarkKind::Transformed(i.kind), range: i.open.clone() })
            }
            kind => {
                out.push(SyntaxMark { kind: SyntaxMarkKind::Delimiter(kind), range: i.open.clone() });
                out.push(SyntaxMark { kind: SyntaxMarkKind::Delimiter(kind), range: i.close.clone() });
            }
        }
    }
}

impl InlineSyntax {
    fn shift(&mut self, d: usize) {
        shift_all(d, [&mut self.range, &mut self.open, &mut self.close]);
        if let Some(l) = &mut self.link {
            shift_all(d, l.destination.iter_mut().chain(l.title.iter_mut()).chain(l.reference.iter_mut()));
        }
    }
}

impl ContainerLine {
    fn shift(&mut self, d: usize) {
        shift_all(d, std::iter::once(&mut self.line).chain(self.prefix.iter_mut()));
    }
}

fn shift_all<'r>(d: usize, ranges: impl IntoIterator<Item = &'r mut Range<usize>>) {
    for r in ranges {
        r.start += d;
        r.end += d;
    }
}
