use super::*;
use nlayer_core::geometry::{Bounds, point, px, size};
use nlayer_core::style::Color;
use nlayer_core::text::{AttributedText, ResolvedTextStyle, TextEngine, TextSpan};
use nlayer_core::{EventKind, ImageSource, Interaction, MountBatch, MountItem, ViewKind};
use objc2::runtime::NSObjectProtocol;
use objc2::{MainThreadOnly, Message};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_ui_kit::{UIGestureRecognizerDelegate, UIGestureRecognizerState};

/// The tests own the main thread (the runner is a `harness = false`
/// binary): UIKit views, CoreText and the run loop's queue are
/// main-thread only.
fn mounter() -> Rc<RefCell<Mounter>> {
    let mtm = MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");
    let root = UIView::new(mtm);
    root.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 640.0)));
    Mounter::new(mtm, root, PathBuf::new())
}

fn frame(x: f32, y: f32, width: f32, height: f32) -> Bounds<Points> {
    Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
}

fn child(id: NodeId, kind: ViewKind, at: Bounds<Points>) -> Vec<MountItem> {
    vec![
        MountItem::Create { id, kind },
        MountItem::Insert {
            parent: ROOT_ID,
            child: id,
            before: None,
        },
        MountItem::Frame { id, frame: at },
    ]
}

fn apply(mounter: &Rc<RefCell<Mounter>>, items: Vec<MountItem>) {
    mounter.borrow_mut().apply(MountBatch { items });
}

pub(crate) fn tests() -> Vec<(&'static str, fn())> {
    vec![
        (
            "mount::a_batch_frames_paints_and_names_a_view",
            a_batch_frames_paints_and_names_a_view as fn(),
        ),
        (
            "mount::a_transform_origin_follows_the_bounds",
            a_transform_origin_follows_the_bounds as fn(),
        ),
        (
            "mount::active_beats_hover_and_both_beat_the_base_paint",
            active_beats_hover_and_both_beat_the_base_paint as fn(),
        ),
        (
            "mount::a_focused_field_lights_the_focus_style_on_its_ancestors",
            a_focused_field_lights_the_focus_style_on_its_ancestors as fn(),
        ),
        (
            "mount::a_vertical_scroll_narrows_its_native_width_until_a_row_grows",
            a_vertical_scroll_narrows_its_native_width_until_a_row_grows as fn(),
        ),
        (
            "mount::scroll_by_and_scroll_to_move_a_horizontal_scroll",
            scroll_by_and_scroll_to_move_a_horizontal_scroll as fn(),
        ),
        (
            "mount::an_input_takes_its_configuration_and_value",
            an_input_takes_its_configuration_and_value as fn(),
        ),
        (
            "mount::a_return_resigns_text_and_email_but_not_number",
            a_return_resigns_text_and_email_but_not_number as fn(),
        ),
        (
            "mount::an_image_records_its_source_and_fit",
            an_image_records_its_source_and_fit as fn(),
        ),
        (
            "mount::pan_handoff_respects_axis_binding",
            pan_handoff_respects_axis_binding as fn(),
        ),
        (
            "mount::pan_touch_tracks_only_single_axis_scrolls",
            pan_touch_tracks_only_single_axis_scrolls as fn(),
        ),
        (
            "mount::refresh_indicator_hides_native_refresh_control",
            refresh_indicator_hides_native_refresh_control as fn(),
        ),
        (
            "mount::an_image_decodes_and_clips_to_its_radius",
            an_image_decodes_and_clips_to_its_radius as fn(),
        ),
        (
            "mount::a_text_block_carries_its_layout_and_runs",
            a_text_block_carries_its_layout_and_runs as fn(),
        ),
        (
            "mount::a_stack_pushes_and_pops_screens_natively",
            a_stack_pushes_and_pops_screens_natively as fn(),
        ),
        (
            "mount::a_screen_header_and_back_policy_reach_uikit",
            a_screen_header_and_back_policy_reach_uikit as fn(),
        ),
        (
            "mount::a_covered_page_owns_the_back_button",
            a_covered_page_owns_the_back_button as fn(),
        ),
        (
            "mount::a_back_swipe_follows_the_mounted_policy",
            a_back_swipe_follows_the_mounted_policy as fn(),
        ),
        (
            "mount::the_modal_presents_over_the_root",
            the_modal_presents_over_the_root as fn(),
        ),
        (
            "mount::a_dialog_popup_lets_presses_through_to_the_backdrop",
            a_dialog_popup_lets_presses_through_to_the_backdrop as fn(),
        ),
    ]
}

fn pan_handoff_respects_axis_binding() {
    let mounter = mounter();
    apply(
        &mounter,
        [
            child(2, ViewKind::View, frame(0.0, 0.0, 100.0, 100.0)),
            vec![MountItem::PanBinding {
                id: 2,
                x: Some(1),
                y: None,
            }],
        ]
        .concat(),
    );
    let m = mounter.borrow();
    assert!(m.pan_should_begin(2, CGPoint::new(80.0, 10.0)));
    assert!(!m.pan_should_begin(2, CGPoint::new(10.0, 80.0)));
    drop(m);

    apply(
        &mounter,
        vec![MountItem::PanBinding {
            id: 2,
            x: Some(1),
            y: Some(2),
        }],
    );
    assert!(
        mounter
            .borrow()
            .pan_should_begin(2, CGPoint::new(10.0, 80.0))
    );
    apply(
        &mounter,
        vec![MountItem::PanBinding {
            id: 2,
            x: None,
            y: None,
        }],
    );
    assert!(
        mounter
            .borrow()
            .pan_should_begin(2, CGPoint::new(10.0, 80.0))
    );
}

