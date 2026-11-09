//! Line prefixes: each container consumes its prefix from the lines it spans, in nesting
//! order, which resolves quote prefixes, list markers, heading markers and fences.

use super::*;

pub(super) struct Lines<'a> {
    src: &'a str,
    /// Per source line: (start, end without newline, next line's start, where enclosing
    /// prefixes end, whether an enclosing container treats it as a lazy continuation).
    lines: Vec<Line>,
}

#[derive(Clone, Copy)]
struct Line {
    start: usize,
    end: usize,
    next: usize,
    cursor: usize,
    lazy: bool,
}

impl<'a> Lines<'a> {
    pub(super) fn new(src: &'a str, range: Range<usize>) -> Self {
        let mut lines = Vec::new();
        let mut start = src[..range.start].rfind('\n').map_or(0, |i| i + 1);
        loop {
            let next = src[start..].find('\n').map_or(src.len(), |i| start + i + 1);
            let end = src[start..next].trim_end_matches(['\n', '\r']).len() + start;
            lines.push(Line { start, end, next, cursor: start, lazy: false });
            if next >= range.end || next == src.len() {
                break;
            }
            start = next;
        }
        Lines { src, lines }
    }

    /// Indexes of the lines a container with `range` spans: from its first line, through lines
    /// whose enclosing prefixes end before the container does.
    fn spanned(&self, range: &Range<usize>) -> Vec<usize> {
        (0..self.lines.len())
            .filter(|&i| {
                let l = &self.lines[i];
                l.next > range.start && l.cursor < range.end
            })
            .collect()
    }

    fn line_of(&self, at: usize) -> Option<Line> {
        self.lines.iter().rev().find(|l| l.start <= at).copied()
    }

    pub(super) fn resolve(&mut self, node: &mut SyntaxNode) {
        match node {
            SyntaxNode::Paragraph(_) | SyntaxNode::Table(_) | SyntaxNode::Rule(_) => {}
            SyntaxNode::Heading(h) => self.heading(h),
            SyntaxNode::Code(c) => self.code(c),
            SyntaxNode::List(l) => {
                for item in &mut l.items {
                    self.item(item);
                }
            }
            SyntaxNode::Quote(q) => {
                for i in self.spanned(&q.range) {
                    let l = self.lines[i];
                    let prefix = (!l.lazy).then(|| quote_prefix(self.src, l.cursor, l.end)).flatten();
                    match &prefix {
                        Some(p) => self.lines[i].cursor = p.end,
                        None => self.lines[i].lazy = true,
                    }
                    q.lines.push(ContainerLine { line: l.start..l.end, prefix });
                }
                for child in &mut q.children {
                    self.resolve(child);
                }
            }
        }
    }

    fn heading(&self, h: &mut HeadingSyntax) {
        let src = self.src;
        let first = self.line_of(h.range.start).map_or(h.range.start..h.range.end, |l| l.start..l.end);
        let indent = skip_ws(src, h.range.start, first.end);
        if src.as_bytes().get(indent) == Some(&b'#') {
            let content = h.content.clone();
            let has_content = content.start < content.end;
            // Pulldown starts the heading after its indentation; keep that with the markers.
            let cursor = self.line_of(h.range.start).map_or(h.range.start, |l| l.cursor);
            let start = if cursor < h.range.start && src[cursor..h.range.start].trim().is_empty() {
                cursor
            } else {
                h.range.start
            };
            h.open = start..if has_content { content.start } else { first.end };
            h.close = if has_content { content.end.max(h.open.end)..first.end } else { first.end..first.end };
        } else {
            h.setext = true;
            h.open = h.range.start..h.range.start;
            let last = self.line_of(h.range.end.saturating_sub(1).max(h.range.start));
            h.close = last.map_or(h.range.end..h.range.end, |l| {
                let start = skip_ws(src, l.cursor, l.end);
                start..src[..l.end].trim_end().len().max(start)
            });
        }
    }

