use std::ops::Range;

use rune::{Cx, El, Env, Runtime, TextStyle, col, text};
use rune_kit::markdown::{Block, SourceClick, rendered_texts, source_region_at, source_regions};
use rune_kit::{Markdown, MarkdownStyle};

fn mono() -> TextStyle {
    TextStyle::new(16.0, 400.0).mono().lh(24.0)
}

fn style() -> MarkdownStyle {
    MarkdownStyle::neutral(false)
        .body_font(mono())
        .heading_font(1, mono())
        .code_block_font(mono())
}

#[derive(Default)]
struct Doc {
    source: String,
    clicked: Option<SourceClick>,
    active: Option<Range<usize>>,
    link_clicks: usize,
    task_clicks: usize,
    editor_clicks: usize,
    prose_code: bool,
}

fn view(s: &Doc, _: &mut Cx<Doc>) -> El {
    let mut md = Markdown::new(&s.source)
        .selectable(true)
        .scroll_wide(true)
        .on_link(|s: &mut Doc, _| s.link_clicks += 1)
        .on_task(|s: &mut Doc, _, _, _| s.task_clicks += 1)
        .on_source_click(|s: &mut Doc, click, _| s.clicked = Some(click));
    if let Some(range) = &s.active {
        md = md.replace_source_range(
            range.clone(),
            text("SOURCE EDITOR")
                .key("bounded-input")
                .on_press(|s: &mut Doc| s.editor_clicks += 1),
        );
    }
    let mut style = style();
    if s.prose_code {
        style = style.code_block_font(TextStyle::new(16.0, 400.0).lh(24.0));
    }
    col().w_full().child(md.render(&style))
}

fn runtime(source: &str, active: Option<Range<usize>>) -> Runtime<Doc> {
    let mut rt = Runtime::new(
        Doc {
            source: source.into(),
            active,
            ..Doc::default()
        },
        view,
        (260.0, 1600.0),
        Env::new(false, 1.0),
    );
    rt.frame(0.0);
    rt
}

fn element(rt: &Runtime<Doc>, key: &str) -> rune::runtime::ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn text_element(rt: &Runtime<Doc>, value: &str) -> rune::runtime::ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.text.as_deref() == Some(value))
        .unwrap_or_else(|| panic!("missing text {value:?}"))
}

fn click(rt: &mut Runtime<Doc>, x: f32, y: f32) -> SourceClick {
    rt.state.clicked = None;
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    rt.state.clicked.clone().expect("source callback")
}

fn click_center(rt: &mut Runtime<Doc>, e: &rune::runtime::ElementInfo) -> SourceClick {
    click(rt, e.rect[0] + e.rect[2] * 0.5, e.rect[1] + e.rect[3] * 0.5)
}

#[test]
fn regions_cover_every_source_byte_and_eof_including_hidden_source() {
    for src in [
        "",
        "\n \t\n",
        "[id]: /target\n",
        "<!-- hidden -->\n",
        "\n# café\n\ntext\n\n[id]: /target\n\n",
    ] {
        let regions = source_regions(src);
        let mut at = 0;
        for region in &regions {
            assert_eq!(region.source_range.start, at);
            assert!(src.get(region.source_range.clone()).is_some());
            at = region.source_range.end;
        }
        assert_eq!(at, src.len());
        assert_eq!(regions.last().unwrap().source_range, src.len()..src.len());
        for i in (0..=src.len()).filter(|&i| src.is_char_boundary(i)) {
            let region = source_region_at(src, i).unwrap();
            assert!(region.source_range.contains(&i) || i == src.len());
        }
        assert!(source_region_at(src, usize::MAX).is_none());
    }
    assert!(source_region_at("Բարև", 1).is_none());
}