fn pan_touch_tracks_only_single_axis_scrolls() {
    let mounter = mounter();
    apply(
        &mounter,
        [
            child(2, ViewKind::View, frame(0.0, 0.0, 200.0, 200.0)),
            vec![MountItem::PanBinding {
                id: 2,
                x: None,
                y: Some(1),
            }],
        ]
        .concat(),
    );
    let panel = mounter.borrow().view_of(2).expect("pan view");
    let mtm = MainThreadMarker::new().expect("main thread");
    let scroll = UIScrollView::new(mtm);
    scroll.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(100.0, 100.0)));
    scroll.setContentSize(CGSize::new(100.0, 300.0));
    panel.addSubview(&scroll);
    let touched = UIView::new(mtm);
    touched.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(100.0, 300.0)));
    scroll.addSubview(&touched);
    mounter.borrow_mut().pan_touch(2, &touched);
    {
        let m = mounter.borrow();
        let tracked = m
            .slot(2)
            .and_then(|slot| slot.pan_scroll.as_ref())
            .expect("tracked scroll");
        assert!(std::ptr::eq(&*tracked.view, &*scroll));
    }

    apply(
        &mounter,
        vec![MountItem::PanBinding {
            id: 2,
            x: Some(1),
            y: Some(2),
        }],
    );
    mounter.borrow_mut().pan_touch(2, &touched);
    assert!(mounter.borrow().slot(2).unwrap().pan_scroll.is_none());
}

fn refresh_indicator_hides_native_refresh_control() {
    let mounter = mounter();
    let mut events = EventMask::default();
    events.set(EventKind::Refresh, true);
    apply(
        &mounter,
        [
            child(2, ViewKind::Scroll, frame(0.0, 0.0, 320.0, 200.0)),
            vec![MountItem::Events { id: 2, events }],
        ]
        .concat(),
    );
    apply(
        &mounter,
        vec![MountItem::ScrollContent {
            id: 2,
            size: size(px(320.0), px(600.0)),
            refresh_indicator: Some(px(60.0)),
            paging: false,
            snap_interval: None,
            snap_offsets: None,
            snap_alignment: 0,
            disable_interval_momentum: false,
            deceleration: 0.998,
        }],
    );
    assert!(mounter.borrow().slot(2).unwrap().refresh_control.is_none());
    apply(
        &mounter,
        vec![MountItem::ScrollContent {
            id: 2,
            size: size(px(320.0), px(600.0)),
            refresh_indicator: None,
            paging: false,
            snap_interval: None,
            snap_offsets: None,
            snap_alignment: 0,
            disable_interval_momentum: false,
            deceleration: 0.998,
        }],
    );
    assert!(mounter.borrow().slot(2).unwrap().refresh_control.is_some());
}

fn a_batch_frames_paints_and_names_a_view() {
    let mounter = mounter();
    let mut style = ViewStyle::default();
    style.opacity = 0.5;
    style.hidden = true;
    style.border_radius = px(8.0);
    style.overflow_hidden = true;
    style.background_color = Color::rgba(1, 2, 3, 255);
    apply(
        &mounter,
        [
            child(2, ViewKind::View, frame(10.0, 20.0, 100.0, 50.0)),
            vec![
                MountItem::ViewStyle {
                    id: 2,
                    style: Box::new(style),
                },
                MountItem::TestId {
                    id: 2,
                    test_id: Some("card".into()),
                },
            ],
        ]
        .concat(),
    );

    let view = mounter.borrow().view_of(2).expect("view");
    assert_eq!(view.frame().origin.x, 10.0);
    assert_eq!(view.frame().origin.y, 20.0);
    assert_eq!(view.frame().size.width, 100.0);
    assert_eq!(view.frame().size.height, 50.0);
    assert!((view.alpha() - 0.5).abs() < 1e-6);
    assert!(view.isHidden());
    assert!(view.layer().masksToBounds());
    assert_eq!(view.layer().cornerRadius(), 8.0);

    let tree = mounter.borrow().scene().tree();
    let node = &tree["children"][0];
    assert_eq!(node["kind"], "view");
    assert_eq!(node["testID"], "card");
    assert_eq!(node["frame"]["x"], 10.0);
    assert_eq!(node["frame"]["height"], 50.0);

    // A delete drops the slot and the native view.
    apply(&mounter, vec![MountItem::Delete { id: 2 }]);
    assert!(mounter.borrow().view_of(2).is_none());
    assert_eq!(
        mounter.borrow().scene().tree()["children"]
            .as_array()
            .map(Vec::len),
        Some(0)
    );
}

fn a_transform_origin_follows_the_bounds() {
    let mounter = mounter();
    let mut parsed = nlayer_core::style::Style::default();
    parsed
        .set(
            "transform",
            nlayer_core::style::StyleValue::Str("rotate(90deg)"),
        )
        .unwrap();
    parsed
        .set(
            "transformOrigin",
            nlayer_core::style::StyleValue::Str("25% 75%"),
        )
        .unwrap();
    apply(
        &mounter,
        [
            child(2, ViewKind::View, frame(0.0, 0.0, 20.0, 10.0)),
            vec![MountItem::ViewStyle {
                id: 2,
                style: Box::new(parsed.view),
            }],
        ]
        .concat(),
    );
    let view = mounter.borrow().view_of(2).expect("view");
    let turned = view.layer().transform();
    // The origin is (5, 7.5), 5 left of and 2.5 below the center.
    assert!((turned.m41 + 2.5).abs() < 1e-6, "{}", turned.m41);
    assert!((turned.m42 - 7.5).abs() < 1e-6, "{}", turned.m42);

    // A percentage origin is of the new size, not the first one.
    apply(
        &mounter,
        vec![MountItem::Frame {
            id: 2,
            frame: frame(0.0, 0.0, 40.0, 20.0),
        }],
    );
    let turned = view.layer().transform();
    assert!((turned.m41 + 5.0).abs() < 1e-6, "{}", turned.m41);
    assert!((turned.m42 - 15.0).abs() < 1e-6, "{}", turned.m42);
}

