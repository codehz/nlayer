use std::sync::Arc;

use taffy::{Dimension, Display, Position};

use super::*;
use crate::{
    geometry::{px, size, size_pt},
    ops::{EventKind, ImageScales, ImageSource, Interaction, KeyboardType, Prop, ResizeMode},
    style::{Color, StylePatch, StyleValue, TextStyle, ViewStyle},
    text::{ApproxTextEngine, AttributedText, TextEngine, TextLayout},
};

/// 8pt per character, 16pt per line, wrapping at `max_width`.
struct FakeEngine;

struct FakeLayout {
    chars: usize,
}

impl TextEngine for FakeEngine {
    type Prepared = usize;

    fn prepare(&self, text: &AttributedText) -> usize {
        text.text.chars().count()
    }

    fn measure(&self, chars: &usize, max_width: Option<Points>) -> Size<Points> {
        let w = *chars as f32 * 8.0;
        match max_width.map(|m| m.0) {
            Some(max) if w > max && max > 0.0 => size_pt(max, (w / max).ceil() * 16.0),
            _ => size_pt(w, 16.0),
        }
    }

    fn layout(&self, chars: &usize, _: Size<Points>) -> TextLayout {
        TextLayout(Arc::new(FakeLayout { chars: *chars }))
    }
}

/// [`FakeEngine`], except `max_lines` clamps the line count (0: none).
struct ClampedEngine;

impl TextEngine for ClampedEngine {
    type Prepared = (usize, u32);

    fn prepare(&self, text: &AttributedText) -> (usize, u32) {
        (text.text.chars().count(), text.max_lines)
    }

    fn measure(&self, prepared: &Self::Prepared, max_width: Option<Points>) -> Size<Points> {
        let (chars, max_lines) = *prepared;
        let w = chars as f32 * 8.0;
        let (width, mut lines) = match max_width.map(|m| m.0) {
            Some(max) if w > max && max > 0.0 => (max, (w / max).ceil()),
            _ => (w, 1.0),
        };
        if max_lines > 0 {
            lines = lines.min(max_lines as f32);
        }
        size_pt(width, lines * 16.0)
    }

    fn layout(&self, _: &Self::Prepared, _: Size<Points>) -> TextLayout {
        TextLayout(Arc::new(()))
    }
}

fn core() -> Core<FakeEngine> {
    Core::new(FakeEngine, size(px(100.0), px(200.0)))
}

fn sized(width: f32, height: f32) -> Style {
    let mut style = Style::default();
    style.layout.size = taffy::Size {
        width: Dimension::length(width),
        height: Dimension::length(height),
    };
    style
}

fn apply_style<M: TextEngine>(core: &mut Core<M>, id: NodeId, style: Style) {
    core.apply(Op::SetStyle {
        id,
        style: Box::new(style),
        patch: StylePatch::default(),
    })
    .unwrap();
}

fn frame(batch: &MountBatch, id: NodeId) -> Bounds<Points> {
    batch
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            MountItem::Frame { id: i, frame } if *i == id => Some(*frame),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no frame for {id}"))
}

fn list_core() -> Core<FakeEngine> {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::List,
    })
    .unwrap();
    apply_style(&mut c, 2, sized(100.0, 200.0));
    c.apply(Op::Insert {
        parent: 1,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::EstimatedItemSize(Points(50.0)),
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Count(100),
    })
    .unwrap();
    c
}

fn cell_in_list(c: &mut Core<FakeEngine>, slot: u32, index: u32, id: NodeId) {
    c.apply(Op::Create { id, tag: Tag::Cell }).unwrap();
    c.apply(Op::SetProp {
        id,
        prop: Prop::Slot(CellSlot::Item(slot)),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: id,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: id + 1,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(c, id + 1, sized(100.0, 20.0));
    c.apply(Op::Insert {
        parent: id,
        child: id + 1,
        before: None,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id,
        prop: Prop::CellIndex(index),
    })
    .unwrap();
}

#[test]
fn a_list_binds_places_and_recycles_cell_roots() {
    let mut c = list_core();
    let _ = c.commit().expect("list");
    let mut binds = Vec::new();
    c.take_list_binds(&mut binds);
    assert_eq!(binds.len(), 1);
    // The viewport's cells bind with the list; the overscan follows.
    let now: Vec<u32> = binds[0].binds.iter().map(|b| b.1).collect();
    assert_eq!(now, vec![0, 1, 2, 3]);
    let (slot, index) = binds[0].binds[0];
    assert_eq!(index, 0);
    let mut paced = Vec::new();
    c.take_paced_list_binds(usize::MAX, &mut paced);
    cell_in_list(&mut c, slot, index, 3);
    let batch = c.commit().expect("cell");
    let placed = frame(&batch, 3);
    assert_eq!(placed.origin.x, Points(0.0));
    assert_eq!(placed.origin.y, Points(0.0));
    assert_eq!(placed.size.height, Points(20.0));
    let mut binds = Vec::new();
    c.take_list_binds(&mut binds);
    c.take_paced_list_binds(usize::MAX, &mut binds);
    // Scrolling moves the window; the same cell slot takes item 50.
    c.set_scroll_offset(2, Point::new(Points(0.0), Points(2500.0)));
    let _ = c.commit();
    let mut binds = Vec::new();
    c.take_list_binds(&mut binds);
    assert_eq!(binds[0].binds[0].1, 50);
    assert_eq!(binds[0].list, 2);
    assert!(binds[0].binds.iter().all(|b| b.1 >= 50));
}

#[test]
fn a_scroll_to_index_resolves_to_a_content_offset_and_binds_there() {
    let mut c = list_core();
    c.apply(Op::Command {
        id: 2,
        command: Command::ScrollToIndex {
            index: 50,
            animated: false,
        },
    })
    .unwrap();
    let batch = c.commit().expect("list");
    let command = batch.items.iter().find_map(|item| match item {
        MountItem::Command {
            id: 2,
            command: Command::ScrollTo { y, animated, .. },
        } => Some((y.0, *animated)),
        _ => None,
    });
    assert_eq!(command, Some((2500.0, false)));
    let mut binds = Vec::new();
    c.take_list_binds(&mut binds);
    assert_eq!(binds[0].binds[0].1, 50);
}

#[test]
fn a_scroll_to_end_resolves_to_the_content_end() {
    let mut c = list_core();
    c.apply(Op::Command {
        id: 2,
        command: Command::ScrollToEnd { animated: false },
    })
    .unwrap();
    let batch = c.commit().expect("list");
    let command = batch.items.iter().find_map(|item| match item {
        MountItem::Command {
            id: 2,
            command: Command::ScrollTo { y, animated, .. },
        } => Some((y.0, *animated)),
        _ => None,
    });
    // 100 rows of 50 in a 200-point viewport: the end is 4800.
    assert_eq!(command, Some((4800.0, false)));
    // The platform is never asked for an end it cannot name.
    assert!(!batch.items.iter().any(|item| matches!(
        item,
        MountItem::Command {
            command: Command::ScrollToEnd { .. },
            ..
        }
    )));
}

#[test]
fn column_flex_stacks_children_and_rounds_in_the_root() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    let mut top = sized(100.0, 20.0);
    top.layout.margin.left = taffy::LengthPercentageAuto::length(0.6);
    top.layout.size.width = Dimension::length(10.4);
    apply_style(&mut c, 2, top);
    c.apply(Op::Create {
        id: 3,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 3, sized(100.0, 30.0));
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 3,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let top = frame(&batch, 2);
    let bottom = frame(&batch, 3);
    assert_eq!(top.origin.x, px(1.0));
    assert_eq!(top.size.width, px(10.0));
    assert_eq!(top.origin.y, px(0.0));
    assert_eq!(top.size.height, px(20.0));
    assert_eq!(bottom.origin.y, px(20.0));
    assert_eq!(bottom.size.height, px(30.0));
    assert!(c.commit().is_none());
}

#[test]
fn a_grid_places_cells_in_document_order() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    let mut grid = sized(100.0, 80.0);
    grid.layout.display = Display::Grid;
    grid.layout.grid_template_columns = vec![
        taffy::style_helpers::length(40.0),
        taffy::style_helpers::length(60.0),
    ];
    grid.layout.grid_template_rows = vec![taffy::style_helpers::length(30.0)];
    apply_style(&mut c, 2, grid);
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    for id in [3, 4] {
        c.apply(Op::Create { id, tag: Tag::View }).unwrap();
        apply_style(&mut c, id, sized(10.0, 10.0));
        c.apply(Op::Insert {
            parent: 2,
            child: id,
            before: None,
        })
        .unwrap();
    }
    let batch = c.commit().unwrap();
    assert_eq!(
        frame(&batch, 3).origin,
        crate::geometry::point(px(0.0), px(0.0))
    );
    assert_eq!(
        frame(&batch, 4).origin,
        crate::geometry::point(px(40.0), px(0.0))
    );
}

#[test]
fn an_absolute_child_is_placed_in_its_containing_block() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    let mut style = sized(10.0, 12.0);
    style.layout.position = Position::Absolute;
    style.layout.inset = taffy::Rect {
        top: taffy::style_helpers::length(4.0),
        left: taffy::style_helpers::length(6.0),
        right: taffy::style_helpers::auto(),
        bottom: taffy::style_helpers::auto(),
    };
    apply_style(&mut c, 2, style);
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let frame = frame(&batch, 2);
    assert_eq!(frame.origin, crate::geometry::point(px(6.0), px(4.0)));
    assert_eq!(frame.size, size(px(10.0), px(12.0)));
}

#[test]
fn display_none_clears_the_frame() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 2, sized(40.0, 20.0));
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let shown = c.commit().unwrap();
    assert_eq!(frame(&shown, 2).size, size(px(40.0), px(20.0)));
    let mut hidden = sized(40.0, 20.0);
    hidden.layout.display = Display::None;
    apply_style(&mut c, 2, hidden);
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 2), Bounds::default());
}

#[test]
fn on_press_is_mounted_with_the_view() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::Listen {
        id: 2,
        event: EventKind::Press,
        enabled: true,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let events = batch.items.iter().find_map(|item| match item {
        MountItem::Events { id: 2, events } => Some(*events),
        _ => None,
    });
    assert_eq!(
        events,
        Some(
            EventMask::default() | {
                let mut mask = EventMask::default();
                mask.set(EventKind::Press, true);
                mask
            }
        )
    );
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Create {
            id: 2,
            kind: ViewKind::View
        }
    )));
}

#[test]
fn a_higher_z_index_is_stacked_above() {
    let mut c = core();
    for id in [2, 3] {
        c.apply(Op::Create { id, tag: Tag::View }).unwrap();
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: id,
            before: None,
        })
        .unwrap();
    }
    c.commit().unwrap();
    let high = Style {
        z_index: 5,
        ..Default::default()
    };
    apply_style(&mut c, 3, high);
    let mid = Style {
        z_index: 1,
        ..Default::default()
    };
    apply_style(&mut c, 2, mid);
    let batch = c.commit().unwrap();
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Insert {
            parent: ROOT_ID,
            child: 2,
            before: Some(3),
        }
    )));
}

#[test]
fn remove_and_release_emit_in_order() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.commit().unwrap();
    c.apply(Op::Remove {
        parent: ROOT_ID,
        child: 2,
    })
    .unwrap();
    c.apply(Op::Release { id: 2 }).unwrap();
    let batch = c.commit().unwrap();
    let kinds: Vec<_> = batch
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::Remove { id: 2 } => Some("remove"),
            MountItem::Delete { id: 2 } => Some("delete"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["remove", "delete"]);
    assert_eq!(
        c.apply(Op::SetStyle {
            id: 2,
            style: Box::new(Style::default()),
            patch: StylePatch::default(),
        }),
        Err(ApplyError::UnknownNode(2))
    );
}

#[test]
fn insert_rejects_a_cycle() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    assert_eq!(
        c.apply(Op::Insert {
            parent: 3,
            child: 2,
            before: None,
        }),
        Err(ApplyError::Cycle(2))
    );
}

fn source(uri: &str, width: f32, height: f32, scales: &[f32]) -> ImageSource {
    ImageSource {
        uri: uri.into(),
        size: Some(size_pt(width, height)),
        scales: scales.iter().copied().collect(),
    }
}

#[test]
fn an_image_uses_its_natural_size_and_aspect_ratio() {
    let mut c = Core::new(FakeEngine, size(px(320.0), px(480.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Image,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Source(Some(source("asset:/logo.png", 200.0, 100.0, &[]))),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Image,
    })
    .unwrap();
    let mut narrow = Style::default();
    narrow.set("width", StyleValue::Number(50.0)).unwrap();
    narrow
        .set("alignSelf", StyleValue::Str("flex-start"))
        .unwrap();
    apply_style(&mut c, 3, narrow);
    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Source(Some(source("asset:/logo.png", 200.0, 100.0, &[]))),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 3,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Create {
            id: 2,
            kind: ViewKind::Image
        }
    )));
    assert_eq!(frame(&batch, 2).size, size(px(320.0), px(160.0)));
    assert_eq!(frame(&batch, 3).size, size(px(50.0), px(25.0)));
}

