//! The editable presentation of a source range: the Markdown a bounded input shows while it is
//! edited, keeping the blocks' look. Syntax the caller has not revealed is hidden through a
//! [`SourceProjection`]; inline styles, heading type, code highlighting, quote rules, list
//! marker columns and code frames come from the same [`MarkdownStyle`] rules and the same
//! cached parse as rendered blocks. Nothing here edits text or keeps state.

use std::collections::BTreeMap;

use rune::{CollapseSide, LineBox, LineMarker, ProjectionBuilder, SourceProjection};

use super::*;
use render::{code_frame, default_link_span, heading_style, inline_style_span};

/// How [`editable`] presents a range; [`Editable::apply`] puts it on the bound input.
pub struct Editable {
    projection: SourceProjection,
    spans: Vec<Span>,
    strikes: Vec<Range<usize>>,
    font: TextStyle,
    color: Color,
    /// The range is one code block, drawn in its frame.
    framed: bool,
    frame_style: MarkdownStyle,
}

impl Editable {
    /// The projection: hidden syntax, decoded entities and per-line layout.
    pub fn projection(&self) -> &SourceProjection {
        &self.projection
    }

    /// The base text style: a lone heading's or code block's, else the body's.
    pub fn font(&self) -> TextStyle {
        self.font
    }

    /// Styles in source bytes, as passed to [`El::spans`].
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Style `input`, a [`rune::text_input`] already bounded to the same range with
    /// [`El::source_range`], and place it in its block's frame (a lone code block's).
    pub fn apply(self, input: El) -> El {
        let strikes: Vec<(usize, usize, Option<Color>)> = self
            .strikes
            .iter()
            .flat_map(|r| self.projection.map_range(r.start, r.end))
            .map(|d| (d.start, d.end, None))
            .collect();
        let input = input.font(self.font).color(self.color).spans(self.spans).source_projection(self.projection).strikes(strikes);
        if self.framed { code_frame(&self.frame_style, input, false) } else { input }
    }
}

/// Where a container puts its content: the left edge in points, and the quote rules to its left.
#[derive(Clone, Default)]
struct Ctx {
    indent: f32,
    rules: Vec<f32>,
}

struct Build<'a> {
    src: &'a str,
    s: &'a MarkdownStyle,
    reveal: &'a [Range<usize>],
    base: f32,
    proj: Option<ProjectionBuilder<'a>>,
    spans: Vec<Span>,
    links: Vec<Span>,
    strikes: Vec<Range<usize>>,
    lines: BTreeMap<usize, (Range<usize>, LineBox)>,
}

/// The editable presentation of `range` of `src` (a bounded input's range). `reveal` lists the
/// source syntax to show; all other syntax in the range is hidden. Uses the cached parse.
pub fn editable(src: &str, range: Range<usize>, reveal: &[Range<usize>], style: &MarkdownStyle) -> Editable {
    let syntax = block_syntax(src);
    let nodes: Vec<&SyntaxNode> = syntax
        .iter()
        .filter(|n| {
            let r = n.range();
            r.start < range.end && range.start < r.end || (range.is_empty() && r.contains(&range.start))
        })
        .collect();
    let (font, color, framed) = match nodes[..] {
        [SyntaxNode::Heading(h)] => {
            let (f, c) = heading_style(style, h.level);
            (f, c, false)
        }
        [SyntaxNode::Code(c)] if c.fence.is_some() || !c.content.is_empty() => (style.code_block, style.text, true),
        _ => (style.body, style.text, false),
    };
    let mut b = Build {
        src,
        s: style,
        reveal,
        base: font.size,
        proj: Some(SourceProjection::builder(src, range.clone())),
        spans: Vec::new(),
        links: Vec::new(),
        strikes: Vec::new(),
        lines: BTreeMap::new(),
    };
    for node in &nodes {
        b.node(node, &Ctx::default(), framed);
    }
    let mut proj = b.proj.take().unwrap();
    for (_, (line, boxed)) in std::mem::take(&mut b.lines) {
        proj = proj.line(line, boxed);
    }
    let mut spans = b.links;
    spans.extend(b.spans);
    Editable { projection: proj.build(), spans, strikes: b.strikes, font, color, framed, frame_style: style.clone() }
}

impl<'a> Build<'a> {
    fn shown(&self, r: &Range<usize>) -> bool {
        self.reveal.iter().any(|x| x.start <= r.start && r.end <= x.end)
    }