fn active_beats_hover_and_both_beat_the_base_paint() {
    let mounter = mounter();
    let mut hover = ViewStyle::default();
    hover.opacity = 0.4;
    let mut active = ViewStyle::default();
    active.opacity = 0.2;
    apply(
        &mounter,
        [
            child(2, ViewKind::View, frame(0.0, 0.0, 100.0, 50.0)),
            vec![
                MountItem::StateStyle {
                    id: 2,
                    interaction: Interaction::Hover,
                    view: Box::new(hover),
                    text: TextStyle::default(),
                },
                MountItem::StateStyle {
                    id: 2,
                    interaction: Interaction::Active,
                    view: Box::new(active),
                    text: TextStyle::default(),
                },
            ],
        ]
        .concat(),
    );
    let view = mounter.borrow().view_of(2).expect("view");
    assert!((view.alpha() - 1.0).abs() < 1e-6);

    mounter
        .borrow_mut()
        .hovered(2, UIGestureRecognizerState::Began);
    assert!((view.alpha() - 0.4).abs() < 1e-6);

    mounter
        .borrow_mut()
        .pressed(2, UIGestureRecognizerState::Began, CGPoint::ZERO);
    assert!((view.alpha() - 0.2).abs() < 1e-6);

    mounter
        .borrow_mut()
        .pressed(2, UIGestureRecognizerState::Ended, CGPoint::ZERO);
    assert!((view.alpha() - 0.4).abs() < 1e-6);

    mounter
        .borrow_mut()
        .hovered(2, UIGestureRecognizerState::Ended);
    assert!((view.alpha() - 1.0).abs() < 1e-6);
}

fn a_focused_field_lights_the_focus_style_on_its_ancestors() {
    let mounter = mounter();
    let mut focus = ViewStyle::default();
    focus.opacity = 0.25;
    apply(
        &mounter,
        [
            child(2, ViewKind::View, frame(0.0, 0.0, 200.0, 100.0)),
            vec![
                MountItem::Create {
                    id: 3,
                    kind: ViewKind::Input,
                },
                MountItem::Insert {
                    parent: 2,
                    child: 3,
                    before: None,
                },
                MountItem::Frame {
                    id: 3,
                    frame: frame(0.0, 0.0, 200.0, 40.0),
                },
                MountItem::StateStyle {
                    id: 2,
                    interaction: Interaction::Focus,
                    view: Box::new(focus),
                    text: TextStyle::default(),
                },
            ],
        ]
        .concat(),
    );
    let parent = mounter.borrow().view_of(2).expect("parent");
    assert!((parent.alpha() - 1.0).abs() < 1e-6);

    mounter.borrow_mut().begin_focus(3);
    assert!((parent.alpha() - 0.25).abs() < 1e-6);

    mounter.borrow_mut().end_focus(3);
    assert!((parent.alpha() - 1.0).abs() < 1e-6);
}

fn a_vertical_scroll_narrows_its_native_width_until_a_row_grows() {
    let mounter = mounter();
    apply(
        &mounter,
        [
            child(2, ViewKind::Scroll, frame(0.0, 0.0, 320.0, 200.0)),
            vec![
                MountItem::ScrollConfig {
                    id: 2,
                    horizontal: false,
                    taps: KeyboardTaps::Never,
                    dismiss: KeyboardDismiss::None,
                },
                MountItem::Create {
                    id: 3,
                    kind: ViewKind::View,
                },
                MountItem::Insert {
                    parent: 2,
                    child: 3,
                    before: None,
                },
                MountItem::Frame {
                    id: 3,
                    frame: frame(0.0, 0.0, 300.0, 50.0),
                },
                MountItem::ScrollContent {
                    id: 2,
                    size: size(px(500.0), px(1000.0)),
                    refresh_indicator: None,
                    paging: false,
                    snap_interval: None,
                    snap_offsets: None,
                    snap_alignment: 0,
                    disable_interval_momentum: false,
                    deceleration: 0.998,
                },
            ],
        ]
        .concat(),
    );
    let scene = mounter.borrow().scene();
    let tree = scene.tree();
    assert_eq!(tree["children"][0]["content"]["width"], 500.0);

    let scroll = mounter.borrow().scroll(2).expect("scroll view");
    // Nothing inside passes the viewport's right edge, so the page
    // would pan over nothing: the native width narrows to the viewport.
    assert_eq!(scroll.contentSize().width, 320.0);
    // The event and the tree still report the engine's size.
    drop(scene);

    apply(
        &mounter,
        vec![MountItem::Frame {
            id: 3,
            frame: frame(0.0, 0.0, 400.0, 50.0),
        }],
    );
    // The row grew past the viewport. The engine sends no new content
    // size for that batch, so the narrowed width is given back here.
    assert_eq!(scroll.contentSize().width, 500.0);
}

fn scroll_by_and_scroll_to_move_a_horizontal_scroll() {
    let mounter = mounter();
    apply(
        &mounter,
        [
            child(2, ViewKind::Scroll, frame(0.0, 0.0, 300.0, 100.0)),
            vec![
                MountItem::ScrollConfig {
                    id: 2,
                    horizontal: true,
                    taps: KeyboardTaps::Never,
                    dismiss: KeyboardDismiss::None,
                },
                MountItem::ScrollContent {
                    id: 2,
                    size: size(px(600.0), px(100.0)),
                    refresh_indicator: None,
                    paging: false,
                    snap_interval: None,
                    snap_offsets: None,
                    snap_alignment: 0,
                    disable_interval_momentum: false,
                    deceleration: 0.998,
                },
                MountItem::ScrollBy {
                    id: 2,
                    dx: px(40.0),
                    dy: px(0.0),
                },
            ],
        ]
        .concat(),
    );
    let scroll = mounter.borrow().scroll(2).expect("scroll view");
    assert_eq!(scroll.contentOffset().x, 40.0);
    assert!(scroll.alwaysBounceHorizontal());

    apply(
        &mounter,
        vec![MountItem::Command {
            id: 2,
            command: nlayer_core::Command::ScrollTo {
                x: px(10.0),
                y: px(0.0),
                animated: false,
            },
        }],
    );
    assert_eq!(scroll.contentOffset().x, 10.0);
}