#[test]
fn object_fit_is_mounted_on_the_image() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Image,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Source(Some(source("https://example.com/a.png", 40.0, 30.0, &[]))),
    })
    .unwrap();
    let mut style = Style::default();
    style.set("objectFit", StyleValue::Str("contain")).unwrap();
    apply_style(&mut c, 2, style);
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let fitted = batch.items.iter().rev().find_map(|item| match item {
        MountItem::Image {
            id: 2,
            resize_mode,
            source,
        } => Some((*resize_mode, source.as_ref().map(|s| s.uri.clone()))),
        _ => None,
    });
    assert_eq!(
        fitted,
        Some((
            ResizeMode::Contain,
            Some(std::sync::Arc::from("https://example.com/a.png"))
        ))
    );
}

#[test]
fn a_bundled_image_picks_the_screen_density() {
    let mut c = core();
    c.set_pixel_ratio(2.0);
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Image,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Source(Some(source(
            "asset:/logo.png",
            40.0,
            30.0,
            &[1.0, 2.0, 2.5],
        ))),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let uri = batch.items.iter().find_map(|item| match item {
        MountItem::Image {
            id: 2,
            source: Some(source),
            ..
        } => Some(source.uri.clone()),
        _ => None,
    });
    assert_eq!(uri.as_deref(), Some("asset:/logo@2x.png"));
    assert_eq!(
        source("asset:/logo.png", 1.0, 1.0, &[1.0, 2.0, 2.5]).scales,
        ImageScales::NONE.with(1.0).with(2.0)
    );
    c.set_pixel_ratio(1.0);
    let batch = c.commit().unwrap();
    let uri = batch.items.iter().find_map(|item| match item {
        MountItem::Image {
            id: 2,
            source: Some(source),
            ..
        } => Some(source.uri.clone()),
        _ => None,
    });
    assert_eq!(uri.as_deref(), Some("asset:/logo.png"));
    c.set_pixel_ratio(1.0);
    assert!(c.commit().is_none());
}

#[test]
fn animated_style_binds_properties_to_nodes() {
    use crate::animation::{AnimationOp, AnimationProp};
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: crate::ops::ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let mut style = sized(10.0, 10.0);
    style.animated = vec![
        (AnimationProp::Opacity, 7),
        (AnimationProp::BackgroundColor, 9),
        (AnimationProp::Transform(0), 11),
    ];
    apply_style(&mut c, 2, style);
    c.apply(Op::Animation(AnimationOp::CreateValue {
        node: 7,
        value: 0.5,
    }))
    .unwrap();
    let batch = c.commit().expect("a frame");
    let animation = batch
        .items
        .iter()
        .position(|item| matches!(item, MountItem::Animation(_)))
        .expect("the graph op");
    let bindings = batch
        .items
        .iter()
        .position(|item| matches!(item, MountItem::AnimationBindings { id: 2, .. }))
        .expect("the bindings");
    assert!(animation < bindings, "graph ops precede bindings");
    let MountItem::AnimationBindings { bindings, .. } = &batch.items[bindings] else {
        unreachable!()
    };
    assert_eq!(
        bindings,
        &[
            (AnimationProp::Opacity, 7),
            (AnimationProp::BackgroundColor, 9),
            (AnimationProp::Transform(0), 11),
        ]
    );

    let mut style = sized(10.0, 10.0);
    style.animated = vec![(AnimationProp::BorderColor, 13)];
    apply_style(&mut c, 2, style);
    let batch = c.commit().expect("a frame");
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::AnimationBindings { id: 2, bindings }
            if bindings == &[(AnimationProp::BorderColor, 13)]
    )));
}

fn mounted_text(batch: &MountBatch, id: NodeId) -> Arc<AttributedText> {
    batch
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            MountItem::Text { id: i, text, .. } if *i == id => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no text for {id}"))
}

#[test]
fn a_text_block_mounts_its_run_and_stretches() {
    let mut c = Core::new(FakeEngine, size(px(320.0), px(480.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 4,
        text: "hello".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    let mut padded = Style::default();
    padded.set("padding", StyleValue::Number(10.0)).unwrap();
    apply_style(&mut c, 2, padded);
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let creates: Vec<_> = batch
        .items
        .iter()
        .filter_map(|item| match item {
            MountItem::Create { id, kind } => Some((*id, *kind)),
            _ => None,
        })
        .collect();
    assert_eq!(creates, vec![(2, ViewKind::View), (3, ViewKind::Text)]);
    assert_eq!(mounted_text(&batch, 3).text, "hello");
    assert_eq!(
        frame(&batch, 2),
        crate::geometry::rect(0.0, 0.0, 320.0, 36.0)
    );
    assert_eq!(
        frame(&batch, 3),
        crate::geometry::rect(10.0, 10.0, 300.0, 16.0)
    );
    assert!(c.commit().is_none());
}

#[test]
fn updating_text_emits_the_string_without_a_new_frame() {
    let mut c = Core::new(FakeEngine, size(px(320.0), px(480.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "a".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.commit().unwrap();
    c.apply(Op::SetText {
        id: 3,
        text: "abc".into(),
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert_eq!(mounted_text(&batch, 2).text, "abc");
    assert_eq!(batch.items.len(), 1);
}

/// One line at the width taffy offers, two once that box rounds down.
struct EdgeEngine;

impl TextEngine for EdgeEngine {
    type Prepared = bool;

    fn prepare(&self, text: &AttributedText) -> bool {
        text.text.chars().count() > 10
    }

    fn measure(&self, long: &bool, max_width: Option<Points>) -> Size<Points> {
        let natural = if *long { 104.3 } else { 16.0 };
        match max_width.map(|m| m.0) {
            Some(max) if natural > max && max > 0.0 => size_pt(max, 32.0),
            _ => size_pt(natural, 16.0),
        }
    }

    fn layout(&self, _: &bool, _: Size<Points>) -> TextLayout {
        TextLayout(Arc::new(()))
    }
}

#[test]
fn text_near_the_box_edge_keeps_a_line_for_the_wrap() {
    // 104.3 of text in a 104.4 box. Rounding takes the box to 104, and
    // the line that fit the offered width needs a second row. Shorter
    // text gives that row back.
    let mut c = Core::new(EdgeEngine, size(px(104.4), px(80.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "xxxxxxxxxxxxx".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(
        frame(&batch, 2).size.height.0 >= 32.0,
        "the second line needs a row, have {}",
        frame(&batch, 2).size.height.0
    );
    c.apply(Op::SetText {
        id: 3,
        text: "hi".into(),
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(
        frame(&batch, 2).size.height.0 < 24.0,
        "a short line drops the extra row, have {}",
        frame(&batch, 2).size.height.0
    );
}

#[test]
fn nested_text_inherits_and_overrides_span_style() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    let mut parent = Style::default();
    parent.set("color", StyleValue::Str("#00ff00")).unwrap();
    apply_style(&mut c, 2, parent);
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 4,
        text: "hi ".into(),
    })
    .unwrap();
    c.apply(Op::Create {
        id: 5,
        tag: Tag::Text,
    })
    .unwrap();
    let mut span = Style::default();
    span.set("color", StyleValue::Str("#ff0000")).unwrap();
    span.set("fontWeight", StyleValue::Str("bold")).unwrap();
    span.set("fontSize", StyleValue::Number(20.0)).unwrap();
    span.set("fontStyle", StyleValue::Str("italic")).unwrap();
    span.set("backgroundColor", StyleValue::Str("#0000ff"))
        .unwrap();
    apply_style(&mut c, 5, span);
    c.apply(Op::CreateText {
        id: 6,
        text: "there".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 5,
        child: 6,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 5,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let text = mounted_text(&batch, 3);
    assert_eq!(text.text, "hi there");
    assert_eq!(text.spans.len(), 2);
    assert_eq!(
        text.spans[0].style.color,
        crate::Color::parse("#00ff00").unwrap()
    );
    assert!(!text.spans[0].style.italic);
    assert_eq!(text.spans[0].style.font_weight, 400);
    assert_eq!(
        text.spans[1].style.color,
        crate::Color::parse("#ff0000").unwrap()
    );
    assert_eq!(text.spans[1].style.font_weight, 700);
    assert_eq!(text.spans[1].style.font_size, px(20.0));
    assert!(text.spans[1].style.italic);
    assert_eq!(
        text.spans[1].style.background.unwrap().color,
        crate::Color::parse("#0000ff").unwrap()
    );
    assert!(!batch.items.iter().any(|item| matches!(
        item,
        MountItem::Create { id: 5, .. } | MountItem::Insert { child: 5, .. }
    )));
}

#[test]
fn hidden_text_keeps_its_string_and_lays_out_nothing() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "check".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    let mut hidden = Style::default();
    hidden.layout.display = Display::None;
    apply_style(&mut c, 2, hidden);
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let (text, chars) = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Text {
                id: 2,
                text,
                layout,
                ..
            } => Some((
                text.text.clone(),
                layout.downcast::<FakeLayout>().unwrap().chars,
            )),
            _ => None,
        })
        .unwrap();
    assert_eq!(text, "check");
    assert_eq!(chars, 0);
    assert_eq!(frame(&batch, 2).size, size(px(0.0), px(0.0)));
}

#[test]
fn number_of_lines_reaches_the_block() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "one two three four".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    let mut clamped = Style::default();
    clamped
        .set("numberOfLines", StyleValue::Number(1.0))
        .unwrap();
    apply_style(&mut c, 2, clamped);
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    assert_eq!(mounted_text(&c.commit().unwrap(), 2).max_lines, 1);
}

#[test]
fn a_frame_change_remounts_the_text_content_box() {
    let mut c = Core::new(FakeEngine, size(px(320.0), px(480.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "hello".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.commit().unwrap();
    let mut padded = Style::default();
    padded.set("padding", StyleValue::Number(10.0)).unwrap();
    apply_style(&mut c, 2, padded);
    let batch = c.commit().unwrap();
    let content = batch.items.iter().rev().find_map(|item| match item {
        MountItem::Text { id: 2, content, .. } => Some(*content),
        _ => None,
    });
    assert_eq!(
        content,
        Some(crate::geometry::rect(10.0, 10.0, 300.0, 16.0))
    );
    assert_eq!(
        frame(&batch, 2),
        crate::geometry::rect(0.0, 0.0, 320.0, 36.0)
    );
}

fn scroll_content(batch: &MountBatch, id: NodeId) -> Size<Points> {
    batch
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            MountItem::ScrollContent { id: i, size, .. } if *i == id => Some(*size),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no scroll content for {id}"))
}

#[test]
fn a_vertical_scroll_reports_its_content_and_keeps_child_heights() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    apply_style(&mut c, 2, sized(100.0, 50.0));
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    for id in [3, 4, 5] {
        c.apply(Op::Create { id, tag: Tag::View }).unwrap();
        apply_style(&mut c, id, sized(100.0, 40.0));
        c.apply(Op::Insert {
            parent: 2,
            child: id,
            before: None,
        })
        .unwrap();
    }
    let batch = c.commit().unwrap();
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Create {
            id: 2,
            kind: ViewKind::Scroll
        }
    )));
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::ScrollConfig {
            id: 2,
            horizontal: false,
            ..
        }
    )));
    assert_eq!(scroll_content(&batch, 2), size_pt(100.0, 120.0));
    // Children are not shrunk to the scroll view's height.
    assert_eq!(frame(&batch, 5).origin.y, px(80.0));
    assert_eq!(frame(&batch, 5).size.height, px(40.0));
}

#[test]
fn a_horizontal_scroll_lays_its_children_out_in_a_row() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Horizontal(true),
    })
    .unwrap();
    apply_style(&mut c, 2, sized(100.0, 40.0));
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    for id in [3, 4] {
        c.apply(Op::Create { id, tag: Tag::View }).unwrap();
        apply_style(&mut c, id, sized(80.0, 40.0));
        c.apply(Op::Insert {
            parent: 2,
            child: id,
            before: None,
        })
        .unwrap();
    }
    let batch = c.commit().unwrap();
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::ScrollConfig {
            id: 2,
            horizontal: true,
            ..
        }
    )));
    assert_eq!(scroll_content(&batch, 2), size_pt(160.0, 40.0));
    assert_eq!(frame(&batch, 4).origin.x, px(80.0));
}

