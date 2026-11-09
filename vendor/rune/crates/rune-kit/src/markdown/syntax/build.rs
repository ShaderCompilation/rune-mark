//! Building syntax nodes, driven by the parser's event walk.

use pulldown_cmark::LinkType;

use super::lines::Lines;
use super::*;

/// Watches the same events as the block builder. The builder calls [`SyntaxBuilder::observe`]
/// before handling an end event and after handling any other, so a block flushed by a start
/// event still sees its own state; it takes a node whenever it pushes a block, which keeps
/// nodes parallel to blocks.
pub(in crate::markdown) struct SyntaxBuilder<'a> {
    src: &'a str,
    frames: Vec<Frame>,
    inline: Option<InlineState>,
    code: Option<(Range<usize>, bool, Option<Range<usize>>)>,
}

struct Frame {
    kind: FrameKind,
    range: Range<usize>,
    children: Vec<SyntaxNode>,
    items: Vec<ListItemSyntax>,
}

enum FrameKind {
    Root,
    Quote,
    List(Option<u64>),
    Item(Option<TaskSyntax>),
    Table,
}

pub(in crate::markdown) struct InlineState {
    block: Option<Range<usize>>,
    heading: Option<u8>,
    content: Option<Range<usize>>,
    /// The end of the last text-like event, to tell an escape's backslash from literal text.
    text_end: usize,
    runs: Vec<Run>,
    done: Vec<InlineSyntax>,
}

struct Run {
    index: usize,
    first: Option<usize>,
    last: Option<usize>,
}

impl<'a> SyntaxBuilder<'a> {
    pub(in crate::markdown) fn new(src: &'a str) -> Self {
        let root = Frame { kind: FrameKind::Root, range: 0..src.len(), children: Vec::new(), items: Vec::new() };
        SyntaxBuilder { src, frames: vec![root], inline: None, code: None }
    }

