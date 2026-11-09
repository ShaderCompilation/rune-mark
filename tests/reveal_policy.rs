use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;

use rune_kit::markdown::{SyntaxMarkKind, SyntaxNode, block_syntax};
use rune_markdown_prototype::markdown::ParsedDocument;
use rune_markdown_prototype::reveal::{
    EditState, Element, Focus, MarkerHit, Reveal, Rule, constructs, marker_target, reveal,
};

const FIXTURE: &str = include_str!("fixtures/live_preview.md");

fn at(needle: &str) -> usize {
    FIXTURE
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not in fixture"))
}

fn range_of(needle: &str) -> Range<usize> {
    let start = at(needle);
    start..start + needle.len()
}

fn caret(p: usize) -> Reveal {
    reveal(&block_syntax(FIXTURE), Some(Focus::caret(p)))
}

fn shown(r: &Reveal) -> Vec<&str> {
    r.ranges.iter().map(|r| &FIXTURE[r.clone()]).collect()
}

#[test]
fn fixture_covers_every_element_in_the_table() {
    let syntax = block_syntax(FIXTURE);
    let elements: HashSet<_> = constructs(&syntax).into_iter().map(|c| c.element).collect();
    use rune_kit::markdown::InlineSyntaxKind::*;
    for e in [
        Element::Heading,
        Element::FencedCode,
        Element::Quote,
        Element::UnorderedList,
        Element::OrderedList,
        Element::TaskItem,
        Element::ItemIndent,
        Element::Inline(Strong),
        Element::Inline(Emphasis),
        Element::Inline(Strikethrough),
        Element::Inline(Code),
        Element::Inline(Link),
        Element::Inline(Image),
        Element::Inline(Autolink),
        Element::Inline(Escape),
        Element::Inline(Entity),
        Element::Inline(HardBreak),
    ] {
        assert!(elements.contains(&e), "fixture lacks {e:?}");
    }
    assert_eq!(Element::Quote.rule(), Rule::AtMarker);
    assert_eq!(Element::Heading.rule(), Rule::WhileEditing);
}

#[test]
fn inactive_editor_reveals_nothing() {
    assert_eq!(reveal(&block_syntax(FIXTURE), None), Reveal::default());
    for c in constructs(&block_syntax(FIXTURE)) {
        assert_eq!(c.state(None), EditState::Inactive);
    }
}

/// Every syntax range is revealed whenever the caret touches it, so arrow keys never stop at an
/// invisible position and Backspace/Delete always remove a visible source character.
#[test]
fn syntax_next_to_any_caret_position_is_revealed() {
    let syntax = block_syntax(FIXTURE);
    let marks: Vec<_> = syntax.iter().flat_map(|n| n.marks()).collect();
    assert!(marks.len() > 50);
    for p in (0..=FIXTURE.len()).filter(|&p| FIXTURE.is_char_boundary(p)) {
        let r = reveal(&syntax, Some(Focus::caret(p)));
        for m in marks
            .iter()
            .filter(|m| m.range.start <= p && p <= m.range.end)
        {
            assert!(
                r.shows(&m.range),
                "caret {p} next to hidden {:?} {:?}",
                m.kind,
                &FIXTURE[m.range.clone()]
            );
        }
    }
}

#[test]
fn headings_reveal_on_their_line_only() {
    let r = caret(at("Heading with") + 3);
    assert!(shown(&r).contains(&"## "));
    assert!(shown(&r).contains(&" ##"));
    // The emphasis inside is not touched by this caret.
    assert!(!shown(&r).contains(&"*"));
    let r = caret(at("Setext heading") + 2);
    assert!(shown(&r).contains(&"--------------"));
    assert!(
        caret(at("A paragraph"))
            .ranges
            .iter()
            .all(|r| !FIXTURE[r.clone()].contains('#'))
    );
}

#[test]
fn fences_reveal_anywhere_in_the_block_and_hide_the_badge() {
    let block = at("```rust");
    let r = caret(at("println!"));
    assert!(shown(&r).contains(&"```rust title=\"main.rs\""));
    assert!(shown(&r).contains(&"```"));
    assert_eq!(r.open_fences.len(), 1);
    assert_eq!(r.open_fences[0].start, block);
    assert!(
        caret(block - 2).open_fences.is_empty(),
        "the blank line before the block"
    );
}