fn an_input_takes_its_configuration_and_value() {
    let mounter = mounter();
    let config = nlayer_core::InputConfig {
        placeholder: "Name".into(),
        secure: true,
        keyboard: nlayer_core::KeyboardType::Email,
        disabled: true,
        ..nlayer_core::InputConfig::default()
    };
    apply(
        &mounter,
        [
            child(2, ViewKind::Input, frame(0.0, 0.0, 200.0, 40.0)),
            vec![
                MountItem::Input {
                    id: 2,
                    config: Box::new(config),
                },
                MountItem::InputValue {
                    id: 2,
                    text: "ada".into(),
                    count: 0,
                },
                MountItem::TestId {
                    id: 2,
                    test_id: Some("name".into()),
                },
            ],
        ]
        .concat(),
    );

    let field = mounter.borrow().field(2).expect("field");
    assert_eq!(field.shown(), "ada");
    assert_eq!(field.placeholder_text(), "Name");
    assert!(field.secure());
    assert_eq!(field.keyboard_name(), "email");
    assert!(field.disabled());

    let tree = mounter.borrow().scene().tree();
    let node = &tree["children"][0];
    assert_eq!(node["kind"], "input");
    assert_eq!(node["value"], "ada");
    assert_eq!(node["placeholder"], "Name");
    assert_eq!(node["secure"], true);
    assert_eq!(node["keyboard"], "email");
    assert_eq!(node["disabled"], true);
}

fn a_return_resigns_text_and_email_but_not_number() {
    let mtm = MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");
    let bounds = CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 480.0));
    let window = crate::app::test_window(mtm, 320.0, 480.0);
    let root = UIView::new(mtm);
    root.setFrame(bounds);
    window.addSubview(&root);
    window.makeKeyAndVisible();
    let mounter = Mounter::new(mtm, root, PathBuf::new());

    let keyboards = [
        (2, nlayer_core::KeyboardType::Text, true),
        (3, nlayer_core::KeyboardType::Email, true),
        (4, nlayer_core::KeyboardType::Number, false),
    ];
    let mut items = Vec::new();
    for (id, keyboard, _) in keyboards {
        items.extend(child(id, ViewKind::Input, frame(0.0, 0.0, 200.0, 40.0)));
        items.push(MountItem::Input {
            id,
            config: Box::new(nlayer_core::InputConfig {
                keyboard,
                ..nlayer_core::InputConfig::default()
            }),
        });
    }
    apply(&mounter, items);

    for (id, keyboard, resigns) in keyboards {
        let field = mounter.borrow().field(id).expect("field").retain();
        assert!(
            field.becomeFirstResponder(),
            "{keyboard:?} takes the keyboard"
        );
        // `EditingDidEndOnExit` is what UIKit sends when Return is pressed;
        // the handler resigns text and email only.
        let relay = mounter.borrow().relay.clone();
        // SAFETY: `Relay` implements `returned:`.
        let _: () = unsafe { objc2::msg_send![&*relay, returned: &*field] };
        assert_eq!(
            field.isFirstResponder(),
            !resigns,
            "{keyboard:?}: Return resigns text and email, not number"
        );
    }
    drop(window);
}

fn an_image_records_its_source_and_fit() {
    let mounter = mounter();
    apply(
        &mounter,
        [
            child(2, ViewKind::Image, frame(0.0, 0.0, 32.0, 32.0)),
            vec![MountItem::Image {
                id: 2,
                source: Some(ImageSource {
                    uri: "asset:/missing.png".into(),
                    size: None,
                    scales: nlayer_core::ImageScales::NONE,
                }),
                resize_mode: ResizeMode::Contain,
            }],
        ]
        .concat(),
    );
    let tree = mounter.borrow().scene().tree();
    assert_eq!(tree["children"][0]["kind"], "image");
    assert_eq!(tree["children"][0]["src"], "asset:/missing.png");
    assert_eq!(tree["children"][0]["fit"], "contain");
}

