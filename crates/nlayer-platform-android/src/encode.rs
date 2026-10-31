//! Encodes a [`MountBatch`] for the Android UI thread: a little-endian
//! stream of 32-bit ints and floats, plus an object table for strings.
//! One hand-off per commit.
//!
//! Opcodes and the words after them are the contract the Java mounter
//! reads. Units are points (dp). A `StaticLayout` measured on the engine
//! thread is an object-table entry; a layout this process did not measure
//! is omitted, and the rest of the batch still encodes.
//!
//! Copied from `nsh-platform-android/src/encode.rs`. Animation, gestures,
//! refresh, zoom, paging, shadows, gradients and the other residual style
//! fields are not encoded. Navigation containers (Stack, Screen, Modal,
//! HeaderItem) and their configurations are encoded.

use nlayer_core::geometry::{DefiniteLength, Points, REM_SIZE};
use nlayer_core::style::{BorderStyle, Color, PositionAxis, TextAlign, TextStyle, ViewStyle};
use nlayer_core::text::AttributedText;
use nlayer_core::{
    Command, EventMask, ImageSource, InputConfig, Interaction, KeyboardDismiss, KeyboardTaps,
    KeyboardType, MountBatch, MountItem, NodeId, ResizeMode, ViewKind,
};

const CREATE: i32 = 0;
const INSERT: i32 = 1;
const REMOVE: i32 = 2;
const DELETE: i32 = 3;
const FRAME: i32 = 4;
const VIEW_STYLE: i32 = 5;
#[allow(dead_code)] // the Android text engine writes this opcode
const TEXT: i32 = 6;
const EVENTS: i32 = 7;
const IMAGE: i32 = 8;
const SCROLL_CONFIG: i32 = 9;
const SCROLL_CONTENT: i32 = 10;
const SCROLL_BY: i32 = 11;
const STATE_STYLE: i32 = 12;
const INPUT: i32 = 13;
const INPUT_VALUE: i32 = 14;
const INPUT_CONTENT: i32 = 15;
const TEST_ID: i32 = 16;
const COMMAND: i32 = 17;
const ACCESSIBILITY: i32 = 18;
const LONG_PRESS_DELAY: i32 = 19;
const SCREEN_CONFIG: i32 = 20;
const MODAL_CONFIG: i32 = 21;

/// A `StaticLayout` measured on the engine thread and drawn on the UI thread.
#[cfg(target_os = "android")]
pub(crate) struct AndroidLayout(pub(crate) jni::objects::GlobalRef);

/// One entry of the object table. Strings become Java `String`s. A text
/// layout becomes the `StaticLayout` the UI thread draws; only the Android
/// text engine produces one.
#[derive(Debug)]
pub enum Object {
    /// A string (an image URI, placeholder, input value or `testID`).
    Str(String),
    /// A `StaticLayout` measured on the engine thread.
    #[cfg(target_os = "android")]
    Layout(jni::objects::GlobalRef),
}

/// An encoded batch.
#[derive(Debug, Default)]
pub struct Encoded {
    /// Little-endian 32-bit words.
    pub bytes: Vec<u8>,
    /// Strings and, on Android, measured text layouts. A missing string is
    /// index `-1`.
    pub objects: Vec<Object>,
}

