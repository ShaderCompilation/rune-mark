use super::*;

fn s(src: &str, r: &Range<usize>) -> String {
    src[r.clone()].to_owned()
}

fn marks(src: &str) -> Vec<(SyntaxMarkKind, String)> {
    block_syntax(src).iter().flat_map(|n| n.marks()).map(|m| (m.kind, s(src, &m.range))).collect()
}

fn inline_of(node: &SyntaxNode) -> &[InlineSyntax] {
    match node {
        SyntaxNode::Paragraph(p) => &p.inline,
        SyntaxNode::Heading(h) => &h.inline,
        other => panic!("no inline syntax in {other:?}"),
    }
}

#[test]
fn nodes_parallel_blocks_and_ranges() {
    let src = "# Բարև\n\n- a\n  > q\n\n> b\n\n```rs\nx\n```\n\n| a | b |\n| - | - |\n\n***\n\ntext";
    let blocks = parse_cached(src);
    let syntax = block_syntax(src);
    assert_eq!(syntax.len(), blocks.len());
    for (node, block) in syntax.iter().zip(blocks.iter()) {
        let ok = matches!(
            (node, block),
            (SyntaxNode::Heading(_), Block::Heading(..))
                | (SyntaxNode::List(_), Block::List { .. })
                | (SyntaxNode::Quote(_), Block::Quote(_))
                | (SyntaxNode::Code(_), Block::Code(_))
                | (SyntaxNode::Table(_), Block::Table(_))
                | (SyntaxNode::Rule(_), Block::Rule)
                | (SyntaxNode::Paragraph(_), Block::Para(_))
        );
        assert!(ok, "{node:?} vs {block:?}");
    }
}

#[test]
fn headings_keep_markers_closing_sequences_and_setext_underlines() {
    let src = "  ## Բարև *em* ##  \n####### not\n\nSetext\n===\n\n> Quoted\n> ---\n";
    let syntax = block_syntax(src);
    let SyntaxNode::Heading(h) = &syntax[0] else { panic!() };
    assert_eq!((h.level, h.setext), (2, false));
    assert_eq!(s(src, &h.open), "  ## ");
    assert_eq!(s(src, &h.content), "Բարև *em*");
    assert_eq!(s(src, &h.close), " ##  ");
    let SyntaxNode::Heading(h) = &syntax[2] else { panic!("{:?}", syntax[2]) };
    assert_eq!((h.level, h.setext), (1, true));
    assert!(h.open.is_empty());
    assert_eq!(s(src, &h.close), "===");
    let SyntaxNode::Quote(q) = &syntax[3] else { panic!() };
    let SyntaxNode::Heading(h) = &q.children[0] else { panic!() };
    assert_eq!((h.level, s(src, &h.close)), (2, "---".into()));
}

#[test]
fn fences_keep_length_info_and_closing_line() {
    let src = "````rust title=\"a.rs\"\nfn a() {}\n````  \n\n> ~~~\n> x\n> ~~~\n\n- a\n\n  ```\n  ```\n\n```py\nopen";
    let syntax = block_syntax(src);
    let fence = |i: usize| match &syntax[i] {
        SyntaxNode::Code(c) => (c.clone(), c.fence.clone().unwrap()),
        SyntaxNode::Quote(q) => match &q.children[0] {
            SyntaxNode::Code(c) => (c.clone(), c.fence.clone().unwrap()),
            _ => panic!(),
        },
        SyntaxNode::List(l) => match &l.items[0].children[1] {
            SyntaxNode::Code(c) => (c.clone(), c.fence.clone().unwrap()),
            other => panic!("{other:?}"),
        },
        _ => panic!(),
    };
    let (c, f) = fence(0);
    assert_eq!(s(src, &f.marker), "````");
    assert_eq!(s(src, &f.info.unwrap()), "rust title=\"a.rs\"");
    assert_eq!(s(src, &f.opening), "````rust title=\"a.rs\"");
    assert_eq!(s(src, &f.closing.unwrap()), "````  ");
    assert_eq!(s(src, &c.content), "fn a() {}\n");
    let (_, f) = fence(1);
    assert_eq!((s(src, &f.marker), s(src, &f.closing.unwrap())), ("~~~".into(), "~~~".into()));
    let (c, f) = fence(2);
    assert!(c.content.is_empty() && f.info.is_none());
    assert_eq!(s(src, &f.closing.unwrap()), "```");
    let (_, f) = fence(3);
    assert_eq!(f.closing, None, "an unclosed fence has no closing line");
}

