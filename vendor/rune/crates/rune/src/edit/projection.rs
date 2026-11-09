//! A display projection of a bounded source range: which source bytes a field shows, hides or
//! shows transformed, and which generated decorations it adds. The model's text, selection,
//! composition and history stay in source bytes; only layout, hit testing, caret geometry,
//! selection painting, navigation and accessibility go through the projection.
//!
//! Several source positions can share one display position (where hidden bytes collapse). A
//! [`CollapseSide`] preference decides which of them a display position maps back to; keyboard
//! movement instead lands on the side it travels into, so it never jumps across hidden bytes
//! without a stop on their near edge.

use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;

use crate::color::Color;
use crate::text::Span;

/// Which source position a collapsed display position maps to: the one before the hidden
/// bytes (with the preceding text) or the one after them (with the following text).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CollapseSide {
    #[default]
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Visible(Range<usize>),
    Hidden { source: Range<usize>, side: CollapseSide },
    /// Source bytes shown as other text (a decoded entity). Maps atomically.
    Replaced { source: Range<usize>, text: Arc<str>, side: CollapseSide },
    /// Generated text with no source bytes. Every position inside it maps to `at`.
    Decoration { at: usize, text: Arc<str> },
}

impl Piece {
    fn source(&self) -> Range<usize> {
        match self {
            Piece::Visible(r) | Piece::Hidden { source: r, .. } | Piece::Replaced { source: r, .. } => r.clone(),
            Piece::Decoration { at, .. } => *at..*at,
        }
    }
}

/// A marker painted in a line's gutter (a list bullet or number, a task box). It is not part of
/// the field's text: it never takes the caret, is never copied and is not read as text.
#[derive(Clone, Debug, PartialEq)]
pub struct LineMarker {
    kind: MarkerKind,
    /// Left edge and width of the column it is centred in, in points from the field's left.
    column: [f32; 2],
    color: Color,
    fill: Option<Color>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MarkerKind {
    Text(Arc<str>),
    Task { checked: bool },
}

impl LineMarker {
    /// Text (a bullet or `3.`) centred in the column `x..x + width`.
    pub fn text(text: impl Into<String>, x: f32, width: f32, color: Color) -> Self {
        LineMarker { kind: MarkerKind::Text(text.into().into()), column: [x, width], color, fill: None }
    }
    /// A task box centred in the column, its border in `color`, filled with `fill` and checked
    /// when `checked`.
    pub fn task(checked: bool, x: f32, width: f32, color: Color, fill: Color) -> Self {
        LineMarker { kind: MarkerKind::Task { checked }, column: [x, width], color, fill: Some(fill) }
    }
    pub(crate) fn kind(&self) -> &MarkerKind {
        &self.kind
    }
    pub(crate) fn column(&self) -> [f32; 2] {
        self.column
    }
    pub(crate) fn color(&self) -> Color {
        self.color
    }
    pub(crate) fn fill(&self) -> Option<Color> {
        self.fill
    }
}

/// How one displayed line (one source line, or several joined by hidden line breaks) is laid out
/// in a projected field: its indents, quote rules, gutter marker and code frame. Structure only;
/// character styles come from spans.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineBox {
    pub(crate) first_indent: f32,
    pub(crate) indent: f32,
    pub(crate) rules: Vec<(f32, f32, Color)>,
    pub(crate) marker: Option<LineMarker>,
    pub(crate) frame: Option<(f32, Color, Color)>,
}

impl LineBox {
    pub fn new() -> Self {
        Self::default()
    }
    /// The left indent of the line's first visual line and of its wrapped lines, in points.
    pub fn indent(mut self, first: f32, rest: f32) -> Self {
        self.first_indent = first.max(0.0);
        self.indent = rest.max(0.0);
        self
    }
    /// A vertical rule `width` points wide at `x` points from the field's left, running the
    /// line's height and joining the same rule on adjacent lines.
    pub fn rule(mut self, x: f32, width: f32, color: Color) -> Self {
        self.rules.push((x, width, color));
        self
    }
    pub fn marker(mut self, marker: LineMarker) -> Self {
        self.marker = Some(marker);
        self
    }
    /// A filled, bordered frame behind the line from `x` points to the field's right edge,
    /// joined with framed neighbours (a code block's frame inside a list or quote).
    pub fn frame(mut self, x: f32, fill: Color, border: Color) -> Self {
        self.frame = Some((x, fill, border));
        self
    }
    /// The gutter marker, if any.
    pub fn gutter_marker(&self) -> Option<&LineMarker> {
        self.marker.as_ref()
    }
    pub(crate) fn has_indent(&self) -> bool {
        self.first_indent > 0.0 || self.indent > 0.0
    }
    pub(crate) fn hash_layout(&self, h: &mut impl Hasher) {
        (self.first_indent.to_bits(), self.indent.to_bits()).hash(h);
    }
}

