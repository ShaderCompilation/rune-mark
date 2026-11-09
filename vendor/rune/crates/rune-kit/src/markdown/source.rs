//! Editable source regions and rendered caret positions from the full-document parse.
use super::*;

pub(super) type SourceWire = Rc<dyn Fn(El, Range<usize>, Option<RenderedText>) -> El>;

/// A rendered top-level block, a non-rendered source line, or the insertion position at EOF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceRegion {
    pub source_range: Range<usize>,
    /// Index in [`parse_cached`], or `None` for source that renders no block.
    pub block_index: Option<usize>,
}

/// A click's global source position and enclosing editable region.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceClick {
    pub source_range: Range<usize>,
    pub position: usize,
}

/// One rendered text run (a paragraph, heading, table cell or code block), including generated
/// inline-code padding. Runs in lists and quotes retain their outer block's range and index.
#[derive(Clone, Debug)]
pub struct RenderedText {
    text: String,
    pub source_range: Range<usize>,
    pub block_index: usize,
    positions: Rc<[usize]>,
}

impl RenderedText {
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Global source UTF-8 byte boundary for a rendered UTF-8 byte offset. Offsets inside a
    /// character round down; offsets past the text clamp to its end. Decoded entities and
    /// escapes map atomically to source boundaries, never inside their spelling. Padding maps
    /// to the nearest mapped text boundary; text without a mapping maps to the block's start.
    pub fn source_position(&self, rendered_byte: usize) -> usize {
        self.positions[self.text.floor_char_boundary(rendered_byte.min(self.text.len()))]
    }

    fn new(src: &str, text: &str, map: Option<&SpanMap>, range: Range<usize>, index: usize, offset: usize) -> Self {
        // Compare literal spans once and binary-search by rendered offset, keeping long
        // paragraphs from rescanning preceding spans for every character.
        let segments: Vec<_> = map
            .into_iter()
            .flat_map(|m| &m.0)
            .map(|(s, o)| {
                let a = o.start.max(offset);
                let b = o.end.min(offset + text.len());
                let literal = a < b
                    && s.len() == o.len()
                    && src.get(s.start + a - o.start..s.start + b - o.start) == text.get(a - offset..b - offset);
                (s, o, literal)
            })
            .collect();
        let mut positions = Vec::with_capacity(text.len() + 1);
        for (at, ch) in text.char_indices() {
            let pos = source_position(src, &segments, at + offset, range.start).clamp(range.start, range.end);
            positions.extend(std::iter::repeat_n(pos, ch.len_utf8()));
        }
        positions.push(source_position(src, &segments, text.len() + offset, range.start).clamp(range.start, range.end));
        Self { text: text.to_owned(), source_range: range, block_index: index, positions: positions.into() }
    }
}

fn source_position(src: &str, segments: &[(&Range<usize>, &Range<usize>, bool)], at: usize, fallback: usize) -> usize {
    // Prefer the following segment at shared boundaries, skipping intervening syntax.
    let i = segments.partition_point(|(_, o, _)| o.end <= at);
    let after = segments.get(i);
    if let Some((s, o, literal)) = after
        && o.contains(&at)
    {
        if !*literal {
            return s.start;
        }
        let mut position = s.start + at - o.start;
        // Pulldown excludes escape backslashes from text-event ranges. At the first
        // character include skipped slashes, without changing search/highlight mappings
        // or treating a literal backslash already covered by the prior span as syntax.
        if at == o.start && src.as_bytes().get(position).is_some_and(u8::is_ascii_punctuation) {
            let previous_end = i.checked_sub(1).map_or(0, |i| segments[i].0.end);
            while position > previous_end && src.as_bytes()[position - 1] == b'\\' {
                position -= 1;
            }
        }
        return src.floor_char_boundary(position);
    }
    match (i.checked_sub(1).and_then(|i| segments.get(i)), after) {
        (Some((_, o, _)), Some((next, n, _))) if n.start - at < at - o.end => next.start,
        (Some((s, _, _)), _) => s.end,
        (_, Some((s, _, _))) => s.start,
        _ => fallback,
    }
}

/// Editable regions covering every source byte, followed by a zero-length EOF insertion
/// region. Non-rendered gaps are split into source lines, retaining reference definitions,
/// raw HTML and blank lines. Empty documents have the single region `0..0`.
pub fn source_regions(src: &str) -> Vec<SourceRegion> {
    regions(src, &block_source_ranges(src))
}

fn regions(src: &str, ranges: &[Range<usize>]) -> Vec<SourceRegion> {
    let mut out = Vec::new();
    let mut at = 0;
    for (i, range) in ranges.iter().enumerate() {
        gap_regions(src, at..range.start, &mut out);
        out.push(SourceRegion { source_range: range.clone(), block_index: Some(i) });
        at = range.end;
    }
    gap_regions(src, at..src.len(), &mut out);
    out.push(SourceRegion { source_range: src.len()..src.len(), block_index: None });
    out
}