#[test]
fn scroll_keyboard_props_are_mounted_and_blur_is_a_command() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::KeyboardTaps(crate::KeyboardTaps::Handled),
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::KeyboardDismiss(crate::KeyboardDismiss::OnDrag),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Command {
        id: ROOT_ID,
        command: Command::Blur,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::ScrollConfig {
            id: 2,
            taps: crate::KeyboardTaps::Handled,
            dismiss: crate::KeyboardDismiss::OnDrag,
            ..
        }
    )));
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Command {
            command: Command::Blur,
            ..
        }
    )));
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::KeyboardTaps(crate::KeyboardTaps::Always),
    })
    .unwrap();
    // A later change is mounted again. Dismiss stays.
    let batch = c.commit().unwrap();
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::ScrollConfig {
            taps: crate::KeyboardTaps::Always,
            dismiss: crate::KeyboardDismiss::OnDrag,
            ..
        }
    )));
}

/// `overflow: "hidden"` clips in layout as well as in paint: what a box
/// clips away must not grow an ancestor scroll view's content size.
#[test]
fn clipped_overflow_does_not_grow_scroll_content() {
    fn content_width(clips: bool) -> f32 {
        let mut c = core();
        c.apply(Op::Create {
            id: 2,
            tag: Tag::Scroll,
        })
        .unwrap();
        apply_style(&mut c, 2, sized(100.0, 80.0));
        c.apply(Op::Insert {
            parent: ROOT_ID,
            child: 2,
            before: None,
        })
        .unwrap();
        c.apply(Op::Create {
            id: 3,
            tag: Tag::View,
        })
        .unwrap();
        let mut boxed = sized(80.0, 40.0);
        boxed.view.overflow_hidden = clips;
        apply_style(&mut c, 3, boxed);
        c.apply(Op::Insert {
            parent: 2,
            child: 3,
            before: None,
        })
        .unwrap();
        c.apply(Op::Create {
            id: 4,
            tag: Tag::View,
        })
        .unwrap();
        let mut child = sized(150.0, 20.0);
        child.layout.position = Position::Absolute;
        child.layout.inset.left = taffy::LengthPercentageAuto::length(0.0);
        child.layout.inset.top = taffy::LengthPercentageAuto::length(0.0);
        apply_style(&mut c, 4, child);
        c.apply(Op::Insert {
            parent: 3,
            child: 4,
            before: None,
        })
        .unwrap();
        scroll_content(&c.commit().unwrap(), 2).width.0
    }
    // Visible: the 150pt child reaches the scroll view.
    assert_eq!(content_width(false), 150.0);
    // Clipped: the scroll view keeps its own width.
    assert_eq!(content_width(true), 100.0);
}

#[test]
fn a_right_to_left_horizontal_scroll_starts_at_its_end() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Horizontal(true),
    })
    .unwrap();
    let mut scroll = sized(150.0, 50.0);
    scroll.direction = Some(true);
    scroll.logical.padding[0] = Some(taffy::LengthPercentage::length(10.0));
    apply_style(&mut c, 2, scroll);
    for id in [3, 4] {
        c.apply(Op::Create { id, tag: Tag::View }).unwrap();
        apply_style(&mut c, id, sized(100.0, 50.0));
        c.apply(Op::Insert {
            parent: 2,
            child: id,
            before: None,
        })
        .unwrap();
    }
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    // Content: 10 (start padding) + 200; the first child at the right end.
    assert_eq!(scroll_content(&batch, 2), size_pt(210.0, 50.0));
    assert_eq!(frame(&batch, 3).origin.x, px(210.0 - 10.0 - 100.0));
    assert_eq!(frame(&batch, 4).origin.x, px(0.0));
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Command {
            id: 2,
            command: Command::ScrollTo { x, .. }
        } if *x == px(60.0)
    )));

    // Wider content keeps its distance from the right end.
    apply_style(&mut c, 3, sized(130.0, 50.0));
    let batch = c.commit().unwrap();
    assert_eq!(scroll_content(&batch, 2).width, px(240.0));
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::ScrollBy { id: 2, dx, .. } if *dx == px(30.0)
    )));
}

#[test]
fn a_scroll_view_mounts_the_scroll_events_it_listens_to() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let kinds = [
        EventKind::Scroll,
        EventKind::ScrollEnd,
        EventKind::ScrollDragStart,
        EventKind::ScrollDragEnd,
        EventKind::MomentumStart,
        EventKind::MomentumEnd,
    ];
    for event in kinds {
        c.apply(Op::Listen {
            id: 2,
            event,
            enabled: true,
        })
        .unwrap();
    }
    let batch = c.commit().unwrap();
    let events = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Events { id: 2, events } => Some(*events),
            _ => None,
        })
        .expect("scroll events");
    for event in kinds {
        assert!(events.contains(event));
    }
    assert!(!events.contains(EventKind::Press));
    assert_eq!(EventKind::from_prop("onScroll"), Some(EventKind::Scroll));
    assert_eq!(
        EventKind::from_prop("onScrollDragEnd"),
        Some(EventKind::ScrollDragEnd)
    );
    assert_eq!(EventKind::MomentumEnd.prop_name(), "onMomentumEnd");
    assert_eq!(
        EventKind::from_raw(EventKind::Scroll as u8),
        Some(EventKind::Scroll)
    );
    assert_eq!(
        EventKind::from_raw(EventKind::MomentumEnd as u8),
        Some(EventKind::MomentumEnd)
    );
    assert_eq!(EventKind::from_raw(99), None);
}

#[test]
fn an_input_measures_its_placeholder_and_reports_its_content_box() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Input,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Placeholder("Name".into()),
    })
    .unwrap();
    let mut padded = Style::default();
    padded.set("padding", StyleValue::Number(6.0)).unwrap();
    apply_style(&mut c, 2, padded);
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    // One line (16pt) plus padding, stretched across the root.
    assert_eq!(frame(&batch, 2).size.height, px(28.0));
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Input { id: 2, config }
            if config.placeholder == "Name"
                && !config.secure
                && !config.disabled
                && config.keyboard == KeyboardType::Text
    )));
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::InputContent { id: 2, content }
            if *content == crate::geometry::rect(6.0, 6.0, 88.0, 16.0)
    )));
    // No text yet: nothing to send.
    assert!(
        !batch
            .items
            .iter()
            .any(|item| matches!(item, MountItem::InputValue { .. }))
    );
    assert_eq!(Tag::from_name("input"), Some(Tag::Input));
}

#[test]
fn header_item_tag_is_lowercase() {
    assert_eq!(Tag::from_name("headeritem"), Some(Tag::HeaderItem));
    assert_eq!(Tag::from_name("headerItem"), None);
}

#[test]
fn an_empty_input_is_one_line_tall() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Input,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    assert_eq!(frame(&c.commit().unwrap(), 2).size, size_pt(100.0, 16.0));
}

#[test]
fn a_single_line_input_stays_one_line_when_its_text_wraps() {
    // 50 characters at 8pt in a 100pt box is four lines unless
    // `max_lines` is 1. [`FakeEngine`] ignores that, so this engine
    // does not.
    let mut c = Core::new(ClampedEngine, size(px(100.0), px(200.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Input,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Value("x".repeat(50)),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    assert_eq!(frame(&c.commit().unwrap(), 2).size.height, px(16.0));
}

#[test]
fn a_multiline_input_wraps_instead_of_clamping_to_one_line() {
    let mut c = Core::new(ClampedEngine, size(px(100.0), px(200.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Input,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Multiline(true),
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Value("x".repeat(50)),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    assert_eq!(frame(&c.commit().unwrap(), 2).size.height, px(64.0));
}

#[test]
fn selected_range_is_sent_after_the_value() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Input,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Value("hello".into()),
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Selection { start: 1, end: 3 },
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let value = batch
        .items
        .iter()
        .position(|item| matches!(item, MountItem::InputValue { id: 2, .. }))
        .expect("the value");
    let selection = batch
        .items
        .iter()
        .position(|item| {
            matches!(
                item,
                MountItem::Command {
                    id: 2,
                    command: Command::SetSelection { start: 1, end: 3 },
                }
            )
        })
        .expect("the selection");
    assert!(value < selection, "the value is applied first");
}

#[test]
fn broken_runs_break_a_long_unbroken_run() {
    let content = "x".repeat(MEASURED_RUN * 2 + 10);
    let broken = super::input::broken_runs(&content);
    assert_eq!(broken.matches('\n').count(), 2);
    assert_eq!(broken.chars().filter(|c| *c != '\n').count(), content.len());
    // Runs a browser would break are left alone.
    let text = "word ".repeat(600);
    assert_eq!(super::input::broken_runs(&text), text);
}

#[test]
fn input_values_from_js_and_native_edits() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Input,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Placeholder("Name".into()),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.commit().unwrap();
    // The user types; JS echoes the same text: nothing goes back.
    c.input_changed(2, "ab", 2);
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Value("ab".into()),
    })
    .unwrap();
    assert!(c.commit().is_none());
    // JS transforms the text: it is sent with the edit count it is based on.
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Value("AB".into()),
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::InputValue { id: 2, text, count: 2 } if text == "AB"
    )));
}

#[test]
fn an_input_mounts_secure_keyboard_disabled_and_inherited_style() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    let mut parent = Style::default();
    parent.set("color", StyleValue::Str("#00ff00")).unwrap();
    apply_style(&mut c, 2, parent);
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Input,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Secure(true),
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Keyboard(KeyboardType::Email),
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Disabled(true),
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Value("secret".into()),
    })
    .unwrap();
    c.apply(Op::Listen {
        id: 3,
        event: EventKind::Change,
        enabled: true,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let config = batch
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            MountItem::Input { id: 3, config } => Some(config),
            _ => None,
        })
        .expect("input config");
    assert!(config.secure);
    assert!(config.disabled);
    assert_eq!(config.keyboard, KeyboardType::Email);
    assert_eq!(config.style.color, crate::Color::parse("#00ff00").unwrap());
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::InputValue { id: 3, text, count: 0 } if text == "secret"
    )));
    let events = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::Events { id: 3, events } => Some(*events),
            _ => None,
        })
        .expect("input events");
    assert!(events.contains(EventKind::Change));
    assert_eq!(EventKind::from_prop("onChange"), Some(EventKind::Change));
    assert_eq!(EventKind::Change.prop_name(), "onChange");
    assert_eq!(KeyboardType::from_name("phone"), Some(KeyboardType::Phone));
    assert_eq!(KeyboardType::from_name("url"), None);

    let mut own = Style::default();
    own.set("color", StyleValue::Str("#ff0000")).unwrap();
    own.set("textAlign", StyleValue::Str("center")).unwrap();
    own.direction = Some(true);
    apply_style(&mut c, 3, own);
    let batch = c.commit().unwrap();
    let config = batch
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            MountItem::Input { id: 3, config } => Some(config),
            _ => None,
        })
        .expect("restyled input");
    assert_eq!(config.style.color, crate::Color::parse("#ff0000").unwrap());
    assert_eq!(config.align, crate::style::TextAlign::Center);
    assert!(config.rtl);
}

#[test]
fn keyboard_inset_pads_and_stops_when_the_term_is_replaced() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    let mut box_style = Style::default();
    box_style.set("height", StyleValue::Number(100.0)).unwrap();
    box_style
        .set(
            "paddingBottom",
            StyleValue::Str("env(keyboard-inset-height)"),
        )
        .unwrap();
    apply_style(&mut c, 2, box_style);
    c.apply(Op::Create {
        id: 3,
        tag: Tag::View,
    })
    .unwrap();
    let mut child = Style::default();
    child.set("flex", StyleValue::Number(1.0)).unwrap();
    apply_style(&mut c, 3, child);
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 3).size.height, px(100.0));

    c.set_keyboard_inset(px(40.0));
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 3).size.height, px(60.0));

    // A negative height is hidden.
    c.set_keyboard_inset(px(-8.0));
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 3).size.height, px(100.0));

    c.set_keyboard_inset(px(40.0));
    c.commit().unwrap();
    let mut plain = Style::default();
    plain.set("height", StyleValue::Number(100.0)).unwrap();
    plain
        .set("paddingBottom", StyleValue::Number(10.0))
        .unwrap();
    apply_style(&mut c, 2, plain);
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 3).size.height, px(90.0));
    c.set_keyboard_inset(Points::ZERO);
    assert!(c.commit().is_none());
}

#[test]
fn safe_area_and_calc_add_to_padding_and_margin() {
    let mut c = core();
    c.set_safe_area(Edges {
        top: px(20.0),
        right: px(4.0),
        bottom: px(34.0),
        left: px(12.0),
    });
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    let mut padded = Style::default();
    padded
        .set(
            "paddingTop",
            StyleValue::Str("calc(env(safe-area-inset-top) + 8px)"),
        )
        .unwrap();
    padded
        .set(
            "paddingBottom",
            StyleValue::Str("env(safe-area-inset-bottom)"),
        )
        .unwrap();
    padded
        .set(
            "marginLeft",
            StyleValue::Str("calc(0.5rem + env(safe-area-inset-left))"),
        )
        .unwrap();
    apply_style(&mut c, 2, padded);
    c.apply(Op::Create {
        id: 3,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 3, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 4, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 4,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    // 8 + 20 top, 34 bottom, around a 10pt child. Margin left is 8 + 12.
    assert_eq!(frame(&batch, 2).size.height, px(72.0));
    assert_eq!(frame(&batch, 2).origin.x, px(20.0));
    assert_eq!(frame(&batch, 4).origin.y, px(72.0));

    c.set_safe_area(Edges {
        top: Points::ZERO,
        bottom: px(34.0),
        left: px(12.0),
        ..Edges::default()
    });
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 2).size.height, px(52.0));
    assert_eq!(frame(&batch, 4).origin.y, px(52.0));
}