/// Encodes `batch`. Items stay in order. A text item whose layout was not
/// measured here is left out.
pub fn encode(batch: MountBatch) -> Encoded {
    let mut e = Encoded {
        bytes: Vec::with_capacity(batch.items.len() * 16),
        objects: Vec::new(),
    };
    for item in batch.items {
        match item {
            MountItem::Create { id, kind } => {
                e.int(CREATE);
                e.id(id);
                e.int(kind_code(kind));
            }
            MountItem::Insert {
                parent,
                child,
                before,
            } => {
                e.int(INSERT);
                e.id(parent);
                e.id(child);
                e.int(before.map_or(-1, |id| id as i32));
            }
            MountItem::Remove { id } => {
                e.int(REMOVE);
                e.id(id);
            }
            MountItem::Delete { id } => {
                e.int(DELETE);
                e.id(id);
            }
            MountItem::Frame { id, frame } => {
                e.int(FRAME);
                e.id(id);
                e.float(frame.origin.x.0);
                e.float(frame.origin.y.0);
                e.float(frame.size.width.0);
                e.float(frame.size.height.0);
            }
            MountItem::Animation(_) | MountItem::AnimationBindings { .. } => {
                // The UI-thread animation graph consumes these.
            }
            MountItem::LongPressDelay { id, delay } => {
                e.int(LONG_PRESS_DELAY);
                e.id(id);
                e.float(delay.unwrap_or(-1.0));
            }
            MountItem::ViewStyle { id, style } => {
                e.int(VIEW_STYLE);
                e.id(id);
                view_style(&mut e, &style);
            }
            MountItem::StateStyle {
                id,
                interaction,
                view,
                text,
            } => {
                e.int(STATE_STYLE);
                e.id(id);
                e.int(match interaction {
                    Interaction::Active => 0,
                    Interaction::Hover => 1,
                    Interaction::Focus => 2,
                });
                let present =
                    view.as_ref() != &ViewStyle::default() || text != TextStyle::default();
                e.bool(present);
                if present {
                    view_style(&mut e, &view);
                    opt_color(&mut e, text.color);
                }
            }
            MountItem::Text {
                id,
                text,
                layout,
                content,
            } => {
                // The UI thread draws the `StaticLayout` measured on the
                // engine thread. A stand-in layout is not that object.
                #[cfg(target_os = "android")]
                if let Some(measured) = layout.downcast::<AndroidLayout>() {
                    let index = e.objects.len() as i32;
                    e.objects.push(Object::Layout(measured.0.clone()));
                    write_text(&mut e, id, index, content, &text);
                }
                #[cfg(not(target_os = "android"))]
                let _ = (id, text, layout, content);
            }
            MountItem::Events { id, events } => {
                e.int(EVENTS);
                e.id(id);
                mask(&mut e, events);
            }
            MountItem::Image {
                id,
                source,
                resize_mode,
            } => {
                e.int(IMAGE);
                e.id(id);
                match source {
                    Some(ImageSource { uri, .. }) => e.string(uri.to_string()),
                    None => e.int(-1),
                }
                e.int(match resize_mode {
                    ResizeMode::Cover => 0,
                    ResizeMode::Contain => 1,
                    ResizeMode::Stretch => 2,
                    ResizeMode::Center => 3,
                    ResizeMode::ScaleDown => 4,
                });
            }
            MountItem::ScrollConfig {
                id,
                horizontal,
                taps,
                dismiss,
            } => {
                e.int(SCROLL_CONFIG);
                e.id(id);
                e.bool(horizontal);
                e.int(match taps {
                    KeyboardTaps::Never => 0,
                    KeyboardTaps::Always => 1,
                    KeyboardTaps::Handled => 2,
                });
                e.int(match dismiss {
                    KeyboardDismiss::None => 0,
                    KeyboardDismiss::OnDrag => 1,
                    KeyboardDismiss::Interactive => 2,
                });
            }
            MountItem::ScrollContent { id, size } => {
                e.int(SCROLL_CONTENT);
                e.id(id);
                e.float(size.width.0);
                e.float(size.height.0);
            }
            MountItem::ScrollBy { id, dx, dy } => {
                e.int(SCROLL_BY);
                e.id(id);
                e.float(dx.0);
                e.float(dy.0);
            }
            MountItem::Input { id, config } => {
                e.int(INPUT);
                e.id(id);
                input(&mut e, &config);
            }
            MountItem::InputValue { id, text, count } => {
                e.int(INPUT_VALUE);
                e.id(id);
                e.int(count as i32);
                e.string(text);
            }
            MountItem::InputContent { id, content } => {
                e.int(INPUT_CONTENT);
                e.id(id);
                e.float(content.origin.x.0);
                e.float(content.origin.y.0);
                e.float(content.size.width.0);
                e.float(content.size.height.0);
            }
            MountItem::TestId { id, test_id } => {
                e.int(TEST_ID);
                e.id(id);
                match test_id {
                    Some(name) => e.string(name.to_string()),
                    None => e.int(-1),
                }
            }
            MountItem::Accessibility { id, accessibility } => {
                let a = accessibility.as_ref();
                e.int(ACCESSIBILITY);
                e.id(id);
                e.string(a.role.name().to_string());
                match &a.label {
                    Some(label) => e.string(label.to_string()),
                    None => e.int(-1),
                }
                match &a.hint {
                    Some(hint) => e.string(hint.to_string()),
                    None => e.int(-1),
                }
                let mut flags = 0;
                if a.hidden {
                    flags |= 1;
                }
                if a.state.disabled {
                    flags |= 2;
                }
                if a.state.selected {
                    flags |= 4;
                }
                e.int(flags);
                e.int(match a.state.checked {
                    None => 0,
                    Some(nlayer_core::Checked::False) => 1,
                    Some(nlayer_core::Checked::True) => 2,
                    Some(nlayer_core::Checked::Mixed) => 3,
                });
                e.int(match a.state.expanded {
                    None => 0,
                    Some(false) => 1,
                    Some(true) => 2,
                });
                let value = a.value.as_ref();
                e.float(value.and_then(|v| v.min).unwrap_or(f32::NAN));
                e.float(value.and_then(|v| v.max).unwrap_or(f32::NAN));
                e.float(value.and_then(|v| v.now).unwrap_or(f32::NAN));
                match value.and_then(|v| v.text.as_deref()) {
                    Some(text) => e.string(text.to_string()),
                    None => e.int(-1),
                }
            }
            MountItem::Command { id, command } => {
                e.int(COMMAND);
                e.id(id);
                match command {
                    Command::ScrollTo { x, y, animated } => {
                        e.int(0);
                        e.float(x.0);
                        e.float(y.0);
                        e.bool(animated);
                    }
                    Command::Blur => e.int(1),
                    Command::SetSelection { start, end } => {
                        e.int(2);
                        e.int(start as i32);
                        e.int(end as i32);
                    }
                    Command::ScrollToIndex { .. }
                    | Command::ScrollToEnd { .. }
                    | Command::Back
                    | Command::GoBack => {
                        // The engine handles navigation commands.
                    }
                }
            }
            MountItem::ScreenConfig { id, config } => {
                e.int(SCREEN_CONFIG);
                e.id(id);
                match &config.title {
                    Some(title) => e.string(title.to_string()),
                    None => e.int(-1),
                }
                e.bool(config.header_visible);
                e.bool(config.header_transparent);
                opt_color(&mut e, config.tint_color);
                opt_color(&mut e, config.background_color);
                opt_color(&mut e, config.title_color);
                e.bool(config.back_visible);
                e.bool(config.gesture_enabled);
                e.bool(config.prevent_back);
                e.int(screen_animation_code(config.animation));
                e.int(config.left.map_or(-1, |id| id as i32));
                e.int(config.right.map_or(-1, |id| id as i32));
                e.int(config.title_view.map_or(-1, |id| id as i32));
                e.float(config.header_height.0);
            }
            MountItem::ModalConfig { id, config } => {
                e.int(MODAL_CONFIG);
                e.id(id);
                e.int(modal_animation_code(config.animation));
                e.int(modal_presentation_code(config.presentation));
                e.bool(config.close_on_escape);
                e.bool(config.close_on_backdrop_press);
                e.bool(config.close_on_swipe);
            }
        }
    }
    e
}