    pub(in crate::markdown) fn observe(&mut self, ev: &Event<'_>, range: &Range<usize>) {
        let src = self.src;
        let range = range.clone();
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => self.inline = Some(InlineState::new(Some(range), None)),
                Tag::Heading { level, .. } => self.inline = Some(InlineState::new(Some(range), Some(*level as u8))),
                Tag::TableCell => self.inline = None,
                Tag::CodeBlock(kind) => self.code = Some((range, matches!(kind, CodeBlockKind::Fenced(_)), None)),
                Tag::BlockQuote(_) => self.push_frame(FrameKind::Quote, range),
                Tag::List(start) => self.push_frame(FrameKind::List(*start), range),
                Tag::Item => self.push_frame(FrameKind::Item(None), range),
                Tag::Table(_) => self.push_frame(FrameKind::Table, range),
                Tag::Emphasis => self.inline().start_run(InlineSyntaxKind::Emphasis, range, None),
                Tag::Strong => self.inline().start_run(InlineSyntaxKind::Strong, range, None),
                Tag::Strikethrough => self.inline().start_run(InlineSyntaxKind::Strikethrough, range, None),
                Tag::Link { link_type, .. } => {
                    let kind = match link_type {
                        LinkType::Autolink | LinkType::Email => InlineSyntaxKind::Autolink,
                        _ => InlineSyntaxKind::Link,
                    };
                    self.inline().start_run(kind, range, Some(*link_type))
                }
                Tag::Image { link_type, .. } => {
                    self.inline().start_run(InlineSyntaxKind::Image, range, Some(*link_type))
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link | TagEnd::Image => {
                    self.inline().end_run(src)
                }
                // Synthesized list ends can reach beyond their start event's range.
                TagEnd::Item | TagEnd::List(_) | TagEnd::BlockQuote(_) | TagEnd::Table => {
                    if self.frames.len() > 1
                        && let Some(f) = self.frames.last_mut()
                    {
                        f.range.end = f.range.end.max(range.end);
                    }
                }
                _ => {}
            },
            Event::Text(t) => match &mut self.code {
                Some((_, _, content)) => {
                    let c = content.get_or_insert(range.clone());
                    c.end = c.end.max(range.end);
                }
                None => self.inline().text(src, range, t),
            },
            Event::Code(_) => self.inline().code(src, range),
            Event::SoftBreak => self.inline().leaf(range),
            Event::HardBreak => self.inline().hard_break(src, range),
            Event::TaskListMarker(checked) => {
                if let Some(Frame { kind: FrameKind::Item(task), .. }) =
                    self.frames.iter_mut().rev().find(|f| matches!(f.kind, FrameKind::Item(_)))
                {
                    *task = Some(TaskSyntax { range, checked: *checked });
                }
            }
            _ => {}
        }
    }

    fn push_frame(&mut self, kind: FrameKind, range: Range<usize>) {
        self.frames.push(Frame { kind, range, children: Vec::new(), items: Vec::new() });
    }

    fn inline(&mut self) -> &mut InlineState {
        // Tight list items have text without a paragraph.
        self.inline.get_or_insert_with(|| InlineState::new(None, None))
    }

    /// The pending inline block's state, taken whenever the builder flushes inline text.
    pub(in crate::markdown) fn take_inline(&mut self) -> Option<InlineState> {
        self.inline.take()
    }

    pub(in crate::markdown) fn take_code(&mut self) -> SyntaxNode {
        let (range, fenced, content) = self.code.take().unwrap_or_default();
        let content = content.unwrap_or_else(|| {
            let second = self.src[range.clone()].find('\n').map_or(range.end, |i| range.start + i + 1);
            second..second
        });
        let fence = fenced.then(|| FenceSyntax { opening: 0..0, marker: 0..0, info: None, closing: None });
        SyntaxNode::Code(CodeSyntax { range, content, fence })
    }

    /// Records a block's node in its container.
    pub(in crate::markdown) fn push(&mut self, node: SyntaxNode) {
        self.frames.last_mut().unwrap().children.push(node);
    }

    /// Closes the innermost container, mirroring the builder. Items are kept by their list;
    /// other containers return the node their block is pushed with.
    pub(in crate::markdown) fn pop(&mut self) -> Option<SyntaxNode> {
        let f = self.frames.pop()?;
        match f.kind {
            FrameKind::Item(task) => {
                let item = ListItemSyntax {
                    range: f.range,
                    marker: 0..0,
                    number: None,
                    task,
                    content_start: 0,
                    lines: Vec::new(),
                    children: f.children,
                };
                if let Some(list) = self.frames.last_mut() {
                    list.items.push(item);
                }
                None
            }
            FrameKind::List(start) => Some(SyntaxNode::List(ListSyntax { range: f.range, start, items: f.items })),
            FrameKind::Quote => {
                Some(SyntaxNode::Quote(QuoteSyntax { range: f.range, lines: Vec::new(), children: f.children }))
            }
            FrameKind::Table => Some(SyntaxNode::Table(f.range)),
            FrameKind::Root => None,
        }
    }

    /// The top-level nodes, with line prefixes, list markers, heading markers and fences
    /// resolved against the source.
    pub(in crate::markdown) fn finish(mut self) -> Vec<SyntaxNode> {
        let mut nodes = std::mem::take(&mut self.frames[0].children);
        for node in &mut nodes {
            let range = node.range();
            let mut lines = Lines::new(self.src, range);
            lines.resolve(node);
        }
        nodes
    }
}

impl InlineState {
    fn new(block: Option<Range<usize>>, heading: Option<u8>) -> Self {
        let text_end = block.as_ref().map_or(0, |b| b.start);
        InlineState { block, heading, content: None, text_end, runs: Vec::new(), done: Vec::new() }
    }

    /// The node for a flushed paragraph or heading.
    pub(in crate::markdown) fn into_node(self) -> SyntaxNode {
        let content = self.content.clone();
        let range = self.block.clone().or(content.clone()).unwrap_or(0..0);
        let content = content.unwrap_or(range.start..range.start);
        // Runs reserve their slot when they open, so the list is already in start order. Runs
        // left unclosed (only by a partial parse) keep empty delimiters.
        let inline = self.done;
        match self.heading {
            Some(level) => SyntaxNode::Heading(HeadingSyntax {
                range,
                level,
                setext: false,
                open: 0..0,
                close: 0..0,
                content,
                inline,
            }),
            None => SyntaxNode::Paragraph(ParagraphSyntax { range, content, inline }),
        }
    }

