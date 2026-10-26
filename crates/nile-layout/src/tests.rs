//! Unit tests for the crate root; split out of lib.rs, which they used to follow.
use super::*;
use nile_style::compute_styles;

#[test]
fn flex_row_splits_width() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div style="display:flex; gap:10px">
             <div id="a" style="flex:1; height:20px"></div>
             <div id="b" style="flex:1; height:20px"></div>
           </div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 210.0, 100.0);
    let find = |id: &str| {
        let n = doc.find(|n| n.attr("id") == Some(id)).unwrap();
        tree.boxes.iter().find(|b| b.node == n).unwrap().rect
    };
    assert_eq!(find("a"), Rect { x: 0.0, y: 0.0, width: 100.0, height: 20.0 });
    assert_eq!(find("b").x, 110.0);
}

#[test]
fn full_height_root_fills_the_viewport_and_still_scrolls() {
    let doc = Document::parse(
        r#"<html style="height:100%"><body style="margin:0;height:100%;display:flex;flex-direction:column">
             <div style="height:30px"></div><div id="a" style="flex:1"></div></body></html>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 200.0, 300.0);
    let a = doc.find(|n| n.attr("id") == Some("a")).unwrap();
    assert_eq!(tree.boxes.iter().find(|b| b.node == a).unwrap().rect.height, 270.0);
    let tall = Document::parse(r#"<html style="height:100%"><body style="margin:0;height:100%"><div style="height:900px"></div></body></html>"#);
    let tree = layout(&tall, &compute_styles(&tall), &mut text, 200.0, 300.0);
    assert_eq!(tree.content_height, 900.0);
}

#[test]
fn paged_elements_stack_sheets_and_map_points_to_flow_positions() {
    let rows: String = (0..10).map(|i| format!("<div class=r id=r{i}></div>")).collect();
    let doc = Document::parse(&format!(
        "<style>@page {{ size: 300px 200px; margin: 20px }} body{{margin:0}} .r{{height:30px;break-inside:avoid}} #d{{page-view:paged}}</style><div id=d>{rows}</div><p id=after style=\"margin:0\">after</p>"
    ));
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
    let id = |n: &str| doc.find(|e| e.attr("id") == Some(n)).unwrap();
    // 160px of content per sheet: five rows, so two sheets of 200px and a 20px gap.
    assert_eq!(tree.pages.len(), 1);
    let pb = &tree.pages[0];
    assert_eq!((pb.rect.width, pb.rect.height, pb.count()), (300.0, 420.0, 2));
    assert_eq!(pb.starts, vec![20.0, 170.0]);
    // The next element comes after both sheets.
    assert_eq!(tree.node_rect(id("after")).unwrap().y, 420.0);
    // Row 5 is the first on sheet two: flow y 170 appears at 220 + 20.
    assert_eq!(pb.to_screen(170.0), 240.0);
    let offsets = ScrollOffsets::new();
    assert_eq!(tree.node_rect_scrolled(id("r5"), &offsets).unwrap().y, 240.0);
    assert_eq!(tree.node_rect_scrolled(id("r4"), &offsets).unwrap().y, 20.0 + 4.0 * 30.0);
    assert_eq!(tree.hit_test_scrolled(50.0, 245.0, &offsets), Some(id("r5")));
    assert_eq!(tree.hit_test_scrolled(50.0, 30.0, &offsets), Some(id("r0")));
    // A margin or the gap between sheets is the element itself, not a row.
    assert_eq!(tree.hit_test_scrolled(50.0, 210.0, &offsets), Some(id("d")));
    assert_eq!(tree.hit_test_scrolled(50.0, 225.0, &offsets), Some(id("d")));
}

fn text_boxes(tree: &LayoutTree) -> Vec<&LayoutBox> {
    tree.boxes.iter().filter(|b| matches!(b.content, BoxContent::Text(_))).collect()
}

#[test]
fn inline_elements_share_one_line() {
    let doc = Document::parse(
        r#"<body style="margin:0"><p style="margin:0">Hello <b>world</b> and <span>more</span>.</p></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
    let texts = text_boxes(&tree);
    assert_eq!(texts.len(), 1, "inline content must be a single run");
    // One line of 16px text at line-height 1.2 (rounded by Taffy).
    assert!(texts[0].rect.height < 16.0 * 1.2 * 1.5, "got {}", texts[0].rect.height);
}

#[test]
fn whitespace_collapses_across_elements() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div>  a <b> b </b> c  </div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let _ = layout(&doc, &styles, &mut text, 800.0, 100.0);
    let mut inline = InlineText::new();
    let div = doc.find(|n| n.tag() == Some("div")).unwrap();
    for &c in &doc.node(div).children {
        inline.collect(&doc, &styles, c, 0, &mut |_| None);
    }
    inline.trim_end();
    assert_eq!(inline.text, "a b c");
}

fn run_width(html: &str) -> (f32, usize) {
    let doc = Document::parse(html);
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
    let t = text_boxes(&tree)[0];
    let BoxContent::Text(c) = &t.content else { unreachable!() };
    (c.layout.full_width(), c.decos.len())
}

#[test]
fn inline_padding_and_border_take_horizontal_space() {
    // sans-serif, not the platform's UI font: a spacer between letters also takes away their kerning.
    let plain = run_width(r#"<body style="margin:0;font-family:sans-serif"><p style="margin:0">a<span>b</span>c</p></body>"#);
    let padded = run_width(
        r#"<body style="margin:0;font-family:sans-serif"><p style="margin:0">a<span style="padding:0 6px; border:2px solid red">b</span>c</p></body>"#,
    );
    assert_eq!(plain.1, 0);
    assert_eq!(padded.1, 1);
    assert!((padded.0 - plain.0 - 16.0).abs() < 0.5, "plain {} padded {}", plain.0, padded.0);
}

#[test]
fn spaces_collapse_around_padded_inline() {
    let a = run_width(r#"<body style="margin:0"><p style="margin:0">a <span style="padding:0 5px"> b </span> c</p></body>"#);
    let b = run_width(r#"<body style="margin:0"><p style="margin:0">a <span style="padding:0 5px">b</span> c</p></body>"#);
    assert!((a.0 - b.0).abs() < 0.5, "{} vs {}", a.0, b.0);
}

#[test]
fn node_rect_for_blocks_and_inline_elements() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div id="d" style="width:200px; padding:10px; height:50px; font-family:sans-serif"><span id="s">hi <b id="b">bold</b></span><i id="none" style="display:none">x</i></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    assert_eq!(tree.node_rect(id("d")).unwrap(), Rect { x: 0.0, y: 0.0, width: 220.0, height: 70.0 });
    let (s, b) = (tree.node_rect(id("s")).unwrap(), tree.node_rect(id("b")).unwrap());
    assert!(s.x >= 10.0 && b.x > s.x && b.x + b.width <= s.x + s.width + 0.5, "{s:?} {b:?}");
    assert!(b.height > 10.0 && b.y >= 10.0);
    assert_eq!(tree.node_rect(id("none")), None);
}

#[test]
fn text_inputs_show_value_or_placeholder_without_wrapping() {
    let doc = Document::parse(
        r#"<body style="margin:0"><input id="a" value="hello world, a long value that must not wrap" style="width:60px">
           <input id="b" placeholder="Name"><input id="c" type="password" value="abc"><input id="d"></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    let text_of = |n: NodeId| {
        tree.boxes.iter().find(|b| b.node == n && matches!(b.content, BoxContent::Text(_))).unwrap()
    };
    let a = text_of(id("a"));
    let BoxContent::Text(c) = &a.content else { unreachable!() };
    assert_eq!(c.layout.lines().count(), 1, "long value stays on one line");
    // The empty input still has a line's height; placeholder is its text.
    let d = tree.node_rect(id("d")).unwrap();
    assert!(d.height > 16.0, "{d:?}");
    let b = text_of(id("b"));
    let BoxContent::Text(c) = &b.content else { unreachable!() };
    assert_eq!(c.owners[0].1, id("b"));
    // Caret mapping: passwords draw bullets (3 bytes each).
    assert_eq!(caret_byte(&doc, id("c"), 2), 6);
    assert_eq!(caret_char(&doc, id("c"), 6), 2);
    assert_eq!(caret_byte(&doc, id("a"), 5), 5);
    let a_rect = tree.node_rect(id("a")).unwrap();
    let idx = tree.text_index_at(id("a"), a_rect.x + 1.0, a_rect.y + a_rect.height / 2.0).unwrap();
    assert!(idx <= 1, "clicking the left edge puts the caret at the start, got {idx}");
    assert!(tree.text_index_at(id("a"), a_rect.x + 500.0, a_rect.y + 8.0).unwrap() > 5);
}

#[test]
fn password_char_is_set_by_css() {
    let doc = Document::parse(
        r#"<style>#c { nile-password-char: "*" } #e { nile-password-char: '●' }</style>
           <body><input id="c" type="password" value="abc"><input id="e" type="password" value="abc"><input id="p" type="password" value="abc"></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    layout(&doc, &styles, &mut text, 800.0, 100.0);
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    assert_eq!(control_display(&doc, id("c")).0, "***");
    assert_eq!(control_display(&doc, id("e")).0, "●●●");
    assert_eq!(control_display(&doc, id("p")).0, "\u{2022}\u{2022}\u{2022}");
    assert_eq!(caret_byte(&doc, id("c"), 2), 2);
    assert_eq!(caret_byte(&doc, id("e"), 2), 6);
    assert_eq!(caret_char(&doc, id("e"), 6), 2);
}

#[test]
fn textareas_wrap_and_move_vertically() {
    let doc = Document::parse(
        r#"<body style="margin:0"><textarea id="t" style="width:80px; height:60px">one two three four five six
second</textarea><textarea id="e" style="padding:3px 4px; border:1px solid #777"></textarea></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 400.0);
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    assert_eq!(control_value(&doc, id("t")).lines().count(), 2);
    let tb = tree
        .boxes
        .iter()
        .find(|b| matches!(&b.content, BoxContent::Text(c) if c.owners.iter().any(|(_, n)| *n == id("t"))))
        .unwrap();
    let BoxContent::Text(c) = &tb.content else { unreachable!() };
    assert!(c.layout.lines().count() >= 3, "wraps and honours the newline: {}", c.layout.lines().count());
    // Down from the start of line one lands later in the text; up returns.
    let down = tree.move_caret_vertically(id("t"), 0, 1).unwrap();
    assert!(down > 0);
    assert_eq!(tree.move_caret_vertically(id("t"), down, -1), Some(0));
    assert_eq!(tree.move_caret_vertically(id("t"), 0, -1), None);
    let rect = tree.node_rect(id("e")).unwrap();
    assert_eq!((rect.width, rect.height), (210.0, 68.0), "the UA size plus padding and border");
}

#[test]
fn select_shows_its_choice_sizes_to_the_longest_option_and_opens_a_menu() {
    let doc = Document::parse(
        r#"<body style="margin:0"><select id="s"><option>Red</option><option selected>A much longer option</option><option disabled>Blue</option></select>
           <select id="e"></select></body>"#,
    );
    let styles = compute_styles(&doc);
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    assert_eq!(selected_index(&doc, id("s")), Some(1));
    assert_eq!(option_label(&doc, select_options(&doc, id("s"))[2]), "Blue");
    assert_eq!(selected_index(&doc, id("e")), None);

    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 400.0);
    let (s, e) = (tree.node_rect(id("s")).unwrap(), tree.node_rect(id("e")).unwrap());
    assert!(s.width > 100.0, "wide enough for the longest option: {s:?}");
    assert!(e.height > 16.0, "an empty select still has a line");
    assert!(tree.menu.is_none());

    let opts = LayoutOptions { preedit: None, menu: Some(MenuRequest { select: id("s"), highlight: Some(0), scroll: 0.0 }), scroll_y: 0.0, offsets: None, images: None };
    let tree = layout_with(&doc, &styles, &mut text, 800.0, 400.0, &opts);
    let m = tree.menu.as_ref().unwrap();
    assert_eq!((m.rect.x, m.rect.y), (s.x, s.y + s.height), "opens right under the select");
    assert_eq!(m.items.len(), 3);
    assert!(m.items[2].disabled && m.selected == Some(1) && m.highlight == Some(0));
    let mid = |row: usize| m.rect.y + 1.0 + m.row_height * (row as f32 + 0.5);
    assert_eq!(m.row_at(m.rect.x + 5.0, mid(1)), Some(1));
    assert_eq!(m.row_at(m.rect.x + 5.0, mid(2)), Some(2));
    assert_eq!(m.row_at(m.rect.x - 5.0, mid(1)), None);
}

#[test]
fn long_menus_scroll_and_open_upward_when_below_is_cramped() {
    let options: String = (0..30).map(|i| format!("<option>Item {i}</option>")).collect();
    let html = |top: u32| {
        format!(r#"<body style="margin:0"><div style="height:{top}px"></div><select id="s">{options}</select></body>"#)
    };
    let menu_for = |top: u32, scroll: f32| {
        let doc = Document::parse(&html(top));
        let styles = compute_styles(&doc);
        let s = doc.find(|n| n.attr("id") == Some("s")).unwrap();
        let opts = LayoutOptions {
            preedit: None,
            menu: Some(MenuRequest { select: s, highlight: None, scroll }),
            scroll_y: 0.0,
            offsets: None,
            images: None,
        };
        let tree = layout_with(&doc, &styles, &mut TextEngine::new(), 400.0, 300.0, &opts);
        (tree.node_rect(s).unwrap(), tree.menu.unwrap())
    };
    // Near the top: opens below and is cut to the viewport, scrollable.
    let (s, m) = menu_for(10, 0.0);
    assert_eq!(m.rect.y, s.y + s.height);
    assert!(m.rect.y + m.rect.height <= 300.0 && m.max_scroll > 0.0, "{:?}", m.rect);
    let (_, m2) = menu_for(10, 1e6);
    assert_eq!(m2.scroll, m2.max_scroll, "scroll is clamped");
    assert_eq!(m2.row_at(m2.rect.x + 5.0, m2.rect.y + 5.0), Some(((m2.scroll + 4.0) / m2.row_height) as usize));
    // Near the bottom: flips above.
    let (s, m) = menu_for(250, 0.0);
    assert_eq!(m.rect.y + m.rect.height, s.y, "sits directly above");
    assert!(m.rect.y >= 0.0);
}

#[test]
fn nowrap_text_still_aligns_in_a_wider_box() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div id="d" style="width:200px; text-align:center; white-space:nowrap">hi</div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
    let d = doc.find(|n| n.attr("id") == Some("d")).unwrap();
    let tb = tree
        .boxes
        .iter()
        .find(|b| matches!(&b.content, BoxContent::Text(c) if c.owners.iter().any(|(_, n)| *n == d)));
    let BoxContent::Text(c) = &tb.unwrap().content else { unreachable!() };
    let line = c.layout.lines().next().unwrap();
    let (x, adv) = (line.metrics().offset, line.metrics().advance);
    assert!(x > 80.0 && x + adv < 130.0, "centered in 200px: offset {x}, advance {adv}");
}

#[test]
fn nowrap_keeps_text_on_one_line() {
    let html = |ws: &str| {
        format!(
            r#"<body style="margin:0"><div id="d" style="width:60px; {ws}">a fairly long run of words</div></body>"#
        )
    };
    let height = |ws: &str| {
        let doc = Document::parse(&html(ws));
        let styles = compute_styles(&doc);
        let mut text = TextEngine::new();
        let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
        tree.node_rect(doc.find(|n| n.attr("id") == Some("d")).unwrap()).unwrap().height
    };
    assert!(height("") > height("white-space: nowrap") * 2.0);
}

#[test]
fn overflow_boxes_scroll_clip_and_hit_test() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div id="box" style="width:100px; height:50px; overflow:auto; border:2px solid red">
             <div id="a" style="height:40px">a</div><div id="b" style="height:40px">b</div><div id="c" style="height:40px">c</div>
           </div><div id="hidden" style="width:50px; height:20px; overflow:hidden"><div style="width:200px; height:10px"></div></div>
           <div id="plain" style="height:10px"></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 400.0);
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    let sb = tree.scroll_box(id("box")).unwrap();
    assert_eq!((sb.clip.width, sb.clip.height), (100.0, 50.0));
    assert_eq!(sb.content_height, 120.0, "three 40px rows");
    assert_eq!(sb.max_offset(), (0.0, 70.0));
    assert!(sb.wheel_y && sb.wheel_x);
    let h = tree.scroll_box(id("hidden")).unwrap();
    assert!(!h.wheel_x && h.max_offset().0 == 150.0, "hidden clips and can be scrolled from script");
    assert!(tree.scroll_box(id("plain")).is_none());

    // Unscrolled: row c is clipped away; scrolled by 70 it is under the pointer.
    let none = ScrollOffsets::new();
    let mut off = ScrollOffsets::new();
    off.insert(id("box"), (0.0, 70.0));
    let (x, y) = (20.0, 2.0 + 45.0); // near the bottom of the viewport of the box
    assert_eq!(tree.hit_test_scrolled(x, y, &none), Some(id("b")));
    assert_eq!(tree.hit_test_scrolled(x, y, &off), Some(id("c")));
    // Outside the clip the scrolled-away content can't be hit; the page behind can.
    assert_ne!(tree.hit_test_scrolled(x, 2.0 + 60.0, &none), Some(id("c")));
    // Rects follow the scroll.
    let r0 = tree.node_rect(id("c")).unwrap();
    let r1 = tree.node_rect_scrolled(id("c"), &off).unwrap();
    assert_eq!(r0.y - r1.y, 70.0);
}

#[test]
fn hit_test_resolves_inline_elements() {
    let doc = Document::parse(
        r#"<body style="margin:0"><p id="p" style="margin:0">Hello <b id="b" style="padding:0 10px">world</b></p></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    let tb = text_boxes(&tree)[0];
    let (t, BoxContent::Text(c)) = (tb.rect, &tb.content) else { unreachable!() };
    let text_w = c.layout.full_width();
    let y = t.y + t.height / 2.0;
    assert_eq!(tree.hit_test(t.x + 2.0, y), Some(id("p")));
    assert_eq!(tree.hit_test(t.x + text_w - 14.0, y), Some(id("b")));
    // Below the text but inside the page: not the paragraph.
    assert_ne!(tree.hit_test(t.x + 2.0, t.y + t.height + 30.0), Some(id("p")));
    assert_eq!(tree.hit_test(-5.0, -5.0), None);
}

#[test]
fn br_breaks_the_line() {
    let doc = Document::parse(
        r#"<body style="margin:0"><p style="margin:0">one<br>two</p></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
    let texts = text_boxes(&tree);
    assert_eq!(texts.len(), 1);
    assert!(texts[0].rect.height > 16.0 * 1.2 * 1.5, "got {}", texts[0].rect.height);
}

#[test]
fn inline_block_sits_in_the_text_flow() {
    let doc = Document::parse(
        r#"<body style="margin:0"><p style="margin:0">before <span id="ib" style="display:inline-block; width:50px; height:30px"></span> after</p></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
    let ib = doc.find(|n| n.attr("id") == Some("ib")).unwrap();
    let r = tree.boxes.iter().find(|b| b.node == ib && matches!(b.content, BoxContent::Element)).unwrap().rect;
    assert_eq!((r.width, r.height), (50.0, 30.0));
    assert!(r.x > 20.0, "box should follow the text, got x={}", r.x);
    let t = text_boxes(&tree)[0];
    assert!(t.rect.height >= 30.0, "line grows to fit the box, got {}", t.rect.height);
}

#[test]
fn inline_block_shrinks_to_fit_content() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div><span id="ib" style="display:inline-block; padding:4px">Hi</span></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
    let ib = doc.find(|n| n.attr("id") == Some("ib")).unwrap();
    let r = tree.boxes.iter().find(|b| b.node == ib && matches!(b.content, BoxContent::Element)).unwrap().rect;
    assert!(r.width > 8.0 && r.width < 60.0, "got {}", r.width);
}

#[test]
fn blocks_split_inline_runs() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div>one <b>two</b><div>three</div>four</div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 800.0, 100.0);
    let texts = text_boxes(&tree);
    assert_eq!(texts.len(), 3);
    assert!(texts[0].rect.y < texts[1].rect.y && texts[1].rect.y < texts[2].rect.y);
}

#[test]
fn text_wraps_inside_fixed_width() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div style="width:60px">a fairly long run of words</div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
    let t = tree.boxes.iter().find(|b| matches!(b.content, BoxContent::Text(_))).unwrap();
    assert_eq!(t.rect.width, 60.0);
    assert!(t.rect.height > 16.0 * 1.2 * 2.0);
}

#[test]
fn calc_mixing_percent_and_px_resolves_against_the_parent() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div style="width:300px; height:100px">
           <div id="a" style="width:calc(100% - 20px); height:calc(50% + 10px); margin-left:calc(10% + 5px)"></div>
           <div id="b" style="width:calc(50% - 10px)"></div></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
    let rect = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap();
    let a = rect("a");
    assert_eq!((a.x, a.width, a.height), (35.0, 280.0, 60.0));
    assert_eq!(rect("b").width, 140.0);
}