fn kind_code(kind: ViewKind) -> i32 {
    match kind {
        ViewKind::View => 0,
        ViewKind::Text => 1,
        ViewKind::Image => 2,
        ViewKind::Scroll => 3,
        ViewKind::Input => 4,
        ViewKind::Stack => 5,
        ViewKind::Screen => 6,
        ViewKind::Modal => 7,
        ViewKind::HeaderItem => 8,
    }
}

fn screen_animation_code(animation: nlayer_core::ScreenAnimation) -> i32 {
    match animation {
        nlayer_core::ScreenAnimation::Default => 0,
        nlayer_core::ScreenAnimation::Fade => 1,
        nlayer_core::ScreenAnimation::None => 2,
    }
}

fn modal_animation_code(animation: nlayer_core::ModalAnimation) -> i32 {
    match animation {
        nlayer_core::ModalAnimation::Slide => 0,
        nlayer_core::ModalAnimation::Fade => 1,
        nlayer_core::ModalAnimation::None => 2,
    }
}

fn modal_presentation_code(presentation: nlayer_core::ModalPresentation) -> i32 {
    match presentation {
        nlayer_core::ModalPresentation::FullScreen => 0,
        nlayer_core::ModalPresentation::PageSheet => 1,
        nlayer_core::ModalPresentation::FormSheet => 2,
    }
}

/// `0xRRGGBBAA` as Android's `0xAARRGGBB`.
fn argb(c: Color) -> i32 {
    c.0.rotate_right(8) as i32
}

fn opt_color(e: &mut Encoded, color: Option<Color>) {
    match color {
        Some(color) => {
            e.bool(true);
            e.int(argb(color));
        }
        None => e.bool(false),
    }
}

/// Visual style the mounter paints: fill, radii, border, opacity,
/// overflow, the decomposed transform, hidden, transform origin and
/// `objectPosition`. Shadows, gradients and the other residual fields
/// are not written.
fn view_style(e: &mut Encoded, style: &ViewStyle) {
    e.int(argb(style.background_color));
    let radii = style.radii();
    for radius in [
        radii.top_left,
        radii.top_right,
        radii.bottom_right,
        radii.bottom_left,
    ] {
        e.float(radius.0);
    }
    e.bool(radii.continuous);
    let borders = &style.borders;
    for width in borders.widths.to_array() {
        e.float(width.0);
    }
    for color in borders.colors {
        e.int(argb(color));
    }
    e.int(match borders.style {
        BorderStyle::Solid => 0,
        BorderStyle::Dashed => 1,
        BorderStyle::Dotted => 2,
    });
    e.float(style.opacity);
    e.bool(style.overflow_hidden);
    for value in view_transform(&style.transform) {
        e.float(value);
    }
    e.bool(style.hidden);
    length_axis(e, style.transform_origin.x);
    length_axis(e, style.transform_origin.y);
    position_axis(e, style.object_position.x);
    position_axis(e, style.object_position.y);
    let ops = style.transform.ops();
    e.int(ops.len() as i32);
    for op in ops {
        e.int(transform_kind(op.kind));
        e.float(op.value);
    }
}