/// The display projection of one source range. Build it with [`SourceProjection::builder`]
/// and pass it to [`El::source_projection`](crate::El::source_projection) beside the same
/// [`El::source_range`](crate::El::source_range). It is transient view state: rebuild it from
/// the source on each render; the model never stores it in history.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceProjection {
    range: Range<usize>,
    /// The source length it was built for; a projection of another revision is ignored.
    doc_len: usize,
    /// Each piece with its display start.
    pieces: Vec<(Piece, usize)>,
    text: String,
    /// Styles of generated text, in display bytes.
    styles: Vec<Span>,
    /// Source line ranges and their layout.
    lines: Vec<(Range<usize>, LineBox)>,
    fingerprint: u64,
}

impl Hash for SourceProjection {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.fingerprint.hash(h);
    }
}

enum Op {
    Hide(Range<usize>, CollapseSide),
    Replace(Range<usize>, String, CollapseSide),
    Decorate(usize, String, Option<Span>),
}

impl Op {
    fn source(&self) -> Range<usize> {
        match self {
            Op::Hide(r, _) | Op::Replace(r, ..) => r.clone(),
            Op::Decorate(at, ..) => *at..*at,
        }
    }
}

/// Collects hidden ranges, replacements, decorations and line layout for one source range.
pub struct ProjectionBuilder<'a> {
    source: &'a str,
    range: Range<usize>,
    ops: Vec<Op>,
    lines: Vec<(Range<usize>, LineBox)>,
}

impl<'a> ProjectionBuilder<'a> {
    /// Hide source bytes. A click where they collapse lands on `side` of them.
    pub fn hide(mut self, range: Range<usize>, side: CollapseSide) -> Self {
        self.ops.push(Op::Hide(range, side));
        self
    }
    /// Show source bytes as `text` (a decoded entity). The caret never stops inside them.
    pub fn replace(mut self, range: Range<usize>, text: impl Into<String>, side: CollapseSide) -> Self {
        self.ops.push(Op::Replace(range, text.into(), side));
        self
    }
    /// Add generated text before source byte `at`, styled by `style` (its range is ignored).
    /// It maps to `at`, so clicking it places the caret there and it is never edited.
    pub fn decorate(mut self, at: usize, text: impl Into<String>, style: Option<Span>) -> Self {
        self.ops.push(Op::Decorate(at, text.into(), style));
        self
    }
    /// Lay out the displayed line starting in `source_line` with `line`.
    pub fn line(mut self, source_line: Range<usize>, line: LineBox) -> Self {
        self.lines.push((source_line, line));
        self
    }