#[test]
fn quote_prefixes_reveal_per_line_at_their_boundary() {
    let line = at("> A quote line");
    // At the start of the quoted content: only this line's prefix.
    assert_eq!(shown(&caret(line + 2)), vec!["> "]);
    // Farther inside the text: hidden.
    assert!(caret(at("A quote line") + 5).ranges.is_empty());
    // A lazy line has no prefix to reveal.
    assert!(caret(at("lazily")).ranges.is_empty());
    // Nested: the caret at the inner content reveals only the inner prefix...
    let nested = at("> > A nested");
    assert_eq!(caret(nested + 4).ranges, vec![nested + 2..nested + 4]);
    // ...and at the inner `>` it reaches both.
    assert_eq!(
        caret(nested + 2).ranges,
        vec![nested..nested + 2, nested + 2..nested + 4]
    );
}

#[test]
fn list_markers_reveal_only_the_relevant_item() {
    let second = at("- Second bullet");
    assert_eq!(shown(&caret(second + 2)), vec!["- "]);
    assert!(
        caret(second + 10).ranges.is_empty(),
        "editing farther inside the item"
    );
    assert_eq!(shown(&caret(at("Star bullet"))), vec!["* "]);
    assert_eq!(shown(&caret(at("Plus bullet"))), vec!["+ "]);
    // Authored numbering that differs from the displayed sequence.
    assert_eq!(shown(&caret(at("Seven,"))), vec!["7. "]);
    assert_eq!(shown(&caret(at("Nine with"))), vec!["9) "]);
    // Task items reveal the list and task markers together.
    assert_eq!(shown(&caret(at("Open task"))), vec!["- [ ] "]);
    // A continuation line's indentation reveals at its boundary.
    let cont = at("  and a continuation");
    assert_eq!(caret(cont + 2).ranges, vec![cont..cont + 2]);
    // Nested items: only the nested marker, not the parent's.
    assert_eq!(shown(&caret(at("Nested done task"))), vec!["- [x] "]);
}

#[test]
fn inline_delimiters_reveal_for_the_constructs_touching_the_caret() {
    let bold = range_of("**bold**");
    assert_eq!(shown(&caret(bold.start + 3)), vec!["**", "**"]);
    // Boundaries count, from either side.
    assert_eq!(shown(&caret(bold.start)), vec!["**", "**"]);
    assert_eq!(shown(&caret(bold.end)), vec!["**", "**"]);
    // Nested: italic inside bold reveals both, while unrelated constructs stay rendered.
    let r = caret(at("*italic* and") + 2);
    assert_eq!(shown(&r), vec!["**", "*", "*", "**"]);
    // Links reveal brackets and the destination and title.
    let r = caret(at("inline link") + 2);
    assert_eq!(shown(&r), vec!["[", "](https://example.com \"Title\")"]);
    assert_eq!(shown(&caret(at("reference link") + 1)), vec!["[", "][ref]"]);
    assert_eq!(shown(&caret(at("collapsed][]") + 1)), vec!["[", "][]"]);
    // Escapes and entities show their spelling while touched.
    assert_eq!(shown(&caret(at("\\*stay"))), vec!["\\"]);
    assert_eq!(shown(&caret(at("&amp;") + 2)), vec!["&amp;"]);
}