/// A transform list as Android view properties: translation x / y (dp),
/// scale x / y, rotation, rotation x / y (degrees) and the perspective
/// term (`-1 / distance`, 0 for none). Degrees are rounded to 1e-4 so a
/// whole degree survives the radians round trip.
fn view_transform(transform: &nlayer_core::transform::Transform) -> [f32; 8] {
    match transform.matrix(|_| None).decompose() {
        Some(decomposed) => [
            decomposed.translate[0].0,
            decomposed.translate[1].0,
            decomposed.scale[0] as f32,
            decomposed.scale[1] as f32,
            degrees(decomposed.rotate[2]),
            degrees(decomposed.rotate[0]),
            degrees(decomposed.rotate[1]),
            decomposed
                .perspective
                .map_or(0.0, |distance| -1.0 / distance.0),
        ],
        None => [0.0; 8],
    }
}

fn transform_kind(kind: nlayer_core::transform::TransformKind) -> i32 {
    use nlayer_core::transform::TransformKind;
    match kind {
        TransformKind::TranslateX => 0,
        TransformKind::TranslateY => 1,
        TransformKind::Scale => 2,
        TransformKind::ScaleX => 3,
        TransformKind::ScaleY => 4,
        TransformKind::Rotate => 5,
        TransformKind::RotateX => 6,
        TransformKind::RotateY => 7,
        TransformKind::Perspective => 10,
        TransformKind::TranslateXFraction => 11,
        TransformKind::TranslateYFraction => 12,
    }
}
fn degrees(angle: nlayer_core::geometry::Radians) -> f32 {
    (angle.to_degrees() * 1e4).round() / 1e4
}

fn length_axis(e: &mut Encoded, length: DefiniteLength) {
    match length {
        DefiniteLength::Absolute(points) => {
            e.int(0);
            e.float(points.to_points(REM_SIZE).0);
        }
        DefiniteLength::Fraction(fraction) => {
            e.int(1);
            e.float(fraction.0);
        }
    }
}

fn position_axis(e: &mut Encoded, axis: PositionAxis) {
    e.bool(axis.from_end);
    length_axis(e, axis.offset);
}

/// An [`EventMask`] as four ints, lowest first (two Java longs).
fn mask(e: &mut Encoded, events: EventMask) {
    let bits = events.bits();
    e.int(bits as i32);
    e.int((bits >> 32) as i32);
    e.int((bits >> 64) as i32);
    e.int((bits >> 96) as i32);
}

fn input(e: &mut Encoded, config: &InputConfig) {
    let style = &config.style;
    e.float(style.font_size.0);
    e.int(style.font_weight as i32);
    e.bool(style.italic);
    e.int(argb(style.color));
    e.float(style.letter_spacing.0);
    e.float(style.line_height.0);
    e.int(align_code(config.align));
    e.string(config.placeholder.clone());
    e.bool(config.secure);
    e.int(match config.keyboard {
        KeyboardType::Text => 0,
        KeyboardType::Email => 1,
        KeyboardType::Number => 2,
        KeyboardType::Phone => 3,
    });
    e.bool(config.disabled);
    e.bool(config.rtl);
    e.bool(config.multiline);
}

fn align_code(align: TextAlign) -> i32 {
    match align {
        TextAlign::Auto => 0,
        TextAlign::Left => 1,
        TextAlign::Center => 2,
        TextAlign::Right => 3,
        TextAlign::Justify => 4,
    }
}