#[test]
fn screen_env_does_not_repeat_the_header_safe_area() {
    let mut c = core();
    c.set_safe_area(Edges {
        top: px(20.0),
        right: px(4.0),
        bottom: px(34.0),
        left: px(12.0),
    });
    let padded = || {
        let mut style = Style::default();
        style
            .set(
                "paddingTop",
                StyleValue::Str("calc(env(safe-area-inset-top) + 8px)"),
            )
            .unwrap();
        style
            .set(
                "paddingBottom",
                StyleValue::Str("env(safe-area-inset-bottom)"),
            )
            .unwrap();
        style
    };
    // On the surface. Style before insert, as the runtime does.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 2, padded());
    c.apply(Op::Create {
        id: 3,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 3, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();

    // On a screen whose native header already covers the top inset.
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Stack,
    })
    .unwrap();
    apply_style(&mut c, 4, sized(100.0, 180.0));
    c.apply(Op::Create {
        id: 6,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 8,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 8, padded());
    c.apply(Op::Create {
        id: 9,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 9, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: 8,
        child: 9,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 6,
        child: 8,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 4,
        child: 6,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 4,
        before: None,
    })
    .unwrap();

    // A transparent native bar still counts: 44pt bar + 20pt status
    // inset, plus the 8pt padding, around a 10pt child.
    c.apply(Op::Create {
        id: 10,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 10,
        prop: Prop::Screen(crate::ops::ScreenProp::HeaderTransparent(true)),
    })
    .unwrap();
    c.apply(Op::Create {
        id: 11,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 11, padded());
    c.apply(Op::Create {
        id: 12,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 12, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: 11,
        child: 12,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 10,
        child: 11,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 4,
        child: 10,
        before: None,
    })
    .unwrap();

    let batch = c.commit().unwrap();
    let pad = |batch: &MountBatch, view: NodeId, child: NodeId| {
        (frame(batch, child).origin.y, frame(batch, view).size.height)
    };
    // Surface: 20 + 8 top, 34 bottom, around a 10pt child.
    assert_eq!(pad(&batch, 2, 3), (px(28.0), px(72.0)));
    // Header covers the top (8 + 10 + 34). The bottom is still the surface's.
    assert_eq!(pad(&batch, 8, 9), (px(8.0), px(52.0)));
    assert_eq!(pad(&batch, 11, 12), (px(72.0), px(116.0)));

    c.set_root_insets(
        6,
        Edges {
            top: px(3.0),
            bottom: px(9.0),
            ..Edges::default()
        },
    );
    let batch = c.commit().unwrap();
    // 3 + 8 top, 9 bottom. The surface and the transparent screen are
    // not in this batch: their insets did not change.
    assert!(
        batch
            .items
            .iter()
            .all(|item| { !matches!(item, MountItem::Frame { id: 3 | 12, .. }) })
    );

    c.set_root_insets(
        10,
        Edges {
            top: px(3.0),
            bottom: px(9.0),
            ..Edges::default()
        },
    );
    let batch = c.commit().unwrap();
    // A report replaces the estimate. It is not added on top of the bar.
    assert_eq!(pad(&batch, 11, 12), (px(11.0), px(30.0)));
}

#[test]
fn drawn_header_keeps_the_surface_top_inset() {
    let mut c = core();
    c.set_header_heights(Points::ZERO, Points::ZERO);
    c.set_safe_area(Edges {
        top: px(47.0),
        right: px(8.0),
        bottom: px(34.0),
        left: px(12.0),
    });
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    apply_style(&mut c, 2, sized(200.0, 400.0));
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::HeaderItem,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 4,
        prop: Prop::HeaderSlot(crate::ops::HeaderSlot::Header),
    })
    .unwrap();
    let inset = || {
        let mut style = Style::default();
        style
            .set("paddingTop", StyleValue::Str("env(safe-area-inset-top)"))
            .unwrap();
        style
            .set("marginLeft", StyleValue::Str("env(safe-area-inset-left)"))
            .unwrap();
        style
    };
    c.apply(Op::Create {
        id: 5,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 5, inset());
    c.apply(Op::Create {
        id: 6,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 6, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: 5,
        child: 6,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 4,
        child: 5,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 7,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 7, inset());
    c.apply(Op::Create {
        id: 8,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 8, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: 7,
        child: 8,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 7,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    // Web reports the top as 0 once the app header covers it, and a
    // different left from the surface.
    c.set_root_insets(
        3,
        Edges {
            top: Points::ZERO,
            right: px(8.0),
            bottom: px(34.0),
            left: px(5.0),
        },
    );
    let batch = c.commit().unwrap();
    // Header slot: surface top (47) plus the 10pt child. Left is the
    // screen's 5, not the surface's 12.
    assert_eq!(frame(&batch, 6).origin.y, px(47.0));
    assert_eq!(frame(&batch, 5).size.height, px(57.0));
    assert_eq!(frame(&batch, 5).origin.x, px(5.0));
    // Content under the slot: top is already covered.
    assert_eq!(frame(&batch, 8).origin.y, px(0.0));
    assert_eq!(frame(&batch, 7).size.height, px(10.0));
    assert_eq!(frame(&batch, 7).origin.x, px(5.0));

    // Transparent: the platform reports the bar's height as the top
    // inset. Content reads it. The header slot still reads the surface.
    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Screen(crate::ops::ScreenProp::HeaderTransparent(true)),
    })
    .unwrap();
    c.set_root_insets(
        3,
        Edges {
            top: px(91.0),
            right: px(8.0),
            bottom: px(34.0),
            left: px(5.0),
        },
    );
    c.commit().unwrap();
    let placed = |id: NodeId| c.nodes.iter().find(|(_, n)| n.id == id).unwrap().1.frame;
    // The slot did not move, so this commit may not mount its frame.
    assert_eq!(placed(6).origin.y, px(47.0));
    assert_eq!(placed(8).origin.y, px(91.0));
    assert_eq!(placed(7).size.height, px(101.0));
}

#[test]
fn max_env_uses_the_larger_inset() {
    let mut c = core();
    c.set_safe_area(Edges {
        bottom: px(34.0),
        ..Edges::default()
    });
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    let mut padded = Style::default();
    padded
        .set(
            "paddingBottom",
            StyleValue::Str(
                "calc(max(env(safe-area-inset-bottom), env(keyboard-inset-height)) + 8px)",
            ),
        )
        .unwrap();
    apply_style(&mut c, 2, padded);
    c.apply(Op::Create {
        id: 3,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 3, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    // Keyboard hidden: safe area 34 + 8, around a 10pt child.
    assert_eq!(frame(&batch, 2).size.height, px(52.0));

    c.set_keyboard_inset(px(300.0));
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 2).size.height, px(318.0));

    c.set_safe_area(Edges {
        bottom: px(400.0),
        ..Edges::default()
    });
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 2).size.height, px(418.0));

    // A shorter keyboard does not shrink below the safe area.
    c.set_safe_area(Edges {
        bottom: px(34.0),
        ..Edges::default()
    });
    c.set_keyboard_inset(px(10.0));
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 2).size.height, px(52.0));
}

#[test]
fn scroll_padding_follows_the_safe_area() {
    let mut c = core();
    c.set_safe_area(Edges {
        top: px(20.0),
        ..Edges::default()
    });
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    let mut scroll = sized(100.0, 80.0);
    scroll
        .set("paddingTop", StyleValue::Str("env(safe-area-inset-top)"))
        .unwrap();
    apply_style(&mut c, 2, scroll);
    c.apply(Op::Create {
        id: 3,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 3, sized(100.0, 50.0));
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 3).origin.y, px(20.0));

    c.set_safe_area(Edges {
        top: px(8.0),
        ..Edges::default()
    });
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 3).origin.y, px(8.0));
}

#[test]
fn scroll_content_follows_safe_area_padding() {
    let mut c = core();
    c.set_safe_area(Edges {
        bottom: px(34.0),
        ..Edges::default()
    });
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    let mut scroll = sized(100.0, 40.0);
    scroll
        .set(
            "paddingBottom",
            StyleValue::Str("env(safe-area-inset-bottom)"),
        )
        .unwrap();
    apply_style(&mut c, 2, scroll);
    c.apply(Op::Create {
        id: 3,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 3, sized(100.0, 50.0));
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert_eq!(scroll_content(&batch, 2), size_pt(100.0, 84.0));

    c.set_safe_area(Edges {
        bottom: px(10.0),
        ..Edges::default()
    });
    let batch = c.commit().unwrap();
    assert_eq!(scroll_content(&batch, 2), size_pt(100.0, 60.0));
}

#[test]
fn a_text_keeps_env_padding_after_leaving_a_span() {
    let mut c = core();
    c.set_safe_area(Edges {
        top: px(20.0),
        ..Edges::default()
    });
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    let mut padded = Style::default();
    padded
        .set("paddingTop", StyleValue::Str("env(safe-area-inset-top)"))
        .unwrap();
    apply_style(&mut c, 2, padded);
    c.apply(Op::CreateText {
        id: 3,
        text: "ab".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 2).size.height, px(36.0));

    // Nested: the layout node goes away. The inset changes while it is a span.
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 4,
        child: 2,
        before: None,
    })
    .unwrap();
    c.set_safe_area(Edges {
        top: px(8.0),
        ..Edges::default()
    });
    c.commit();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 2).size.height, px(24.0));
}

#[test]
fn horizontal_rtl_scroll_keeps_safe_area_on_the_physical_left() {
    let mut c = core();
    c.set_safe_area(Edges {
        left: px(20.0),
        ..Edges::default()
    });
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Horizontal(true),
    })
    .unwrap();
    let mut scroll = sized(150.0, 50.0);
    scroll.direction = Some(true);
    scroll
        .set("paddingLeft", StyleValue::Str("env(safe-area-inset-left)"))
        .unwrap();
    apply_style(&mut c, 2, scroll);
    for id in [3, 4] {
        c.apply(Op::Create { id, tag: Tag::View }).unwrap();
        apply_style(&mut c, id, sized(100.0, 50.0));
        c.apply(Op::Insert {
            parent: 2,
            child: id,
            before: None,
        })
        .unwrap();
    }
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    // End padding after the row is mirrored back to the physical left.
    assert_eq!(scroll_content(&batch, 2), size_pt(220.0, 50.0));
    assert_eq!(frame(&batch, 3).origin.x, px(120.0));
    assert_eq!(frame(&batch, 4).origin.x, px(20.0));
}

#[test]
fn state_styles_paint_without_changing_layout() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 2, sized(80.0, 40.0));
    let mut active = Style::default();
    active.set("width", StyleValue::Number(10.0)).unwrap();
    active
        .set("backgroundColor", StyleValue::Str("#ff0000"))
        .unwrap();
    active.text.color = Some(Color(0x00ff00ff));
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::StateStyle {
            interaction: Interaction::Active,
            paint: Some(Box::new((active.view, active.text))),
        },
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert_eq!(frame(&batch, 2).size.width, px(80.0));
    let (view, text) = batch
        .items
        .iter()
        .find_map(|item| match item {
            MountItem::StateStyle {
                id: 2,
                interaction: Interaction::Active,
                view,
                text,
            } => Some((view, text)),
            _ => None,
        })
        .expect("active paint");
    assert_eq!(view.background_color, Color::rgba(255, 0, 0, 255));
    assert_eq!(text.color, Some(Color(0x00ff00ff)));

    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::StateStyle {
            interaction: Interaction::Active,
            paint: None,
        },
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::StateStyle {
            id: 2,
            interaction: Interaction::Active,
            view,
            text,
        } if **view == ViewStyle::default() && *text == TextStyle::default()
    )));
    assert!(
        batch
            .items
            .iter()
            .all(|item| !matches!(item, MountItem::Frame { id: 2, .. }))
    );
}

#[test]
fn a_view_mounts_its_test_id_and_a_clear() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::TestId(Some("box".into())),
    })
    .unwrap();
    apply_style(&mut c, 2, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::TestId { id: 2, test_id } if test_id.as_deref() == Some("box")
    )));
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::TestId(None),
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::TestId {
            id: 2,
            test_id: None
        }
    )));
    assert!(
        batch
            .items
            .iter()
            .all(|item| !matches!(item, MountItem::Frame { .. }))
    );
}