#[test]
fn text_transform_and_letter_spacing() {
    let (plain, _) = run_width(r#"<body><p style="margin:0">hello world</p></body>"#);
    let (spaced, _) = run_width(r#"<body><p style="margin:0; letter-spacing: 2px">hello world</p></body>"#);
    assert!((spaced - plain - 22.0).abs() < 1.0, "11 characters x 2px: {plain} -> {spaced}");
    assert_eq!(transform_text("it's a dog", TextTransform::Capitalize), "It's A Dog");
    assert_eq!(transform_text("Straße", TextTransform::Uppercase), "STRASSE");
    assert_eq!(transform_text("ABC", TextTransform::Lowercase), "abc");
}

#[test]
fn position_relative_and_absolute() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div id="box" style="position:relative; width:200px; height:100px">
           <span id="a" style="position:absolute; top:10px; left:20px; width:50px; height:30px"></span>
           <div id="b" style="position:absolute; right:0; bottom:0; width:50px; height:30px"></div>
           <div id="c" style="height:20px"></div>
           <div id="d" style="position:relative; top:5px; left:7px; height:10px"></div></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 300.0);
    let rect = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap();
    let a = rect("a");
    assert_eq!((a.x, a.y, a.width, a.height), (20.0, 10.0, 50.0, 30.0));
    let b = rect("b");
    assert_eq!((b.x, b.y), (150.0, 70.0));
    assert_eq!(rect("c").y, 0.0); // out-of-flow boxes take no space
    let d = rect("d");
    assert_eq!((d.x, d.y), (7.0, 25.0)); // 20 (after c) + 5
}