impl Encoded {
    fn int(&mut self, value: i32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn id(&mut self, id: NodeId) {
        self.int(id as i32);
    }

    fn float(&mut self, value: f32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn bool(&mut self, value: bool) {
        self.int(i32::from(value));
    }

    fn string(&mut self, value: String) {
        let index = self.objects.len() as i32;
        self.objects.push(Object::Str(value));
        self.int(index);
    }
}

/// Words a text item appends after its layout index, once the Android
/// text engine supplies that layout. Kept next to the encoder so the
/// mounter and the engine thread agree.
#[allow(dead_code)] // the Android text engine writes text once it has a layout
pub(crate) fn write_text(
    encoded: &mut Encoded,
    id: NodeId,
    layout: i32,
    content: nlayer_core::geometry::Bounds<Points>,
    text: &AttributedText,
) {
    encoded.int(TEXT);
    encoded.id(id);
    encoded.int(layout);
    encoded.float(content.origin.x.0);
    encoded.float(content.origin.y.0);
    encoded.float(content.size.width.0);
    encoded.float(content.size.height.0);
    encoded.int(text.links.len() as i32);
    for link in &text.links {
        encoded.int(link.start as i32);
        encoded.int(link.end as i32);
        encoded.id(link.node);
        mask(encoded, link.events);
    }
    encoded.int(text.test_ids.len() as i32);
    for span in &text.test_ids {
        encoded.int(span.start as i32);
        encoded.int(span.end as i32);
        encoded.string(span.test_id.to_string());
    }
    encoded.int(text.spans.len() as i32);
    for span in &text.spans {
        encoded.int(span.start as i32);
        encoded.int(span.end as i32);
        encoded.int(span.style.font_weight as i32);
        encoded.bool(span.style.italic);
        encoded.int(argb(span.style.color));
        encoded.float(span.style.font_size.0);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nlayer_core::geometry::{Bounds, Size, point, px, size};
    use nlayer_core::style::{BorderStyle, Borders, Color, TextStyle, ViewStyle};
    use nlayer_core::text::{AttributedText, SpanTestId, TextLayout, TextLink};
    use nlayer_core::{
        Command, EventKind, EventMask, ImageSource, InputConfig, Interaction, KeyboardDismiss,
        KeyboardTaps, KeyboardType, MountBatch, MountItem, ResizeMode, ViewKind,
    };

    use super::{CREATE, Encoded, FRAME, IMAGE, INPUT, Object, SCROLL_CONFIG, TEST_ID, TEXT};

    struct Words<'a> {
        bytes: &'a [u8],
    }

    impl Words<'_> {
        fn int(&mut self) -> i32 {
            let word = self.bytes[..4].try_into().unwrap();
            self.bytes = &self.bytes[4..];
            i32::from_le_bytes(word)
        }

        fn float(&mut self) -> f32 {
            f32::from_bits(self.int() as u32)
        }

        fn done(&self) -> bool {
            self.bytes.is_empty()
        }
    }

    fn read(encoded: &Encoded) -> Words<'_> {
        Words {
            bytes: &encoded.bytes,
        }
    }

    fn span(start: u32, end: u32, id: &str) -> SpanTestId {
        let mut span = SpanTestId::default();
        span.start = start;
        span.end = end;
        span.test_id = Arc::from(id);
        span
    }

    fn object(encoded: &Encoded, index: i32) -> &str {
        match &encoded.objects[index as usize] {
            Object::Str(value) => value,
        }
    }
    fn skip_style(words: &mut Words<'_>) {
        let _ = words.int();
        for _ in 0..4 {
            let _ = words.float();
        }
        let _ = words.int();
        for _ in 0..4 {
            let _ = words.float();
        }
        for _ in 0..4 {
            let _ = words.int();
        }
        let _ = words.int();
        let _ = words.float();
        let _ = words.int();
        for _ in 0..8 {
            let _ = words.float();
        }
        let _ = words.int();
        for _ in 0..2 {
            let _ = words.int();
            let _ = words.float();
        }
        for _ in 0..2 {
            let _ = words.int();
            let _ = words.float();
        }
        let ops = words.int();
        for _ in 0..ops {
            let _ = words.int();
            let _ = words.float();
        }
    }
    fn style() -> ViewStyle {
        let mut style = ViewStyle::default();
        style.background_color = Color::rgba(1, 2, 3, 4);
        style.border_radius = px(8.0);
        style.borders = Borders {
            widths: nlayer_core::geometry::Edges::all(px(1.0)),
            colors: [Color::rgba(9, 8, 7, 255); 4],
            style: BorderStyle::Dashed,
        };
        style.opacity = 0.5;
        style.overflow_hidden = true;
        style
    }

    #[test]
    fn create_insert_and_frame_keep_order() {
        let encoded = super::encode(MountBatch {
            items: vec![
                MountItem::Create {
                    id: 2,
                    kind: ViewKind::View,
                },
                MountItem::Insert {
                    parent: 1,
                    child: 2,
                    before: None,
                },
                MountItem::Frame {
                    id: 2,
                    frame: Bounds {
                        origin: point(px(3.0), px(4.0)),
                        size: size(px(10.0), px(20.0)),
                    },
                },
                MountItem::Delete { id: 2 },
            ],
        });
        let mut words = read(&encoded);
        assert_eq!(words.int(), CREATE);
        assert_eq!(words.int(), 2);
        assert_eq!(words.int(), 0);
        assert_eq!(words.int(), super::INSERT);
        assert_eq!((words.int(), words.int(), words.int()), (1, 2, -1));
        assert_eq!(words.int(), FRAME);
        assert_eq!(words.int(), 2);
        assert_eq!(
            (words.float(), words.float(), words.float(), words.float()),
            (3.0, 4.0, 10.0, 20.0)
        );
        assert_eq!(words.int(), super::DELETE);
        assert_eq!(words.int(), 2);
        assert!(words.done());
        assert!(encoded.objects.is_empty());
    }