/// 8pt per character, and counts typesetting so a test id can be shown
/// not to redo it.
struct Counted {
    prepares: std::cell::Cell<u32>,
    layouts: std::cell::Cell<u32>,
    measures: std::cell::Cell<u32>,
}

impl TextEngine for Counted {
    type Prepared = usize;

    fn prepare(&self, text: &AttributedText) -> usize {
        self.prepares.set(self.prepares.get() + 1);
        text.text.chars().count()
    }

    fn measure(&self, chars: &usize, max_width: Option<Points>) -> Size<Points> {
        self.measures.set(self.measures.get() + 1);
        let w = *chars as f32 * 8.0;
        match max_width.map(|m| m.0) {
            Some(max) if w > max && max > 0.0 => size_pt(max, (w / max).ceil() * 16.0),
            _ => size_pt(w, 16.0),
        }
    }

    fn layout(&self, chars: &usize, _: Size<Points>) -> TextLayout {
        self.layouts.set(self.layouts.get() + 1);
        TextLayout(Arc::new(*chars))
    }
}

#[test]
fn a_spans_test_id_is_metadata_on_its_block() {
    let mut c = Core::new(
        Counted {
            prepares: std::cell::Cell::new(0),
            layouts: std::cell::Cell::new(0),
            measures: std::cell::Cell::new(0),
        },
        size(px(320.0), px(480.0)),
    );
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "see ".into(),
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 5,
        text: "docs".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 4,
        child: 5,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(mounted_text(&batch, 2).test_ids.is_empty());
    let (prepares, layouts, measures) = (
        c.text_engine.prepares.get(),
        c.text_engine.layouts.get(),
        c.text_engine.measures.get(),
    );
    c.apply(Op::SetProp {
        id: 4,
        prop: Prop::TestId(Some("docs".into())),
    })
    .unwrap();
    let text = mounted_text(&c.commit().unwrap(), 2);
    assert_eq!(
        text.test_ids,
        [crate::text::SpanTestId {
            start: 4,
            end: 8,
            test_id: "docs".into(),
        }]
    );
    assert_eq!(c.text_engine.prepares.get(), prepares);
    assert_eq!(c.text_engine.layouts.get(), layouts);
    assert_eq!(c.text_engine.measures.get(), measures);
    c.apply(Op::SetProp {
        id: 4,
        prop: Prop::TestId(None),
    })
    .unwrap();
    assert!(mounted_text(&c.commit().unwrap(), 2).test_ids.is_empty());
    assert_eq!(c.text_engine.layouts.get(), layouts);
    c.apply(Op::Listen {
        id: 4,
        event: EventKind::Press,
        enabled: true,
    })
    .unwrap();
    let text = mounted_text(&c.commit().unwrap(), 2);
    assert_eq!(text.links.len(), 1);
    assert_eq!(text.links[0].node, 4);
    assert_eq!(text.links[0].start, 4);
    assert_eq!(text.links[0].end, 8);
}

#[test]
fn screen_push_pop_mount_ordering() {
    let mut c = core();
    // Create stack and screens.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();

    let batch = c.commit().unwrap();
    // Stack and first screen are created and inserted.
    assert!(batch.items.iter().any(|m| matches!(
        m,
        MountItem::Create {
            id: 2,
            kind: ViewKind::Stack
        }
    )));
    assert!(batch.items.iter().any(|m| matches!(
        m,
        MountItem::Create {
            id: 3,
            kind: ViewKind::Screen
        }
    )));

    // Push second screen.
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();

    let batch = c.commit().unwrap();
    // Second screen created and inserted after first.
    assert!(batch.items.iter().any(|m| matches!(
        m,
        MountItem::Create {
            id: 4,
            kind: ViewKind::Screen
        }
    )));

    // Pop second screen (remove from stack).
    c.apply(Op::Remove {
        parent: 2,
        child: 4,
    })
    .unwrap();

    let batch = c.commit().unwrap();
    // Screen removed from parent.
    assert!(
        batch
            .items
            .iter()
            .any(|m| matches!(m, MountItem::Remove { id: 4 }))
    );
}

#[test]
fn header_height_layout_accounting() {
    let mut c = core();
    // Set header heights (normal, large).
    c.set_header_heights(Points(44.0), Points(96.0));

    // Create stack and screen.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();

    // Set screen title to trigger config emission.
    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Screen(crate::ops::ScreenProp::Title(Some("Test".into()))),
    })
    .unwrap();

    let batch = c.commit().unwrap();
    // ScreenConfig is emitted with title.
    let config_item = batch.items.iter().find_map(|m| {
        if let MountItem::ScreenConfig { id, config } = m {
            if *id == 3 { Some(config) } else { None }
        } else {
            None
        }
    });
    assert!(config_item.is_some());
    assert_eq!(config_item.unwrap().title.as_deref(), Some("Test"));

    // Platform reports measured header height.
    c.set_screen_header(3, Some(Points(64.0)));

    // Verify layout is marked as needing update.
    c.commit();
}

#[test]
fn modal_config_emission() {
    let mut c = core();

    // Create modal.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Modal,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();

    // Set modal prop.
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Modal(crate::ops::ModalProp::Animation(
            crate::ops::ModalAnimation::Fade,
        )),
    })
    .unwrap();

    let batch = c.commit().unwrap();

    // ModalConfig is emitted.
    let config_item = batch.items.iter().find_map(|m| {
        if let MountItem::ModalConfig { id, config } = m {
            if *id == 2 { Some(config) } else { None }
        } else {
            None
        }
    });
    assert!(config_item.is_some());
    assert_eq!(
        config_item.unwrap().animation,
        crate::ops::ModalAnimation::Fade
    );
}

#[test]
fn handle_back_with_screen() {
    let mut c = core();

    // Create stack with two screens so the top one can pop.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.commit();

    // Listen to BackRequest on top screen.
    c.apply(Op::Listen {
        id: 4,
        event: EventKind::BackRequest,
        enabled: true,
    })
    .unwrap();
    c.commit();

    // Handle back - should fire event and return true (can pop).
    let consumed = c.handle_back();
    assert!(consumed);

    // Check that BackRequest event was fired.
    let mut events = Vec::new();
    c.take_events(&mut events);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, 4);
    assert_eq!(events[0].1, EventKind::BackRequest);
}

#[test]
fn handle_back_with_prevent() {
    let mut c = core();

    // Create stack with screen that has popOnBack=false.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    // Set popOnBack=false (prevent_back=true).
    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Screen(crate::ops::ScreenProp::PopOnBack(false)),
    })
    .unwrap();
    c.commit();

    // Handle back - should return false (cannot pop).
    let consumed = c.handle_back();
    assert!(!consumed);
}

#[test]
fn root_screen_cannot_pop() {
    let mut c = core();

    // Create stack with single screen (root screen).
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.commit();

    // Root screen should not consume back.
    let consumed = c.handle_back();
    assert!(!consumed, "root screen should not pop");

    // But onBackPress should still fire if listening.
    c.apply(Op::Listen {
        id: 3,
        event: EventKind::BackRequest,
        enabled: true,
    })
    .unwrap();
    c.commit();

    let consumed = c.handle_back();
    assert!(!consumed, "root screen should not pop even with listener");

    let mut events = Vec::new();
    c.take_events(&mut events);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, 3);
    assert_eq!(events[0].1, EventKind::BackRequest);
}

#[test]
fn second_screen_pops() {
    let mut c = core();

    // Create stack with two screens.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.commit();

    // Second screen should pop.
    let consumed = c.handle_back();
    assert!(consumed, "second screen should pop");
}

#[test]
fn modal_priority_over_screen() {
    let mut c = core();

    // Create stack with screen and a modal.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 5,
        tag: Tag::Modal,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 5,
        before: None,
    })
    .unwrap();
    c.commit();

    // Modal should handle back, not the screen.
    c.apply(Op::Listen {
        id: 5,
        event: EventKind::BackRequest,
        enabled: true,
    })
    .unwrap();
    c.commit();

    let consumed = c.handle_back();
    assert!(consumed, "modal should handle back");

    let mut events = Vec::new();
    c.take_events(&mut events);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, 5, "modal should receive event, not screen");
}

#[test]
fn modal_close_on_escape_false() {
    let mut c = core();

    // Create modal with closeOnEscape=false.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Modal,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Modal(crate::ops::ModalProp::CloseOnEscape(false)),
    })
    .unwrap();
    c.commit();

    // Should still consume back (not fall through to platform default).
    let consumed = c.handle_back();
    assert!(
        consumed,
        "modal with closeOnEscape=false should still consume back"
    );
}

#[test]
fn back_event_fires_even_when_prevented() {
    let mut c = core();

    // Create stack with screen that prevents back.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 4,
        prop: Prop::Screen(crate::ops::ScreenProp::PopOnBack(false)),
    })
    .unwrap();
    c.apply(Op::Listen {
        id: 4,
        event: EventKind::BackRequest,
        enabled: true,
    })
    .unwrap();
    c.commit();

    // The stack owns back while it has a non-root screen, even if that
    // screen prevents automatic popping.
    let consumed = c.handle_back();
    assert!(
        consumed,
        "prevented back must not fall through to BackHandler"
    );

    let mut events = Vec::new();
    c.take_events(&mut events);
    assert_eq!(
        events.len(),
        1,
        "onBackPress should fire even when prevented"
    );
    assert_eq!(events[0].0, 4);
    assert_eq!(events[0].1, EventKind::BackRequest);
}

#[test]
fn screen_layout_in_stack() {
    let mut c = Core::new(FakeEngine, size_pt(400.0, 900.0));

    // Create a stack at 390x800.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::SetStyle {
        id: 2,
        style: Box::new(Style {
            layout: taffy::Style {
                size: taffy::Size {
                    width: Dimension::length(390.0),
                    height: Dimension::length(800.0),
                },
                ..Default::default()
            },
            ..Default::default()
        }),
        patch: StylePatch::default(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();

    // Create two screens.
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.commit();

    // Both screens should be laid out at 390x800.
    let frame3 = c.nodes.iter().find(|(_, n)| n.id == 3).unwrap().1.frame;
    let frame4 = c.nodes.iter().find(|(_, n)| n.id == 4).unwrap().1.frame;
    assert_eq!(frame3.size.width.0, 390.0);
    assert_eq!(frame3.size.height.0, 800.0);
    assert_eq!(frame4.size.width.0, 390.0);
    assert_eq!(frame4.size.height.0, 800.0);

    // Frames should have been emitted.
    let batch = c.commit();
    assert!(batch.is_none(), "no changes, no batch");
}

#[test]
fn header_item_layout_and_config() {
    let mut c = Core::new(FakeEngine, size_pt(400.0, 800.0));
    c.set_header_heights(Points::ZERO, Points::ZERO);

    // Create stack and screen.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::SetStyle {
        id: 2,
        style: Box::new(Style {
            layout: taffy::Style {
                size: taffy::Size {
                    width: Dimension::length(400.0),
                    height: Dimension::length(800.0),
                },
                ..Default::default()
            },
            ..Default::default()
        }),
        patch: StylePatch::default(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();

    // Create header items.
    c.apply(Op::Create {
        id: 4,
        tag: Tag::HeaderItem,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 4,
        prop: Prop::HeaderSlot(crate::ops::HeaderSlot::Header),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 5,
        tag: Tag::HeaderItem,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 5,
        prop: Prop::HeaderSlot(crate::ops::HeaderSlot::Left),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 5,
        before: None,
    })
    .unwrap();

    // Add content to header to give it height.
    c.apply(Op::Create {
        id: 6,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::SetStyle {
        id: 6,
        style: Box::new(Style {
            layout: taffy::Style {
                size: taffy::Size {
                    width: Dimension::length(100.0),
                    height: Dimension::length(50.0),
                },
                ..Default::default()
            },
            ..Default::default()
        }),
        patch: StylePatch::default(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 4,
        child: 6,
        before: None,
    })
    .unwrap();

    let batch = c.commit().expect("changes");

    // Header should be laid out (has non-zero dimensions).
    let header_frame = c.nodes.iter().find(|(_, n)| n.id == 4).unwrap().1.frame;
    assert!(header_frame.size.width.0 > 0.0, "header should have width");
    assert!(
        header_frame.size.height.0 > 0.0,
        "header should have height"
    );

    // ScreenConfig should include header_height and slots.
    let config_item = batch
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            MountItem::ScreenConfig { id, config } if *id == 3 => Some(config),
            _ => None,
        })
        .expect("ScreenConfig emitted");
    assert!(
        config_item.header_height.0 > 0.0,
        "config should have header height"
    );
    assert_eq!(config_item.header_view, Some(4));
    assert_eq!(config_item.left, Some(5));
}

#[test]
fn content_after_a_header_item_lays_out() {
    let mut c = Core::new(FakeEngine, size_pt(400.0, 800.0));
    c.set_header_heights(Points::ZERO, Points::ZERO);
    for (id, tag) in [(2, Tag::Stack), (3, Tag::Screen), (4, Tag::HeaderItem)] {
        c.apply(Op::Create { id, tag }).unwrap();
    }
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 4,
        prop: Prop::HeaderSlot(crate::ops::HeaderSlot::Header),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 4,
        before: None,
    })
    .unwrap();

    // The header item is a layout root of its own: the content after it
    // is the screen layout's first child.
    c.apply(Op::Create {
        id: 5,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 5,
        before: None,
    })
    .unwrap();
    c.commit().expect("the screen lays out");

    let view = c.nodes.iter().find(|(_, node)| node.id == 5).unwrap().1;
    assert_eq!(view.frame.size.width.0, 400.0);
}