#[test]
fn generated_content_flows_and_hits_its_element() {
    let doc = Document::parse(
        r#"<style>p::before { content: "ab"; } p::after { content: ""; display: block; height: 10px }
           #m::before { content: ""; display: inline-block; width: 30px; height: 12px }</style>
           <body style="margin:0"><p id="p" style="margin:0">text</p><div id="m" style="width:200px"></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 300.0, 200.0);
    let p = doc.find(|n| n.attr("id") == Some("p")).unwrap();
    let rect = tree.node_rect(p).unwrap();
    // One text line plus the 10px block ::after.
    assert!(rect.height > 10.0 + 16.0 && rect.height < 10.0 + 16.0 * 1.2 + 2.0, "height {}", rect.height);
    let runs = text_boxes(&tree);
    let BoxContent::Text(c) = &runs[0].content else { unreachable!() };
    assert!(c.owners.iter().all(|(_, n)| *n == p), "generated text belongs to its element");
    assert_eq!(tree.hit_test(2.0, 2.0), Some(p));
    // The generated block is a box with a generated id, but hits and rects map to the element.
    assert!(tree.boxes.iter().any(|b| styles.is_pseudo(b.node)));
    assert_eq!(tree.hit_test(2.0, rect.y + rect.height - 3.0), Some(p));
    // An inline-block ::before gives the (otherwise empty) div a line, like any inline content.
    let m = doc.find(|n| n.attr("id") == Some("m")).unwrap();
    assert_eq!(tree.node_rect(m).unwrap().height, 19.0);
}

#[test]
fn absolute_boxes_use_the_nearest_positioned_ancestor() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div style="height:30px"></div>
           <div id="cb" style="position:relative; margin-left:40px; width:200px; height:100px">
             <div style="padding:10px"><div id="inner" style="position:absolute; right:0; bottom:0; width:50px; height:20px"></div></div>
           </div>
           <div><div id="page" style="position:absolute; top:5px; left:6px; width:10px; height:10px"></div></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 300.0);
    let rect = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap();
    // Against #cb (x 40, y 30, 200x100), not its static parent.
    let r = rect("inner");
    assert_eq!((r.x, r.y), (40.0 + 200.0 - 50.0, 30.0 + 100.0 - 20.0));
    // No positioned ancestor: the page.
    let p = rect("page");
    assert_eq!((p.x, p.y), (6.0, 5.0));
}