#[test]
fn formatted_text_maps_exactly_to_global_source_boundaries() {
    let src = "# Բարև **bold** [link][id] and `code`\n\n[id]: /target\n";
    let runs = rendered_texts(src);
    let t = &runs[0];
    for word in ["Բարև", "bold", "link", "code"] {
        let rendered = t.text().find(word).unwrap();
        let source = src.find(word).unwrap();
        for (offset, _) in word.char_indices() {
            assert_eq!(
                t.source_position(rendered + offset),
                source + offset,
                "{word}"
            );
        }
    }
    let code = t.text().find("code").unwrap();
    assert!(
        (src.find("`code`").unwrap()..=src.find("code").unwrap())
            .contains(&t.source_position(code - 1))
    );
    assert_eq!(
        t.source_position(t.text().len()),
        src.find("code").unwrap() + 4
    );
    assert_eq!(t.source_position(3), src.find("Բ").unwrap() + "Բ".len());
    let Block::Heading(_, inline) = &rune_kit::markdown::parse_cached(src)[0] else {
        panic!()
    };
    assert_eq!(inline.links[0].2, "/target");
}

#[test]
fn entities_escapes_and_normalized_code_are_atomic_and_utf8_safe() {
    let src = "café &amp; &#x1F642; &NotEqualTilde; &nLt; \\* `a\nb`";
    let runs = rendered_texts(src);
    let t = &runs[0];
    for (rendered, raw) in [
        ("&", "&amp;"),
        ("🙂", "&#x1F642;"),
        ("≂\u{338}", "&NotEqualTilde;"),
        ("≪\u{20d2}", "&nLt;"),
    ] {
        let at = t.text().find(rendered).unwrap();
        let start = src.find(raw).unwrap();
        for (i, _) in rendered.char_indices() {
            assert_eq!(t.source_position(at + i), start, "{raw}");
        }
        assert_eq!(
            t.source_position(at + rendered.len()),
            start + raw.len(),
            "{raw} end"
        );
    }
    let literal_code = rendered_texts("`\\*`");
    assert_eq!(
        literal_code[0].source_position(literal_code[0].text().find('*').unwrap()),
        2
    );
    let escaped = rendered_texts("a \\*literal");
    let literal_start = escaped[0].text().find("*literal").unwrap();
    assert_eq!(escaped[0].source_position(literal_start), 2);
    assert_eq!(escaped[0].source_position(literal_start + 1), 4);
    let star = t.source_position(t.text().find('*').unwrap());
    assert_eq!(star, src.find("\\*").unwrap());
    let code = t.text().find("a b").unwrap();
    assert_eq!(t.source_position(code), src.find("a\nb").unwrap());
    assert_eq!(t.source_position(code + 2), src.find("\nb").unwrap() + 1);
    for i in 0..=t.text().len() + 1 {
        let pos = t.source_position(i);
        assert!(src.is_char_boundary(pos));
        for raw in ["&amp;", "&#x1F642;", "&NotEqualTilde;", "&nLt;"] {
            let start = src.find(raw).unwrap();
            assert!(!(start + 1..start + raw.len()).contains(&pos));
        }
    }
}

#[test]
fn nested_containers_tables_unicode_and_incomplete_input_have_global_mappings() {
    let src = "- parent\n  > Բարև **nested**\n\n| a | café |\n| - | - |\n| \\| | &amp; |\n\n```rust\nfn café() {";
    let runs = rendered_texts(src);
    for word in ["parent", "Բարև", "nested", "café", "fn café() {"] {
        let t = runs.iter().find(|t| t.text().contains(word)).unwrap();
        assert_eq!(
            t.source_position(t.text().find(word).unwrap()),
            src.find(word).unwrap()
        );
    }
    let nested = runs.iter().find(|t| t.text().contains("nested")).unwrap();
    assert_eq!(nested.block_index, 0);
    assert_eq!(nested.source_range.start, 0);
    assert!(nested.source_range.end <= src.find('|').unwrap());
    for run in runs {
        for i in 0..=run.text().len() {
            assert!(src.is_char_boundary(run.source_position(i)));
        }
    }
}