#[test]
fn header_slot_replacement_updates_screen_config() {
    let mut c = Core::new(FakeEngine, size_pt(400.0, 800.0));
    c.set_header_heights(Points::ZERO, Points::ZERO);
    for (id, tag) in [(2, Tag::Stack), (3, Tag::Screen)] {
        c.apply(Op::Create { id, tag }).unwrap();
    }
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    fn insert_header(c: &mut Core<FakeEngine>, id: NodeId) -> Option<NodeId> {
        c.apply(Op::Create {
            id,
            tag: Tag::HeaderItem,
        })
        .unwrap();
        c.apply(Op::SetProp {
            id,
            prop: Prop::HeaderSlot(crate::ops::HeaderSlot::Header),
        })
        .unwrap();
        c.apply(Op::Insert {
            parent: 3,
            child: id,
            before: None,
        })
        .unwrap();
        c.commit()
            .and_then(|batch| {
                batch.items.iter().rev().find_map(|item| match item {
                    MountItem::ScreenConfig { id, config } if *id == 3 => Some(config.header_view),
                    _ => None,
                })
            })
            .expect("a screen config")
    }
    assert_eq!(insert_header(&mut c, 4), Some(4));
    c.apply(Op::Remove {
        parent: 3,
        child: 4,
    })
    .unwrap();
    assert_eq!(insert_header(&mut c, 5), Some(5));
}

#[test]
fn native_header_moves_content() {
    let mut c = Core::new(FakeEngine, size_pt(400.0, 800.0));
    c.set_header_heights(Points(64.0), Points(96.0));

    // Create stack and screen.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::SetStyle {
        id: 2,
        style: Box::new(Style {
            layout: taffy::Style {
                size: taffy::Size {
                    width: Dimension::length(400.0),
                    height: Dimension::length(800.0),
                },
                ..Default::default()
            },
            ..Default::default()
        }),
        patch: StylePatch::default(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();

    // Add a child view.
    c.apply(Op::Create {
        id: 4,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::SetStyle {
        id: 4,
        style: Box::new(Style {
            layout: taffy::Style {
                size: taffy::Size {
                    width: Dimension::length(100.0),
                    height: Dimension::length(100.0),
                },
                ..Default::default()
            },
            ..Default::default()
        }),
        patch: StylePatch::default(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 4,
        before: None,
    })
    .unwrap();

    c.commit();

    // Content should start at y=64 (native header height).
    let child_frame = c.nodes.iter().find(|(_, n)| n.id == 4).unwrap().1.frame;
    assert_eq!(child_frame.origin.y.0, 64.0);

    // With headerTransparent, content should start at y=0.
    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Screen(crate::ops::ScreenProp::HeaderTransparent(true)),
    })
    .unwrap();
    c.commit();

    let child_frame = c.nodes.iter().find(|(_, n)| n.id == 4).unwrap().1.frame;
    assert_eq!(child_frame.origin.y.0, 0.0);
}
#[test]
fn transparent_toggle_keeps_inset_padded_content_put() {
    let mut c = Core::new(FakeEngine, size_pt(400.0, 800.0));
    c.set_header_heights(Points(56.0), Points(56.0));
    c.set_safe_area(Edges {
        top: px(24.0),
        ..Edges::default()
    });
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    apply_style(&mut c, 2, sized(400.0, 800.0));
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::View,
    })
    .unwrap();
    let mut pad = Style::default();
    pad.set(
        "paddingTop",
        StyleValue::Str("calc(env(safe-area-inset-top) + 16px)"),
    )
    .unwrap();
    apply_style(&mut c, 4, pad);
    c.apply(Op::Create {
        id: 5,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 5, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: 4,
        child: 5,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 4,
        before: None,
    })
    .unwrap();
    // Opaque: the bar covers the top, and the platform measured it.
    c.set_screen_header(3, Some(Points(80.0)));
    c.set_root_insets(
        3,
        Edges {
            top: px(0.0),
            ..Edges::default()
        },
    );
    c.commit();
    let place = |c: &Core<FakeEngine>| {
        let y = |id: NodeId| {
            c.nodes
                .iter()
                .find(|(_, n)| n.id == id)
                .unwrap()
                .1
                .frame
                .origin
                .y
                .0
        };
        y(4) + y(5)
    };
    assert_eq!(place(&c), 96.0, "opaque: the bar plus 16pt of padding");

    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Screen(crate::ops::ScreenProp::HeaderTransparent(true)),
    })
    .unwrap();
    c.commit();
    assert_eq!(
        place(&c),
        96.0,
        "the inset takes the bar's place before the platform reports"
    );

    // A transparent bar used to report height 0, which wiped the measurement.
    c.set_screen_header(3, Some(Points(0.0)));
    c.set_root_insets(
        3,
        Edges {
            top: px(80.0),
            ..Edges::default()
        },
    );
    c.commit();
    assert_eq!(place(&c), 96.0);

    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Screen(crate::ops::ScreenProp::HeaderTransparent(false)),
    })
    .unwrap();
    c.commit();
    assert_eq!(
        place(&c),
        96.0,
        "a zero measurement must not drop the content"
    );
}

#[test]
fn drawn_header_transparent_lets_content_start_at_top() {
    let mut c = Core::new(FakeEngine, size_pt(400.0, 800.0));
    c.set_header_heights(Points::ZERO, Points::ZERO);
    for (id, tag) in [(2, Tag::Stack), (3, Tag::Screen), (4, Tag::HeaderItem)] {
        c.apply(Op::Create { id, tag }).unwrap();
    }
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 4,
        prop: Prop::HeaderSlot(crate::ops::HeaderSlot::Header),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 5,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::SetStyle {
        id: 5,
        style: Box::new(Style {
            layout: taffy::Style {
                size: taffy::Size {
                    width: Dimension::length(100.0),
                    height: Dimension::length(50.0),
                },
                ..Default::default()
            },
            ..Default::default()
        }),
        patch: StylePatch::default(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 4,
        child: 5,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 6,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 6,
        before: None,
    })
    .unwrap();
    c.commit().expect("the screen lays out");

    let content = c.nodes.iter().find(|(_, n)| n.id == 6).unwrap().1.frame;
    let header = c.nodes.iter().find(|(_, n)| n.id == 4).unwrap().1.frame;
    assert_eq!(header.size.height.0, 50.0);
    assert_eq!(content.origin.y.0, 50.0);

    c.apply(Op::SetProp {
        id: 3,
        prop: Prop::Screen(crate::ops::ScreenProp::HeaderTransparent(true)),
    })
    .unwrap();
    c.commit().expect("transparent relayout");

    let content = c.nodes.iter().find(|(_, n)| n.id == 6).unwrap().1.frame;
    let header = c.nodes.iter().find(|(_, n)| n.id == 4).unwrap().1.frame;
    assert_eq!(header.size.height.0, 50.0, "the bar still lays out");
    assert_eq!(content.origin.y.0, 0.0);
}

#[test]
fn modal_layout_at_reported_size() {
    let mut c = core();

    c.apply(Op::Create {
        id: 2,
        tag: Tag::Modal,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();

    // Set modal size.
    c.set_root_size(2, size_pt(350.0, 600.0), Edges::default());
    c.commit();

    // Modal should be laid out at that size.
    let frame = c.nodes.iter().find(|(_, n)| n.id == 2).unwrap().1.frame;
    assert_eq!(frame.size.width.0, 350.0);
    assert_eq!(frame.size.height.0, 600.0);
}

#[test]
fn screen_and_modal_config_emission() {
    let mut c = core();

    // Create screen.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::SetStyle {
        id: 2,
        style: Box::new(Style {
            layout: taffy::Style {
                size: taffy::Size {
                    width: Dimension::length(400.0),
                    height: Dimension::length(800.0),
                },
                ..Default::default()
            },
            ..Default::default()
        }),
        patch: StylePatch::default(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();

    // Create modal.
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Modal,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 4,
        before: None,
    })
    .unwrap();

    let batch = c.commit().expect("changes");

    // ScreenConfig should be emitted.
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::ScreenConfig { id, .. } if *id == 3
    )));

    // ModalConfig should be emitted.
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::ModalConfig { id, .. } if *id == 4
    )));
}

#[test]
fn topmost_modal_wins_back() {
    let mut c = core();

    // Create two modals.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Modal,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Modal,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 3,
        before: None,
    })
    .unwrap();
    c.commit();

    // Listen on both.
    c.apply(Op::Listen {
        id: 2,
        event: EventKind::BackRequest,
        enabled: true,
    })
    .unwrap();
    c.apply(Op::Listen {
        id: 3,
        event: EventKind::BackRequest,
        enabled: true,
    })
    .unwrap();
    c.commit();

    // The last created modal (3) should handle back.
    c.handle_back();
    let mut events = Vec::new();
    c.take_events(&mut events);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, 3, "topmost modal should receive back");
}

#[test]
fn modal_back_always_consumed() {
    let mut c = core();

    // Test closeOnEscape=true: consumes back and emits RequestClose.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Modal,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Modal(crate::ops::ModalProp::CloseOnEscape(true)),
    })
    .unwrap();
    c.apply(Op::Listen {
        id: 2,
        event: EventKind::RequestClose,
        enabled: true,
    })
    .unwrap();
    c.commit();

    let consumed = c.handle_back();
    assert!(
        consumed,
        "modal with closeOnEscape=true should consume back"
    );
    let mut events = Vec::new();
    c.take_events(&mut events);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, 2);
    assert_eq!(events[0].1, EventKind::RequestClose);

    // Test closeOnEscape=false: still consumes back and emits RequestClose.
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Modal(crate::ops::ModalProp::CloseOnEscape(false)),
    })
    .unwrap();
    c.commit();

    let consumed = c.handle_back();
    assert!(
        consumed,
        "modal with closeOnEscape=false should still consume back (not fall through)"
    );
    let mut events = Vec::new();
    c.take_events(&mut events);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, 2);
    assert_eq!(events[0].1, EventKind::RequestClose);
}

#[test]
fn removed_modal_does_not_keep_back() {
    let mut c = core();

    // A page that can pop, with a modal on it. The modal is removed the
    // way the web runtime drops one (unlink now, release when the JS
    // object is gone). Back after that has to reach the page.
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Stack,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Screen,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 5,
        tag: Tag::Modal,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 4,
        child: 5,
        before: None,
    })
    .unwrap();
    c.apply(Op::Listen {
        id: 5,
        event: EventKind::RequestClose,
        enabled: true,
    })
    .unwrap();
    c.apply(Op::Listen {
        id: 4,
        event: EventKind::Dismiss,
        enabled: true,
    })
    .unwrap();
    c.commit();

    assert!(c.handle_back(), "an open modal takes back");
    let mut events = Vec::new();
    c.take_events(&mut events);
    assert_eq!(events, vec![(5, EventKind::RequestClose)]);

    c.apply(Op::Remove {
        parent: 4,
        child: 5,
    })
    .unwrap();
    c.commit();

    assert!(c.handle_back(), "the page under a removed modal still pops");
    events.clear();
    c.take_events(&mut events);
    assert_eq!(
        events,
        vec![(4, EventKind::Dismiss)],
        "a removed modal must not swallow back"
    );
}