#[test]
fn list_items_keep_markers_numbers_tasks_and_lines() {
    let src = "- [ ] task\n  cont\nlazy\n* b\n   1. one\n   7) seven\n\n+\n  below\n";
    let syntax = block_syntax(src);
    let SyntaxNode::List(l) = &syntax[0] else { panic!() };
    let item = &l.items[0];
    assert_eq!(s(src, &item.marker), "-");
    let task = item.task.as_ref().unwrap();
    assert_eq!((s(src, &task.range), task.checked), ("[ ]".into(), false));
    assert_eq!(&src[item.content_start..item.content_start + 4], "task");
    let prefixes: Vec<_> = item.lines.iter().map(|l| l.prefix.as_ref().map(|p| s(src, p))).collect();
    assert_eq!(prefixes, vec![Some("- ".into()), Some("  ".into()), None]);
    assert_eq!(marks(src)[..3].iter().map(|m| m.1.as_str()).collect::<Vec<_>>(), ["- [ ] ", "  ", "* "]);

    // `* b` starts a second list; its nested ordered lists keep their authored numbers.
    let SyntaxNode::List(l) = &syntax[1] else { panic!("{:?}", syntax[1]) };
    let b = &l.items[0];
    let nested: Vec<_> = b
        .children
        .iter()
        .filter_map(|c| match c {
            SyntaxNode::List(l) => Some((l.start, s(src, &l.items[0].marker), l.items[0].number)),
            _ => None,
        })
        .collect();
    assert_eq!(nested, vec![(Some(1), "1.".into(), Some(1)), (Some(7), "7)".into(), Some(7))]);
    let SyntaxNode::List(one) = &b.children[1] else { panic!() };
    assert_eq!(s(src, one.items[0].lines[0].prefix.as_ref().unwrap()), " 1. ");
    assert_eq!(s(src, b.lines[1].prefix.as_ref().unwrap()), "  ");

    let SyntaxNode::List(l) = &syntax[2] else { panic!() };
    let item = &l.items[0];
    assert_eq!(s(src, &item.marker), "+");
    assert_eq!(item.content_start, item.lines[0].line.end, "content starts below the marker line");
    assert_eq!(s(src, item.lines[1].prefix.as_ref().unwrap()), "  ");
}

#[test]
fn quotes_keep_each_levels_prefix_and_lazy_lines() {
    let src = "> one\n> > two\nlazy\n\n- item\n  > in item\n  >\n  > more\n";
    let syntax = block_syntax(src);
    let SyntaxNode::Quote(q) = &syntax[0] else { panic!() };
    let prefixes: Vec<_> = q.lines.iter().map(|l| l.prefix.as_ref().map(|p| s(src, p))).collect();
    assert_eq!(prefixes, vec![Some("> ".into()), Some("> ".into()), None]);
    let SyntaxNode::Quote(inner) = &q.children[1] else { panic!("{:?}", q.children) };
    assert_eq!(inner.lines[0].prefix, Some(8..10));
    assert_eq!(inner.lines[1].prefix, None);
    let SyntaxNode::List(l) = &syntax[1] else { panic!() };
    let SyntaxNode::Quote(q) = &l.items[0].children[1] else { panic!() };
    let starts: Vec<_> = q.lines.iter().map(|l| l.prefix.clone().unwrap()).map(|p| (p.start, s(src, &p))).collect();
    let line = |n: usize| src.match_indices('\n').nth(n - 1).unwrap().0 + 1;
    assert_eq!(starts, vec![(line(5) + 2, "> ".into()), (line(6) + 2, ">".into()), (line(7) + 2, "> ".into())]);
}