#[test]
fn empty_hidden_blank_and_trailing_regions_are_real_click_targets() {
    for src in ["#", "![](image.png)"] {
        let mut rt = runtime(src, None);
        let key = if source_region_at(src, 0).unwrap().block_index.is_some() {
            "b0"
        } else {
            "source-gap-0"
        };
        let e = element(&rt, key);
        assert!(e.rect[3] > 0.0);
        assert_eq!(click_center(&mut rt, &e).position, 0);
    }

    for src in [
        "",
        "\n\n",
        "[id]: /target\n",
        "<!-- hidden -->\n",
        "text\n\n",
    ] {
        let mut rt = runtime(src, None);
        for region in source_regions(src)
            .iter()
            .filter(|r| r.block_index.is_none())
        {
            let e = element(&rt, &format!("source-gap-{}", region.source_range.start));
            assert!(e.rect[3] > 0.0);
            let click = click_center(&mut rt, &e);
            assert_eq!(click.source_range, region.source_range);
            assert_eq!(click.position, region.source_range.start);
        }
    }
}

#[test]
fn actual_text_clicks_map_entities_and_links_without_following_them() {
    let src = "A &amp; [link][id]\n\n[id]: /target\n";
    let mut rt = runtime(src, None);
    let e = text_element(&rt, "A & link");
    let entity_x = rune::text::advance("A ", mono());
    let entity_w = rune::text::advance("&", mono());
    let link_x = rune::text::advance("A & l", mono());
    let y = e.rect[1] + 12.0;
    assert_eq!(
        click(&mut rt, e.rect[0] + entity_x + entity_w * 0.1, y).position,
        src.find("&amp;").unwrap()
    );
    assert_eq!(
        click(&mut rt, e.rect[0] + entity_x + entity_w * 0.9, y).position,
        src.find("&amp;").unwrap() + 5
    );
    assert_eq!(
        click(&mut rt, e.rect[0] + link_x + 0.1, y).position,
        src.find("link").unwrap() + 1
    );
    assert_eq!(rt.state.link_clicks, 0);
}

#[test]
fn wrapped_clicks_use_the_shaped_layout() {
    let src = "one two three four five six seven eight nine ten eleven twelve";
    let mut rt = runtime(src, None);
    let e = text_element(&rt, src);
    assert!(e.rect[3] > 24.0);
    let click = click(&mut rt, e.rect[0] + 0.5, e.rect[1] + 36.0);
    assert!(click.position > 0 && click.position < src.len());
    assert_eq!(click.source_range, 0..src.len());
    assert_eq!(src.as_bytes()[click.position - 1], b' ');
}

#[test]
fn decorations_and_task_controls_map_to_the_outer_block_start() {
    let src = "intro\n\n- bullet\n  - nested\n- [ ] task\n\n> quote\n\n---";
    let mut rt = runtime(src, None);
    let bullet = text_element(&rt, "•");
    assert_eq!(
        click_center(&mut rt, &bullet).position,
        src.find("- bullet").unwrap()
    );
    let control = element(&rt, "control");
    assert_eq!(
        click_center(&mut rt, &control).position,
        src.find("- bullet").unwrap()
    );
    assert_eq!(rt.state.task_clicks, 0);
    let rule = element(&rt, "b3");
    assert_eq!(
        click_center(&mut rt, &rule).position,
        src.find("---").unwrap()
    );
    let quote = element(&rt, "b2");
    assert_eq!(
        click(&mut rt, quote.rect[0] + 0.5, quote.rect[1] + 8.0).position,
        src.find('>').unwrap()
    );
}

#[test]
fn replacement_keeps_reference_context_and_preserves_surrounding_keys() {
    let src = "# head\n\n[link][id]\n\n[id]: /target\n\nafter";
    let start = src.find("[id]:").unwrap();
    let mut rt = runtime(src, Some(start..start + "[id]: /target\n".len()));
    let link = text_element(&rt, "link");
    assert_eq!(link.text.as_deref(), Some("link"));
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("source-editor"))
    );
    assert!(
        element(&rt, "bounded-input")
            .path
            .ends_with("source-editor/bounded-input")
    );
    assert_eq!(element(&rt, "b0").text.as_deref(), Some("head"));
    assert_eq!(element(&rt, "b2").text.as_deref(), Some("after"));
    let editor = element(&rt, "source-editor");
    rt.pointer_down(editor.rect[0] + 2.0, editor.rect[1] + 8.0);
    rt.pointer_up(editor.rect[0] + 2.0, editor.rect[1] + 8.0);
    rt.frame(0.0);
    assert_eq!(rt.state.editor_clicks, 1);
}