    #[test]
    fn view_style_packs_color_and_the_identity_transform() {
        let encoded = super::encode(MountBatch {
            items: vec![MountItem::ViewStyle {
                id: 4,
                style: Box::new(style()),
            }],
        });
        let mut words = read(&encoded);
        assert_eq!(words.int(), super::VIEW_STYLE);
        assert_eq!(words.int(), 4);
        assert_eq!(words.int(), 0x0401_0203);
        assert_eq!(
            (words.float(), words.float(), words.float(), words.float()),
            (8.0, 8.0, 8.0, 8.0)
        );
        assert_eq!(words.int(), 0);
        assert_eq!(
            (words.float(), words.float(), words.float(), words.float()),
            (1.0, 1.0, 1.0, 1.0)
        );
        assert_eq!(
            (words.int(), words.int(), words.int(), words.int()),
            (
                0xFF09_0807u32 as i32,
                0xFF09_0807u32 as i32,
                0xFF09_0807u32 as i32,
                0xFF09_0807u32 as i32
            )
        );
        assert_eq!(words.int(), 1);
        assert_eq!(words.float(), 0.5);
        assert_eq!(words.int(), 1);
        assert_eq!(
            (
                words.float(),
                words.float(),
                words.float(),
                words.float(),
                words.float(),
                words.float(),
                words.float(),
                words.float()
            ),
            (0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0)
        );
        assert_eq!(words.int(), 0);
        assert_eq!(words.int(), 1);
        assert_eq!(words.float(), 0.5);
        assert_eq!(words.int(), 1);
        assert_eq!(words.float(), 0.5);
        assert_eq!(words.int(), 0);
        assert_eq!(words.int(), 1);
        assert_eq!(words.float(), 0.5);
        assert_eq!(words.int(), 0);
        assert_eq!(words.int(), 1);
        assert_eq!(words.float(), 0.5);
        assert_eq!(words.int(), 0);
        assert!(words.done());
    }

    #[test]
    fn a_default_state_style_clears_and_a_color_sets() {
        let cleared = super::encode(MountBatch {
            items: vec![MountItem::StateStyle {
                id: 3,
                interaction: Interaction::Hover,
                view: Box::default(),
                text: TextStyle::default(),
            }],
        });
        let mut words = read(&cleared);
        assert_eq!(words.int(), super::STATE_STYLE);
        assert_eq!((words.int(), words.int(), words.int()), (3, 1, 0));
        assert!(words.done());

        let text = TextStyle {
            color: Some(Color::rgba(255, 0, 0, 255)),
            ..TextStyle::default()
        };
        let set = super::encode(MountBatch {
            items: vec![MountItem::StateStyle {
                id: 3,
                interaction: Interaction::Active,
                view: Box::new(style()),
                text,
            }],
        });
        let mut words = read(&set);
        assert_eq!(words.int(), super::STATE_STYLE);
        assert_eq!((words.int(), words.int(), words.int()), (3, 0, 1));
        skip_style(&mut words);
        assert_eq!(words.int(), 1);
        assert_eq!(words.int(), 0xFFFF_0000u32 as i32);
        assert!(words.done());
    }

    #[test]
    fn scroll_image_input_and_names_share_the_object_table() {
        let mut events = EventMask::default();
        events.set(EventKind::Press, true);
        let encoded = super::encode(MountBatch {
            items: vec![
                MountItem::Create {
                    id: 5,
                    kind: ViewKind::Scroll,
                },
                MountItem::ScrollConfig {
                    id: 5,
                    horizontal: true,
                    taps: KeyboardTaps::Never,
                    dismiss: KeyboardDismiss::None,
                },
                MountItem::ScrollContent {
                    id: 5,
                    size: Size {
                        width: px(40.0),
                        height: px(10.0),
                    },
                },
                MountItem::ScrollBy {
                    id: 5,
                    dx: px(2.0),
                    dy: px(0.0),
                },
                MountItem::Command {
                    id: 5,
                    command: Command::ScrollTo {
                        x: px(8.0),
                        y: px(0.0),
                        animated: false,
                    },
                },
                MountItem::Image {
                    id: 6,
                    source: Some(ImageSource {
                        uri: Arc::from("asset:/img/a.png"),
                        ..ImageSource::default()
                    }),
                    resize_mode: ResizeMode::Contain,
                },
                MountItem::Events { id: 6, events },
                MountItem::TestId {
                    id: 6,
                    test_id: Some(Arc::from("mark")),
                },
                MountItem::Input {
                    id: 7,
                    config: Box::new(InputConfig {
                        placeholder: "mail".to_owned(),
                        secure: true,
                        keyboard: KeyboardType::Email,
                        disabled: true,
                        rtl: true,
                        ..InputConfig::default()
                    }),
                },
                MountItem::InputValue {
                    id: 7,
                    text: "a".to_owned(),
                    count: 2,
                },
            ],
        });
        let mut words = read(&encoded);
        assert_eq!((words.int(), words.int(), words.int()), (CREATE, 5, 3));
        assert_eq!(words.int(), SCROLL_CONFIG);
        assert_eq!(
            (words.int(), words.int(), words.int(), words.int()),
            (5, 1, 0, 0)
        );
        assert_eq!(words.int(), super::SCROLL_CONTENT);
        assert_eq!(words.int(), 5);
        assert_eq!((words.float(), words.float()), (40.0, 10.0));
        assert_eq!(words.int(), super::SCROLL_BY);
        assert_eq!(words.int(), 5);
        assert_eq!((words.float(), words.float()), (2.0, 0.0));
        assert_eq!(words.int(), super::COMMAND);
        assert_eq!(words.int(), 5);
        assert_eq!(words.int(), 0);
        assert_eq!((words.float(), words.float(), words.int()), (8.0, 0.0, 0));
        assert_eq!(words.int(), IMAGE);
        assert_eq!(words.int(), 6);
        assert_eq!(object(&encoded, words.int()), "asset:/img/a.png");
        assert_eq!(words.int(), 1);
        assert_eq!(words.int(), super::EVENTS);
        assert_eq!(words.int(), 6);
        assert_eq!(words.int(), 1);
        assert_eq!((words.int(), words.int(), words.int()), (0, 0, 0));
        assert_eq!(words.int(), TEST_ID);
        assert_eq!(words.int(), 6);
        assert_eq!(object(&encoded, words.int()), "mark");
        assert_eq!(words.int(), INPUT);
        assert_eq!(words.int(), 7);
        assert_eq!(words.float(), 14.0);
        assert_eq!(words.int(), 400);
        assert_eq!(words.int(), 0);
        assert_eq!(words.int(), 0xFF00_0000u32 as i32);
        assert_eq!((words.float(), words.float()), (0.0, 0.0));
        assert_eq!(words.int(), 0);
        assert_eq!(object(&encoded, words.int()), "mail");
        assert_eq!(
            (
                words.int(),
                words.int(),
                words.int(),
                words.int(),
                words.int()
            ),
            (1, 1, 1, 1, 0)
        );
        assert_eq!(words.int(), super::INPUT_VALUE);
        assert_eq!((words.int(), words.int()), (7, 2));
        assert_eq!(object(&encoded, words.int()), "a");
        assert!(words.done());
    }