#[test]
fn inline_delimiters_links_and_transforms() {
    let src = "**b** ***c*** ~s~ ~~t~~ `x` ``a ` b`` [l](<u v> \"t\") [r][ref] [c][] [s] ![i](p) <http://a.b> \\* &amp; **\\*e** a  \nb\n\n[ref]: /x\n[c]: /c\n[s]: /y\n";
    let syntax = block_syntax(src);
    let inline = inline_of(&syntax[0]);
    let got: Vec<_> = inline.iter().map(|i| (i.kind, s(src, &i.open), s(src, &i.close))).collect();
    use InlineSyntaxKind::*;
    let want: Vec<(InlineSyntaxKind, &str, &str)> = vec![
        (Strong, "**", "**"),
        (Emphasis, "*", "*"),
        (Strong, "**", "**"),
        (Strikethrough, "~", "~"),
        (Strikethrough, "~~", "~~"),
        (Code, "`", "`"),
        (Code, "``", "``"),
        (Link, "[", "](<u v> \"t\")"),
        (Link, "[", "][ref]"),
        (Link, "[", "][]"),
        (Link, "[", "]"),
        (Image, "![", "](p)"),
        (Autolink, "<", ">"),
        (Escape, "\\", ""),
        (Entity, "&amp;", ""),
        (Strong, "**", "**"),
        (Escape, "\\", ""),
        (HardBreak, "  ", ""),
    ];
    assert_eq!(got, want.into_iter().map(|(k, a, b)| (k, a.to_owned(), b.to_owned())).collect::<Vec<_>>());
    assert_eq!(inline[2].parent, Some(1), "strong inside emphasis");
    assert_eq!(inline[16].parent, Some(15), "escape inside strong");
    assert_eq!(s(src, &inline[6].content()), "a ` b");
    let link = |i: usize| inline[i].link.clone().unwrap();
    assert_eq!(s(src, &link(7).destination.unwrap()), "<u v>");
    assert_eq!(s(src, &link(7).title.unwrap()), "\"t\"");
    assert_eq!((link(8).kind, s(src, &link(8).reference.unwrap())), (LinkSyntaxKind::Reference, "ref".into()));
    assert_eq!((link(9).kind, link(9).reference), (LinkSyntaxKind::Collapsed, None));
    assert_eq!(link(10).kind, LinkSyntaxKind::Shortcut);
    assert_eq!(s(src, &link(12).destination.unwrap()), "http://a.b");
}

#[test]
fn inline_syntax_in_containers_and_multiline_code_spans() {
    let src = "> - **a** `x\n>   y` [l](u)\n";
    let syntax = block_syntax(src);
    let SyntaxNode::Quote(q) = &syntax[0] else { panic!() };
    let SyntaxNode::List(l) = &q.children[0] else { panic!() };
    let inline = inline_of(&l.items[0].children[0]);
    let got: Vec<_> = inline.iter().map(|i| s(src, &i.range)).collect();
    assert_eq!(got, ["**a**", "`x\n>   y`", "[l](u)"]);
    assert_eq!(s(src, &inline[1].content()), "x\n>   y");
}

#[test]
fn numbered_interruptions_keep_authored_markers() {
    let src = "**More juice**\n4. Hash ripple.\n5. More café.\n";
    let syntax = block_syntax(src);
    let SyntaxNode::List(l) = &syntax[1] else { panic!("{syntax:?}") };
    let got: Vec<_> =
        l.items.iter().map(|i| (s(src, &i.marker), i.number, &src[i.content_start..i.content_start + 4])).collect();
    assert_eq!(got, vec![("4.".into(), Some(4), "Hash"), ("5.".into(), Some(5), "More")]);
}

#[test]
fn syntax_reuses_the_parse_and_growing_documents_match_a_full_parse() {
    let source = "# Բարև *em*\n\n- [x] first `c`\n  - nested &amp;\n\n> quote\n> > deeper\n\n```rust\nfn main() {}\n```\n\nlast **b**";
    for end in (0..=source.len()).filter(|&i| source.is_char_boundary(i)) {
        let src = &source[..end];
        let cached = block_syntax(src);
        let before = PARSES.with(Cell::get);
        assert!(Rc::ptr_eq(&cached, &block_syntax(src)), "caret-only rereads share the cached syntax");
        parse_cached(src);
        assert_eq!(PARSES.with(Cell::get), before);
        assert_eq!(&*cached, &parse_doc(src, true).6[..], "{src:?}");
        for m in cached.iter().flat_map(|n| n.marks()) {
            assert!(src.get(m.range.clone()).is_some(), "{src:?} {m:?}");
        }
    }
}

#[test]
fn incomplete_syntax_stays_in_range() {
    for src in ["**open", "[label](", "`tick", "> ", "- ", "1.", "```", "# ", "[x]: /u\n[x", "- [ ]"] {
        for m in block_syntax(src).iter().flat_map(|n| n.marks()) {
            assert!(src.get(m.range.clone()).is_some(), "{src:?} {m:?}");
        }
    }
}
