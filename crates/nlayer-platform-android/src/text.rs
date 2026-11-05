//! `StaticLayout` text engine, driven through JNI on the engine thread.
//!
//! `TextLayouts.prepare` builds the styled text once per content, `measure`
//! lays it out at any width, `layout` makes the final `StaticLayout` that
//! the UI thread only draws. Inline attachments are replacement spans of
//! the attachment's size; `attachments` reports where the layout put them
//! and `hit_test` is the caret a selection or an `href` press uses.
//!
//! Extracted from `nsh-platform-android/src/text.rs`. Custom fonts, shadows
//! and decorations are not measured.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use jni::{
    objects::{JFloatArray, JObject},
    refs::Global,
};
use nlayer_core::geometry::{Point, Points, Size, px};
use nlayer_core::style::{Color, EllipsizeMode, TextAlign, VerticalAlign};
use nlayer_core::text::{
    Affinity, AttachmentAlignments, AttributedText, TextEngine, TextLayout, TextLine, TextPosition,
};

use crate::encode::AndroidLayout;
use crate::java::{TextLayouts, with_env};

/// Ints per span: start, end, weight, flags, color.
const SPAN_INTS: usize = 5;
/// Floats per span: size, line height (0: the font's).
const SPAN_FLOATS: usize = 2;
const ITALIC: i32 = 1;

/// `0xRRGGBBAA` as Android's `0xAARRGGBB`.
fn argb(color: Color) -> i32 {
    color.0.rotate_right(8) as i32
}