fn gap_regions(src: &str, range: Range<usize>, out: &mut Vec<SourceRegion>) {
    let mut start = range.start;
    for line in src[range].split_inclusive('\n') {
        out.push(SourceRegion { source_range: start..start + line.len(), block_index: None });
        start += line.len();
    }
}

/// Region containing a valid source byte boundary. EOF belongs to the insertion region;
/// at other shared boundaries the following region wins. Invalid UTF-8 offsets return `None`.
pub fn source_region_at(src: &str, position: usize) -> Option<SourceRegion> {
    if !src.is_char_boundary(position) {
        return None;
    }
    source_regions(src)
        .into_iter()
        .find(|r| r.source_range.contains(&position) || (position == src.len() && r.source_range.is_empty()))
}

/// Rendered text and source caret positions from the same cached full-document parse used
/// by [`Markdown`]. No parsing of isolated blocks, so reference links keep their context.
pub fn rendered_texts(src: &str) -> Vec<RenderedText> {
    fn walk(
        src: &str,
        blocks: &[Block],
        maps: &[BlockMap],
        range: &Range<usize>,
        index: usize,
        out: &mut Vec<RenderedText>,
    ) {
        for (b, m) in blocks.iter().zip(maps) {
            match (b, m) {
                (Block::Para(i) | Block::Heading(_, i), BlockMap::Inline(m)) => {
                    out.push(RenderedText::new(src, &i.text, Some(m), range.clone(), index, 0))
                }
                (Block::Code(t) | Block::Diff(t) | Block::Mermaid(t), BlockMap::Code(m)) => {
                    out.push(RenderedText::new(src, t, Some(m), range.clone(), index, 0))
                }
                (Block::List { items, .. }, BlockMap::List(ms)) => {
                    for (bs, ms) in items.iter().zip(ms) {
                        walk(src, bs, ms, range, index, out);
                    }
                }
                (Block::Quote(bs), BlockMap::Quote(ms)) => walk(src, bs, ms, range, index, out),
                (Block::Table(t), BlockMap::Table { head, rows }) => {
                    for (i, m) in
                        t.head.iter().zip(head).chain(t.rows.iter().zip(rows).flat_map(|(r, m)| r.iter().zip(m)))
                    {
                        out.push(RenderedText::new(src, &i.text, Some(m), range.clone(), index, 0));
                    }
                }
                _ => {}
            }
        }
    }
    let (blocks, _, _, ranges, maps) = doc_cached(src);
    let mut out = Vec::new();
    for (i, range) in ranges.iter().enumerate() {
        walk(src, &blocks[i..i + 1], &maps[i..i + 1], range, i, &mut out);
    }
    out
}

impl Markdown {
    /// Receives primary pointer-downs as global source caret positions. Replaces link/task
    /// activation and rendered selection while installed. Blank and non-rendered source lines
    /// gain line-height click targets, including an empty document and an EOF insertion row.
    /// Decorations without mapped text report the outer block's start.
    pub fn on_source_click<S: 'static>(mut self, f: impl Fn(&mut S, SourceClick, &mut Tx) + 'static) -> Self {
        let f = Rc::new(f);
        self.source_click = Some(Rc::new(move |el, range, mapping| {
            let f = f.clone();
            el.on_text_press(move |s: &mut S, at, tx| {
                let position = mapping.as_ref().map_or(range.start, |m| m.source_position(at));
                tx.instant();
                f(s, SourceClick { source_range: range.clone(), position }, tx);
            })
        }));
        self
    }

    /// Render `editor` once in place of the source range. The range expands to whole editable
    /// regions it touches (lists/quotes stay whole); zero-length ranges choose the region at
    /// that position. Invalid ranges are ignored. Surrounding blocks still use the complete
    /// source and its reference definitions. Does not create an editing buffer or history.
    /// The editor retains its own key beneath a `source-editor` holder. This hook disables
    /// streaming-tail presentation, since an editor uses settled boundaries.
    pub fn replace_source_range(mut self, range: Range<usize>, editor: El) -> Self {
        self.replacement = Some((range, editor));
        self
    }
}

impl Renderer {
    pub(super) fn click_decoration(&self, el: El) -> El {
        match &self.source_click {
            Some(wire) => wire(el, self.current_source_range.borrow().clone(), None),
            None => el,
        }
    }

