//! Live Preview reveal policy: which Markdown syntax is visible for a given caret or selection.
//!
//! Derived purely from Rune Kit's cached syntax metadata ([`rune_kit::markdown::block_syntax`])
//! and the shared editor's selection. Revealing or hiding changes no source bytes, selection or
//! history; it only decides what a projection shows.
//!
//! # States
//!
//! Every construct (a heading, a fence, one quote line's prefix, one list item's marker, one
//! inline construct...) is in exactly one [`EditState`] for a focus:
//!
//! - **Inactive**: the editor is unfocused, or the focus does not touch the construct's scope.
//! - **Content editing**: a collapsed caret is inside the scope but not touching its syntax.
//! - **Marker editing**: a collapsed caret touches one of its syntax ranges, boundaries
//!   included (the caret just before `>` or just after `- ` both count).
//! - **Selection**: a nonempty selection touches the scope.
//!
//! "Touches" uses closed intervals, so a position at either end of a range touches it.
//!
//! # Rules (the Live Preview table)
//!
//! | Element | Rule |
//! | --- | --- |
//! | Heading | [`Rule::WhileEditing`]: markers show while the caret is anywhere on the heading line. |
//! | Fenced code | [`Rule::WhileEditing`]: both fence lines and the info string show while the caret is anywhere in the block; the language badge hides meanwhile. |
//! | Quote | [`Rule::AtMarker`]: a line's own `>` shows when the caret reaches it or the start of that line's quoted content. |
//! | Unordered / ordered list, task item | [`Rule::AtMarker`]: the authored marker (and task box) shows when the caret reaches it or the start of the item's content. |
//! | List continuation indent | [`Rule::AtMarker`]: shows when the caret reaches it or the content after it. |
//! | Bold, italic, strikethrough, inline code, link, image, autolink | [`Rule::WhileEditing`]: delimiters (and a link's destination or reference) show while the caret is in the construct or on its boundaries. |
//! | Escape, entity, hard break | [`Rule::WhileEditing`]: the authored spelling shows while the caret touches it. |
//!
//! Nested constructs are evaluated independently: each enclosing inline construct touching the
//! caret reveals, keeping its combined styling, while for nested quotes and lists only the
//! prefixes the caret actually reaches reveal. Pointer hover never reveals.
//!
//! A nonempty selection reveals [`Rule::WhileEditing`] syntax whose construct it touches, and
//! [`Rule::AtMarker`] syntax it touches itself, so the selected source bytes are readable.
//!
//! # Boundaries and editing
//!
//! - Because states use closed intervals, every hidden syntax range is already revealed when
//!   the caret stands next to it. An arrow key therefore enters revealed syntax: it never
//!   lands on an invisible stop, and a projection never needs a hidden caret position.
//! - Backspace and Delete remove the adjacent *source* character. By the previous point that
//!   character's syntax is visible before deletion. There is no paired-delimiter deletion:
//!   deleting one `*` leaves its partner, and reparsing shows whatever remains.
//! - Copy, cut and paste use the selected source bytes, Markdown included.
//! - Clicking a rendered marker (bullet, number, task box, quote rule) targets that item's or
//!   quote line's own source marker ([`marker_target`]); clicking content targets the content
//!   position given by the rendered-text mapping.

use std::ops::Range;

use rune_kit::markdown::{InlineSyntaxKind, ListItemSyntax, SyntaxNode};

/// What a construct is, for choosing its [`Rule`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Element {
    Heading,
    FencedCode,
    Quote,
    UnorderedList,
    OrderedList,
    TaskItem,
    ItemIndent,
    Inline(InlineSyntaxKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EditState {
    Inactive,
    ContentEditing,
    MarkerEditing,
    Selection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    /// Revealed in every state except [`EditState::Inactive`].
    WhileEditing,
    /// Revealed while marker editing, or by a selection touching the syntax itself.
    AtMarker,
}

impl Element {
    pub const fn rule(self) -> Rule {
        match self {
            Element::Heading | Element::FencedCode | Element::Inline(_) => Rule::WhileEditing,
            Element::Quote
            | Element::UnorderedList
            | Element::OrderedList
            | Element::TaskItem
            | Element::ItemIndent => Rule::AtMarker,
        }
    }
}

/// The editor's focus as global source byte offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Focus {
    pub anchor: usize,
    pub head: usize,
}