/// A `TextLayouts.Prepared` object.
#[derive(Default)]
pub(crate) struct Prepared(Option<Global<JObject<'static>>>);

/// The display density (pixels per dp).
#[derive(Clone)]
pub(crate) struct Density(Arc<AtomicU32>);

impl Density {
    pub(crate) fn new(value: f32) -> Self {
        Self(Arc::new(AtomicU32::new(value.to_bits())))
    }

    #[allow(dead_code)]
    pub(crate) fn set(&self, value: f32) {
        self.0.store(value.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// [`TextEngine`] backed by Android's `StaticLayout`.
pub(crate) struct AndroidTextEngine {
    pub(crate) density: Density,
}

impl TextEngine for AndroidTextEngine {
    type Prepared = Prepared;

    fn prepare(&self, text: &AttributedText) -> Prepared {
        self.prepare_aligned(text, &AttachmentAlignments::default())
    }

    fn prepare_aligned(
        &self,
        text: &AttributedText,
        alignments: &AttachmentAlignments,
    ) -> Prepared {
        let mut ints = Vec::with_capacity(text.spans.len() * SPAN_INTS);
        let mut floats = Vec::with_capacity(text.spans.len() * SPAN_FLOATS);
        for span in &text.spans {
            let style = &span.style;
            ints.extend_from_slice(&[
                span.start as i32,
                span.end as i32,
                style.font_weight as i32,
                i32::from(style.italic) * ITALIC,
                argb(style.color),
            ]);
            floats.extend_from_slice(&[style.font_size.0, style.line_height.0]);
        }
        let align = match text.align {
            TextAlign::Auto => 0,
            TextAlign::Left => 1,
            TextAlign::Center => 2,
            TextAlign::Right => 3,
            TextAlign::Justify => 4,
        } | match text.writing_direction {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        } << 4
            | i32::from(text.rtl) << 8;
        let ellipsize = match text.ellipsize {
            EllipsizeMode::Tail => 0,
            EllipsizeMode::Head => 1,
            EllipsizeMode::Middle => 2,
            EllipsizeMode::Clip => 3,
        };
        let density = self.density.get();
        let max_lines = text.max_lines as i32;
        // Offset, width, height and `verticalAlign` (0 baseline, 1 text
        // top, 2 text bottom, 3 middle). `TextLayouts.Attachment` places
        // it as `TextAttachment::extent` does, with the run's font metrics.
        let attachments: Vec<f32> = text
            .attachments
            .iter()
            .flat_map(|attachment| {
                let align = match alignments.get(attachment.index) {
                    VerticalAlign::Baseline => 0.0,
                    VerticalAlign::TextTop => 1.0,
                    VerticalAlign::TextBottom => 2.0,
                    VerticalAlign::Middle => 3.0,
                };
                [
                    attachment.index as f32,
                    attachment.width.0,
                    attachment.height.0,
                    align,
                ]
            })
            .collect();
        Prepared(with_env(|env| {
            let string = env.new_string(&text.text)?;
            let int_array = jni::objects::JIntArray::new(env, ints.len())?;
            int_array.set_region(env, 0, &ints)?;
            let float_array = JFloatArray::new(env, floats.len())?;
            float_array.set_region(env, 0, &floats)?;
            let attachment_array = JFloatArray::new(env, attachments.len())?;
            attachment_array.set_region(env, 0, &attachments)?;
            let prepared = TextLayouts::prepare(
                env,
                &string,
                &int_array,
                &float_array,
                align,
                max_lines,
                ellipsize,
                density,
                &attachment_array,
            )?;
            env.new_global_ref(prepared).map(Some)
        }))
    }

    fn measure(&self, prepared: &Prepared, max_width: Option<Points>) -> Size<Points> {
        let Some(obj) = &prepared.0 else {
            return Size::new(px(0.0), px(0.0));
        };
        let packed = with_env(|env| {
            TextLayouts::measure(
                env,
                obj.as_obj(),
                max_width.map_or(f32::NAN, |width| width.0),
            )
        });
        let width = f32::from_bits((packed as u64 >> 32) as u32);
        let height = f32::from_bits(packed as u32);
        Size::new(Points(width), Points(height))
    }

    fn layout(&self, prepared: &Prepared, size: Size<Points>) -> TextLayout {
        let layout = prepared.0.as_ref().and_then(|obj| {
            with_env(|env| {
                let layout = TextLayouts::layout(env, obj.as_obj(), size.width.0, size.height.0)?;
                env.new_global_ref(layout).map(Some)
            })
        });
        match layout {
            Some(layout) => TextLayout(Arc::new(AndroidLayout(Arc::new(layout)))),
            None => TextLayout(Arc::new(())),
        }
    }

    fn lines(&self, layout: &TextLayout) -> Vec<TextLine> {
        let Some(AndroidLayout(obj)) = layout.downcast::<AndroidLayout>() else {
            return Vec::new();
        };
        let floats = with_env(|env| {
            let array = TextLayouts::lines(env, obj.as_obj(), self.density.get())?;
            let len = array.len(env)?;
            let mut out = vec![0f32; len];
            array.get_region(env, 0, &mut out)?;
            Ok(out)
        });
        floats
            .chunks_exact(10)
            .map(|value| TextLine {
                text: String::new(),
                start: value[0] as u32,
                end: value[1] as u32,
                x: px(value[2]),
                y: px(value[3]),
                width: px(value[4]),
                height: px(value[5]),
                ascender: px(value[6]),
                descender: px(value[7]),
                cap_height: px(value[8]),
                x_height: px(value[9]),
            })
            .collect()
    }

    fn attachments(&self, layout: &TextLayout) -> Vec<Option<Point<Points>>> {
        let Some(AndroidLayout(obj)) = layout.downcast::<AndroidLayout>() else {
            return Vec::new();
        };
        let floats = with_env(|env| {
            let array = TextLayouts::attachments(env, obj.as_obj(), self.density.get())?;
            let len = array.len(env)?;
            let mut out = vec![0f32; len];
            array.get_region(env, 0, &mut out)?;
            Ok(out)
        });
        floats
            .chunks_exact(2)
            .map(|value| {
                (!value[0].is_nan())
                    .then_some(nlayer_core::geometry::point(px(value[0]), px(value[1])))
            })
            .collect()
    }

    fn hit_test(&self, layout: &TextLayout, at: Point<Points>) -> TextPosition {
        let Some(AndroidLayout(obj)) = layout.downcast::<AndroidLayout>() else {
            return TextPosition::default();
        };
        let packed = with_env(|env| {
            TextLayouts::hit_test(env, obj.as_obj(), at.x.0, at.y.0, self.density.get())
        });
        // The offset in the low 32 bits, bit 32 set at the end of a wrapped
        // line (`TextLayouts.hitTest`).
        let offset = u32::try_from(packed & 0xffff_ffff).unwrap_or_default();
        let affinity = if (packed >> 32) & 1 != 0 {
            Affinity::Upstream
        } else {
            Affinity::Downstream
        };
        TextPosition::new(offset, affinity)
    }
}
