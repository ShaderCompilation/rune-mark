//! What a [`TextEdit`] draws: the display text with any preedit spliced in, and the spans,
//! links, pills and blocks mapped onto it.

use super::*;
use crate::text::Span;

impl TextEdit {
    /// Map a model byte range `start..end` (in `self.text`) onto `self.display().0` byte ranges,
    /// shifting or splitting around any active IME `preedit` splice.
    pub(crate) fn map_range_to_display(&self, start: usize, end: usize) -> Vec<(u32, u32)> {
        if (self.secure && !self.revealed) || start >= end {
            return Vec::new();
        }
        let r = self.input_bounds();
        let s = Self::clamp_char_boundary(&self.text, start.clamp(r.start, r.end));
        let e = Self::clamp_char_boundary(&self.text, end.clamp(r.start, r.end));
        if s >= e {
            return Vec::new();
        }
        let local = |s: usize, e: usize| ((s - r.start) as u32, (e - r.start) as u32);
        let Some((ref p, _)) = self.preedit else { return vec![local(s, e)]; };
        let (a, b) = self.range();
        let mut out = Vec::new();
        if s < a { out.push(local(s, e.min(a))); }
        if e > b { out.push(local(s.max(b) - (b - a) + p.len(), e - (b - a) + p.len())); }
        out
    }

    /// Build the [`Span`]s, strikethrough ranges, [`Link`]s, and per-line [`Block`]s for
    /// `self.display()` so `text_input` renders inline marks, pending IME style, and blocks.
    pub(crate) fn rich_display_specs(
        &self,
        base_style: crate::tok::TextStyle,
        style: RichStyle,
    ) -> (Vec<Span>, Vec<(u32, u32, Option<Color>)>, Vec<Link>, Vec<Block>) {
        if self.secure {
            return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        }
        let mut spans: Vec<Span> = Vec::new();
        let mut strikes: Vec<(u32, u32, Option<Color>)> = Vec::new();
        let mut links: Vec<Link> = Vec::new();

        let push_mark_span = |r_start: u32,
                              r_end: u32,
                              mark: &Mark,
                              spans: &mut Vec<Span>,
                              strikes: &mut Vec<(u32, u32, Option<Color>)>,
                              links: &mut Vec<Link>| {
            if r_start >= r_end {
                return;
            }
            match mark {
                Mark::Bold => {
                    spans.push(Span {
                        start: r_start,
                        end: r_end,
                        weight: Some((base_style.weight + 250.0).clamp(600.0, 900.0)),
                        ..Span::default()
                    });
                }
                Mark::Italic => {
                    spans.push(Span {
                        start: r_start,
                        end: r_end,
                        italic: true,
                        ..Span::default()
                    });
                }
                Mark::Underline => {
                    spans.push(Span {
                        start: r_start,
                        end: r_end,
                        underline: Some(Color::TRANSPARENT),
                        ..Span::default()
                    });
                }
                Mark::Strike => {
                    strikes.push((r_start, r_end, None));
                }
                Mark::Code => {
                    spans.push(Span {
                        start: r_start,
                        end: r_end,
                        mono: true,
                        color: style.code_fg,
                        bg: Some(style.code_bg),
                        bg_pad: 2.0,
                        ..Span::default()
                    });
                }
                Mark::Link(url) => {
                    spans.push(Span {
                        start: r_start,
                        end: r_end,
                        color: Some(style.link),
                        underline: Some(style.link.alpha(0.65)),
                        ..Span::default()
                    });
                    links.push(Link::new(r_start as usize..r_end as usize, url.clone()));
                }
                // The chip and its colours come from `display_pills`.
                Mark::Pill(_) => {}
            }
        };

        for m in &self.marks {
            for (ds, de) in self.map_range_to_display(m.start, m.end) {
                push_mark_span(ds, de, &m.mark, &mut spans, &mut strikes, &mut links);
            }
        }

        // If IME preedit is active, style the preedit range with `self.active_marks()` (which
        // includes any pending typing style or inherited marks at the caret).
        if let Some((ref p, _)) = self.preedit
            && !p.is_empty()
        {
            let (a, _) = self.range();
            let ps = (a - self.input_bounds().start) as u32;
            let pe = ps + p.len() as u32;
            for m in self.active_marks().iter().filter(|m| !m.is_pill()) {
                push_mark_span(ps, pe, m, &mut spans, &mut strikes, &mut links);
            }
        }

        // Compute display blocks per `\n`-separated line of `display().0`, and add `mono: true`
        // spans for any `Block::Code` lines.
        let (disp_text, _) = self.display();
        let display_blocks = self.display_blocks(&disp_text);
        let mut line_start = 0usize;
        let mut line_idx = 0usize;
        for (i, b) in disp_text.bytes().enumerate() {
            if b == b'\n' {
                if display_blocks.get(line_idx) == Some(&Block::Code) && line_start < i {
                    spans.push(Span {
                        start: line_start as u32,
                        end: i as u32,
                        mono: true,
                        color: style.code_fg,
                        ..Span::default()
                    });
                }
                line_start = i + 1;
                line_idx += 1;
            }
        }
        if display_blocks.get(line_idx) == Some(&Block::Code) && line_start < disp_text.len() {
            spans.push(Span {
                start: line_start as u32,
                end: disp_text.len() as u32,
                mono: true,
                color: style.code_fg,
                ..Span::default()
            });
        }

        (spans, strikes, links, display_blocks)
    }