impl Focus {
    pub fn caret(at: usize) -> Self {
        Focus {
            anchor: at,
            head: at,
        }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.head
    }

    fn touches(&self, r: &Range<usize>) -> bool {
        let f = self.range();
        f.start <= r.end && r.start <= f.end
    }
}

/// One revealable unit of syntax.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Construct {
    pub element: Element,
    /// The extent focus is judged against.
    pub scope: Range<usize>,
    /// The source syntax this construct hides while inactive. Nonempty ranges only.
    pub syntax: Vec<Range<usize>>,
}

impl Construct {
    pub fn state(&self, focus: Option<Focus>) -> EditState {
        let Some(focus) = focus.filter(|f| f.touches(&self.scope)) else {
            return EditState::Inactive;
        };
        if !focus.is_collapsed() {
            EditState::Selection
        } else if self.syntax.iter().any(|r| focus.touches(r)) {
            EditState::MarkerEditing
        } else {
            EditState::ContentEditing
        }
    }

    pub fn revealed(&self, focus: Option<Focus>) -> bool {
        match (self.element.rule(), self.state(focus)) {
            (_, EditState::Inactive) => false,
            (Rule::WhileEditing, _) => true,
            (Rule::AtMarker, EditState::MarkerEditing) => true,
            (Rule::AtMarker, EditState::Selection) => {
                focus.is_some_and(|f| self.syntax.iter().any(|r| f.touches(r)))
            }
            (Rule::AtMarker, _) => false,
        }
    }
}

/// Every revealable construct in `syntax`, in document order of their scopes' starts.
pub fn constructs(syntax: &[SyntaxNode]) -> Vec<Construct> {
    let mut out = Vec::new();
    for node in syntax {
        collect(node, &mut out);
    }
    out.retain(|c| !c.syntax.is_empty());
    out
}

fn collect(node: &SyntaxNode, out: &mut Vec<Construct>) {
    let mut push = |element, scope: Range<usize>, syntax: Vec<Range<usize>>| {
        let syntax = syntax.into_iter().filter(|r| !r.is_empty()).collect();
        out.push(Construct {
            element,
            scope,
            syntax,
        });
    };
    match node {
        SyntaxNode::Paragraph(p) => inline(&p.inline, out),
        SyntaxNode::Heading(h) => {
            let end = h.close.end.max(h.content.end).max(h.open.end);
            push(
                Element::Heading,
                h.open.start.min(h.range.start)..end,
                vec![h.open.clone(), h.close.clone()],
            );
            inline(&h.inline, out);
        }
        SyntaxNode::Code(c) => {
            if let Some(f) = &c.fence {
                let syntax = std::iter::once(f.opening.clone())
                    .chain(f.closing.clone())
                    .collect();
                push(Element::FencedCode, c.range.clone(), syntax);
            }
        }
        SyntaxNode::Quote(q) => {
            for line in &q.lines {
                if let Some(prefix) = &line.prefix {
                    push(
                        Element::Quote,
                        prefix.start..line.line.end,
                        vec![prefix.clone()],
                    );
                }
            }
            for child in &q.children {
                collect(child, out);
            }
        }
        SyntaxNode::List(l) => {
            for item in &l.items {
                list_item(item, out);
            }
        }
        _ => {}
    }
}

fn list_item(item: &ListItemSyntax, out: &mut Vec<Construct>) {
    let element = match (&item.task, item.number) {
        (Some(_), _) => Element::TaskItem,
        (None, Some(_)) => Element::OrderedList,
        (None, None) => Element::UnorderedList,
    };
    for (i, line) in item.lines.iter().enumerate() {
        let Some(prefix) = &line.prefix else { continue };
        let (element, marker) = if i == 0 {
            (element, prefix.start..item.content_start.max(prefix.end))
        } else {
            (Element::ItemIndent, prefix.clone())
        };
        if !marker.is_empty() {
            let scope = marker.start..line.line.end.max(marker.end);
            out.push(Construct {
                element,
                scope,
                syntax: vec![marker],
            });
        }
    }
    for child in &item.children {
        collect(child, out);
    }
}