#[test]
fn z_index_orders_painting_and_hit_testing() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div style="position:relative; width:100px; height:100px">
           <div id="hi" style="position:absolute; z-index:5; left:0; top:0; width:50px; height:50px"></div>
           <div id="lo" style="position:absolute; z-index:-1; left:0; top:0; width:50px; height:50px"></div>
           <div id="plain" style="position:absolute; left:0; top:0; width:50px; height:50px"></div>
           <div id="flow" style="height:20px"></div></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 300.0);
    let node = |id: &str| doc.find(|n| n.attr("id") == Some(id)).unwrap();
    let order: Vec<NodeId> = tree.boxes.iter().map(|b| b.node).collect();
    let at = |id: &str| order.iter().position(|n| *n == node(id)).unwrap();
    assert!(at("lo") < at("flow") && at("flow") < at("plain") && at("plain") < at("hi"));
    assert_eq!(tree.hit_test(5.0, 5.0), Some(node("hi")));
}

#[test]
fn grid_tracks_gaps_spans_and_areas() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div style="display:grid; width:320px; grid-template-columns: 100px 1fr 2fr; grid-auto-rows: 20px; gap: 10px 5px">
           <div id="a"></div><div id="b" style="grid-column: span 2"></div><div id="c"></div></div>
           <div style="display:grid; width:200px; height:100px; grid-template-columns: 50px 1fr; grid-template-rows: 30px 1fr;
                       grid-template-areas: 'h h' 's m'">
           <div id="m" style="grid-area: m"></div><div id="h" style="grid-area: h"></div></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 300.0);
    let r = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap();
    // 320 - 100 - 2 gaps of 5 = 210 split 1:2 -> 70 and 140.
    let (a, b, c) = (r("a"), r("b"), r("c"));
    assert_eq!((a.x, a.width, a.y, a.height), (0.0, 100.0, 0.0, 20.0));
    assert_eq!((b.x, b.width), (105.0, 70.0 + 5.0 + 140.0));
    assert_eq!((c.x, c.y), (0.0, 30.0));
    let (m, h) = (r("m"), r("h"));
    assert_eq!((h.x, h.y, h.width, h.height), (0.0, 50.0, 200.0, 30.0));
    assert_eq!((m.x, m.width, m.height), (50.0, 150.0, 70.0));
}

#[test]
fn text_beside_a_float_widens_below_it() {
    let doc = Document::parse(
        r#"<body style="margin:0;font-family:sans-serif;font-size:16px"><div id="c" style="width:300px"><div style="float:left;width:100px;height:40px;margin:0 10px 0 0"></div><span id="t">One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty.</span></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
    let run = text_boxes(&tree)[0];
    let BoxContent::Text(c) = &run.content else { panic!() };
    // Lines that start above the float's bottom (40px) are pushed right of it (110px); later ones start at 0.
    let lines: Vec<(f32, f32, f32)> = c.layout.lines().map(|l| (run.rect.y + l.metrics().block_min_coord, l.metrics().inline_min_coord, l.metrics().inline_max_coord)).collect();
    assert!(lines.len() >= 4, "{lines:?}");
    for (top, min, max) in &lines {
        if *top < 40.0 {
            assert_eq!((*min + run.rect.x, *max + run.rect.x), (110.0, 300.0), "beside the float: {lines:?}");
        } else {
            assert_eq!((*min + run.rect.x, *max + run.rect.x), (0.0, 300.0), "below it: {lines:?}");
        }
    }
    assert!(lines.iter().any(|l| l.0 >= 40.0), "{lines:?}");
    // The block is as tall as the re-broken lines, not the old narrow ones.
    let c_id = doc.find(|n| n.attr("id") == Some("c")).unwrap();
    let block = tree.node_rect(c_id).unwrap();
    assert!((block.height - c.layout.height()).abs() < 1.0, "{} vs {}", block.height, c.layout.height());
}

#[test]
fn text_follows_a_floats_shape_outside_outline() {
    let doc = Document::parse(
        r#"<body style="margin:0;font-family:sans-serif;font-size:16px"><div style="width:300px"><div style="float:left;width:100px;height:100px;shape-outside:circle(50%)"></div><span>One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty.</span></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 100.0);
    let run = text_boxes(&tree)[0];
    let BoxContent::Text(c) = &run.content else { panic!() };
    let lines: Vec<(f32, f32)> = c.layout.lines().map(|l| (run.rect.y + l.metrics().block_min_coord, l.metrics().inline_min_coord + run.rect.x)).collect();
    // The first line sits beside the circle's narrow top, further left than the line through its middle.
    let first = lines[0].1;
    let middle = lines.iter().filter(|l| l.0 + 8.0 > 40.0 && l.0 < 60.0).map(|l| l.1).fold(0.0, f32::max);
    assert!(first > 0.0 && first < middle - 10.0, "{lines:?}");
    assert!(middle <= 100.5, "never past the float's own box: {lines:?}");
    // Below the float's 100px the text runs from the left edge again.
    assert!(lines.iter().any(|l| l.0 >= 100.0 && l.1 == 0.0), "{lines:?}");
}

#[test]
fn floats_sit_side_by_side_and_clear_drops_below() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div style="width:300px"><div id="a" style="float:left; width:50px; height:20px"></div>
           <div id="b" style="float:left; width:60px; height:30px"></div><div id="r" style="float:right; width:40px; height:10px"></div>
           <div id="c" style="clear:both; height:5px"></div></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 300.0);
    let r = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap();
    assert_eq!((r("a").x, r("a").y), (0.0, 0.0));
    assert_eq!((r("b").x, r("b").y), (50.0, 0.0));
    assert_eq!((r("r").x, r("r").y), (260.0, 0.0));
    assert_eq!(r("c").y, 30.0); // below the tallest float
}

#[test]
fn images_with_both_dimensions_ignore_the_pictures_proportions() {
    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let html = format!(
        r#"<body style="margin:0"><img id="a" src="{PNG}" style="width:100px;height:60px"><p style="margin:0">x <img id="b" src="{PNG}" style="width:100px;height:60px"></p>
           <img id="c" src="{PNG}" width="80" height="20"></body>"#
    );
    let doc = Document::parse(&html);
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let loader = FileImages::new("/nonexistent");
    let options = LayoutOptions { images: Some(&loader), ..Default::default() };
    let tree = layout_with(&doc, &styles, &mut text, 600.0, 300.0, &options);
    let size = |id: &str| tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).map(|r| (r.width, r.height)).unwrap();
    assert_eq!(size("a"), (100.0, 60.0));
    assert_eq!(size("b"), (100.0, 60.0));
    assert_eq!(size("c"), (80.0, 20.0));
}

#[test]
fn images_size_from_the_picture_or_attributes() {
    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    // The test picture is 1x1; scale it up with attributes and CSS.
    let html = format!(
        r#"<body style="margin:0"><img id="a" src="{PNG}"><img id="b" src="{PNG}" width="40"><img id="c" src="{PNG}" style="height: 30px">
           <img id="d" src="missing.png" width="20" height="10"><div id="e" style="width:50px; height:50px; background: url({PNG})"></div></body>"#
    );
    let doc = Document::parse(&html);
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let loader = FileImages::new("/nonexistent");
    let options = LayoutOptions { images: Some(&loader), ..Default::default() };
    let tree = layout_with(&doc, &styles, &mut text, 600.0, 300.0, &options);
    let find = |id: &str| doc.find(|n| n.attr("id") == Some(id)).unwrap();
    let size = |id: &str| tree.node_rect(find(id)).map(|r| (r.width, r.height)).unwrap();
    assert_eq!(size("a"), (1.0, 1.0));
    assert_eq!(size("b"), (40.0, 40.0)); // aspect ratio keeps it square
    assert_eq!(size("c"), (30.0, 30.0));
    assert_eq!(size("d"), (20.0, 10.0)); // no picture: attributes still size it
    let boxes = |id: &str| tree.boxes.iter().find(|b| b.node == find(id) && matches!(b.content, BoxContent::Element)).unwrap();
    assert!(boxes("a").image.is_some() && boxes("d").image.is_none());
    assert!(boxes("e").bg_image.is_some());
}