    /// Extends the block's content and the innermost run's child bounds.
    fn cover(&mut self, range: &Range<usize>) {
        if range.is_empty() {
            return;
        }
        self.cover_block(range);
        if let Some(run) = self.runs.last_mut() {
            run.first.get_or_insert(range.start);
            run.last = Some(run.last.map_or(range.end, |l| l.max(range.end)));
        }
    }

    fn cover_block(&mut self, range: &Range<usize>) {
        let c = self.content.get_or_insert(range.clone());
        c.start = c.start.min(range.start);
        c.end = c.end.max(range.end);
    }

    fn atom(&mut self, kind: InlineSyntaxKind, range: Range<usize>, open: Range<usize>, close: Range<usize>) {
        self.cover(&range);
        let parent = self.runs.last().map(|r| r.index);
        self.done.push(InlineSyntax { kind, range, open, close, parent, link: None });
    }

    fn leaf(&mut self, range: Range<usize>) {
        self.cover(&range);
        self.text_end = self.text_end.max(range.end);
    }

    fn text(&mut self, src: &str, range: Range<usize>, text: &str) {
        let s = range.start;
        let first = text.chars().next();
        // Pulldown leaves an escape's backslash out of its text event's range.
        if s > self.text_end
            && src.as_bytes()[s - 1] == b'\\'
            && first.is_some_and(|c| c.is_ascii_punctuation())
            && src[s..].starts_with(first.unwrap())
        {
            self.atom(InlineSyntaxKind::Escape, s - 1..s + 1, s - 1..s, s + 1..s + 1);
        } else if !range.is_empty() && src.get(range.clone()).is_some_and(|raw| raw != text && raw.starts_with('&')) {
            self.atom(InlineSyntaxKind::Entity, range.clone(), range.clone(), range.end..range.end);
        }
        self.leaf(range);
    }

    fn code(&mut self, src: &str, range: Range<usize>) {
        let raw = &src.as_bytes()[range.clone()];
        let open = raw.iter().take_while(|&&b| b == b'`').count();
        let close = raw[open..].iter().rev().take_while(|&&b| b == b'`').count();
        let (a, b) = (range.start + open, range.end - close);
        self.atom(InlineSyntaxKind::Code, range.clone(), range.start..a, b..range.end);
        self.text_end = self.text_end.max(range.end);
    }

    fn hard_break(&mut self, src: &str, range: Range<usize>) {
        let spelling = src[range.clone()].trim_end_matches(['\n', '\r']).len();
        let open = range.start..range.start + spelling;
        self.atom(InlineSyntaxKind::HardBreak, range.clone(), open, range.end..range.end);
        self.text_end = self.text_end.max(range.end);
    }

    fn start_run(&mut self, kind: InlineSyntaxKind, range: Range<usize>, link: Option<LinkType>) {
        self.cover(&range);
        let parent = self.runs.last().map(|r| r.index);
        let index = self.done.len();
        let link = link.map(|t| LinkSyntax {
            kind: match t {
                LinkType::Inline => LinkSyntaxKind::Inline,
                LinkType::Reference | LinkType::ReferenceUnknown => LinkSyntaxKind::Reference,
                LinkType::Collapsed | LinkType::CollapsedUnknown => LinkSyntaxKind::Collapsed,
                LinkType::Autolink => LinkSyntaxKind::Autolink,
                LinkType::Email => LinkSyntaxKind::Email,
                _ => LinkSyntaxKind::Shortcut,
            },
            destination: None,
            title: None,
            reference: None,
        });
        let (open, close) = (range.start..range.start, range.end..range.end);
        self.done.push(InlineSyntax { kind, range, open, close, parent, link });
        self.runs.push(Run { index, first: None, last: None });
    }