    pub fn build(self) -> SourceProjection {
        let src = self.source;
        let snap = |i: usize| src.floor_char_boundary(i.min(src.len()));
        let range = snap(self.range.start)..snap(self.range.end.max(self.range.start));
        // Clip to the range and snap to boundaries; decorations sort before ops at their offset.
        let mut ops: Vec<Op> = self
            .ops
            .into_iter()
            .filter_map(|op| {
                let r = op.source();
                let r = snap(r.start.max(range.start))..snap(r.end.min(range.end));
                if r.start > r.end || r.start < range.start || r.end > range.end {
                    return None;
                }
                Some(match op {
                    Op::Hide(_, side) => Op::Hide(r, side),
                    Op::Replace(_, text, side) => Op::Replace(r, text, side),
                    Op::Decorate(_, text, style) => Op::Decorate(r.start, text, style),
                })
            })
            .filter(|op| match op {
                Op::Hide(r, _) | Op::Replace(r, ..) => !r.is_empty(),
                Op::Decorate(_, text, _) => !text.is_empty(),
            })
            .collect();
        ops.sort_by_key(|op| {
            let r = op.source();
            (r.start, !matches!(op, Op::Decorate(..)), std::cmp::Reverse(r.end))
        });
        let mut pieces = Vec::new();
        let mut styles = Vec::new();
        let mut text = String::new();
        let mut at = range.start;
        let push = |piece: Piece, shown: &str, pieces: &mut Vec<(Piece, usize)>, text: &mut String| {
            pieces.push((piece, text.len()));
            text.push_str(shown);
        };
        for op in ops {
            let r = op.source();
            if r.start < at {
                // Overlaps an earlier hidden or replaced range: merge a hide, drop the rest.
                if let (Op::Hide(_, _), Some((Piece::Hidden { source, .. }, _))) = (&op, pieces.last_mut())
                    && r.end > source.end
                {
                    source.end = r.end;
                    at = r.end;
                }
                continue;
            }
            if r.start > at {
                push(Piece::Visible(at..r.start), &src[at..r.start], &mut pieces, &mut text);
            }
            match op {
                Op::Hide(r, side) => {
                    if let Some((Piece::Hidden { source, .. }, _)) = pieces.last_mut()
                        && source.end == r.start
                    {
                        source.end = r.end;
                    } else {
                        push(Piece::Hidden { source: r.clone(), side }, "", &mut pieces, &mut text);
                    }
                }
                Op::Replace(r, shown, side) => {
                    push(Piece::Replaced { source: r.clone(), text: shown.as_str().into(), side }, &shown, &mut pieces, &mut text);
                }
                Op::Decorate(at, shown, style) => {
                    let start = text.len();
                    push(Piece::Decoration { at, text: shown.as_str().into() }, &shown, &mut pieces, &mut text);
                    if let Some(mut s) = style {
                        s.start = start as u32;
                        s.end = text.len() as u32;
                        styles.push(s);
                    }
                }
            }
            at = at.max(r.end);
        }
        if at < range.end || pieces.is_empty() {
            push(Piece::Visible(at..range.end), &src[at..range.end], &mut pieces, &mut text);
        }
        let mut lines = self.lines;
        lines.retain(|(r, _)| r.start <= r.end);
        lines.sort_by_key(|(r, _)| r.start);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (&range, src.len(), &text).hash(&mut h);
        for (p, d) in &pieces {
            (p.source(), *d, matches!(p, Piece::Hidden { side: CollapseSide::After, .. })).hash(&mut h);
        }
        for s in &styles {
            format!("{s:?}").hash(&mut h);
        }
        for (r, l) in &lines {
            r.hash(&mut h);
            format!("{l:?}").hash(&mut h);
        }
        SourceProjection { range, doc_len: src.len(), pieces, text, styles, lines, fingerprint: h.finish() }
    }
}