    fn code(&self, c: &mut CodeSyntax) {
        let Some(fence) = &mut c.fence else { return };
        let src = self.src;
        let bytes = src.as_bytes();
        let first_end = self.line_of(c.range.start).map_or(c.range.end, |l| l.end);
        let start = skip_ws(src, c.range.start, first_end);
        let Some(&ch) = bytes.get(start) else { return };
        let len = bytes[start..first_end].iter().take_while(|&&b| b == ch).count();
        fence.opening = start..first_end;
        fence.marker = start..start + len;
        let info = src[start + len..first_end].trim();
        if !info.is_empty() {
            let at = start + len + src[start + len..first_end].find(info).unwrap_or(0);
            fence.info = Some(at..at + info.len());
        }
        let Some(last) = self.line_of(c.range.end.saturating_sub(1).max(c.range.start)) else { return };
        let text_end = if c.content.is_empty() { last.start } else { c.content.end };
        if last.start <= c.range.start || last.start < text_end {
            return;
        }
        let at = skip_ws(src, last.cursor, last.end);
        let run = bytes[at..last.end].iter().take_while(|&&b| b == ch).count();
        if run >= len && src[at + run..last.end].trim().is_empty() {
            fence.closing = Some(at..last.end);
        }
    }

    fn item(&mut self, item: &mut ListItemSyntax) {
        let src = self.src;
        let bytes = src.as_bytes();
        let spanned = self.spanned(&item.range);
        let Some((&first, rest)) = spanned.split_first() else { return };
        let l = self.lines[first];
        let m = skip_ws(src, item.range.start, l.end);
        let digits = bytes[m..l.end].iter().take_while(|b| b.is_ascii_digit()).count();
        let marker_len = if digits > 0 { digits + 1 } else { 1 };
        item.marker = m..(m + marker_len).min(l.end);
        item.number = (digits > 0).then(|| src[m..m + digits].parse().ok()).flatten();
        let after = item.marker.end;
        let ws_end = skip_ws(src, after, l.end);
        // Content on the first line, or (1-4 columns of spacing) the rest after one space when
        // five or more start indented code.
        let (content, indent) = if ws_end == l.end {
            (l.end, cols(src, l.start, after) - cols(src, l.start, l.cursor) + 1)
        } else if cols(src, l.start, ws_end) - cols(src, l.start, after) >= 5 {
            (after + 1, cols(src, l.start, after) + 1 - cols(src, l.start, l.cursor))
        } else {
            (ws_end, cols(src, l.start, ws_end) - cols(src, l.start, l.cursor))
        };
        item.lines.push(ContainerLine { line: l.start..l.end, prefix: Some(l.cursor..content) });
        self.lines[first].cursor = content;
        item.content_start = match &item.task {
            Some(t) => t.range.end + usize::from(bytes.get(t.range.end).is_some_and(|b| matches!(b, b' ' | b'\t'))),
            None => content,
        };
        for &i in rest {
            let l = self.lines[i];
            if l.lazy {
                item.lines.push(ContainerLine { line: l.start..l.end, prefix: None });
                continue;
            }
            let base = cols(src, l.start, l.cursor);
            let mut p = l.cursor;
            while p < l.end && matches!(bytes[p], b' ' | b'\t') && cols(src, l.start, p) - base < indent {
                p += 1;
            }
            let blank = src[l.cursor..l.end].trim().is_empty();
            let prefix = (blank || cols(src, l.start, p) - base >= indent).then(|| l.cursor..p);
            match &prefix {
                Some(_) => self.lines[i].cursor = p,
                None => self.lines[i].lazy = true,
            }
            item.lines.push(ContainerLine { line: l.start..l.end, prefix });
        }
        for child in &mut item.children {
            self.resolve(child);
        }
    }
}

/// A quote marker after up to three spaces from `at`, with one following space or tab.
fn quote_prefix(src: &str, at: usize, end: usize) -> Option<Range<usize>> {
    let b = src.as_bytes();
    let gt = skip_ws(src, at, end);
    if gt >= end || b[gt] != b'>' || gt - at > 3 {
        return None;
    }
    let after = gt + 1 + usize::from(b.get(gt + 1).is_some_and(|c| matches!(c, b' ' | b'\t')) && gt + 1 < end);
    Some(at..after)
}

fn skip_ws(src: &str, mut at: usize, end: usize) -> usize {
    let b = src.as_bytes();
    while at < end && matches!(b[at], b' ' | b'\t') {
        at += 1;
    }
    at
}

/// The column of byte `at` on the line starting at `line`, with tab stops every four columns.
fn cols(src: &str, line: usize, at: usize) -> usize {
    src[line..at].chars().fold(0, |c, ch| if ch == '\t' { c + 4 - c % 4 } else { c + 1 })
}