    #[test]
    fn a_text_item_without_a_measured_layout_is_omitted() {
        let encoded = super::encode(MountBatch {
            items: vec![
                MountItem::Text {
                    id: 8,
                    text: Arc::new(AttributedText {
                        text: "hi".to_owned(),
                        links: vec![TextLink {
                            start: 0,
                            end: 2,
                            node: 9,
                            events: EventMask::default(),
                        }],
                        test_ids: vec![span(0, 2, "span")],
                        ..AttributedText::default()
                    }),
                    layout: TextLayout(Arc::new(())),
                    content: Bounds::default(),
                },
                MountItem::Remove { id: 8 },
            ],
        });
        let mut words = read(&encoded);
        assert_ne!(words.int(), TEXT);
        assert_eq!(words.int(), 8);
        assert!(words.done());
    }

    #[test]
    fn text_words_name_press_spans_and_test_ids() {
        let mut encoded = Encoded::default();
        let mut events = EventMask::default();
        events.set(EventKind::Press, true);
        super::write_text(
            &mut encoded,
            8,
            0,
            Bounds {
                origin: point(px(1.0), px(2.0)),
                size: size(px(30.0), px(16.0)),
            },
            &AttributedText {
                links: vec![TextLink {
                    start: 0,
                    end: 2,
                    node: 9,
                    events,
                }],
                test_ids: vec![span(1, 2, "span")],
                ..AttributedText::default()
            },
        );
        let mut words = read(&encoded);
        assert_eq!(words.int(), TEXT);
        assert_eq!((words.int(), words.int()), (8, 0));
        assert_eq!(
            (words.float(), words.float(), words.float(), words.float()),
            (1.0, 2.0, 30.0, 16.0)
        );
        assert_eq!(words.int(), 1);
        assert_eq!((words.int(), words.int(), words.int()), (0, 2, 9));
        assert_eq!(words.int(), 1);
        assert_eq!((words.int(), words.int(), words.int()), (0, 0, 0));
        assert_eq!(words.int(), 1);
        assert_eq!((words.int(), words.int()), (1, 2));
        assert_eq!(object(&encoded, words.int()), "span");
        assert_eq!(words.int(), 0);
        assert!(words.done());
    }

    #[test]
    fn navigation_containers_encode_with_correct_kind_codes() {
        let encoded = super::encode(MountBatch {
            items: vec![
                MountItem::Create {
                    id: 10,
                    kind: ViewKind::Stack,
                },
                MountItem::Create {
                    id: 11,
                    kind: ViewKind::Screen,
                },
                MountItem::Create {
                    id: 12,
                    kind: ViewKind::Modal,
                },
                MountItem::Create {
                    id: 13,
                    kind: ViewKind::HeaderItem,
                },
            ],
        });
        let mut words = read(&encoded);
        assert_eq!(words.int(), CREATE);
        assert_eq!(words.int(), 10);
        assert_eq!(words.int(), 5); // Stack
        assert_eq!(words.int(), CREATE);
        assert_eq!(words.int(), 11);
        assert_eq!(words.int(), 6); // Screen
        assert_eq!(words.int(), CREATE);
        assert_eq!(words.int(), 12);
        assert_eq!(words.int(), 7); // Modal
        assert_eq!(words.int(), CREATE);
        assert_eq!(words.int(), 13);
        assert_eq!(words.int(), 8); // HeaderItem
        assert!(words.done());
    }