    /// Each [`Mark::Pill`]'s byte range in `self.display().0`, with its id.
    pub(crate) fn display_pills(&self) -> Vec<(u32, u32, &str)> {
        if self.secure {
            return Vec::new();
        }
        self.marks
            .iter()
            .filter_map(|m| {
                let id = m.mark.pill_id()?;
                match self.map_range_to_display(m.start, m.end)[..] {
                    [(s, e)] if (e - s) as usize == m.end - m.start => Some((s, e, id)),
                    _ => None,
                }
            })
            .collect()
    }

    fn display_blocks(&self, disp_text: &str) -> Vec<Block> {
        if self.preedit.is_none() {
            let first = self.line_of_byte(self.input_bounds().start);
            let count = 1 + count_newlines(disp_text.as_bytes());
            return (first..first + count).map(|i| self.blocks.get(i).copied().unwrap_or(Block::Paragraph)).collect();
        }
        self.display_line_models(disp_text).into_iter()
            .map(|i| self.blocks.get(i).copied().unwrap_or(Block::Paragraph)).collect()
    }

    fn display_line_models(&self, disp_text: &str) -> Vec<usize> {
        let mut model = self.input_bounds().start;
        let mut line_index = self.line_of_byte(model);
        let mut at = 0;
        disp_text.split('\n').map(|line| {
            let next = self.model_index(at);
            line_index += count_newlines(&self.text.as_bytes()[model..next]);
            model = next;
            at += line.len() + 1;
            line_index
        }).collect()
    }

    pub(crate) fn display_soft_breaks(&self) -> Vec<u32> {
        if self.soft_breaks.is_empty() { return Vec::new(); }
        let (text, _) = self.display();
        let mut previous = None;
        self.display_line_models(&text).into_iter().enumerate().filter_map(|(i, line)| {
            let original = previous != Some(line);
            previous = Some(line);
            (i > 0 && original && self.soft_breaks.binary_search(&(line as u32)).is_ok()).then_some(i as u32)
        }).collect()
    }

    /// The text to lay out: committed text with any preedit spliced in at the caret, and the
    /// byte range of the preedit within it.
    pub fn display(&self) -> (String, Option<(usize, usize)>) {
        let r = self.input_bounds();
        let visible = &self.text[r.clone()];
        if self.secure {
            let text = if self.revealed {
                visible.to_owned()
            } else if self.is_single_line() {
                "•".repeat(visible.chars().count())
            } else if visible.is_empty() {
                String::new()
            } else {
                MULTILINE_SECURE_MASK.to_owned()
            };
            return (text, None);
        }
        match &self.preedit {
            Some((p, _)) => {
                let (a, b) = self.range();
                let mut s = String::with_capacity(visible.len() + p.len());
                s.push_str(&self.text[r.start..a]);
                s.push_str(p);
                s.push_str(&self.text[b..r.end]);
                (s, Some((a - r.start, a - r.start + p.len())))
            }
            None => (visible.to_owned(), None),
        }
    }
}