#[test]
fn transforms_move_hits_and_bounding_boxes() {
    let doc = Document::parse(
        r#"<body style="margin:0"><div id="a" style="width:100px; height:20px; transform: rotate(90deg)"></div>
           <div id="b" style="width:50px; height:50px; margin-top:200px; transform: translate(100px, 0) scale(2); transform-origin: 0 0"></div></body>"#,
    );
    let styles = compute_styles(&doc);
    let mut text = TextEngine::new();
    let tree = layout(&doc, &styles, &mut text, 400.0, 400.0);
    let a = doc.find(|n| n.attr("id") == Some("a")).unwrap();
    let b = doc.find(|n| n.attr("id") == Some("b")).unwrap();
    // Rotated about its center (50, 10): now 20 wide, 100 tall.
    let r = tree.node_rect_scrolled(a, &ScrollOffsets::new()).unwrap();
    assert!((r.x - 40.0).abs() < 0.01 && (r.y + 40.0).abs() < 0.01 && (r.width - 20.0).abs() < 0.01 && (r.height - 100.0).abs() < 0.01, "{r:?}");
    // A point inside the rotated box but outside the original one hits it.
    assert_eq!(tree.hit_test_scrolled(50.0, -30.0, &ScrollOffsets::new()), Some(a));
    assert_ne!(tree.hit_test_scrolled(90.0, 10.0, &ScrollOffsets::new()), Some(a));
    // b: layout rect (0,220,50,50) -> translated and doubled: x 100..200, y 220..320.
    let rb = tree.node_rect_scrolled(b, &ScrollOffsets::new()).unwrap();
    assert_eq!((rb.x, rb.y, rb.width, rb.height), (100.0, 220.0, 100.0, 100.0));
    assert_eq!(tree.hit_test_scrolled(190.0, 310.0, &ScrollOffsets::new()), Some(b));
    assert_ne!(tree.hit_test_scrolled(40.0, 240.0, &ScrollOffsets::new()), Some(b));
}

fn rects(html: &str) -> (Document, LayoutTree) {
    let doc = Document::parse(html);
    let styles = compute_styles(&doc);
    let tree = layout(&doc, &styles, &mut TextEngine::new(), 800.0, 400.0);
    (doc, tree)
}

fn rect_of(doc: &Document, tree: &LayoutTree, id: &str) -> Rect {
    tree.node_rect(doc.find(|n| n.attr("id") == Some(id)).unwrap()).unwrap()
}

#[test]
fn an_open_select_list_takes_the_selects_own_styling() {
    let doc = Document::parse(
        r#"<body style="margin:0"><select id="s" style="background:#102030; border:2px solid #3040a0; border-radius:12px; accent-color:#ff8800; padding:0 20px">
             <option>One</option><option style="background:#405060">Two</option></select>
           <select id="plain"><option>One</option></select></body>"#,
    );
    let styles = compute_styles(&doc);
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    let mut text = TextEngine::new();
    let menu_of = |select: &str, text: &mut TextEngine| {
        let opts = LayoutOptions { preedit: None, menu: Some(MenuRequest { select: id(select), highlight: None, scroll: 0.0 }), scroll_y: 0.0, offsets: None, images: None };
        layout_with(&doc, &styles, text, 800.0, 400.0, &opts).menu.unwrap()
    };
    let m = menu_of("s", &mut text);
    assert_eq!(m.background.map(|c| (c.r, c.g, c.b)), Some((0x10, 0x20, 0x30)));
    assert_eq!(m.border.map(|c| (c.r, c.g, c.b)), Some((0x30, 0x40, 0xa0)));
    assert_eq!((m.radius, m.accent.map(|c| (c.r, c.g, c.b))), (12.0, Some((0xff, 0x88, 0x00))));
    assert_eq!(m.items[0].background, None);
    assert_eq!(m.items[1].background.map(|c| (c.r, c.g, c.b)), Some((0x40, 0x50, 0x60)), "an option's own background colours its row");
    // A select with no styling of its own gets its list from the platform theme's colours, so no row has one of its own.
    let plain = menu_of("plain", &mut text);
    assert!(plain.items.iter().all(|i| i.background.is_none()));
    // The closed select is wide enough for its text plus the chevron, whatever padding the page chose.
    let tree = layout(&doc, &styles, &mut text, 800.0, 400.0);
    let r = tree.node_rect(id("s")).unwrap();
    assert!(r.width >= m.items[0].text.full_width().max(m.items[1].text.full_width()) + 40.0 + 4.0 + 4.0, "{r:?}");
}

#[test]
fn a_flex_button_lays_out_its_own_children_and_block_buttons_shrink_to_fit() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><button id="f" style="display:flex; gap:10px; padding:0; border:0"><span id="a" style="width:30px; height:10px"></span><span id="b" style="width:40px; height:10px"></span></button>
           <button id="k" style="display:block; padding:0; border:0"><span style="display:block; width:50px; height:10px"></span></button></body>"#,
    );
    let (a, b, f, k) = (rect_of(&doc, &t, "a"), rect_of(&doc, &t, "b"), rect_of(&doc, &t, "f"), rect_of(&doc, &t, "k"));
    assert_eq!(a.y, b.y, "the children sit in a row");
    assert_eq!(b.x - a.x, 40.0, "30px wide plus the 10px gap");
    assert_eq!(f.width, 80.0, "the button is as wide as its content, not the page");
    assert_eq!(k.width, 50.0, "a block button shrinks to fit too");
}

#[test]
fn inline_flex_and_inline_grid_are_atomic_inline_boxes_that_shrink_to_fit() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><div id="f" style="display:inline-flex; gap:10px"><span style="width:30px; height:10px"></span><span style="width:40px; height:10px"></span></div>
           <div id="g" style="display:inline-grid; grid-template-columns: 20px 20px"><span style="height:10px"></span><span style="height:10px"></span></div>
           <p id="after" style="margin:0">next</p></body>"#,
    );
    let (f, g, after) = (rect_of(&doc, &t, "f"), rect_of(&doc, &t, "g"), rect_of(&doc, &t, "after"));
    assert_eq!(f.width, 80.0, "as wide as its content, not the page: {f:?}");
    assert_eq!(g.width, 40.0, "{g:?}");
    assert_eq!(f.y, g.y, "both sit in the same line");
    assert!(g.x >= f.x + f.width, "side by side: {f:?} {g:?}");
    assert!(after.y >= f.y + f.height, "the next block starts below them");
}

#[test]
fn a_full_width_inline_block_fills_its_container() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><div style="width:300px; padding:10px"><label id="l">Name</label><input id="i" style="width:100%; box-sizing:border-box"><select id="s" style="width:100%; box-sizing:border-box"></select><input id="h" style="width:50%; box-sizing:border-box"></div></body>"#,
    );
    let (i, s, h, l) = (rect_of(&doc, &t, "i"), rect_of(&doc, &t, "s"), rect_of(&doc, &t, "h"), rect_of(&doc, &t, "l"));
    assert_eq!(i.width, 300.0, "{i:?}");
    assert_eq!(s.width, 300.0, "{s:?}");
    assert!(i.y >= l.y + l.height - 1.0, "the field is on its own line below the label: {i:?} {l:?}");
    assert!(h.width < 300.0, "a half-width one still sizes to its content for now: {h:?}");
}

#[test]
fn three_d_transforms_move_hit_testing_and_bounds() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><div style="perspective:200px; width:200px; height:100px">
             <div id="c" style="width:100px; height:100px; transform: rotateY(70deg)"></div></div></body>"#,
    );
    let id = doc.find(|n| n.attr("id") == Some("c")).unwrap();
    let none = ScrollOffsets::default();
    // The 100px-wide square turned 70 degrees is narrow: its centre is still hit, the old
    // left edge is not.
    assert_eq!(t.hit_test_scrolled(50.0, 50.0, &none), Some(id));
    assert_ne!(t.hit_test_scrolled(2.0, 50.0, &none), Some(id));
    let r = t.node_rect_scrolled(id, &none).unwrap();
    assert!(r.width < 70.0 && r.width > 10.0, "{r:?}");
}

#[test]
fn absolute_boxes_without_insets_stay_at_their_static_position() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><div style="position:relative; padding:5px">
             <div id="wrap" style="padding:10px; margin-left:20px"><p id="p" style="margin:0; height:30px">one</p>
               <div id="a" style="position:absolute; width:10px; height:10px; margin-left:4px"></div>
               <div id="b" style="position:absolute; width:10px; height:10px; left:7px; top:9px"></div></div></div></body>"#,
    );
    let (p, a, b) = (rect_of(&doc, &t, "p"), rect_of(&doc, &t, "a"), rect_of(&doc, &t, "b"));
    assert_eq!(a.y, p.y + p.height, "below the paragraph that precedes it: {a:?} vs {p:?}");
    assert_eq!(a.x, p.x + 4.0, "at its parent's content edge plus its margin");
    assert_eq!((b.x, b.y), (7.0, 9.0), "explicit insets still win, from the padding edge");
}

