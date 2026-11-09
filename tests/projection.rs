//! Rune's display projection: source/display mapping, collapsed-boundary sides, replacements,
//! generated decorations and caret stops, independent of any Markdown.
use rune::{CollapseSide, SourceProjection};

#[test]
fn hidden_ranges_collapse_and_map_back_by_side() {
    let src = "a **b** c";
    let p = SourceProjection::builder(src, 0..src.len())
        .hide(2..4, CollapseSide::After)
        .hide(5..7, CollapseSide::Before)
        .build();
    assert_eq!(p.text(), "a b c");
    let display: Vec<usize> = (0..=src.len()).map(|i| p.to_display(i)).collect();
    assert_eq!(display, [0, 1, 2, 2, 2, 3, 3, 3, 4, 5]);
    // Several source positions share display 2 and 3; each prefers its own side.
    assert_eq!(
        p.to_source(2),
        4,
        "an opening delimiter maps to the content after it"
    );
    assert_eq!(
        p.to_source(3),
        5,
        "a closing delimiter maps to the content before it"
    );
    assert_eq!(p.to_source(0), 0);
    assert_eq!(p.to_source(5), 9);
    assert_eq!(p.to_source_side(2, CollapseSide::Before), 2);
    assert_eq!(p.map_range(0, src.len()), [0..5]);
    assert_eq!(p.map_range(2, 7), [2..3], "only the visible content shows");
}

#[test]
fn caret_stops_land_on_the_near_side_of_hidden_bytes() {
    let src = "a **b** c";
    let p = SourceProjection::builder(src, 0..src.len())
        .hide(2..4, CollapseSide::After)
        .hide(5..7, CollapseSide::Before)
        .build();
    // Right stops before the hidden delimiter (a reveal would show it there), never inside.
    assert_eq!(p.stop_after(1), Some(2));
    assert_eq!(p.stop_after(2), Some(5));
    assert_eq!(p.stop_after(4), Some(5));
    assert_eq!(p.stop_after(8), Some(9));
    assert_eq!(p.stop_after(9), None);
    // Left stops after it.
    assert_eq!(p.stop_before(8), Some(7));
    assert_eq!(p.stop_before(7), Some(4));
    assert_eq!(p.stop_before(4), Some(1));
    assert_eq!(p.stop_before(0), None);
}

#[test]
fn replacements_map_atomically() {
    let src = "x &amp; y";
    let p = SourceProjection::builder(src, 0..src.len())
        .replace(2..7, "&", CollapseSide::After)
        .build();
    assert_eq!(p.text(), "x & y");
    for i in 2..7 {
        assert_eq!(p.to_display(i), 2, "source byte {i} inside the entity");
    }
    assert_eq!(p.to_display(7), 3);
    assert_eq!(p.to_source(2), 2);
    assert_eq!(p.to_source(3), 7);
    assert_eq!(
        p.map_range(3, 4),
        [2..3],
        "part of an entity shows all of it"
    );
    assert_eq!(p.stop_after(2), Some(7));
    assert_eq!(p.stop_before(7), Some(2));
}

#[test]
fn decorations_are_generated_text_that_maps_to_their_anchor() {
    let src = "- item";
    let p = SourceProjection::builder(src, 0..src.len())
        .hide(0..2, CollapseSide::After)
        .decorate(0, "• ", None)
        .build();
    assert_eq!(p.text(), "• item");
    let deco = "• ".len();
    assert_eq!(p.decorations(), [0..deco]);
    // Inside the decoration every position is its anchor, the item's marker.
    for d in 0..deco {
        assert_eq!(p.to_source(d), 0);
    }
    // Its end is also where the hidden marker collapses: content, by the marker's side.
    assert_eq!(p.to_source(deco), 2);
    assert_eq!(p.to_display(0), 0, "the anchor shows before the decoration");
    assert_eq!(p.to_display(2), deco);
    assert_eq!(p.map_range(0, src.len()), [deco..deco + 4]);
    assert_eq!(
        p.stop_after(0),
        Some(2),
        "Right from the marker reaches the content start"
    );
    assert_eq!(p.stop_before(3), Some(2));
    assert_eq!(p.stop_before(2), Some(0));
}

#[test]
fn unicode_and_clipping() {
    let src = "## Բարև *café*\nnext";
    let line = src.find('\n').unwrap();
    let em = src.find('*').unwrap();
    let p = SourceProjection::builder(src, 0..line)
        .hide(0..3, CollapseSide::After)
        .hide(em..em + 1, CollapseSide::After)
        .hide(line - 1..line, CollapseSide::Before)
        // Outside the range: ignored.
        .hide(line..src.len(), CollapseSide::Before)
        // Overlapping hides merge.
        .hide(1..2, CollapseSide::Before)
        .build();
    assert_eq!(p.text(), "Բարև café");
    assert_eq!(p.range(), 0..line);
    assert_eq!(p.to_display(3 + "Բ".len()), "Բ".len());
    assert_eq!(p.to_source("Բարև ".len()), em + 1);
    assert_eq!(
        p.to_display(src.len()),
        p.text().len(),
        "clamps to the range"
    );
}