fn an_image_decodes_and_clips_to_its_radius() {
    // A 4×2 red PNG: the smallest file ImageIO still decodes.
    const PNG: &[u8] = &[
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4, 0, 0, 0, 2, 8, 6,
        0, 0, 0, 127, 168, 125, 99, 0, 0, 0, 18, 73, 68, 65, 84, 120, 218, 99, 248, 207, 192, 240,
        31, 25, 51, 160, 11, 0, 0, 15, 33, 15, 241, 254, 69, 20, 99, 0, 0, 0, 0, 73, 69, 78, 68,
        174, 66, 96, 130,
    ];
    let mtm = MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");
    let root = UIView::new(mtm);
    root.setFrame(CGRect::new(CGPoint::ZERO, CGSize::new(320.0, 640.0)));
    let assets = std::env::temp_dir().join(format!(
        "nlayer-image-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&assets).expect("temporary assets");
    std::fs::write(assets.join("pic.png"), PNG).expect("a PNG on disk");
    let mounter = Mounter::new(mtm, root, assets.clone());

    let mut parsed = nlayer_core::style::Style::default();
    parsed
        .set("borderRadius", nlayer_core::style::StyleValue::Number(8.0))
        .unwrap();
    apply(
        &mounter,
        [
            child(2, ViewKind::Image, frame(0.0, 0.0, 32.0, 32.0)),
            vec![
                MountItem::Image {
                    id: 2,
                    source: Some(ImageSource {
                        uri: "asset:/pic.png".into(),
                        size: None,
                        scales: nlayer_core::ImageScales::NONE,
                    }),
                    resize_mode: ResizeMode::Contain,
                },
                MountItem::ViewStyle {
                    id: 2,
                    style: Box::new(parsed.view),
                },
            ],
        ]
        .concat(),
    );

    let decoded = crate::image::load(&assets, "asset:/pic.png").expect("ImageIO decodes the PNG");
    assert_eq!((decoded.width, decoded.height), (4, 2), "the PNG's pixels");
    let generation = mounter
        .borrow()
        .slot(2)
        .and_then(|slot| slot.picture.as_ref())
        .expect("picture")
        .generation;
    mounter
        .borrow_mut()
        .finish_image(2, generation, 2.0, Ok(decoded));

    let mounter = mounter.borrow();
    let picture = mounter
        .slot(2)
        .and_then(|slot| slot.picture.as_ref())
        .expect("picture");
    assert_eq!(
        picture.natural,
        Some(Size::new(Points(2.0), Points(1.0))),
        "4×2 pixels at scale 2 are 2×1 points"
    );
    assert_eq!(picture.layer.contentsScale(), 2.0);
    assert!(picture.clip.masksToBounds(), "the picture is clipped");
    assert_eq!(picture.clip.cornerRadius(), 8.0, "the radius clips it");
    let fitted = picture.layer.frame();
    assert_eq!(
        (
            fitted.origin.x,
            fitted.origin.y,
            fitted.size.width,
            fitted.size.height
        ),
        (0.0, 8.0, 32.0, 16.0),
        "contain fits 2×1 into 32×32"
    );
    drop(mounter);
    let _ = std::fs::remove_dir_all(&assets);
}

fn a_text_block_carries_its_layout_and_runs() {
    let mounter = mounter();
    let value = "Hello";
    let attributed = Arc::new(AttributedText {
        text: value.into(),
        spans: vec![TextSpan {
            start: 0,
            end: value.encode_utf16().count() as u32,
            style: ResolvedTextStyle::default(),
        }],
        ..AttributedText::default()
    });
    let engine = crate::text::CoreTextEngine::default();
    let prepared = engine.prepare(&attributed);
    let layout = engine.layout(&prepared, size(px(200.0), px(40.0)));
    apply(
        &mounter,
        [
            child(2, ViewKind::Text, frame(0.0, 0.0, 200.0, 20.0)),
            vec![MountItem::Text {
                id: 2,
                text: Arc::clone(&attributed),
                layout,
                content: frame(0.0, 0.0, 200.0, 20.0),
            }],
        ]
        .concat(),
    );

    let scene = mounter.borrow().scene();
    assert_eq!(scene.texts(), ["Hello"]);
    let tree = scene.tree();
    assert_eq!(tree["children"][0]["text"], "Hello");
    assert_eq!(tree["children"][0]["spans"][0]["text"], "Hello");
    assert_eq!(tree["children"][0]["spans"][0]["fontSize"], 14.0);
    assert_eq!(tree["children"][0]["spans"][0]["fontWeight"], 400);
}

fn a_stack_pushes_and_pops_screens_natively() {
    let mounter = mounter();

    // Create stack
    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 2,
                kind: ViewKind::Stack,
            },
            MountItem::Insert {
                parent: ROOT_ID,
                child: 2,
                before: None,
            },
            MountItem::Frame {
                id: 2,
                frame: frame(0.0, 0.0, 320.0, 640.0),
            },
        ],
    );

    // Create first screen
    let mut config1 = nlayer_core::ScreenConfig::default();
    config1.title = Some("One".into());
    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 3,
                kind: ViewKind::Screen,
            },
            MountItem::ScreenConfig {
                id: 3,
                config: Box::new(config1),
            },
            MountItem::Insert {
                parent: 2,
                child: 3,
                before: None,
            },
        ],
    );

    // Assert first screen is in the stack
    {
        let borrowed = mounter.borrow();
        let stack = borrowed.stack(2).unwrap();
        assert_eq!(stack.nav.viewControllers().count(), 1);

        let top = stack.nav.topViewController().expect("top controller");
        let screen_controller = top
            .downcast_ref::<crate::navigation::ScreenController>()
            .expect("top should be a ScreenController");
        assert_eq!(screen_controller.screen_id(), 3);
    }

    // Create second screen
    let mut config2 = nlayer_core::ScreenConfig::default();
    config2.title = Some("Two".into());
    // A push animates by default, and UIKit applies an animated push on
    // the next run-loop turn; the test asserts the resulting stack now.
    config2.animation = nlayer_core::ScreenAnimation::None;
    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 4,
                kind: ViewKind::Screen,
            },
            MountItem::ScreenConfig {
                id: 4,
                config: Box::new(config2),
            },
            MountItem::Insert {
                parent: 2,
                child: 4,
                before: None,
            },
        ],
    );

    // Assert both screens are in the stack in order
    {
        let borrowed = mounter.borrow();
        let stack = borrowed.stack(2).unwrap();
        assert_eq!(stack.nav.viewControllers().count(), 2);

        let controllers = stack.nav.viewControllers();
        let first = controllers.objectAtIndex(0);
        let second = controllers.objectAtIndex(1);

        let first_screen = first
            .downcast_ref::<crate::navigation::ScreenController>()
            .expect("first should be a ScreenController");
        let second_screen = second
            .downcast_ref::<crate::navigation::ScreenController>()
            .expect("second should be a ScreenController");

        assert_eq!(first_screen.screen_id(), 3);
        assert_eq!(second_screen.screen_id(), 4);
    }

    // The user pops natively (back button or a swipe): the delegate's
    // did-show reports the screen until the app removes its route.
    let (nav, delegate, top) = {
        let borrowed = mounter.borrow();
        let stack = borrowed.stack(2).unwrap();
        stack.nav.popViewControllerAnimated(false);
        assert_eq!(stack.nav.viewControllers().count(), 1);
        (
            stack.nav.clone(),
            stack.nav.delegate().expect("delegate"),
            stack.nav.topViewController().expect("top"),
        )
    };
    // SAFETY: `StackDelegate` implements the did-show callback. The
    // mounter must not be borrowed: the callback takes it.
    let _: () = unsafe {
        objc2::msg_send![
            &*delegate,
            navigationController: &*nav,
            didShowViewController: &*top,
            animated: false
        ]
    };
    {
        let borrowed = mounter.borrow();
        let stack = borrowed.stack(2).unwrap();
        assert_eq!(stack.popped, vec![4]);
    }
    apply(&mounter, vec![MountItem::Remove { id: 4 }]);

    // Assert only first screen remains
    {
        let borrowed = mounter.borrow();
        let stack = borrowed.stack(2).unwrap();
        assert_eq!(stack.nav.viewControllers().count(), 1);

        let top = stack.nav.topViewController().expect("top controller");
        let screen_controller = top
            .downcast_ref::<crate::navigation::ScreenController>()
            .expect("top should be a ScreenController");
        assert_eq!(screen_controller.screen_id(), 3);
        assert_eq!(stack.screens, vec![3]);
    }
}