#[test]
fn stacking_contexts_order_descendants_by_z_index() {
    let (doc, t) = rects(
        r#"<body style="margin:0">
           <div id="a"><div id="pa" style="position:absolute; z-index:10; width:10px; height:10px"></div></div>
           <div id="b" style="position:relative; z-index:1; height:10px"></div>
           <div id="c" style="position:relative; z-index:1"><div id="pc" style="position:absolute; z-index:99; width:10px; height:10px"></div></div>
           <div id="d" style="position:relative; z-index:2; height:10px"></div>
           <div style="display:flex"><div id="f1" style="z-index:2; width:20px; height:10px"></div><div id="f2" style="z-index:1; margin-left:-10px; width:20px; height:10px"></div></div>
           <div style="overflow:hidden; position:relative"><div id="pe" style="position:absolute; z-index:10; width:10px; height:10px"></div></div>
           <div id="g" style="position:relative; z-index:1; height:10px"></div></body>"#,
    );
    let at = |id: &str| t.boxes.iter().position(|b| matches!(b.content, BoxContent::Element) && doc.node(b.node).attr("id") == Some(id)).unwrap_or_else(|| panic!("no box for {id}"));
    // A positioned box inside a plain wrapper competes in the page's context: above `b`.
    assert!(at("pa") > at("b"), "pa should paint after b");
    // Inside a stacking context a high z-index stays inside it: `d` is above `pc`.
    assert!(at("pc") < at("d"), "pc is confined to c");
    // z-index applies to flex items even when static.
    assert!(at("f1") > at("f2"), "f1 has the higher z-index");
    // A clipping ancestor that is the child's containing block keeps it in place (`g` paints later).
    assert!(at("pe") < at("g"));
}

#[test]
fn fixed_tables_take_column_widths_from_cols_and_the_first_row() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><table id="t" style="table-layout:fixed; width:300px; border-spacing:0; border-collapse:separate">
             <col width="60"><col><col width="50%">
             <tr><td id="a" style="padding:0">a very long cell that would widen an auto column</td><td id="b" style="padding:0">b</td><td id="c" style="padding:0">c</td></tr>
           </table>
           <table style="table-layout:fixed; width:200px; border-spacing:0"><tr><td id="d" style="padding:0;width:20%">d</td><td id="e" style="padding:0">e</td><td id="f" style="padding:0">f</td></tr></table></body>"#,
    );
    let (a, b, c) = (rect_of(&doc, &t, "a"), rect_of(&doc, &t, "b"), rect_of(&doc, &t, "c"));
    assert_eq!((a.width, c.width), (60.0, 150.0), "from <col>: {a:?} {c:?}");
    assert_eq!(b.width, 90.0, "the unsized column takes what's left");
    let (d, e, f) = (rect_of(&doc, &t, "d"), rect_of(&doc, &t, "e"), rect_of(&doc, &t, "f"));
    assert_eq!((d.width, e.width, f.width), (40.0, 80.0, 80.0), "first-row width, then equal shares");
}

#[test]
fn tables_size_columns_from_content_and_honour_spans_and_spacing() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><table id="t" style="border-spacing:4px">
             <tr><td id="a" style="width:100px;padding:0;height:20px">a</td><td id="b" style="width:50px;padding:0">b</td></tr>
             <tr><td id="c" colspan="2" style="padding:0;height:30px">c</td></tr>
             <tr><td id="d" rowspan="1" style="padding:0">d</td><td id="e" style="padding:0">e</td></tr>
           </table></body>"#,
    );
    let (a, b, c, tb) = (rect_of(&doc, &t, "a"), rect_of(&doc, &t, "b"), rect_of(&doc, &t, "c"), rect_of(&doc, &t, "t"));
    assert_eq!((a.x, a.width, b.width), (4.0, 100.0, 50.0), "columns follow their widest cell");
    assert_eq!(b.x, a.x + a.width + 4.0, "border-spacing between columns");
    assert_eq!(c.width, 154.0, "colspan covers both columns and the gap");
    assert_eq!(c.y, a.y + 20.0 + 4.0, "rows stack with spacing");
    assert_eq!(tb.width, 4.0 + 100.0 + 4.0 + 50.0 + 4.0, "the table shrinks to its columns");
}

#[test]
fn a_legend_sits_on_the_fieldset_border_and_leaves_a_gap_in_it() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><fieldset id="f" style="margin:0"><legend id="l" style="margin-left:10px">Title</legend><p>x</p></fieldset></body>"#,
    );
    let (f, l) = (rect_of(&doc, &t, "f"), rect_of(&doc, &t, "l"));
    assert!(l.y < f.y + 8.0 && l.y + l.height > f.y + 4.0, "the legend straddles the top border: {l:?} in {f:?}");
    let b = t.boxes.iter().find(|b| doc.node(b.node).attr("id") == Some("f")).unwrap();
    let (x0, x1) = b.border_gap.expect("a gap for the legend");
    assert!(x0 <= l.x && x1 >= l.x + l.width);
    // The legend shrinks to its text, so the notch is only as wide as the label.
    assert!(l.width < 200.0 && x1 - x0 < 220.0, "legend {} wide, gap {}", l.width, x1 - x0);
}

#[test]
fn width_fit_content_shrinks_a_block_to_its_content_but_not_past_its_container() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><div id="a" style="width:fit-content;padding:0 10px">short</div>
           <div id="b" style="width:fit-content">a long line of text that wraps because it is wider than the container allows it to be, again and again</div>
           <div id="c" style="width:max-content">x</div></body>"#,
    );
    let (a, b, c) = (rect_of(&doc, &t, "a"), rect_of(&doc, &t, "b"), rect_of(&doc, &t, "c"));
    assert!(a.width > 20.0 && a.width < 120.0, "shrunk: {}", a.width);
    assert!(b.width <= 800.0 && b.width > 300.0, "capped at the container: {}", b.width);
    assert!(c.width < 40.0, "{}", c.width);
}

#[test]
fn pointer_events_and_visibility_let_hits_through_and_order_sorts_items() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><div id="under" style="width:100px;height:100px">
             <div id="none" style="position:absolute;left:0;top:0;width:100px;height:100px;pointer-events:none">
               <div id="kid" style="position:absolute;left:0;top:0;width:20px;height:20px;pointer-events:auto"></div></div>
           </div>
           <div id="hid" style="width:100px;height:30px;visibility:hidden"><span id="vis" style="visibility:visible">x</span></div>
           <div style="display:flex;width:300px"><i id="a" style="order:2;width:10px">a</i><i id="b" style="order:1;width:10px">b</i><i id="c" style="width:10px">c</i></div>
           <div id="ratio" style="width:80px;aspect-ratio:2"></div></body>"#,
    );
    let node = |id: &str| doc.find(|n| n.attr("id") == Some(id)).unwrap();
    let none = Default::default();
    assert_eq!(t.hit_test_scrolled(50.0, 50.0, &none), Some(node("under")), "pointer-events: none is transparent to hits");
    assert_eq!(t.hit_test_scrolled(5.0, 5.0, &none), Some(node("kid")), "a child can turn them back on");
    let hidden = rect_of(&doc, &t, "hid");
    assert!(hidden.height >= 30.0, "visibility: hidden keeps the space");
    assert_ne!(t.hit_test_scrolled(50.0, 105.0, &none), Some(node("hid")), "and is not hit");
    let (a, b, c) = (rect_of(&doc, &t, "a"), rect_of(&doc, &t, "b"), rect_of(&doc, &t, "c"));
    assert!(c.x < b.x && b.x < a.x, "order puts c (0), b (1), a (2): {c:?} {b:?} {a:?}");
    assert_eq!(rect_of(&doc, &t, "ratio").height, 40.0);
}

