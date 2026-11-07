use super::*;

#[test]
fn source_ranges_share_the_parse_cache_and_keep_containers_whole() {
    let src = "# Բարև\n\n- item\n  > nested\n  >\n  > quote\n- next\n\n> outer\n> - nested\n\nend";
    let blocks = parse_cached(src);
    let before = PARSES.with(Cell::get);
    let ranges = block_source_ranges(src);
    assert_eq!(PARSES.with(Cell::get), before);
    assert_eq!(blocks.len(), ranges.len());
    assert_eq!(
        ranges.iter().map(|r| &src[r.clone()]).collect::<Vec<_>>(),
        vec!["# Բարև\n", "- item\n  > nested\n  >\n  > quote\n- next\n\n", "> outer\n> - nested\n", "end",]
    );
    assert!(Rc::ptr_eq(&ranges, &block_source_ranges(src)));
}

#[test]
fn interrupted_numbered_lists_have_disjoint_ranges() {
    let src = "**More juice**\n4. Hash ripple.\n5. More café.\n\nAfter";
    let ranges = block_source_ranges(src);
    assert_eq!(
        ranges.iter().map(|r| &src[r.clone()]).collect::<Vec<_>>(),
        vec!["**More juice**\n", "4. Hash ripple.\n5. More café.\n", "After",]
    );
}

#[test]
fn growing_source_ranges_match_a_full_parse() {
    let source = "# Բարև\n\n- item\n  - nested\n\n> quote\n> café\n\n| a | b |\n| - | - |\n| c | d |\n\n```rust\nfn main() {\n```\n\n[x]: /target\n";
    for end in (0..=source.len()).filter(|&end| source.is_char_boundary(end)) {
        let src = &source[..end];
        let ranges = block_source_ranges(src);
        assert_eq!(&*ranges, &parse_doc(src, true).4, "source {src:?}");
        for range in ranges.iter() {
            assert!(src.get(range.clone()).is_some(), "source {src:?}, range {range:?}");
        }
    }
}