fn inline(inline: &[rune_kit::markdown::InlineSyntax], out: &mut Vec<Construct>) {
    for i in inline {
        let syntax = [i.open.clone(), i.close.clone()]
            .into_iter()
            .filter(|r| !r.is_empty())
            .collect();
        out.push(Construct {
            element: Element::Inline(i.kind),
            scope: i.range.clone(),
            syntax,
        });
    }
}

/// The syntax a focus reveals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reveal {
    /// Revealed source syntax ranges, sorted by start. May overlap where constructs nest.
    pub ranges: Vec<Range<usize>>,
    /// Fenced code blocks whose fences show, so their language badge hides.
    pub open_fences: Vec<Range<usize>>,
}

impl Reveal {
    /// Whether every byte of `range` is inside a revealed range.
    pub fn shows(&self, range: &Range<usize>) -> bool {
        self.ranges
            .iter()
            .any(|r| r.start <= range.start && range.end <= r.end)
    }
}

pub fn reveal(syntax: &[SyntaxNode], focus: Option<Focus>) -> Reveal {
    let mut out = Reveal::default();
    for c in constructs(syntax) {
        if !c.revealed(focus) {
            continue;
        }
        if c.element == Element::FencedCode {
            out.open_fences.push(c.scope.clone());
        }
        out.ranges.extend(c.syntax);
    }
    out.ranges.sort_by_key(|r| (r.start, r.end));
    out.ranges.dedup();
    out
}

/// A rendered marker decoration that was clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerHit {
    /// A bullet or number.
    ListMarker,
    /// A task checkbox.
    TaskMarker,
    /// A quote rule, counted from the outermost quote on the line.
    QuoteRule { depth: usize },
}

/// The source position a marker click places the caret at: the marker's own first byte (a
/// bullet, number, `[` or `>`), for the innermost list item (or the `depth`th quote) whose
/// line contains `line_position`. A task click on an item without a task box targets its marker.
pub fn marker_target(
    src: &str,
    syntax: &[SyntaxNode],
    hit: MarkerHit,
    line_position: usize,
) -> Option<usize> {
    match hit {
        MarkerHit::ListMarker | MarkerHit::TaskMarker => {
            let mut best: Option<&ListItemSyntax> = None;
            walk_items(syntax, &mut |item| {
                let first = &item.lines.first()?.line;
                (first.start <= line_position && line_position <= first.end)
                    .then(|| best = Some(item))
            });
            let item = best?;
            match (hit, &item.task) {
                (MarkerHit::TaskMarker, Some(task)) => Some(task.range.start),
                _ => Some(item.marker.start),
            }
        }
        MarkerHit::QuoteRule { depth } => {
            let mut markers = Vec::new();
            walk_quote_lines(syntax, &mut |line, prefix| {
                if line.start <= line_position && line_position <= line.end {
                    // A prefix may start with up to three spaces before its `>`.
                    markers.push(prefix.start + src[prefix.clone()].find('>').unwrap_or(0));
                }
            });
            markers.sort_unstable();
            markers.get(depth).copied()
        }
    }
}

fn walk_items<'a>(nodes: &'a [SyntaxNode], f: &mut impl FnMut(&'a ListItemSyntax) -> Option<()>) {
    for node in nodes {
        match node {
            SyntaxNode::List(l) => {
                for item in &l.items {
                    f(item);
                    walk_items(&item.children, f);
                }
            }
            SyntaxNode::Quote(q) => walk_items(&q.children, f),
            _ => {}
        }
    }
}

fn walk_quote_lines(nodes: &[SyntaxNode], f: &mut impl FnMut(&Range<usize>, &Range<usize>)) {
    for node in nodes {
        match node {
            SyntaxNode::Quote(q) => {
                for line in &q.lines {
                    if let Some(prefix) = &line.prefix {
                        f(&line.line, prefix);
                    }
                }
                walk_quote_lines(&q.children, f);
            }
            SyntaxNode::List(l) => l
                .items
                .iter()
                .for_each(|i| walk_quote_lines(&i.children, f)),
            _ => {}
        }
    }
}