#[test]
fn gesture_bindings_emit_mount_items() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();

    // Pan with both axes
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Pan {
            x: Some(10),
            y: Some(11),
        },
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(
        batch.items.iter().any(|item| matches!(
            item,
            MountItem::PanBinding {
                id: 2,
                x: Some(10),
                y: Some(11)
            }
        )),
        "PanBinding should be emitted with both axes"
    );
    // Bindings applied before the view was made still come after it
    // (a platform ignores them for a view it does not have yet).
    let made = batch
        .items
        .iter()
        .position(|item| matches!(item, MountItem::Create { id: 2, .. }))
        .expect("the view is made");
    let bound = batch
        .items
        .iter()
        .position(|item| matches!(item, MountItem::PanBinding { id: 2, .. }))
        .expect("the pan is bound");
    assert!(made < bound, "the view is made before its pan is bound");

    // Pan with only x
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Pan {
            x: Some(12),
            y: None,
        },
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(
        batch.items.iter().any(|item| matches!(
            item,
            MountItem::PanBinding {
                id: 2,
                x: Some(12),
                y: None
            }
        )),
        "PanBinding should emit with only x"
    );

    // Clear pan
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Pan { x: None, y: None },
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(
        batch.items.iter().any(|item| matches!(
            item,
            MountItem::PanBinding {
                id: 2,
                x: None,
                y: None
            }
        )),
        "PanBinding should clear both axes"
    );

    // Pinch
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Pinch { scale: Some(20) },
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(
        batch.items.iter().any(|item| matches!(
            item,
            MountItem::PinchBinding {
                id: 2,
                scale: Some(20)
            }
        )),
        "PinchBinding should be emitted"
    );

    // Rotation
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Rotation { angle: Some(30) },
    })
    .unwrap();
    let batch = c.commit().unwrap();
    assert!(
        batch.items.iter().any(|item| matches!(
            item,
            MountItem::RotationBinding {
                id: 2,
                angle: Some(30)
            }
        )),
        "RotationBinding should be emitted"
    );

    // All three coexisting
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Pan {
            x: Some(40),
            y: Some(41),
        },
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Pinch { scale: Some(42) },
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Rotation { angle: Some(43) },
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let has_pan = batch.items.iter().any(|item| {
        matches!(
            item,
            MountItem::PanBinding {
                id: 2,
                x: Some(40),
                y: Some(41)
            }
        )
    });
    let has_pinch = batch.items.iter().any(|item| {
        matches!(
            item,
            MountItem::PinchBinding {
                id: 2,
                scale: Some(42)
            }
        )
    });
    let has_rotation = batch.items.iter().any(|item| {
        matches!(
            item,
            MountItem::RotationBinding {
                id: 2,
                angle: Some(43)
            }
        )
    });
    assert!(
        has_pan && has_pinch && has_rotation,
        "all three gesture bindings should coexist"
    );
}

#[test]
fn gesture_event_kinds_have_correct_discriminants() {
    assert_eq!(EventKind::PanStart as u8, 17);
    assert_eq!(EventKind::PanEnd as u8, 18);
    assert_eq!(EventKind::PinchStart as u8, 19);
    assert_eq!(EventKind::PinchEnd as u8, 20);
    assert_eq!(EventKind::RotationStart as u8, 21);
    assert_eq!(EventKind::RotationEnd as u8, 22);
}

#[test]
fn refresh_event_fires_once_per_cycle() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::RefreshIndicator(Some(Points(50.0))),
    })
    .unwrap();
    c.apply(Op::Listen {
        id: 2,
        event: EventKind::Refresh,
        enabled: true,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.commit().unwrap();

    // Pull down beyond threshold
    c.update_scroll_position(2, Point::new(Points(0.0), Points(-60.0)));
    let mut events = vec![];
    c.take_events(&mut events);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, 2);
    assert_eq!(events[0].1, EventKind::Refresh);

    // Pull again - should not fire again
    c.update_scroll_position(2, Point::new(Points(0.0), Points(-70.0)));
    events.clear();
    c.take_events(&mut events);
    assert_eq!(
        events.len(),
        0,
        "refresh should not fire twice in same cycle"
    );

    // Set refreshing to true then false - resets the cycle
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Refreshing(true),
    })
    .unwrap();
    c.commit().unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Refreshing(false),
    })
    .unwrap();
    c.commit().unwrap();

    // Now pull again - should fire
    c.update_scroll_position(2, Point::new(Points(0.0), Points(-60.0)));
    events.clear();
    c.take_events(&mut events);
    assert_eq!(
        events.len(),
        1,
        "refresh should fire again after cycle reset"
    );
    assert_eq!(events[0].1, EventKind::Refresh);
}

#[test]
fn refresh_does_not_fire_below_threshold() {
    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::RefreshIndicator(Some(Points(50.0))),
    })
    .unwrap();
    c.apply(Op::Listen {
        id: 2,
        event: EventKind::Refresh,
        enabled: true,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.commit().unwrap();

    // Pull down but not beyond threshold
    c.update_scroll_position(2, Point::new(Points(0.0), Points(-40.0)));
    let mut events = vec![];
    c.take_events(&mut events);
    assert_eq!(events.len(), 0, "refresh should not fire below threshold");
}

#[test]
fn pull_value_updates_with_pull_distance() {
    use crate::animation::AnimationOp;

    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::RefreshIndicator(Some(Points(50.0))),
    })
    .unwrap();
    // Bind pullValue to animation node 100
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::PullValue(Some(100)),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.commit().unwrap();

    // Pull down 30pt
    c.update_scroll_position(2, Point::new(Points(0.0), Points(-30.0)));
    let batch = c.commit().unwrap();
    let anim_set = batch.items.iter().find_map(|item| match item {
        MountItem::Animation(AnimationOp::Set { node, value }) if *node == 100 => Some(*value),
        _ => None,
    });
    assert_eq!(anim_set, Some(30.0), "pullValue should be 30.0");

    // Pull down 70pt
    c.update_scroll_position(2, Point::new(Points(0.0), Points(-70.0)));
    let batch = c.commit().unwrap();
    let anim_set = batch.items.iter().find_map(|item| match item {
        MountItem::Animation(AnimationOp::Set { node, value }) if *node == 100 => Some(*value),
        _ => None,
    });
    assert_eq!(anim_set, Some(70.0), "pullValue should be 70.0");

    // Release (scroll back to 0)
    c.update_scroll_position(2, Point::new(Points(0.0), Points(0.0)));
    let batch = c.commit().unwrap();
    let anim_set = batch.items.iter().find_map(|item| match item {
        MountItem::Animation(AnimationOp::Set { node, value }) if *node == 100 => Some(*value),
        _ => None,
    });
    assert_eq!(anim_set, Some(0.0), "pullValue should be 0.0 on release");
}

#[test]
fn pull_value_clamps_to_indicator_height_when_refreshing() {
    use crate::animation::AnimationOp;

    let mut c = core();
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Scroll,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::RefreshIndicator(Some(Points(50.0))),
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::PullValue(Some(100)),
    })
    .unwrap();
    c.apply(Op::Listen {
        id: 2,
        event: EventKind::Refresh,
        enabled: true,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.commit().unwrap();

    // Pull down 60pt to trigger refresh
    c.update_scroll_position(2, Point::new(Points(0.0), Points(-60.0)));
    let mut events = vec![];
    c.take_events(&mut events);
    assert_eq!(events.len(), 1, "first refresh should fire");
    c.commit().unwrap();

    // Set refreshing to true
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Refreshing(true),
    })
    .unwrap();
    c.commit().unwrap();

    // Pull further while refreshing - should clamp to 50.0
    c.update_scroll_position(2, Point::new(Points(0.0), Points(-80.0)));
    let batch = c.commit().unwrap();
    let anim_set = batch.items.iter().find_map(|item| match item {
        MountItem::Animation(AnimationOp::Set { node, value }) if *node == 100 => Some(*value),
        _ => None,
    });
    assert_eq!(
        anim_set,
        Some(50.0),
        "pullValue should clamp to indicator height (50.0) when refreshing"
    );

    // Set refreshing to false - should reset refresh_reported
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Refreshing(false),
    })
    .unwrap();
    c.commit().unwrap();

    // Pull again - should trigger refresh again
    c.update_scroll_position(2, Point::new(Points(0.0), Points(-60.0)));
    let mut events = vec![];
    c.take_events(&mut events);
    assert_eq!(
        events.len(),
        1,
        "refresh should fire again after refreshing becomes false"
    );
}
#[test]
fn user_select_resolves_from_views_and_defaults_off() {
    let mut c = Core::new(ApproxTextEngine, size(px(200.0), px(200.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::View,
    })
    .unwrap();
    c.apply(Op::Create {
        id: 3,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 4,
        text: "read me".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 3,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().expect("a frame");
    assert_eq!(mounted_text(&batch, 3).selectable, None, "auto is off");

    let mut style = Style::default();
    style.set("userSelect", StyleValue::Str("text")).unwrap();
    apply_style(&mut c, 2, style);
    let batch = c.commit().expect("a frame");
    assert_eq!(mounted_text(&batch, 3).selectable, Some(true));

    let mut off = Style::default();
    off.set("userSelect", StyleValue::Str("none")).unwrap();
    apply_style(&mut c, 3, off);
    let batch = c.commit().expect("a frame");
    assert_eq!(mounted_text(&batch, 3).selectable, Some(false));

    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Selectable(true),
    })
    .unwrap();
    let batch = c.commit().expect("a frame");
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Selectable {
            id: 2,
            selectable: true
        }
    )));
}