#[test]
fn selections_reveal_what_they_touch() {
    let syntax = block_syntax(FIXTURE);
    let select = |a: usize, b: usize| reveal(&syntax, Some(Focus { anchor: a, head: b }));
    // Inside a quote line's content: no prefix.
    let text = range_of("A quote line");
    assert!(
        select(text.start + 2, text.end)
            .ranges
            .iter()
            .all(|r| !FIXTURE[r.clone()].contains('>'))
    );
    // Including the prefix (backwards selection works the same).
    let r = select(text.end, text.start - 2);
    assert!(shown(&r).contains(&"> "));
    // Touching a bold construct reveals its delimiters.
    let bold = range_of("**bold**");
    let r = select(bold.start + 2, bold.start + 4);
    assert_eq!(shown(&r), vec!["**", "**"]);
    // A selection ending exactly at a construct's opener touches it.
    let nested = range_of("**bold with");
    let r = select(nested.start - 4, nested.start);
    assert!(r.shows(&(nested.start..nested.start + 2)));
    for c in constructs(&syntax)
        .iter()
        .filter(|c| c.scope.contains(&(bold.start + 3)))
    {
        assert_eq!(
            c.state(Some(Focus {
                anchor: bold.start + 2,
                head: bold.start + 4
            })),
            EditState::Selection
        );
    }
    // A selection across a whole list reveals every marker in it.
    let list = range_of("- First bullet");
    let end = at("+ Plus bullet") + 3;
    let r = select(list.start, end);
    for marker in ["- First", "- Second", "* Star", "+ Plus"] {
        let m = at(marker);
        assert!(r.shows(&(m..m + 2)), "{marker}");
    }
}

#[test]
fn edit_states_distinguish_marker_and_content_editing() {
    let syntax = block_syntax(FIXTURE);
    let second = at("- Second bullet");
    let marker = constructs(&syntax)
        .into_iter()
        .find(|c| c.scope.start == second)
        .unwrap();
    assert_eq!(marker.element, Element::UnorderedList);
    assert_eq!(
        marker.state(Some(Focus::caret(second + 1))),
        EditState::MarkerEditing
    );
    assert_eq!(
        marker.state(Some(Focus::caret(second + 9))),
        EditState::ContentEditing
    );
    assert_eq!(
        marker.state(Some(Focus::caret(at("* Star")))),
        EditState::Inactive
    );
}

#[test]
fn marker_clicks_target_their_own_source_marker() {
    let syntax = block_syntax(FIXTURE);
    let target = |hit, p| marker_target(FIXTURE, &syntax, hit, p);
    let second = at("- Second bullet");
    assert_eq!(target(MarkerHit::ListMarker, second + 20), Some(second));
    let task = at("- [ ] Open task");
    assert_eq!(target(MarkerHit::TaskMarker, task + 8), Some(task + 2));
    assert_eq!(target(MarkerHit::ListMarker, task + 8), Some(task));
    // The nested item, not its parent.
    let nested = at("- [x] Nested");
    assert_eq!(target(MarkerHit::ListMarker, nested + 8), Some(nested));
    let ordered = at("1. Nested ordered");
    assert_eq!(target(MarkerHit::ListMarker, ordered + 5), Some(ordered));
    // Quote rules by depth on a nested line.
    let line = at("> > A nested");
    assert_eq!(
        target(MarkerHit::QuoteRule { depth: 0 }, line + 6),
        Some(line)
    );
    assert_eq!(
        target(MarkerHit::QuoteRule { depth: 1 }, line + 6),
        Some(line + 2)
    );
    assert_eq!(target(MarkerHit::QuoteRule { depth: 2 }, line + 6), None);
    // A quote inside a list item.
    let inner = at("> A quote inside");
    assert_eq!(
        target(MarkerHit::QuoteRule { depth: 0 }, inner + 4),
        Some(inner)
    );
}

#[test]
fn parsed_document_reuses_syntax_across_caret_changes() {
    let doc = ParsedDocument::new(FIXTURE);
    let shared: Rc<[SyntaxNode]> = block_syntax(FIXTURE);
    assert_eq!(doc.syntax(), &*shared);
    assert_eq!(doc.syntax().len(), doc.blocks().len());
    assert_eq!(
        doc.reveal(Some(Focus::caret(at("Star bullet"))))
            .ranges
            .len(),
        1
    );
    let kinds: HashSet<_> = doc
        .syntax()
        .iter()
        .flat_map(|n| n.marks())
        .map(|m| m.kind)
        .collect();
    for k in [
        SyntaxMarkKind::HeadingMarker,
        SyntaxMarkKind::Fence,
        SyntaxMarkKind::QuotePrefix,
        SyntaxMarkKind::ListMarker,
    ] {
        assert!(kinds.contains(&k), "{k:?}");
    }
}