    fn proj(&mut self, f: impl FnOnce(ProjectionBuilder<'a>) -> ProjectionBuilder<'a>) {
        let p = self.proj.take().expect("projection builder");
        self.proj = Some(f(p));
    }

    /// Hide `r` unless revealed; revealed syntax draws muted.
    fn syntax(&mut self, r: Range<usize>, side: CollapseSide) {
        if r.is_empty() {
            return;
        }
        if self.shown(&r) {
            self.spans.push(Span { color: Some(self.s.muted), ..Span::new(r.start, r.end) });
        } else {
            self.proj(|p| p.hide(r, side));
        }
    }

    fn line(&mut self, line: Range<usize>, ctx: &Ctx, first: f32, rest: f32, marker: Option<LineMarker>) {
        let mut boxed = LineBox::new().indent(first, rest);
        for &x in &ctx.rules {
            boxed = boxed.rule(x, 2.0, self.s.quote_rule);
        }
        // An inner container's line keeps an outer item's gutter marker on the same line.
        let kept = self.lines.get(&line.start).and_then(|(_, b)| b.gutter_marker().cloned());
        if let Some(m) = marker.or(kept) {
            boxed = boxed.marker(m);
        }
        self.lines.insert(line.start, (line, boxed));
    }

    fn node(&mut self, node: &SyntaxNode, ctx: &Ctx, framed: bool) {
        match node {
            SyntaxNode::Paragraph(p) => self.inline(&p.inline),
            SyntaxNode::Heading(h) => {
                self.syntax(h.open.clone(), CollapseSide::After);
                // A setext underline hides with the line break before it.
                let close = if h.setext { h.content.end.min(h.close.start)..h.close.end } else { h.close.clone() };
                self.syntax(close, CollapseSide::Before);
                let (font, color) = heading_style(self.s, h.level);
                if font.size != self.base || ctx.indent > 0.0 {
                    let r = h.content.clone();
                    self.spans.push(Span { size: Some(font.size), weight: Some(font.weight), color: Some(color), ..Span::new(r.start, r.end) });
                }
                self.inline(&h.inline);
            }
            SyntaxNode::Code(c) => self.code(c, ctx, framed),
            SyntaxNode::Quote(q) => {
                let inner = Ctx { indent: ctx.indent + 14.0, rules: ctx.rules.iter().copied().chain([ctx.indent]).collect() };
                for line in &q.lines {
                    if let Some(prefix) = &line.prefix {
                        self.syntax(prefix.clone(), CollapseSide::After);
                    }
                    self.line(line.line.clone(), &inner, inner.indent, inner.indent, None);
                }
                for child in &q.children {
                    self.node(child, &inner, false);
                }
            }
            SyntaxNode::List(l) => {
                let w = self.s.list_indent;
                let inner = Ctx { indent: ctx.indent + w, rules: ctx.rules.clone() };
                for (k, item) in l.items.iter().enumerate() {
                    for (i, line) in item.lines.iter().enumerate() {
                        let Some(prefix) = &line.prefix else {
                            self.line(line.line.clone(), &inner, inner.indent, inner.indent, None);
                            continue;
                        };
                        if i > 0 {
                            self.syntax(prefix.clone(), CollapseSide::After);
                            self.line(line.line.clone(), &inner, inner.indent, inner.indent, None);
                            continue;
                        }
                        let marker = prefix.start..item.content_start.max(prefix.end);
                        if self.shown(&marker) {
                            // The authored marker takes the bullet's place in the gutter.
                            self.syntax(marker, CollapseSide::After);
                            self.line(line.line.clone(), &inner, ctx.indent, inner.indent, None);
                            continue;
                        }
                        self.syntax(marker, CollapseSide::After);
                        let gutter = match (&item.task, l.start) {
                            (Some(t), _) => LineMarker::task(t.checked, ctx.indent, w, self.s.task.border_on, self.s.task.fill_on),
                            (None, Some(n)) => LineMarker::text(format!("{}.", n + k as u64), ctx.indent, w, self.s.muted),
                            (None, None) => LineMarker::text("•", ctx.indent, w, self.s.muted),
                        };
                        self.line(line.line.clone(), &inner, inner.indent, inner.indent, Some(gutter));
                    }
                    for child in &item.children {
                        self.node(child, &inner, false);
                    }
                }
            }
            SyntaxNode::Table(_) | SyntaxNode::Rule(_) => {}
        }
    }

    fn code(&mut self, c: &CodeSyntax, ctx: &Ctx, framed: bool) {
        let src = self.src;
        if let Some(f) = &c.fence {
            let open_line = f.opening.start..(f.opening.end + 1).min(c.range.end.max(f.opening.end));
            let open_line = if src[f.opening.end..].starts_with('\n') { open_line } else { f.opening.clone() };
            self.syntax_block(f.opening.clone(), open_line, CollapseSide::After);
            if let Some(closing) = &f.closing {
                // The line break before the fence line goes with it, past any container prefix
                // (a list's indent, a quote's `>`) that the container hides on its own.
                let line_start = src[..closing.start].rfind('\n').filter(|&i| i >= c.range.start || i + 1 == c.range.start);
                let with_break = match line_start {
                    Some(i) if i > f.opening.end || i + 1 == closing.start => i..closing.end,
                    _ => closing.clone(),
                };
                self.syntax_block(closing.clone(), with_break, CollapseSide::Before);
            }
        }
        let content = c.content.clone();
        if !framed && !content.is_empty() {
            let cb = self.s.code_block;
            self.spans.push(Span { mono: true, size: Some(cb.size), ..Span::new(content.start, content.end) });
        }
        // Top-level code colours like the rendered block; nested code keeps container prefixes
        // on its lines, which the scanners would misread, so it stays plain.
        if ctx.indent == 0.0 && self.s.highlight && !content.is_empty() {
            let lang = c.fence.as_ref().and_then(|f| f.info.clone()).map(|i| parser::fence_word(&src[i]).to_owned());
            if let Some(lang) = lang.filter(|l| crate::syntax::known(l)) {
                let hl = crate::syntax::highlight(Some(&lang), &src[content.clone()]);
                self.spans.extend(self.s.syntax.spans(&hl, content.start));
            }
        }
        if ctx.indent > 0.0 || !ctx.rules.is_empty() {
            let pad = self.s.code_block_pad[0];
            let mut at = c.range.start;
            for line in src[c.range.clone()].split_inclusive('\n') {
                let end = at + line.trim_end_matches(['\n', '\r']).len();
                let mut boxed = LineBox::new().indent(ctx.indent + pad, ctx.indent + pad).frame(ctx.indent, self.s.code_block_fill, self.s.code_block_border);
                for &x in &ctx.rules {
                    boxed = boxed.rule(x, 2.0, self.s.quote_rule);
                }
                self.lines.insert(at, (at..end, boxed));
                at += line.len();
            }
        }
    }

    /// A fence line: shown muted while revealed, else hidden with its line break.
    fn syntax_block(&mut self, line: Range<usize>, with_break: Range<usize>, side: CollapseSide) {
        if self.shown(&line) {
            self.spans.push(Span { color: Some(self.s.muted), ..Span::new(line.start, line.end) });
        } else if !with_break.is_empty() {
            self.proj(|p| p.hide(with_break, side));
        }
    }

    fn inline(&mut self, inline: &[InlineSyntax]) {
        for i in inline.iter().filter(|i| matches!(i.kind, InlineSyntaxKind::Link | InlineSyntaxKind::Image | InlineSyntaxKind::Autolink)) {
            let label = i.content();
            if !label.is_empty() {
                self.links.push(default_link_span(self.s, label));
            }
        }
        for i in inline {
            match i.kind {
                InlineSyntaxKind::Escape | InlineSyntaxKind::HardBreak => {
                    self.syntax(i.open.clone(), if i.kind == InlineSyntaxKind::Escape { CollapseSide::After } else { CollapseSide::Before });
                }
                InlineSyntaxKind::Entity => {
                    let r = i.open.clone();
                    if self.shown(&r) {
                        self.spans.push(Span { color: Some(self.s.muted), ..Span::new(r.start, r.end) });
                    } else {
                        let decoded = decode(&self.src[r.clone()]);
                        self.proj(|p| p.replace(r, decoded, CollapseSide::After));
                    }
                }
                kind => {
                    // Hidden backticks leave inline code the rendered chip's narrow padding,
                    // generated text that maps to the delimiters' outer edges.
                    if kind == InlineSyntaxKind::Code && !self.shown(&i.open) && !self.shown(&i.close) {
                        let (open, close) = (i.open.start, i.close.end);
                        self.proj(|p| p.decorate(open, parser::PAD, None).decorate(close, parser::PAD, None));
                    }
                    self.syntax(i.open.clone(), CollapseSide::After);
                    self.syntax(i.close.clone(), CollapseSide::Before);
                    let style = match kind {
                        InlineSyntaxKind::Emphasis => Some(Style::Em),
                        InlineSyntaxKind::Strong => Some(Style::Strong),
                        InlineSyntaxKind::Code => Some(Style::Code),
                        InlineSyntaxKind::Strikethrough => {
                            self.strikes.push(i.content());
                            None
                        }
                        _ => None,
                    };
                    if let Some(st) = style {
                        // Styles cover the delimiters too, so revealed ones keep the construct's look.
                        let r = if st == Style::Code { i.content() } else { i.range.clone() };
                        let in_link = self.links.iter().find(|l| l.start as usize <= r.start && r.end <= l.end as usize).cloned();
                        self.spans.extend(inline_style_span(self.s, st, r, self.base, in_link.as_ref()));
                    }
                }
            }
        }
    }
}

/// An entity's or character reference's text, decoded by the same parser as the document.
fn decode(spelling: &str) -> String {
    Parser::new(spelling)
        .filter_map(|e| match e {
            Event::Text(t) => Some(t.into_string()),
            _ => None,
        })
        .collect()
}