impl SourceProjection {
    /// A projection of `range` in `source`, showing every byte until something is hidden.
    pub fn builder(source: &str, range: Range<usize>) -> ProjectionBuilder<'_> {
        ProjectionBuilder { source, range, ops: Vec::new(), lines: Vec::new() }
    }

    /// The source range it projects.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// The displayed text, without any composition.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether it was built for this source range of a document `doc_len` bytes long.
    pub(crate) fn applies(&self, range: &Range<usize>, doc_len: usize) -> bool {
        self.range == *range && self.doc_len == doc_len
    }

    /// Styles of generated text, in display bytes.
    pub fn decoration_styles(&self) -> &[Span] {
        &self.styles
    }

    fn display_len(&self, i: usize) -> usize {
        match &self.pieces[i].0 {
            Piece::Visible(r) => r.len(),
            Piece::Hidden { .. } => 0,
            Piece::Replaced { text, .. } | Piece::Decoration { text, .. } => text.len(),
        }
    }

    /// The display position of a global source position: the first piece that holds it
    /// (so a position at a decoration shows before it, and hidden bytes show where they
    /// collapse). Positions outside the range clamp to it.
    pub fn to_display(&self, source: usize) -> usize {
        let p = source.clamp(self.range.start, self.range.end);
        for (i, (piece, d)) in self.pieces.iter().enumerate() {
            match piece {
                Piece::Visible(r) if r.start <= p && p <= r.end => return d + p - r.start,
                Piece::Hidden { source, .. } if source.start <= p && p <= source.end => return *d,
                Piece::Replaced { source, .. } if source.start <= p && p <= source.end => {
                    return if p == source.end && p > source.start { d + self.display_len(i) } else { *d };
                }
                Piece::Decoration { at, .. } if *at == p => return *d,
                _ => {}
            }
        }
        self.text.len()
    }

    /// The source positions a display position stands for, as a closed interval, with the
    /// side preferred where they differ. `None` inside a decoration means exactly its anchor.
    fn candidates(&self, display: usize) -> (usize, usize, CollapseSide) {
        let d = self.text.floor_char_boundary(display.min(self.text.len()));
        let (mut lo, mut hi, mut side) = (usize::MAX, 0, None);
        let mut add = |p: usize, s: Option<CollapseSide>, side: &mut Option<CollapseSide>| {
            lo = lo.min(p);
            hi = hi.max(p);
            if side.is_none() {
                *side = s;
            }
        };
        for (i, (piece, start)) in self.pieces.iter().enumerate() {
            let end = start + self.display_len(i);
            if d < *start {
                break;
            }
            if d > end {
                continue;
            }
            match piece {
                Piece::Visible(r) => add(r.start + d - start, None, &mut side),
                Piece::Hidden { source, side: s } => {
                    add(source.start, Some(*s), &mut side);
                    add(source.end, Some(*s), &mut side);
                }
                Piece::Replaced { source, side: s, .. } => {
                    if d == *start {
                        add(source.start, None, &mut side);
                    } else if d == end {
                        add(source.end, None, &mut side);
                    } else {
                        let p = if *s == CollapseSide::After { source.end } else { source.start };
                        return (p, p, *s);
                    }
                }
                Piece::Decoration { at, .. } => {
                    if d < end {
                        return (*at, *at, CollapseSide::Before);
                    }
                    add(*at, None, &mut side);
                }
            }
        }
        if lo == usize::MAX {
            return (self.range.end, self.range.end, CollapseSide::Before);
        }
        (lo, hi, side.unwrap_or_default())
    }

    /// The source position a display position (a click, a vertical move) maps to, choosing the
    /// collapsed hidden bytes' preferred [`CollapseSide`].
    pub fn to_source(&self, display: usize) -> usize {
        let (lo, hi, side) = self.candidates(display);
        if side == CollapseSide::After { hi } else { lo }
    }

    /// [`Self::to_source`] with an explicit side.
    pub fn to_source_side(&self, display: usize, side: CollapseSide) -> usize {
        let (lo, hi, _) = self.candidates(display);
        if side == CollapseSide::After { hi } else { lo }
    }

    /// The next caret stop after `source` for Right: the first display boundary past it whose
    /// source is ahead, on the near (first) side of any hidden bytes there unless that is where
    /// the caret already is (a decoration's anchor). `None` at the end.
    pub fn stop_after(&self, source: usize) -> Option<usize> {
        let d0 = self.to_display(source);
        self.text
            .grapheme_indices(true)
            .map(|(i, g)| i + g.len())
            .filter(|&d| d > d0)
            .find_map(|d| {
                let (lo, hi, _) = self.candidates(d);
                if lo > source { Some(lo) } else { (hi > source).then_some(hi) }
            })
    }

    /// The previous caret stop before `source` for Left, on the near (last) side of hidden bytes.
    pub fn stop_before(&self, source: usize) -> Option<usize> {
        let d0 = self.to_display(source);
        let bounds: Vec<usize> = self.text.grapheme_indices(true).map(|(i, _)| i).filter(|&d| d < d0).collect();
        bounds.into_iter().rev().find_map(|d| {
            let (lo, hi, _) = self.candidates(d);
            if hi < source { Some(hi) } else { (lo < source).then_some(lo) }
        })
    }

    /// Display byte ranges showing source bytes `start..end`: visible bytes, and whole
    /// replacements they overlap. Hidden bytes and decorations show nothing.
    pub fn map_range(&self, start: usize, end: usize) -> Vec<Range<usize>> {
        let mut out: Vec<Range<usize>> = Vec::new();
        for (i, (piece, d)) in self.pieces.iter().enumerate() {
            let shown = match piece {
                Piece::Visible(r) => {
                    let (a, b) = (start.max(r.start), end.min(r.end));
                    (a < b).then(|| d + a - r.start..d + b - r.start)
                }
                Piece::Replaced { source, .. } => {
                    (start < source.end && source.start < end).then(|| *d..d + self.display_len(i))
                }
                _ => None,
            };
            if let Some(r) = shown {
                match out.last_mut() {
                    Some(last) if last.end == r.start => last.end = r.end,
                    _ => out.push(r),
                }
            }
        }
        out
    }

    /// Display ranges of generated decorations.
    pub fn decorations(&self) -> Vec<Range<usize>> {
        self.pieces
            .iter()
            .enumerate()
            .filter(|(_, (p, _))| matches!(p, Piece::Decoration { .. }))
            .map(|(i, (_, d))| *d..d + self.display_len(i))
            .collect()
    }

    /// The layout of the displayed line whose first character is source position `source`:
    /// the line range containing it (the later one at a shared boundary).
    pub(crate) fn line_at(&self, source: usize) -> Option<&LineBox> {
        self.lines.iter().rev().find(|(r, _)| r.start <= source && source <= r.end).map(|(_, l)| l)
    }

    pub(crate) fn has_lines(&self) -> bool {
        !self.lines.is_empty()
    }
}