#[test]
fn text_indent_line_clamp_and_word_break_change_the_lines() {
    let height = |style: &str, body: &str| {
        let (doc, t) = rects(&format!(r#"<body style="margin:0"><p id="p" style="margin:0;width:120px;{style}">{body}</p></body>"#));
        rect_of(&doc, &t, "p").height
    };
    let words = "one two three four five six seven eight nine ten eleven twelve";
    let plain = height("", words);
    let two = height("display:-webkit-box;-webkit-line-clamp:2", words);
    assert!(plain > two * 1.5, "clamped to two lines: {two} vs {plain}");
    // A long word only wraps when allowed to break.
    let long = "Supercalifragilisticexpialidocious";
    assert!(height("word-break:break-all", long) > height("", long));
    assert!(height("overflow-wrap:anywhere", long) > height("", long));
    // The indent pushes the first line in: it is that much longer.
    let first_line = |style: &str| {
        let (_, t) = rects(&format!(r#"<body style="margin:0"><p style="margin:0;width:400px;{style}">aaaa bbbb</p></body>"#));
        let b = t.boxes.iter().find(|b| matches!(b.content, BoxContent::Text(_))).unwrap();
        let BoxContent::Text(c) = &b.content else { unreachable!() };
        let advance = c.layout.lines().next().unwrap().metrics().advance;
        advance
    };
    let shift = first_line("text-indent:60px") - first_line("");
    assert!((shift - 60.0).abs() < 1.5, "indent of 60px, got {shift}");
}

#[test]
fn balanced_text_is_narrower_but_keeps_its_line_count() {
    let lines = |style: &str| {
        let (_, t) = rects(&format!(r#"<body style="margin:0"><p id="p" style="margin:0;width:300px;font-size:20px;{style}">Aa bb cc dd ee ff gg hh ii jj kk ll mm nn oo pp</p></body>"#));
        let b = t.boxes.iter().find(|b| matches!(b.content, BoxContent::Text(_))).unwrap();
        let BoxContent::Text(c) = &b.content else { unreachable!() };
        let widths: Vec<f32> = c.layout.lines().map(|l| l.metrics().advance).collect();
        widths
    };
    let normal = lines("");
    let balanced = lines("text-wrap:balance");
    assert_eq!(normal.len(), balanced.len());
    let spread = |w: &[f32]| w.iter().cloned().fold(0.0_f32, f32::max) - w.iter().cloned().fold(f32::MAX, f32::min);
    assert!(spread(&balanced) < spread(&normal), "{balanced:?} vs {normal:?}");
}

#[test]
fn inline_horizontal_margins_take_space_without_stretching_the_line() {
    let (doc, t) = rects(r#"<body style="margin:0;font-size:16px"><p id="pa" style="margin:0"><span id="a" style="margin-right:40px">aa</span><span id="b">bb</span></p><p id="q" style="margin:0">aabb</p></body>"#);
    let (a, q, pa) = (rect_of(&doc, &t, "a"), rect_of(&doc, &t, "q"), rect_of(&doc, &t, "pa"));
    assert!((pa.height - q.height).abs() < 0.5, "the spacer does not enlarge the line: {pa:?} vs {q:?}");
    let b = rect_of(&doc, &t, "b");
    assert!(b.x >= a.x + 40.0 + 8.0, "b starts after a's text and its 40px margin: {a:?} {b:?}");
}

#[test]
fn a_turned_away_backface_hidden_box_is_not_hit() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><div style="width:100px;height:100px;perspective:400px">
             <div id="in" style="position:relative;width:100px;height:100px;transform:rotateY(180deg)">
               <div id="front" style="position:absolute;left:0;top:0;width:100px;height:100px;backface-visibility:hidden"><span id="label">x</span></div>
               <div id="back" style="position:absolute;left:0;top:0;width:100px;height:100px;backface-visibility:hidden;transform:rotateY(180deg)"></div>
             </div></div></body>"#,
    );
    let node = |id: &str| doc.find(|n| n.attr("id") == Some(id)).unwrap();
    let hit = t.hit_test_scrolled(50.0, 50.0, &Default::default());
    assert_eq!(hit, Some(node("back")), "the front is turned away, the back faces us");
    assert_ne!(hit, Some(node("front")));
    assert!(t.boxes.iter().any(|b| b.node == node("front") && b.backface_hidden));
}

#[test]
fn captions_can_sit_below_and_row_groups_paint_behind_their_rows() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><table id="t" style="border-spacing:0; caption-side:bottom">
             <caption id="cap" style="padding:0">Cap</caption>
             <thead id="h" style="background:#ccc"><tr><td id="a" style="padding:0;height:10px">a</td></tr></thead>
             <tbody><tr><td id="b" style="padding:0;height:10px">b</td></tr></tbody>
             <tfoot id="f" style="background:#fcc"><tr><td id="c" style="padding:0;height:10px">c</td></tr></tfoot>
           </table></body>"#,
    );
    let (a, b, c, cap) = (rect_of(&doc, &t, "a"), rect_of(&doc, &t, "b"), rect_of(&doc, &t, "c"), rect_of(&doc, &t, "cap"));
    assert!(cap.y >= c.y + c.height, "the caption is below the last row: {cap:?} vs {c:?}");
    assert!(a.y < b.y && b.y < c.y);
    let (h, f) = (rect_of(&doc, &t, "h"), rect_of(&doc, &t, "f"));
    assert_eq!((h.y, h.height), (a.y, a.height), "the header group's box covers its rows");
    assert_eq!((f.y, f.height), (c.y, c.height), "so does the footer group's");
}

#[test]
fn automatic_columns_share_extra_width_in_proportion_to_their_content() {
    let (doc, t) = rects(
        r#"<body style="margin:0;font:16px sans-serif"><table style="width:600px; border-spacing:0">
             <tr><td id="a" style="padding:0">aaaa</td><td id="b" style="padding:0">aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</td></tr>
           </table></body>"#,
    );
    let (a, b) = (rect_of(&doc, &t, "a"), rect_of(&doc, &t, "b"));
    assert_eq!(a.width + b.width, 600.0);
    assert!(b.width > a.width * 5.0, "the wider content gets proportionally more: {a:?} {b:?}");
}

#[test]
fn collapsed_borders_resolve_conflicts_by_width_style_and_position() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><table style="border-collapse:collapse">
             <tr><td id="a" style="border:3px solid;padding:0;width:30px;height:10px">a</td><td id="b" style="border:1px solid;padding:0;width:30px">b</td><td id="c" style="border:1px dashed;padding:0;width:30px">c</td></tr>
             <tr><td id="d" style="border:1px solid;padding:0;height:10px">d</td><td id="e" style="border:1px solid;padding:0">e</td><td id="f" style="border:1px solid;padding:0">f</td></tr>
           </table></body>"#,
    );
    let hidden = |id: &str| {
        let node = doc.find(|n| n.attr("id") == Some(id)).expect("cell");
        t.boxes.iter().find(|b| b.node == node && matches!(b.content, BoxContent::Element)).expect("box").border_hidden
    };
    // a (3px) beats b (1px) on their shared edge; b's right (solid) beats c's left (dashed).
    assert!(!hidden("a").right && hidden("b").left);
    assert!(!hidden("b").right && hidden("c").left);
    // Between equal solid 1px borders the upper cell wins; a (3px) beats d below it.
    assert!(hidden("d").top && !hidden("a").bottom);
    assert!(hidden("e").top && !hidden("b").bottom);
    // Equal on both sides: the left cell keeps the line.
    assert!(!hidden("d").right && hidden("e").left);
    // Outer edges have no neighbour to lose to.
    assert_eq!((hidden("a").top, hidden("a").left, hidden("c").right), (false, false, false));
}

#[test]
fn collapsed_borders_overlap_and_rowspans_fill_their_rows() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><table style="border-collapse:collapse">
             <tr><td id="a" rowspan="2" style="border:1px solid;padding:0;width:40px">a</td><td id="b" style="border:1px solid;padding:0;width:40px;height:10px">b</td></tr>
             <tr><td id="c" style="border:1px solid;padding:0;height:10px">c</td></tr>
           </table></body>"#,
    );
    let (a, b, c) = (rect_of(&doc, &t, "a"), rect_of(&doc, &t, "b"), rect_of(&doc, &t, "c"));
    assert_eq!(b.x, a.x + a.width - 1.0, "neighbouring borders share a pixel");
    assert_eq!(c.y, b.y + b.height - 1.0);
    assert_eq!(a.height, b.height + c.height - 1.0, "a row-spanning cell fills both rows");
}

#[test]
fn button_labels_are_centred_in_a_taller_box() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><button id="b" style="width:100px;height:60px;padding:0;border:0;margin:0"><span id="s">Hi</span></button></body>"#,
    );
    let (b, s) = (rect_of(&doc, &t, "b"), rect_of(&doc, &t, "s"));
    let text = t.boxes.iter().rfind(|x| matches!(x.content, BoxContent::Text(_))).unwrap().rect;
    let (above, below) = (text.y - b.y, b.y + b.height - (text.y + text.height));
    assert!((above - below).abs() <= 1.0 && above > 10.0, "label centred vertically: {above} vs {below}");
    assert!(text.x >= b.x && s.width > 0.0);
}

