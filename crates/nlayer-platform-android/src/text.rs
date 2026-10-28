//! `StaticLayout` text engine, driven through JNI on the engine thread.
//!
//! `TextLayouts.prepare` builds the styled text once per content, `measure`
//! lays it out at any width, `layout` makes the final `StaticLayout` that
//! the UI thread only draws.
//!
//! Extracted from `nsh-platform-android/src/text.rs`. Custom fonts, shadows,
//! decorations and inline attachments are not measured: v1 text is color,
//! size, weight, italic and line height on the system font.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use jni::{objects::JValue, sys::jvalue};
use nlayer_core::geometry::{Points, Size, px};
use nlayer_core::style::EllipsizeMode;
use nlayer_core::style::{Color, TextAlign};
use nlayer_core::text::{AttributedText, TextEngine, TextLayout, TextLine};

use crate::encode::AndroidLayout;
use crate::java::{java, with_env};

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
pub(crate) struct Prepared(Option<jni::objects::GlobalRef>);

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
        Prepared(with_env(|env| {
            let cached = java();
            let string = env.new_string(&text.text)?;
            let int_array = env.new_int_array(ints.len() as i32)?;
            env.set_int_array_region(&int_array, 0, &ints)?;
            let float_array = env.new_float_array(floats.len() as i32)?;
            env.set_float_array_region(&float_array, 0, &floats)?;
            let args = [
                JValue::Object(&string).as_jni(),
                JValue::Object(&int_array).as_jni(),
                JValue::Object(&float_array).as_jni(),
                JValue::Int(align).as_jni(),
                JValue::Int(max_lines).as_jni(),
                JValue::Int(ellipsize).as_jni(),
                JValue::Float(density).as_jni(),
            ];
            // SAFETY: the method id and argument types match the signature
            // checked in `java::init`.
            let prepared = unsafe {
                env.call_static_method_unchecked(
                    <&jni::objects::JClass<'_>>::from(cached.text.as_obj()),
                    cached.text_prepare,
                    jni::signature::ReturnType::Object,
                    &args,
                )
            }?
            .l()?;
            env.new_global_ref(prepared).map(Some)
        }))
    }

    fn measure(&self, prepared: &Prepared, max_width: Option<Points>) -> Size<Points> {
        let Some(obj) = &prepared.0 else {
            return Size::new(px(0.0), px(0.0));
        };
        let packed = with_env(|env| {
            let cached = java();
            let args: [jvalue; 2] = [
                JValue::Object(obj.as_obj()).as_jni(),
                JValue::Float(max_width.map_or(f32::NAN, |width| width.0)).as_jni(),
            ];
            // SAFETY: matches the signature checked in `java::init`.
            unsafe {
                env.call_static_method_unchecked(
                    <&jni::objects::JClass<'_>>::from(cached.text.as_obj()),
                    cached.text_measure,
                    jni::signature::ReturnType::Primitive(jni::signature::Primitive::Long),
                    &args,
                )
            }?
            .j()
        });
        let width = f32::from_bits((packed as u64 >> 32) as u32);
        let height = f32::from_bits(packed as u32);
        Size::new(Points(width), Points(height))
    }

    fn layout(&self, prepared: &Prepared, size: Size<Points>) -> TextLayout {
        let layout = prepared.0.as_ref().and_then(|obj| {
            with_env(|env| {
                let cached = java();
                let args: [jvalue; 3] = [
                    JValue::Object(obj.as_obj()).as_jni(),
                    JValue::Float(size.width.0).as_jni(),
                    JValue::Float(size.height.0).as_jni(),
                ];
                // SAFETY: matches the signature checked in `java::init`.
                let layout = unsafe {
                    env.call_static_method_unchecked(
                        <&jni::objects::JClass<'_>>::from(cached.text.as_obj()),
                        cached.text_layout,
                        jni::signature::ReturnType::Object,
                        &args,
                    )
                }?
                .l()?;
                env.new_global_ref(layout).map(Some)
            })
        });
        match layout {
            Some(layout) => TextLayout(Arc::new(AndroidLayout(layout))),
            None => TextLayout(Arc::new(())),
        }
    }

    fn lines(&self, layout: &TextLayout) -> Vec<TextLine> {
        let Some(AndroidLayout(obj)) = layout.downcast::<AndroidLayout>() else {
            return Vec::new();
        };
        let floats = with_env(|env| {
            let cached = java();
            let args: [jvalue; 2] = [
                JValue::Object(obj.as_obj()).as_jni(),
                JValue::Float(self.density.get()).as_jni(),
            ];
            // SAFETY: matches the signature checked in `java::init`.
            let array = unsafe {
                env.call_static_method_unchecked(
                    <&jni::objects::JClass<'_>>::from(cached.text.as_obj()),
                    cached.text_lines,
                    jni::signature::ReturnType::Array,
                    &args,
                )
            }?
            .l()?;
            let array = jni::objects::JFloatArray::from(array);
            let len = env.get_array_length(&array)? as usize;
            let mut out = vec![0f32; len];
            env.get_float_array_region(&array, 0, &mut out)?;
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
}