    #[test]
    fn screen_config_encodes_all_fields() {
        use nlayer_core::ScreenConfig;
        let encoded = super::encode(MountBatch {
            items: vec![MountItem::ScreenConfig {
                id: 20,
                config: Box::new(ScreenConfig {
                    title: Some(Arc::from("Test Screen")),
                    header_visible: true,
                    header_transparent: false,
                    tint_color: Some(Color::rgba(255, 0, 0, 255)),
                    background_color: None,
                    title_color: Some(Color::rgba(0, 0, 255, 255)),
                    back_visible: false,
                    gesture_enabled: true,
                    prevent_back: false,
                    animation: nlayer_core::ScreenAnimation::Fade,
                    left: Some(100),
                    right: None,
                    title_view: Some(101),
                    header_view: None,
                    header_height: nlayer_core::geometry::Points(44.0),
                }),
            }],
        });
        let mut words = read(&encoded);
        assert_eq!(words.int(), super::SCREEN_CONFIG);
        assert_eq!(words.int(), 20);
        assert_eq!(object(&encoded, words.int()), "Test Screen");
        assert_eq!(words.int(), 1); // header_visible true
        assert_eq!(words.int(), 0); // header_transparent false
        assert_eq!(words.int(), 1); // tint_color present
        assert_eq!(words.int(), 0xFFFF0000u32 as i32); // tint_color ARGB
        assert_eq!(words.int(), 0); // background_color absent
        assert_eq!(words.int(), 1); // title_color present
        assert_eq!(words.int(), 0xFF0000FFu32 as i32); // title_color ARGB
        assert_eq!(words.int(), 0); // back_visible false
        assert_eq!(words.int(), 1); // gesture_enabled true
        assert_eq!(words.int(), 0); // prevent_back false
        assert_eq!(words.int(), 1); // animation Fade
        assert_eq!(words.int(), 100); // left id
        assert_eq!(words.int(), -1); // right id (None)
        assert_eq!(words.int(), 101); // title_view id
        assert_eq!(words.float(), 44.0); // header_height
        assert!(words.done());
    }

    #[test]
    fn screen_config_with_absent_title() {
        use nlayer_core::ScreenConfig;
        let encoded = super::encode(MountBatch {
            items: vec![MountItem::ScreenConfig {
                id: 21,
                config: Box::new(ScreenConfig {
                    title: None,
                    ..ScreenConfig::default()
                }),
            }],
        });
        let mut words = read(&encoded);
        assert_eq!(words.int(), super::SCREEN_CONFIG);
        assert_eq!(words.int(), 21);
        assert_eq!(words.int(), -1); // title absent
        assert_eq!(words.int(), 1); // header_visible (default true)
        assert_eq!(words.int(), 0); // header_transparent (default false)
        assert_eq!(words.int(), 0); // tint_color absent
        assert_eq!(words.int(), 0); // background_color absent
        assert_eq!(words.int(), 0); // title_color absent
        assert_eq!(words.int(), 1); // back_visible (default true)
        assert_eq!(words.int(), 1); // gesture_enabled (default true)
        assert_eq!(words.int(), 0); // prevent_back (default false)
        assert_eq!(words.int(), 0); // animation Default
        assert_eq!(words.int(), -1); // left absent
        assert_eq!(words.int(), -1); // right absent
        assert_eq!(words.int(), -1); // title_view absent
        assert_eq!(words.float(), 0.0); // header_height (default 0)
        assert!(words.done());
    }

    #[test]
    fn modal_config_encodes_all_fields() {
        use nlayer_core::ModalConfig;
        let encoded = super::encode(MountBatch {
            items: vec![MountItem::ModalConfig {
                id: 30,
                config: ModalConfig {
                    animation: nlayer_core::ModalAnimation::Fade,
                    presentation: nlayer_core::ModalPresentation::PageSheet,
                    close_on_escape: false,
                    close_on_backdrop_press: true,
                    close_on_swipe: false,
                },
            }],
        });
        let mut words = read(&encoded);
        assert_eq!(words.int(), super::MODAL_CONFIG);
        assert_eq!(words.int(), 30);
        assert_eq!(words.int(), 1); // animation Fade
        assert_eq!(words.int(), 1); // presentation PageSheet
        assert_eq!(words.int(), 0); // close_on_escape false
        assert_eq!(words.int(), 1); // close_on_backdrop_press true
        assert_eq!(words.int(), 0); // close_on_swipe false
        assert!(words.done());
    }
}