#[test]
fn replacing_lists_or_multiple_regions_does_not_shift_later_task_metadata() {
    let src =
        "- [ ] old\n  - [x] nested\n\n```rust\nfn old() {}\n```\n\n- [x] later\n\n```json\n{}\n```";
    let end = src.find("- [x] later").unwrap();
    // No source-click hook in this test: the surviving checkbox must still dispatch the
    // checked value and marker position from the full parse, and syntax highlighting aligns.
    #[derive(Default)]
    struct S {
        languages: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
        task: Option<(usize, bool)>,
        source: &'static str,
        end: usize,
    }
    let mut rt = Runtime::new(
        S {
            source: src,
            end,
            ..S::default()
        },
        |s: &S, _: &mut Cx<S>| {
            let languages = s.languages.clone();
            Markdown::new(s.source)
                .replace_source_range(0..s.end, text("editor"))
                .highlight(move |lang, _| {
                    languages
                        .borrow_mut()
                        .push(lang.unwrap_or_default().to_owned());
                    Vec::new()
                })
                .on_task(|s: &mut S, _, at, checked| s.task = Some((at, checked)))
                .render(&style())
        },
        (300.0, 800.0),
        Env::new(false, 1.0),
    );
    rt.frame(0.0);
    assert!(
        !rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("old"))
    );
    let control = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("control"))
        .unwrap();
    let x = control.rect[0] + 8.0;
    let y = control.rect[1] + 8.0;
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.0);
    assert_eq!(rt.state.task, Some((end + 2, false)));
    assert!(!rt.state.languages.borrow().is_empty());
    assert!(
        rt.state
            .languages
            .borrow()
            .iter()
            .all(|lang| lang == "json")
    );
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("{}"))
    );
}

#[test]
fn replacement_handles_empty_eof_gaps_and_expands_nested_blocks() {
    for (src, range) in [
        ("", 0..0),
        ("text", 4..4),
        ("text\n\n", 5..6),
        ("- outer\n  - inner\n\nafter", 14..14),
    ] {
        let rt = runtime(src, Some(range));
        assert_eq!(
            rt.elements()
                .iter()
                .filter(|e| e.text.as_deref() == Some("SOURCE EDITOR"))
                .count(),
            1
        );
        if src.starts_with('-') {
            assert!(
                !rt.elements()
                    .iter()
                    .any(|e| e.text.as_deref() == Some("inner"))
            );
            text_element(&rt, "after");
        }
    }
    let rt = runtime("Բարև", Some(1..2));
    text_element(&rt, "Բարև");
    assert!(
        !rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("SOURCE EDITOR"))
    );
}

#[test]
fn diff_lines_and_expanded_tabs_use_the_correct_source_positions() {
    let src = "```diff\n+first\n\n-second\n```";
    let mut rt = runtime(src, None);
    let second = text_element(&rt, "-second");
    let prefix = rune::text::advance("-s", mono());
    assert_eq!(
        click(
            &mut rt,
            second.rect[0] + prefix + 0.1,
            second.rect[1] + 12.0
        )
        .position,
        src.find("-second").unwrap() + 2
    );
    let blank = text_element(&rt, " ");
    assert_eq!(click_center(&mut rt, &blank).position, 0);

    let mut rt = runtime("```\na\tb\n```", None);
    rt.update(|s, _| s.prose_code = true);
    rt.frame(0.0);
    let e = text_element(&rt, "a\t  b");
    let mut positions = Vec::new();
    for x in (0..100).step_by(2) {
        positions.push(click(&mut rt, e.rect[0] + x as f32, e.rect[1] + 12.0).position);
    }
    assert!(positions.contains(&"```\na\t".len()));
    assert!(positions.iter().all(|&p| (4..=7).contains(&p)));
}