fn a_screen_header_and_back_policy_reach_uikit() {
    let mounter = mounter();

    // Create stack
    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 2,
                kind: ViewKind::Stack,
            },
            MountItem::Insert {
                parent: ROOT_ID,
                child: 2,
                before: None,
            },
            MountItem::Frame {
                id: 2,
                frame: frame(0.0, 0.0, 320.0, 640.0),
            },
        ],
    );

    // Create first screen with back disabled
    let mut config = nlayer_core::ScreenConfig::default();
    config.title = Some("Inbox".into());
    config.hides_back_button = true;
    config.prevent_back = true;
    config.gesture_enabled = false;

    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 3,
                kind: ViewKind::Screen,
            },
            MountItem::ScreenConfig {
                id: 3,
                config: Box::new(config),
            },
            MountItem::Insert {
                parent: 2,
                child: 3,
                before: None,
            },
        ],
    );

    // Assert screen configuration reached UIKit
    {
        let borrowed = mounter.borrow();
        let screen = borrowed.screen(3).unwrap();
        let nav_item = screen.controller.navigationItem();

        assert_eq!(
            nav_item.title().map(|t| t.to_string()),
            Some("Inbox".to_string())
        );
        assert!(nav_item.hidesBackButton());

        assert!(!borrowed.stack_can_pop(2));
        assert_eq!(borrowed.stack_back_press(2), Some((3, false, true)));
    }

    // Add second screen with back enabled
    let mut config2 = nlayer_core::ScreenConfig::default();
    config2.prevent_back = false;
    config2.gesture_enabled = true;

    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 4,
                kind: ViewKind::Screen,
            },
            MountItem::ScreenConfig {
                id: 4,
                config: Box::new(config2),
            },
            MountItem::Insert {
                parent: 2,
                child: 4,
                before: None,
            },
        ],
    );

    {
        let borrowed = mounter.borrow();
        assert!(borrowed.stack_can_pop(2));
        assert_eq!(borrowed.stack_back_press(2), Some((4, false, false)));
    }
}
fn a_covered_page_owns_the_back_button() {
    use nlayer_core::{
        BackButtonDisplayMode, BarAppearance, BarItem, BarShadow, LargeTitleDisplayMode,
    };
    use objc2::rc::Retained;
    use objc2::{msg_send, sel};
    use objc2_foundation::NSString;
    use objc2_ui_kit::{
        UINavigationItemBackButtonDisplayMode, UINavigationItemLargeTitleDisplayMode,
    };

    let mounter = mounter();
    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 2,
                kind: ViewKind::Stack,
            },
            MountItem::Insert {
                parent: ROOT_ID,
                child: 2,
                before: None,
            },
            MountItem::Frame {
                id: 2,
                frame: frame(0.0, 0.0, 320.0, 640.0),
            },
        ],
    );

    let mut below = nlayer_core::ScreenConfig::default();
    below.title = Some("Inbox".into());
    below.subtitle = Some("12 unread".into());
    below.back_button_title = Some("Mail".into());
    below.back_button_display_mode = BackButtonDisplayMode::Minimal;
    below.large_title = LargeTitleDisplayMode::Automatic;
    below.animation = nlayer_core::ScreenAnimation::None;
    below.standard = BarAppearance {
        background_color: Some(Color::rgba(255, 255, 255, 255)),
        title_color: Some(Color::rgba(0, 0, 0, 255)),
        shadow: Some(BarShadow::None),
        ..BarAppearance::default()
    };
    below.leading = vec![
        vec![BarItem::Button {
            id: 10,
            title: Some("Outer".into()),
            image: None,
            enabled: true,
            tint_color: None,
        }],
        vec![BarItem::Button {
            id: 11,
            title: Some("Inner".into()),
            image: None,
            enabled: false,
            tint_color: Some(Color::rgba(0, 122, 255, 255)),
        }],
    ];
    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 3,
                kind: ViewKind::Screen,
            },
            MountItem::ScreenConfig {
                id: 3,
                config: Box::new(below),
            },
            MountItem::Insert {
                parent: 2,
                child: 3,
                before: None,
            },
        ],
    );

    let mut above = nlayer_core::ScreenConfig::default();
    above.animation = nlayer_core::ScreenAnimation::None;
    above.hides_back_button = false;
    above.left_items_supplement_back_button = true;
    above.large_title = LargeTitleDisplayMode::Never;
    above.scroll_edge = BarAppearance {
        background_color: Some(Color::TRANSPARENT),
        ..BarAppearance::default()
    };
    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 4,
                kind: ViewKind::Screen,
            },
            MountItem::ScreenConfig {
                id: 4,
                config: Box::new(above),
            },
            MountItem::Insert {
                parent: 2,
                child: 4,
                before: None,
            },
        ],
    );

    {
        let borrowed = mounter.borrow();
        let covered = borrowed.screen(3).unwrap().controller.navigationItem();
        let current = borrowed.screen(4).unwrap().controller.navigationItem();
        assert_eq!(
            covered.backButtonTitle().map(|title| title.to_string()),
            Some("Mail".to_string())
        );
        assert_eq!(
            covered.backButtonDisplayMode(),
            UINavigationItemBackButtonDisplayMode::Minimal
        );
        assert_eq!(
            covered.largeTitleDisplayMode(),
            UINavigationItemLargeTitleDisplayMode::Automatic
        );
        assert!(!current.hidesBackButton());
        assert!(current.leftItemsSupplementBackButton());
        assert_eq!(
            current.largeTitleDisplayMode(),
            UINavigationItemLargeTitleDisplayMode::Never
        );
        assert!(current.scrollEdgeAppearance().is_some());
        assert!(covered.scrollEdgeAppearance().is_none());
        let standard = covered.standardAppearance().expect("standard appearance");
        assert!(standard.backgroundColor().is_some());
        assert!(standard.shadowColor().is_none());
        assert!(
            borrowed
                .stack(2)
                .unwrap()
                .nav
                .navigationBar()
                .prefersLargeTitles()
        );

        let titles = leading_titles(&covered);
        assert_eq!(titles, vec!["Outer".to_string(), "Inner".to_string()]);

        if covered.respondsToSelector(sel!(subtitle)) {
            // SAFETY: the item responds to `subtitle`.
            let subtitle: Option<Retained<NSString>> = unsafe { msg_send![&*covered, subtitle] };
            assert_eq!(
                subtitle.as_deref().map(|text| text.to_string()),
                Some("12 unread".to_string())
            );
            assert!(covered.titleView().is_none());
        } else {
            assert!(covered.titleView().is_some());
        }
    }

    let mut below = nlayer_core::ScreenConfig::default();
    below.title = Some("Inbox".into());
    below.back_button_title = Some("Box".into());
    below.back_button_display_mode = BackButtonDisplayMode::Generic;
    below.large_title = LargeTitleDisplayMode::Automatic;
    below.animation = nlayer_core::ScreenAnimation::None;
    apply(
        &mounter,
        vec![MountItem::ScreenConfig {
            id: 3,
            config: Box::new(below),
        }],
    );
    let borrowed = mounter.borrow();
    let covered = borrowed.screen(3).unwrap().controller.navigationItem();
    assert_eq!(
        covered.backButtonTitle().map(|title| title.to_string()),
        Some("Box".to_string())
    );
    assert_eq!(
        covered.backButtonDisplayMode(),
        UINavigationItemBackButtonDisplayMode::Generic
    );
}