#[test]
fn white_space_pre_keeps_spaces_and_newlines_and_pre_line_keeps_only_newlines() {
    let lines = |ws: &str| {
        let (doc, t) = rects(&format!(
            "<body style=\"margin:0\"><div id=\"d\" style=\"white-space:{ws}; width:600px; font-family:monospace; font-size:10px\">a   b\n  c\td</div></body>"
        ));
        let b = t.boxes.iter().rfind(|b| matches!(b.content, BoxContent::Text(_))).unwrap();
        let BoxContent::Text(c) = &b.content else { unreachable!() };
        let _ = &doc;
        (c.text.clone(), b.rect.height)
    };
    let (normal, h1) = lines("normal");
    assert_eq!(normal, "a b c d");
    let (pre, h2) = lines("pre");
    assert_eq!(pre, "a   b\n  c     d", "spaces kept, tab to the next multiple of 8");
    assert!(h2 > h1 * 1.5, "the newline breaks the line");
    let (line, _) = lines("pre-line");
    assert_eq!(line, "a b\nc d", "spaces collapse, the newline stays and eats the spaces after it");
    assert_eq!(lines("pre-wrap").0, pre);
}

#[test]
fn marker_rules_and_inside_position() {
    let (doc, t) = rects(
        r#"<style>
             #c::marker { color: #ff0000 }
             #t::marker { content: ">> " }
             #h::marker { content: "" }
             .in { list-style-position: inside }
           </style>
           <body style="margin:0"><ul style="margin:0;padding:0 0 0 40px"><li id="c">c</li><li id="t">t</li><li id="h">h</li><li id="p">p</li><li id="o" class="in">o</li></ul></body>"#,
    );
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    assert_eq!(t.marker(id("c")).unwrap().color, Some(nile_style::Color::rgb(255, 0, 0)));
    assert_eq!(t.marker(id("p")).unwrap().color, None, "no rule: the item's own colour is used live");
    assert!(matches!(t.marker(id("t")).unwrap().kind, MarkerKind::Text(_)), "content replaces the bullet");
    assert!(t.marker(id("h")).is_none(), "content: \"\" hides it");
    // Outside, the marker hangs left of the item; inside, it starts at the item's content edge
    // and the text is pushed past it.
    let (plain, inside) = (rect_of(&doc, &t, "p"), rect_of(&doc, &t, "o"));
    assert!(t.marker(id("p")).unwrap().rect.x < plain.x);
    let m = t.marker(id("o")).unwrap().rect;
    assert!(m.x >= inside.x && m.x + m.width <= inside.x + 10.0, "{m:?} in {inside:?}");
}

#[test]
fn list_items_get_bullets_and_counter_markers() {
    let (doc, t) = rects(
        r#"<body style="margin:0"><ul><li id="u">bullet</li></ul>
           <ol start="3"><li id="a">a</li><li id="b" value="9">b</li><li id="c">c</li></ol>
           <ol reversed type="x"><li id="r1">r</li><li id="r2">r</li></ol>
           <ol style="list-style-type: upper-roman"><li id="v">v</li></ol>
           <ul style="list-style: none"><li id="n">none</li></ul></body>"#,
    );
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    let label = |s: &str| match &t.marker(id(s)).map(|m| &m.kind) {
        Some(MarkerKind::Text(l)) => {
            // The shaped text isn't exposed; compare widths of known labels instead.
            Some(l.full_width())
        }
        Some(_) => Some(-1.0),
        None => None,
    };
    let item = rect_of(&doc, &t, "u");
    let m = t.marker(id("u")).unwrap();
    assert!(matches!(m.kind, MarkerKind::Disc));
    assert!(m.rect.x + m.rect.width < item.x, "bullet sits left of the item: {:?} vs {item:?}", m.rect);
    assert_eq!(item.x, 40.0, "ul pads its items by 40px");
    assert!(label("a").unwrap() > 0.0 && label("b").unwrap() > label("a").unwrap() - 0.01, "9 is not narrower than 3");
    assert!(t.marker(id("n")).is_none(), "list-style: none");
    // Numbers: start=3, then value=9 resets, then 10; reversed counts down from 2.
    let numbers = list_numbers(&doc, &compute_styles(&doc), doc.node(id("a")).parent.unwrap());
    assert_eq!([numbers[&id("a")], numbers[&id("b")], numbers[&id("c")]], [3, 9, 10]);
    let rev = list_numbers(&doc, &compute_styles(&doc), doc.node(id("r1")).parent.unwrap());
    assert_eq!([rev[&id("r1")], rev[&id("r2")]], [2, 1]);
    assert_eq!(marker_text(nile_style::ListStyleType::UpperRoman, 14).as_deref(), Some("XIV."));
    assert_eq!(marker_text(nile_style::ListStyleType::LowerAlpha, 28).as_deref(), Some("ab."));
    assert!(label("v").unwrap() > 0.0);
}

#[test]
fn text_overflow_ellipsis_applies_to_clipped_single_lines() {
    let flag = |style: &str| {
        let (_, t) = rects(&format!("<body style=\"margin:0\"><div style=\"width:60px;{style}\">some long text here</div></body>"));
        let b = t.boxes.iter().rfind(|b| matches!(b.content, BoxContent::Text(_))).unwrap();
        let BoxContent::Text(c) = &b.content else { unreachable!() };
        c.ellipsis
    };
    assert!(flag("overflow:hidden;white-space:nowrap;text-overflow:ellipsis"));
    assert!(!flag("white-space:nowrap;text-overflow:ellipsis"), "needs overflow other than visible");
    assert!(!flag("overflow:hidden;text-overflow:ellipsis"), "needs a single line");
    assert!(!flag("overflow:hidden;white-space:nowrap"));
}

#[test]
fn dom_points_map_to_the_run_and_back_through_collapsed_whitespace() {
    let (doc, t) = rects(
        r#"<body style="margin:0; user-select:text"><p id="p" style="margin:0">Hello  <b id="b">big</b> <i>x</i><br>tail</p><p id="e" style="margin:0"></p></body>"#,
    );
    let styles = compute_styles(&doc);
    let id = |s: &str| doc.find(|n| n.attr("id") == Some(s)).unwrap();
    let p = id("p");
    let hello = doc.node(p).children[0];
    let big = doc.node(id("b")).children[0];
    let run_text = |pt: TextPoint| match &t.boxes[pt.run].content {
        BoxContent::Text(c) => c.text.clone(),
        _ => unreachable!(),
    };
    // "Hello  " collapses to "Hello " in the run: offset 6 (the second space) sits at byte 6.
    let at = |node, offset| t.text_point_of(&doc, DomPoint { node, offset }).unwrap();
    assert_eq!(run_text(at(hello, 0)), "Hello big x\ntail");
    assert_eq!((at(hello, 0).byte, at(hello, 5).byte, at(hello, 6).byte, at(hello, 7).byte), (0, 5, 6, 6));
    assert_eq!(at(big, 0).byte, 6);
    assert_eq!(at(big, 3).byte, 9);
    // Element positions: before the <b>, and after the whole paragraph's last child.
    assert_eq!(at(p, 1).byte, 6, "child 1 is the <b>");
    assert_eq!((at(p, 3).byte, at(p, 4).byte), (10, 11), "children 3 and 4 are <i> and <br>");
    // And back.
    let back = |byte| t.dom_point_of(&doc, TextPoint { run: at(hello, 0).run, byte }).unwrap();
    assert_eq!(back(3), DomPoint { node: hello, offset: 3 });
    assert_eq!(back(7), DomPoint { node: big, offset: 1 });
    // The caret has a line's height and moves right along the line.
    let (c0, c1) = (t.dom_caret_rect(&doc, &styles, DomPoint { node: hello, offset: 0 }).unwrap().0, t.dom_caret_rect(&doc, &styles, DomPoint { node: hello, offset: 5 }).unwrap().0);
    assert!(c1.x > c0.x + 20.0 && c0.height > 10.0 && c0.y == c1.y);
    // An empty paragraph still has a place for the caret.
    let e = t.dom_caret_rect(&doc, &styles, DomPoint { node: id("e"), offset: 0 }).unwrap().0;
    assert!(e.height > 10.0);
}