    fn end_run(&mut self, src: &str) {
        let Some(run) = self.runs.pop() else { return };
        // Pulldown leaves a collapsed reference's `[]` out of the link's range.
        let collapsed = self.done[run.index].link.as_ref().is_some_and(|l| l.kind == LinkSyntaxKind::Collapsed);
        if collapsed && src[self.done[run.index].range.end..].starts_with("[]") {
            let end = self.done[run.index].range.end;
            self.done[run.index].range.end += 2;
            self.cover_block(&(end..end + 2));
        }
        let s = &mut self.done[run.index];
        let r = s.range.clone();
        let bytes = src.as_bytes();
        match s.kind {
            InlineSyntaxKind::Emphasis | InlineSyntaxKind::Strong | InlineSyntaxKind::Strikethrough => {
                // A delimiter run is capped by the content: `***a***` is emphasis around strong,
                // and `**\*a**` keeps the escape's backslash out of the opener.
                let ch = bytes[r.start];
                let first = run.first.unwrap_or(r.end);
                let last = run.last.unwrap_or(first).max(first);
                let open = bytes[r.start..first].iter().take_while(|&&b| b == ch).count();
                let close = bytes[last..r.end].iter().rev().take_while(|&&b| b == ch).count();
                s.open = r.start..r.start + open;
                s.close = r.end - close..r.end;
            }
            InlineSyntaxKind::Autolink => {
                s.open = r.start..r.start + 1;
                s.close = r.end - 1..r.end;
                if let Some(l) = &mut s.link {
                    l.destination = Some(s.open.end..s.close.start);
                }
            }
            _ => {
                let open = if s.kind == InlineSyntaxKind::Image { 2 } else { 1 };
                s.open = r.start..r.start + open;
                let label_end = run.last.unwrap_or(s.open.end).max(s.open.end);
                s.close = label_end..r.end;
                if let Some(l) = &mut s.link {
                    link_tail(src, s.close.clone(), l);
                }
            }
        }
        self.text_end = self.text_end.max(r.end);
    }
}

/// Destination, title and reference inside a link's closing `](…)` or `][…]`.
fn link_tail(src: &str, close: Range<usize>, link: &mut LinkSyntax) {
    let tail = &src[close.clone()];
    let at = |i: usize| close.start + i;
    let b = tail.as_bytes();
    if !tail.starts_with(']') {
        return;
    }
    match link.kind {
        LinkSyntaxKind::Reference if tail.starts_with("][") && tail.ends_with(']') && b.len() > 3 => {
            link.reference = Some(at(2)..at(b.len() - 1));
        }
        LinkSyntaxKind::Inline if tail.starts_with("](") && tail.ends_with(')') => {
            let end = b.len() - 1;
            let mut i = 2;
            let skip_ws = |mut i: usize| {
                while i < end && b[i].is_ascii_whitespace() {
                    i += 1;
                }
                i
            };
            i = skip_ws(i);
            let dest_start = i;
            if b.get(i) == Some(&b'<') {
                while i < end && b[i] != b'>' {
                    i += if b[i] == b'\\' { 2 } else { 1 };
                }
                i = (i + 1).min(end);
            } else {
                let mut depth = 0usize;
                while i < end && !b[i].is_ascii_whitespace() {
                    match b[i] {
                        b'\\' => i += 1,
                        b'(' => depth += 1,
                        b')' if depth == 0 => break,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                i = i.min(end);
            }
            if i > dest_start {
                link.destination = Some(at(dest_start)..at(i));
            }
            let t = skip_ws(i);
            if t < end && matches!(b[t], b'"' | b'\'' | b'(') {
                let closer = if b[t] == b'(' { b')' } else { b[t] };
                let mut j = t + 1;
                while j < end && b[j] != closer {
                    j += if b[j] == b'\\' { 2 } else { 1 };
                }
                link.title = Some(at(t)..at((j + 1).min(end)));
            }
        }
        _ => {}
    }
}