fn leading_titles(item: &objc2_ui_kit::UINavigationItem) -> Vec<String> {
    use objc2::sel;

    let title = |button: &objc2_ui_kit::UIBarButtonItem| {
        button
            .title()
            .map(|text| text.to_string())
            .unwrap_or_default()
    };
    if item.respondsToSelector(sel!(leadingItemGroups)) {
        let mut titles = Vec::new();
        for group in item.leadingItemGroups().iter() {
            let buttons = group.barButtonItems();
            titles.extend(buttons.iter().map(|button| title(&button)));
        }
        titles
    } else {
        item.leftBarButtonItems()
            .map(|items| items.iter().map(|button| title(&button)).collect())
            .unwrap_or_default()
    }
}

fn a_back_swipe_follows_the_mounted_policy() {
    use objc2_ui_kit::UIGestureRecognizer;

    let mounter = mounter();
    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 2,
                kind: ViewKind::Stack,
            },
            MountItem::Insert {
                parent: ROOT_ID,
                child: 2,
                before: None,
            },
            MountItem::Frame {
                id: 2,
                frame: frame(0.0, 0.0, 320.0, 640.0),
            },
        ],
    );
    let mut root = nlayer_core::ScreenConfig::default();
    root.header_visible = false;
    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 3,
                kind: ViewKind::Screen,
            },
            MountItem::ScreenConfig {
                id: 3,
                config: Box::new(root),
            },
            MountItem::Insert {
                parent: 2,
                child: 3,
                before: None,
            },
        ],
    );

    // iOS 26 can start a back swipe from the content recognizer too: it
    // carries the same delegate as the edge recognizer.
    fn recognizers(mounter: &Rc<RefCell<Mounter>>) -> Vec<Retained<UIGestureRecognizer>> {
        let borrowed = mounter.borrow();
        let stack = borrowed.stack(2).expect("stack");
        let mut found = Vec::new();
        if let Some(edge) = stack.nav.interactivePopGestureRecognizer() {
            found.push(edge);
        }
        if let Some(content) = stack.nav.interactiveContentPopGestureRecognizer() {
            found.push(content);
        }
        found
    }
    fn begins(mounter: &Rc<RefCell<Mounter>>) -> Vec<bool> {
        recognizers(mounter)
            .iter()
            .map(|recognizer| {
                recognizer
                    .delegate()
                    .expect("the stack installs its delegate")
                    .gestureRecognizerShouldBegin(recognizer)
            })
            .collect()
    }

    // The root keeps the user.
    assert!(!mounter.borrow().stack_can_pop(2));
    if mounter
        .borrow()
        .stack(2)
        .expect("stack")
        .nav
        .respondsToSelector(objc2::sel!(interactiveContentPopGestureRecognizer))
    {
        // iOS 26+: the content recognizer takes part too.
        assert_eq!(recognizers(&mounter).len(), 2);
    }
    assert_eq!(begins(&mounter), vec![false; recognizers(&mounter).len()]);

    let mut page = nlayer_core::ScreenConfig::default();
    page.animation = nlayer_core::ScreenAnimation::None;
    page.header_visible = false;
    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 4,
                kind: ViewKind::Screen,
            },
            MountItem::ScreenConfig {
                id: 4,
                config: Box::new(page.clone()),
            },
            MountItem::Insert {
                parent: 2,
                child: 4,
                before: None,
            },
        ],
    );

    // `popOnSwipe` and `popOnBack` allow it on a page above the root.
    assert_eq!(begins(&mounter), vec![true; recognizers(&mounter).len()]);

    page.gesture_enabled = false;
    apply(
        &mounter,
        vec![MountItem::ScreenConfig {
            id: 4,
            config: Box::new(page.clone()),
        }],
    );
    assert_eq!(begins(&mounter), vec![false; recognizers(&mounter).len()]);

    page.gesture_enabled = true;
    page.prevent_back = true;
    apply(
        &mounter,
        vec![MountItem::ScreenConfig {
            id: 4,
            config: Box::new(page.clone()),
        }],
    );
    assert_eq!(begins(&mounter), vec![false; recognizers(&mounter).len()]);
}

