use rune_markdown_prototype::markdown::BlockKind;
use rune_markdown_prototype::state::{EditingMode, State};

#[test]
fn nested_lists_and_quotes_are_whole_blocks_with_global_byte_ranges() {
    let heading = "# Բարև\n";
    let list = "- parent\n  - nested café\n\n    continuation\n- sibling\n";
    let quote = "> مرحبا\n>\n> - item\n>   - nested\n>\n> last paragraph\n";
    let source = format!("{heading}\n{list}\n{quote}\ntail");
    let mut state = State::new(&source);
    let blocks = state.blocks();
    assert_eq!(blocks.len(), 4);
    assert_eq!(blocks[0].kind, BlockKind::Heading(1));
    assert_eq!(blocks[1].kind, BlockKind::List);
    assert_eq!(blocks[2].kind, BlockKind::Quote);
    assert_eq!(blocks[3].kind, BlockKind::Paragraph);
    assert_eq!(&source[blocks[0].source_range.clone()], heading);
    // The parser includes the separating blank line at the end of a loose list.
    assert_eq!(&source[blocks[1].source_range.clone()], format!("{list}\n"));
    assert_eq!(&source[blocks[2].source_range.clone()], quote);
    assert_eq!(&source[blocks[3].source_range.clone()], "tail");

    let list_range = blocks[1].source_range.clone();
    let quote_range = blocks[2].source_range.clone();
    drop(blocks);
    assert!(state.activate_block_at(source.find("café").unwrap()));
    assert_eq!(state.view().active_source_range(), Some(list_range));
    assert!(state.activate_block_at(source.find("last paragraph").unwrap()));
    assert_eq!(state.view().active_source_range(), Some(quote_range));
}

#[test]
fn mode_changes_preserve_the_entire_editor_including_selection_composition_and_history() {
    let mut state = State::new("# café\n\nمرحبا");
    state.editor.insert("x");
    state.editor.insert("y");
    state.editor.undo();
    state.editor.anchor = "x# ".len();
    state.editor.focus = "x# café".len();
    state.editor.preedit = Some(("Բարև".into(), Some((0, "Բ".len()))));
    let editor = state.editor.clone();

    for mode in [EditingMode::LivePreview, EditingMode::Source] {
        state.set_mode(mode);
        assert_eq!(state.view().mode(), mode);
        assert_eq!(state.editor, editor);
    }
}

#[test]
fn block_activation_only_moves_caret_and_focus_and_preserves_redo() {
    let original = "# Բարև\n\nمرحبا";
    let mut state = State::new(original);
    state.editor.insert("first ");
    state.editor.insert("second ");
    state.editor.undo();
    let before = state.editor.clone();
    let offset = state.editor.text.find("مرحبا").unwrap();
    let mut expected = before.clone();
    expected.anchor = offset;
    expected.focus = offset;
    expected.focused = true;

    state.set_mode(EditingMode::LivePreview);
    assert!(state.activate_block_at(offset));
    assert_eq!(state.editor, expected);
    let active = state.view().active_source_range();
    state.set_mode(EditingMode::Source);
    state.set_mode(EditingMode::LivePreview);
    assert_eq!(state.view().active_source_range(), active);
    assert_eq!(state.editor, expected);

    state.editor.redo();
    assert_eq!(state.editor.text, format!("first second {original}"));
    state.editor.undo();
    assert_eq!(state.editor.text, before.text);
    state.editor.undo();
    assert_eq!(state.editor.text, original);
}

#[test]
fn invalid_offsets_do_not_change_the_editor() {
    let source = "# Բարև\n\ntext\n\n[id]: /target\n";
    let mut state = State::new(source);
    assert!(state.activate_block_at(0));
    let before = state.editor.clone();
    let active = state.view().active_source_range();
    for offset in [3, usize::MAX] {
        assert!(!state.activate_block_at(offset), "offset {offset}");
        assert_eq!(state.editor, before);
        assert_eq!(state.view().active_source_range(), active);
    }
    for source in ["", "\n \t\n", "[id]: /target\n"] {
        let mut state = State::new(source);
        assert!(state.blocks().is_empty());
        assert!(state.activate_block_at(0));
        assert_eq!(state.editor.text, source);
    }
}