#[test]
fn an_inline_view_is_measured_and_placed_on_the_baseline() {
    let mut c = Core::new(ApproxTextEngine, size(px(100.0), px(200.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "ab".into(),
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 4, sized(18.0, 18.0));
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().expect("a frame");
    let text = mounted_text(&batch, 2);
    assert_eq!(text.text, "ab\u{FFFC}");
    assert_eq!(text.attachments.len(), 1);
    assert_eq!(
        text.attachments[0].width,
        Points(18.0),
        "its own content size"
    );
    let baseline = frame(&batch, 4);
    assert_eq!(baseline.size, size(px(18.0), px(18.0)));
    assert_eq!(baseline.origin.x, Points(px(2.0).0 * 14.0 * 0.55));
    assert_eq!(baseline.origin.y, Points(0.0));

    let mut middle = sized(18.0, 18.0);
    middle
        .set("verticalAlign", StyleValue::Str("middle"))
        .unwrap();
    apply_style(&mut c, 4, middle);
    let batch = c.commit().expect("a frame");
    assert!(
        frame(&batch, 4).origin.y > baseline.origin.y,
        "middle sits lower than the baseline"
    );
}

#[test]
fn a_later_layout_keeps_an_inline_view_on_its_placeholder() {
    let mut c = Core::new(ApproxTextEngine, size(px(100.0), px(200.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "ab".into(),
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 4, sized(18.0, 18.0));
    c.apply(Op::Create {
        id: 5,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 5, sized(10.0, 10.0));
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 5,
        before: None,
    })
    .unwrap();
    let placed = frame(&c.commit().expect("a frame"), 4);
    assert!(placed.origin.x > Points::ZERO, "{placed:?}");
    apply_style(&mut c, 5, sized(10.0, 40.0));
    let batch = c.commit().expect("the sibling's layout");
    let again = batch.items.iter().rev().find_map(|item| match item {
        MountItem::Frame { id: 4, frame } => Some(*frame),
        _ => None,
    });
    if let Some(again) = again {
        assert_eq!(
            again.origin, placed.origin,
            "a later layout put the attachment back at the text's origin: {again:?}"
        );
    }
}

#[test]
fn attachments_cut_off_by_number_of_lines_are_out_of_sight() {
    let mut c = Core::new(ApproxTextEngine, size(px(100.0), px(200.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "a".repeat(20),
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::View,
    })
    .unwrap();
    apply_style(&mut c, 4, sized(18.0, 18.0));
    let mut one_line = Style::default();
    one_line
        .set("numberOfLines", StyleValue::Number(1.0))
        .unwrap();
    apply_style(&mut c, 2, one_line);
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().expect("a frame");
    assert_eq!(mounted_text(&batch, 2).max_lines, 1);
    assert!(
        frame(&batch, 4).origin.x < Points(-99_000.0),
        "the cut-off attachment has no place"
    );
}

#[test]
fn a_short_press_opens_a_checked_href() {
    let mut c = Core::new(ApproxTextEngine, size(px(200.0), px(200.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "Terms".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Href(Some("https://example.com/terms".into())),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.commit().expect("a frame");
    assert_eq!(c.press(2).as_deref(), Some("https://example.com/terms"));
    let batch = c.commit().expect("the open command");
    assert!(batch.items.iter().any(|item| matches!(
        item,
        MountItem::Command {
            command: Command::OpenUrl { url },
            ..
        } if url == "https://example.com/terms"
    )));

    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Href(Some("javascript:alert(1)".into())),
    })
    .unwrap();
    c.commit().expect("a frame");
    assert_eq!(c.press(2), None);
    let batch = c.commit();
    assert!(
        batch.is_none_or(|batch| !batch.items.iter().any(|item| matches!(
            item,
            MountItem::Command {
                command: Command::OpenUrl { .. },
                ..
            }
        ))),
        "a refused address opens nothing"
    );
}
#[test]
fn the_href_at_a_point_is_the_innermost() {
    let mut c = Core::new(ApproxTextEngine, size(px(200.0), px(200.0)));
    c.apply(Op::Create {
        id: 2,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 3,
        text: "pre ".into(),
    })
    .unwrap();
    c.apply(Op::Create {
        id: 4,
        tag: Tag::Text,
    })
    .unwrap();
    c.apply(Op::CreateText {
        id: 5,
        text: "terms".into(),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 4,
        child: 5,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Href(Some("https://example.com/block".into())),
    })
    .unwrap();
    c.apply(Op::SetProp {
        id: 4,
        prop: Prop::Href(Some("https://example.com/span".into())),
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    c.commit().expect("a frame");
    let point = |x: f32| Point::new(Points(x), Points(1.0));
    assert_eq!(
        c.press_at(2, point(1.0)).as_deref(),
        Some("https://example.com/block")
    );
    c.commit();
    assert_eq!(
        c.press_at(2, point(32.0)).as_deref(),
        Some("https://example.com/span")
    );
}
use crate::tailwind::state;

fn view(c: &mut Core<FakeEngine>, id: NodeId) {
    c.apply(Op::Create { id, tag: Tag::View }).unwrap();
}

fn set_class(c: &mut Core<FakeEngine>, id: NodeId, class: &str) {
    c.apply(Op::SetProp {
        id,
        prop: Prop::Class(class.into()),
    })
    .unwrap();
}

#[test]
fn pointer_events_none_with_auto_inside_mounts_box_none() {
    let mut c = core();
    view(&mut c, 2);
    view(&mut c, 3);
    view(&mut c, 4);
    set_class(&mut c, 2, "pointer-events-none");
    set_class(&mut c, 4, "pointer-events-auto");
    for (parent, child) in [(2, 3), (3, 4), (ROOT_ID, 2)] {
        c.apply(Op::Insert {
            parent,
            child,
            before: None,
        })
        .unwrap();
    }
    assert_eq!(c.mounted_pointer(c.key(2).unwrap()), PointerEvents::BoxNone);
    assert_eq!(c.mounted_pointer(c.key(3).unwrap()), PointerEvents::BoxNone);
    assert_eq!(c.mounted_pointer(c.key(4).unwrap()), PointerEvents::Auto);
    set_class(&mut c, 4, "");
    assert_eq!(c.mounted_pointer(c.key(2).unwrap()), PointerEvents::None);
}

fn node(c: &Core<FakeEngine>, id: NodeId) -> &Node {
    &c.nodes[c.key(id).unwrap() as usize]
}
fn layout_of(c: &Core<FakeEngine>, id: NodeId) -> &taffy::Style {
    let layout = node(c, id).layout.expect("view has a layout node");
    c.layout.style(layout)
}

fn length(value: taffy::LengthPercentage) -> f32 {
    match value.expand() {
        taffy::style::ExpandedLengthPercentage::Length(v) => v,
        other => panic!("expected a length, got {other:?}"),
    }
}

fn margin_length(value: taffy::LengthPercentageAuto) -> f32 {
    match value.expand() {
        taffy::style::ExpandedLengthPercentageAuto::Length(v) => v,
        other => panic!("expected a length, got {other:?}"),
    }
}

/// Padding top, right, bottom, left.
fn padding(c: &Core<FakeEngine>, id: NodeId) -> [f32; 4] {
    let p = layout_of(c, id).padding;
    [
        length(p.top),
        length(p.right),
        length(p.bottom),
        length(p.left),
    ]
}

#[test]
fn later_class_wins_and_style_replaces_one_side() {
    let mut c = core();
    view(&mut c, 2);
    set_class(&mut c, 2, "p-4");
    assert_eq!(padding(&c, 2), [16.0, 16.0, 16.0, 16.0]);
    set_class(&mut c, 2, "p-2 p-4");
    assert_eq!(padding(&c, 2), [16.0, 16.0, 16.0, 16.0]);

    set_class(&mut c, 2, "pl-4 pr-4");
    let mut style = Style::default();
    let mut patch = StylePatch::default();
    patch
        .set(&mut style, "paddingLeft", StyleValue::Number(8.0))
        .unwrap();
    c.apply(Op::SetStyle {
        id: 2,
        style: Box::new(style),
        patch,
    })
    .unwrap();
    assert_eq!(padding(&c, 2), [0.0, 16.0, 0.0, 8.0]);

    // Clearing the class restores the style prop, not the class padding.
    set_class(&mut c, 2, "");
    assert_eq!(padding(&c, 2), [0.0, 0.0, 0.0, 8.0]);
}

#[test]
fn hover_changes_padding_only_while_hovered() {
    let mut c = core();
    view(&mut c, 2);
    set_class(&mut c, 2, "hover:p-4");
    assert_eq!(padding(&c, 2), [0.0, 0.0, 0.0, 0.0]);
    c.set_states(2, state::HOVERED);
    assert_eq!(padding(&c, 2), [16.0, 16.0, 16.0, 16.0]);
    c.set_states(2, 0);
    assert_eq!(padding(&c, 2), [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn hover_class_tracks_hover_until_the_class_is_cleared() {
    let mut c = core();
    view(&mut c, 2);
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let created = c.commit().unwrap();
    assert!(
        !created
            .items
            .iter()
            .any(|item| matches!(item, MountItem::TrackStates { .. })),
        "a view with no classes does not ask for states"
    );

    set_class(&mut c, 2, "hover:p-4");
    let batch = c.commit().unwrap();
    let states = track_states(&batch, 2);
    assert_eq!(states & state::HOVERED, state::HOVERED);
    assert_eq!(states & state::PRESSED, 0);

    set_class(&mut c, 2, "");
    let batch = c.commit().unwrap();
    assert_eq!(track_states(&batch, 2), 0);

    // The mask stays 0, so `p-4` does not ask again.
    set_class(&mut c, 2, "p-4");
    if let Some(batch) = c.commit() {
        assert!(
            !batch.items.iter().any(|item| matches!(
                item,
                MountItem::TrackStates { id: 2, states } if *states != 0
            )),
            "{:?}",
            batch.items
        );
    }

    // Classes already set when the native view is created.
    view(&mut c, 3);
    set_class(&mut c, 3, "hover:p-4");
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 3,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let states = track_states(&batch, 3);
    assert_eq!(states & state::HOVERED, state::HOVERED);
    assert_eq!(states & state::PRESSED, 0);
}

fn track_states(batch: &MountBatch, id: NodeId) -> u16 {
    batch
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            MountItem::TrackStates { id: view, states } if *view == id => Some(*states),
            _ => None,
        })
        .expect("TrackStates")
}

#[test]
fn disabled_pressed_skips_active_and_applies_disabled() {
    let mut c = core();
    view(&mut c, 2);
    set_class(&mut c, 2, "active:p-8 disabled:opacity-50");
    c.set_states(2, state::PRESSED);
    assert_eq!(padding(&c, 2), [32.0, 32.0, 32.0, 32.0]);
    assert_eq!(node(&c, 2).view_style.opacity, 1.0);
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::Disabled(true),
    })
    .unwrap();
    assert_eq!(padding(&c, 2), [0.0, 0.0, 0.0, 0.0]);
    assert_eq!(node(&c, 2).view_style.opacity, 0.5);
    // A press reported while disabled does not bring `active:` back.
    c.set_states(2, state::PRESSED);
    assert_eq!(node(&c, 2).states & state::PRESSED, 0);
    assert_eq!(padding(&c, 2), [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn class_list_applies_after_class_and_a_later_list_drops_it() {
    let mut c = core();
    view(&mut c, 2);
    set_class(&mut c, 2, "p-2");
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::ClassList(vec!["p-4".into()]),
    })
    .unwrap();
    assert_eq!(padding(&c, 2), [16.0, 16.0, 16.0, 16.0]);
    c.apply(Op::SetProp {
        id: 2,
        prop: Prop::ClassList(Vec::new()),
    })
    .unwrap();
    assert_eq!(padding(&c, 2), [8.0, 8.0, 8.0, 8.0]);
}

#[test]
fn space_x_margins_stop_at_the_last_element_child() {
    let mut c = core();
    view(&mut c, 2);
    view(&mut c, 3);
    view(&mut c, 4);
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: 2,
        child: 4,
        before: None,
    })
    .unwrap();
    set_class(&mut c, 2, "space-x-2");
    let margin = |id| layout_of(&c, id).margin;
    let first = margin(3);
    let last = margin(4);
    assert_eq!(margin_length(first.right), 8.0);
    assert_eq!(margin_length(first.left), 0.0);
    assert_eq!(margin_length(last.right), 0.0);
    assert_eq!(margin_length(last.left), 0.0);
}

#[test]
fn focus_within_follows_the_child_until_blur() {
    let mut c = core();
    view(&mut c, 2);
    view(&mut c, 3);
    set_class(&mut c, 2, "focus-within:p-4");
    c.apply(Op::Insert {
        parent: 2,
        child: 3,
        before: None,
    })
    .unwrap();
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    assert_eq!(padding(&c, 2), [0.0, 0.0, 0.0, 0.0]);
    c.set_states(3, state::FOCUSED);
    // The chain is resolved at commit, not when the bit is reported.
    assert_eq!(padding(&c, 2), [0.0, 0.0, 0.0, 0.0]);
    c.commit();
    assert_eq!(padding(&c, 2), [16.0, 16.0, 16.0, 16.0]);
    c.set_states(3, 0);
    c.commit();
    assert_eq!(padding(&c, 2), [0.0, 0.0, 0.0, 0.0]);
}

struct ClassLog(std::sync::Mutex<Vec<String>>);

impl log::Log for ClassLog {
    fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
        meta.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            self.0.lock().unwrap().push(record.args().to_string());
        }
    }

    fn flush(&self) {}
}

fn class_log() -> &'static ClassLog {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    static LOGGER: ClassLog = ClassLog(std::sync::Mutex::new(Vec::new()));
    ONCE.call_once(|| {
        let _ = log::set_logger(&LOGGER);
        log::set_max_level(log::LevelFilter::Warn);
    });
    &LOGGER
}

#[test]
fn a_class_string_warns_once() {
    let log = class_log();
    log.0.lock().unwrap().clear();
    let mut c = core();
    view(&mut c, 2);
    set_class(&mut c, 2, "wat blur-sm");
    let first: Vec<_> = log.0.lock().unwrap().clone();
    assert_eq!(
        first
            .iter()
            .filter(|m| *m == "unknown classes: wat")
            .count(),
        1
    );
    assert_eq!(
        first
            .iter()
            .filter(|m| *m == "unsupported classes: blur-sm")
            .count(),
        1
    );
    log.0.lock().unwrap().clear();
    set_class(&mut c, 2, "wat blur-sm");
    let again = log.0.lock().unwrap().clone();
    assert!(
        !again
            .iter()
            .any(|m| m.contains("wat") || m.contains("blur-sm")),
        "{again:?}"
    );
}

#[test]
fn flex_1_in_an_auto_height_column_keeps_its_content() {
    let mut c = Core::new(FakeEngine, size(px(390.0), px(700.0)));
    view(&mut c, 2);
    view(&mut c, 3);
    view(&mut c, 4);
    view(&mut c, 5);
    view(&mut c, 6);
    set_class(&mut c, 2, "flex flex-col");
    set_class(&mut c, 3, "flex flex-1 flex-col");
    set_class(&mut c, 4, "h-20");
    set_class(&mut c, 5, "flex flex-1 flex-col");
    set_class(&mut c, 6, "h-5");
    for (parent, child) in [(2, 3), (3, 4), (2, 5), (5, 6)] {
        c.apply(Op::Insert {
            parent,
            child,
            before: None,
        })
        .unwrap();
    }
    c.apply(Op::Insert {
        parent: ROOT_ID,
        child: 2,
        before: None,
    })
    .unwrap();
    let batch = c.commit().unwrap();
    let column = frame(&batch, 2);
    let tall = frame(&batch, 3);
    let short = frame(&batch, 5);
    let block = frame(&batch, 4);
    assert!(
        tall.size.height.0 + 0.5 >= block.size.height.0,
        "flex-1 item {} shorter than its content {}",
        tall.size.height.0,
        block.size.height.0
    );
    assert!(
        tall.origin.y.0 + tall.size.height.0 <= short.origin.y.0 + 0.5,
        "items overlap: {tall:?} {short:?}"
    );
    assert!(
        column.size.height.0 + 0.5 >= tall.size.height.0 + short.size.height.0,
        "column {} does not hold {} + {}",
        column.size.height.0,
        tall.size.height.0,
        short.size.height.0
    );
}

#[test]
fn flex_1_in_a_definite_row_splits_the_width() {
    let mut c = Core::new(FakeEngine, size(px(300.0), px(200.0)));
    view(&mut c, 2);
    view(&mut c, 3);
    view(&mut c, 4);
    set_class(&mut c, 2, "flex w-full flex-row");
    set_class(&mut c, 3, "flex-1");
    set_class(&mut c, 4, "flex-1");
    for (parent, child) in [(2, 3), (2, 4), (ROOT_ID, 2)] {
        c.apply(Op::Insert {
            parent,
            child,
            before: None,
        })
        .unwrap();
    }
    let batch = c.commit().unwrap();
    let left = frame(&batch, 3);
    let right = frame(&batch, 4);
    assert_eq!(left.size.width, right.size.width);
    assert!(left.size.width.0 > 100.0, "{}", left.size.width.0);
}