    pub(super) fn click_text(&self, el: El, text: &str, map: Option<&SpanMap>, offset: usize, expand_tabs: bool) -> El {
        let Some(wire) = &self.source_click else { return el };
        let range = self.current_source_range.borrow().clone();
        let mut mapping = RenderedText::new(&self.source, text, map, range.clone(), 0, offset);
        if expand_tabs && text.contains('\t') {
            let mut shown = String::new();
            let mut positions = Vec::new();
            let mut col = 0;
            for (i, c) in text.char_indices() {
                shown.push(c);
                positions.extend(std::iter::repeat_n(mapping.source_position(i), c.len_utf8()));
                if c == '\t' {
                    let pad = 3 - col % 4;
                    shown.extend(std::iter::repeat_n(' ', pad));
                    positions.extend(std::iter::repeat_n(mapping.source_position(i), pad));
                    col += pad + 1;
                } else {
                    col = if c == '\n' { 0 } else { col + 1 };
                }
            }
            positions.push(mapping.source_position(text.len()));
            mapping.text = shown;
            mapping.positions = positions.into();
        }
        let el = if text.is_empty() { el.min_h(self.s.body.line_height.max(1.0)) } else { el };
        wire(el, range, Some(mapping))
    }

    pub(super) fn document(
        &self,
        blocks: &[Block],
        maps: &[BlockMap],
        ranges: &[Range<usize>],
        keys: &[String],
        replacement: Option<(Range<usize>, El)>,
    ) -> El {
        if self.source_click.is_none() && replacement.is_none() {
            return self.blocks(blocks, maps, keys, true);
        }
        let regions = regions(&self.source, ranges);
        let replacement = replacement.and_then(|(range, editor)| {
            if range.start > range.end
                || !self.source.is_char_boundary(range.start)
                || !self.source.is_char_boundary(range.end)
            {
                return None;
            }
            let touched: Vec<usize> = regions
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    if range.is_empty() {
                        r.source_range.contains(&range.start)
                            || (range.start == self.source.len() && r.source_range.is_empty())
                    } else {
                        r.source_range.start < range.end && range.start < r.source_range.end
                    }
                })
                .map(|(i, _)| i)
                .collect();
            Some((*touched.first()?..*touched.last()? + 1, editor))
        });
        let (replaced_regions, mut editor) =
            replacement.map_or((None, None), |(range, editor)| (Some(range), Some(editor)));
        let mut root = col().w_full();
        let names = rendered_keys(blocks.len(), keys);
        let mut prev_mb = 0.0f32;
        let mut first_block = true;
        for (j, region) in regions.iter().enumerate() {
            let replaced = replaced_regions.as_ref().is_some_and(|r| r.contains(&j));
            if replaced {
                if let Some(i) = region.block_index {
                    self.skip_block(&blocks[i]);
                }
                if let Some(view) = editor.take() {
                    // The editor keeps the margin its first block had when rendered.
                    let first = replaced_regions.clone().unwrap_or_default().find_map(|k| regions[k].block_index);
                    let top = match first {
                        Some(i) if !first_block => render::margins(&blocks[i], &self.s).0.max(prev_mb),
                        _ => 0.0,
                    };
                    root = root.child(
                        col().key("source-editor").w_full().mt(top).transition(rune::Transition::none()).child(view),
                    );
                }
                if let Some(i) = region.block_index {
                    prev_mb = render::margins(&blocks[i], &self.s).1;
                    first_block = false;
                }
                continue;
            }
            self.current_source_range.replace(region.source_range.clone());
            if let Some(i) = region.block_index {
                self.current_block_key.replace(names[i].clone().into());
                let (mt, mb) = render::margins(&blocks[i], &self.s);
                let top = if first_block { 0.0 } else { mt.max(prev_mb) };
                let view = self.block(&blocks[i], &maps[i]);
                // Paragraphs/headings already carry the text mapper on this very element.
                // A container gets a fallback for its border, padding and generated content.
                let view = if matches!(blocks[i], Block::Para(_) | Block::Heading(..)) {
                    view
                } else {
                    self.click_decoration(view)
                };
                root = root.child(view.key(names[i].clone()).mt(top));
                prev_mb = mb;
                first_block = false;
            } else if self.source_click.is_some() {
                root = root.child(
                    self.click_decoration(el().w_full().h(self.s.body.line_height.max(1.0)))
                        .key(format!("source-gap-{}", region.source_range.start)),
                );
            }
        }
        root
    }

    fn skip_block(&self, block: &Block) {
        match block {
            Block::Code(_) | Block::Mermaid(_) => self.next_source.set(self.next_source.get() + 1),
            Block::List { items, .. } => {
                for item in items {
                    self.next_task.set(self.next_task.get() + 1);
                    for b in item {
                        self.skip_block(b);
                    }
                }
            }
            Block::Quote(bs) => {
                for b in bs {
                    self.skip_block(b);
                }
            }
            _ => {}
        }
    }
}