#[test]
fn metadata_refresh_tracks_edits_undo_and_redo_without_editing_the_buffer() {
    let mut state = State::new("# heading");
    assert!(!state.sync_document());
    state.activate_block_at(0);
    state.editor.anchor = state.editor.text.len();
    state.editor.focus = state.editor.anchor;
    state.editor.insert("\n\n- item\n  - nested");
    let edited = state.editor.clone();
    assert!(state.sync_document());
    assert_eq!(state.editor, edited);
    assert_eq!(state.blocks().len(), 2);
    assert_eq!(state.blocks()[1].kind, BlockKind::List);
    assert_eq!(
        state.view().active_source_range(),
        Some(0..state.editor.text.len())
    );
    assert!(!state.sync_document());

    state.editor.undo();
    let undone = state.editor.clone();
    assert!(state.sync_document());
    assert_eq!(state.blocks().len(), 1);
    assert_eq!(state.editor, undone);
    state.editor.redo();
    assert!(state.sync_document());
    assert_eq!(state.blocks().len(), 2);
    assert_eq!(state.editor.text, edited.text);
}

#[test]
fn metadata_reads_are_reusable_and_current_without_a_frame_poll() {
    let mut state = State::new("# heading");
    let original_blocks = state.blocks().as_ptr();
    state.set_mode(EditingMode::LivePreview);
    state.editor.anchor = 2;
    state.editor.focus = 3;
    let blocks = state.blocks();
    let also_blocks = state.blocks();
    assert_eq!(blocks.as_ptr(), original_blocks);
    assert_eq!(also_blocks.as_ptr(), original_blocks);
    assert_eq!(state.view().mode(), EditingMode::LivePreview);
    drop(also_blocks);
    drop(blocks);

    state.editor.anchor = 0;
    state.editor.focus = state.editor.text.len();
    state.editor.insert("");
    assert!(state.blocks().is_empty());
    state.editor.undo();
    assert_eq!(state.blocks()[0].kind, BlockKind::Heading(1));
}

#[test]
fn all_rendered_block_kinds_and_incomplete_markdown_have_valid_source_slices() {
    let source = "# h\n\nparagraph **unfinished\n\n---\n\n| a | b |\n| - | - |\n| café | Բարև |\n\n```diff\n+added\n```\n\n```mermaid\ngraph TD\n```\n\n```rust\nfn main() {";
    let state = State::new(source);
    let kinds: Vec<_> = state.blocks().iter().map(|block| block.kind).collect();
    assert_eq!(
        kinds,
        vec![
            BlockKind::Heading(1),
            BlockKind::Paragraph,
            BlockKind::Rule,
            BlockKind::Table,
            BlockKind::Diff,
            BlockKind::Mermaid,
            BlockKind::Code
        ]
    );
    let mut previous_end = 0;
    for block in state.blocks().iter() {
        assert!(block.source_range.start >= previous_end);
        assert!(source.get(block.source_range.clone()).is_some());
        assert!(!block.source_range.is_empty());
        previous_end = block.source_range.end;
    }
    assert_eq!(previous_end, source.len());
}

#[test]
fn blank_hidden_and_trailing_regions_are_editable_without_changing_history() {
    let source = "# Բարև\n\ntext\n\n[id]: /target\n";
    let mut state = State::new(source);
    for offset in [
        source.find("\n\n").unwrap() + 1,
        source.find("[id]").unwrap(),
        source.len(),
    ] {
        let before = state.editor.clone();
        assert!(state.activate_block_at(offset));
        assert_eq!(state.editor.text, before.text);
        assert_eq!(state.editor.focus, offset);
        let range = state.view().active_source_range().unwrap();
        assert!(range.contains(&offset) || (range.is_empty() && offset == source.len()));
        state.editor.undo();
        assert_eq!(state.editor.text, source);
    }
    assert_eq!(
        state.regions().last().unwrap().source_range,
        source.len()..source.len()
    );
}