fn the_modal_presents_over_the_root() {
    use objc2_ui_kit::{UIPresentationController, UIViewController};

    let mounter = mounter();
    let mtm = MainThreadMarker::new().expect("the iOS host test runner runs on the main thread");

    let window = crate::app::test_window(mtm, 320.0, 640.0);

    let controller = UIViewController::new(mtm);
    let root_view = mounter.borrow().view_of(ROOT_ID).unwrap();
    controller.view().unwrap().addSubview(&root_view);

    window.setRootViewController(Some(&controller));
    window.makeKeyAndVisible();
    controller.view().unwrap().layoutIfNeeded();

    // Create modal
    let config = nlayer_core::ModalConfig {
        animation: nlayer_core::ModalAnimation::None,
        presentation: nlayer_core::ModalPresentation::PageSheet,
        close_on_swipe: false,
        close_on_escape: false,
        close_on_backdrop_press: false,
    };

    apply(
        &mounter,
        vec![
            MountItem::Create {
                id: 5,
                kind: ViewKind::Modal,
            },
            MountItem::ModalConfig { id: 5, config },
            MountItem::Insert {
                parent: ROOT_ID,
                child: 5,
                before: None,
            },
        ],
    );

    // Assert modal configuration
    {
        let borrowed = mounter.borrow();
        let modal = borrowed.modal(5).unwrap();
        assert!(modal.controller.isModalInPresentation());

        // Note: UIKit presentation may be asynchronous. The `presented` flag is set immediately,
        // but `controller.presentedViewController()` might not return the modal controller
        // until the presentation animation completes (even with ModalAnimation::None).
        // We only assert the `presented` flag here.
        assert!(modal.presented, "modal should be marked as presented");

        // `closeOnSwipe` false: a swipe asks (`onRequestClose`) but the
        // sheet stays.
        let presentation =
            UIPresentationController::initWithPresentedViewController_presentingViewController(
                UIPresentationController::alloc(mtm),
                &modal.controller,
                Some(&controller),
            );
        let accepts: bool = unsafe {
            objc2::msg_send![
                &*modal.controller,
                presentationControllerShouldDismiss: &*presentation
            ]
        };
        assert!(!accepts);
    }

    // `closeOnSwipe` applies while presented.
    apply(
        &mounter,
        vec![MountItem::ModalConfig {
            id: 5,
            config: nlayer_core::ModalConfig {
                close_on_swipe: true,
                ..config
            },
        }],
    );
    {
        let borrowed = mounter.borrow();
        let modal = borrowed.modal(5).unwrap();
        assert!(!modal.controller.isModalInPresentation());
        let presentation =
            UIPresentationController::initWithPresentedViewController_presentingViewController(
                UIPresentationController::alloc(mtm),
                &modal.controller,
                Some(&controller),
            );
        let accepts: bool = unsafe {
            objc2::msg_send![
                &*modal.controller,
                presentationControllerShouldDismiss: &*presentation
            ]
        };
        assert!(accepts);
    }

    // Remove modal
    apply(&mounter, vec![MountItem::Remove { id: 5 }]);

    {
        let borrowed = mounter.borrow();
        let modal = borrowed.modal(5).unwrap();
        assert!(
            !modal.presented,
            "modal should no longer be presented after removal"
        );
    }
}

fn hit_tag(view: &UIView, x: f64, y: f64) -> isize {
    view.hitTest_withEvent(CGPoint::new(x, y), None)
        .map(|hit| hit.tag())
        .unwrap_or(-1)
}

fn a_dialog_popup_lets_presses_through_to_the_backdrop() {
    let mounter = mounter();
    let full = frame(0.0, 0.0, 320.0, 640.0);
    let mut items = vec![
        MountItem::Create {
            id: 5,
            kind: ViewKind::Modal,
        },
        MountItem::Insert {
            parent: ROOT_ID,
            child: 5,
            before: None,
        },
        MountItem::Frame { id: 5, frame: full },
    ];
    for (id, parent, at) in [
        (6, 5, full),
        (7, 5, full),
        (8, 7, frame(80.0, 200.0, 160.0, 80.0)),
    ] {
        items.push(MountItem::Create {
            id,
            kind: ViewKind::View,
        });
        items.push(MountItem::Insert {
            parent,
            child: id,
            before: None,
        });
        items.push(MountItem::Frame { id, frame: at });
    }
    apply(&mounter, items);

    let modal = mounter.borrow().view(5).unwrap().retain();
    assert_eq!(
        hit_tag(&modal, 100.0, 240.0),
        8,
        "the card still takes presses"
    );
    assert_eq!(
        hit_tag(&modal, 10.0, 10.0),
        6,
        "empty popup space hits the backdrop"
    );

    apply(&mounter, vec![MountItem::Remove { id: 6 }]);
    let modal = mounter.borrow().view(5).unwrap().retain();
    assert_eq!(
        hit_tag(&modal, 10.0, 10.0),
        7,
        "a popup alone takes presses"
    );
    assert_eq!(
        hit_tag(&modal, 100.0, 240.0),
        8,
        "the card still takes presses"
    );
}
