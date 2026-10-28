//! CoreText text engine: typesets on the engine thread, draws on the main
//! thread from finished `CTLine`s, so every text is laid out once.
//!
//! Extracted from `nsh-platform-ios/src/text.rs`. Inline attachments, custom
//! font files, strikethrough, span backgrounds and caret geometry are not
//! copied: v1 text is nested spans (color, size, weight, italic) plus
//! alignment, line height and a solid underline.

use std::{cell::RefCell, ptr::NonNull};

use nlayer_core::{
    geometry::{Points, Size},
    style::{Color, EllipsizeMode, FontFamily, TextAlign},
    text::{AttributedText, ResolvedTextStyle, TextEngine, TextLayout, TextLine},
};
use objc2::rc::Retained;
use objc2_core_foundation::{
    CFAttributedString, CFDictionary, CFIndex, CFMutableAttributedString, CFNumber, CFRange,
    CFRetained, CFString, CFType, CGFloat, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::CGColor;
use objc2_core_text::{
    CTFont, CTFontDescriptor, CTFontSymbolicTraits, CTFramesetter, CTLine, CTLineTruncationType,
    CTParagraphStyle, CTParagraphStyleSetting, CTParagraphStyleSpecifier, CTTextAlignment,
    CTTypesetter, CTWritingDirection, kCTFontAttributeName, kCTFontFamilyNameAttribute,
    kCTFontFeatureSettingsAttribute, kCTFontOpenTypeFeatureTag, kCTFontOpenTypeFeatureValue,
    kCTFontTraitsAttribute, kCTFontWeightTrait, kCTForegroundColorAttributeName,
    kCTKernAttributeName, kCTParagraphStyleAttributeName, kCTUnderlineColorAttributeName,
    kCTUnderlineStyleAttributeName,
};
use objc2_ui_kit::{
    UIFont, UIFontWeight, UIFontWeightBlack, UIFontWeightBold, UIFontWeightHeavy,
    UIFontWeightLight, UIFontWeightMedium, UIFontWeightRegular, UIFontWeightSemibold,
    UIFontWeightThin, UIFontWeightUltraLight,
};
use rustc_hash::FxHashMap;

/// Attribute-carrying font cache. Bounded: an app animating `fontSize`
/// would otherwise grow it without limit.
#[derive(Default)]
struct FontCache {
    fonts: RefCell<FxHashMap<FontKey, CFRetained<CTFont>>>,
}

type FontKey = (u32, u16, Option<FontFamily>, bool, u8);

impl FontCache {
    const MAX: usize = 64;

    fn get(&self, style: &ResolvedTextStyle) -> CFRetained<CTFont> {
        let key = (
            style.font_size.0.to_bits(),
            style.font_weight,
            style.font_family,
            style.italic,
            style.font_variant.0,
        );
        let mut fonts = self.fonts.borrow_mut();
        if let Some(font) = fonts.get(&key) {
            return font.clone();
        }
        if fonts.len() >= Self::MAX {
            fonts.clear();
        }
        let font = make_font(style);
        fonts.insert(key, font.clone());
        font
    }
}

fn make_font(style: &ResolvedTextStyle) -> CFRetained<CTFont> {
    let size = style.font_size.0 as CGFloat;
    let font = style
        .font_family
        .and_then(|family| {
            let name = family.name();
            generic_font(&name, size, style.font_weight)
                .unwrap_or_else(|| family_font(&name, size, style.font_weight))
        })
        .unwrap_or_else(|| system_font(size, style.font_weight));
    let font = if style.italic {
        let italic = CTFontSymbolicTraits::TraitItalic;
        // SAFETY: a valid font; size 0 keeps the size, a null matrix keeps it.
        unsafe { font.copy_with_symbolic_traits(0.0, std::ptr::null(), italic, italic) }
            .unwrap_or(font)
    } else {
        font
    };
    with_features(font, style.font_variant.0)
}

/// The same font [`make_font`] builds, as a `UIFont` a text field can hold.
pub(crate) fn ui_font(style: &ResolvedTextStyle) -> Retained<UIFont> {
    let font = make_font(style);
    let ptr = CFRetained::into_raw(font).cast::<UIFont>();
    // SAFETY: UIFont and CTFont are toll-free. The +1 reference moves over.
    unsafe { Retained::from_raw(ptr.as_ptr()).expect("non-null font") }
}

fn with_features(font: CFRetained<CTFont>, variant: u8) -> CFRetained<CTFont> {
    if variant == 0 {
        return font;
    }
    let one = CFNumber::new_i32(1);
    let features: Vec<CFRetained<CFDictionary<CFString, CFType>>> =
        nlayer_core::style::FontVariant(variant)
            .tags()
            .map(|tag| {
                let tag = CFString::from_str(tag);
                // SAFETY: immutable CoreText constants.
                let keys = unsafe { [kCTFontOpenTypeFeatureTag, kCTFontOpenTypeFeatureValue] };
                CFDictionary::from_slices(&keys, &[tag.as_ref(), one.as_ref()])
            })
            .collect();
    let features = objc2_core_foundation::CFArray::from_retained_objects(&features);
    // SAFETY: an immutable CoreText constant.
    let key = unsafe { kCTFontFeatureSettingsAttribute };
    let attributes = CFDictionary::<CFString, CFType>::from_slices(&[key], &[features.as_ref()]);
    // SAFETY: attribute values have the types CoreText documents; size 0
    // and a null matrix keep the font's.
    unsafe {
        let descriptor = CTFontDescriptor::with_attributes(attributes.as_opaque());
        font.copy_with_attributes(0.0, std::ptr::null(), Some(&descriptor))
    }
}

/// CSS generic families (`monospace`, `serif`): the system font's designs.
/// `Some(None)` is the plain system font; `None` is not generic.
fn generic_font(family: &str, size: CGFloat, weight: u16) -> Option<Option<CFRetained<CTFont>>> {
    use objc2_ui_kit::{
        UIFontDescriptorSystemDesign, UIFontDescriptorSystemDesignRounded,
        UIFontDescriptorSystemDesignSerif,
    };
    let design: &UIFontDescriptorSystemDesign = match family {
        "system-ui" | "sans-serif" | "ui-sans-serif" | "-apple-system" => return Some(None),
        "monospace" | "ui-monospace" => {
            let font = UIFont::monospacedSystemFontOfSize_weight(size, font_weight(weight));
            return Some(Some(bridge(font)));
        }
        // SAFETY: immutable UIKit constants.
        "serif" | "ui-serif" => unsafe { UIFontDescriptorSystemDesignSerif },
        "ui-rounded" => unsafe { UIFontDescriptorSystemDesignRounded },
        _ => return None,
    };
    let base = UIFont::systemFontOfSize_weight(size, font_weight(weight));
    // SAFETY: a plain getter of a valid font.
    let descriptor = unsafe { base.fontDescriptor() };
    let font = descriptor
        .fontDescriptorWithDesign(design)
        .map(|d| UIFont::fontWithDescriptor_size(&d, size))
        .unwrap_or(base);
    Some(Some(bridge(font)))
}

fn bridge(font: Retained<UIFont>) -> CFRetained<CTFont> {
    let ptr = Retained::into_raw(font).cast::<CTFont>();
    // SAFETY: toll-free bridging; ownership of the +1 reference moves over.
    unsafe { CFRetained::from_raw(NonNull::new(ptr).expect("non-null font")) }
}

fn family_font(family: &str, size: CGFloat, weight: u16) -> Option<CFRetained<CTFont>> {
    let name = CFString::from_str(family);
    let weight = CFNumber::new_cgfloat(font_weight(weight));
    let traits = CFDictionary::<CFString, CFType>::from_slices(
        // SAFETY: immutable CoreText constants.
        &[unsafe { kCTFontWeightTrait }],
        &[weight.as_ref()],
    );
    // SAFETY: immutable CoreText constants.
    let keys = unsafe { [kCTFontFamilyNameAttribute, kCTFontTraitsAttribute] };
    let attributes =
        CFDictionary::<CFString, CFType>::from_slices(&keys, &[name.as_ref(), traits.as_ref()]);
    // SAFETY: attribute values have the types CoreText documents.
    let font = unsafe {
        let descriptor = CTFontDescriptor::with_attributes(attributes.as_opaque());
        CTFont::with_font_descriptor(&descriptor, size, std::ptr::null())
    };
    // An unknown family silently resolves to a default font: detect it.
    // SAFETY: a valid font.
    let resolved = unsafe { font.family_name() }.to_string();
    if resolved.eq_ignore_ascii_case(family) {
        Some(font)
    } else {
        log::warn!("font family `{family}` is not available; using the system font");
        None
    }
}

fn system_font(size: CGFloat, weight: u16) -> CFRetained<CTFont> {
    let font: Retained<UIFont> = UIFont::systemFontOfSize_weight(size, font_weight(weight));
    bridge(font)
}

fn font_weight(weight: u16) -> UIFontWeight {
    // SAFETY: UIKit's font weight constants are immutable statics.
    unsafe {
        match weight {
            0..=149 => UIFontWeightUltraLight,
            150..=249 => UIFontWeightThin,
            250..=349 => UIFontWeightLight,
            350..=449 => UIFontWeightRegular,
            450..=549 => UIFontWeightMedium,
            550..=649 => UIFontWeightSemibold,
            650..=749 => UIFontWeightBold,
            750..=849 => UIFontWeightHeavy,
            _ => UIFontWeightBlack,
        }
    }
}

pub(crate) fn cg_color(color: Color) -> CFRetained<CGColor> {
    let (r, g, b, a) = color.to_f64();
    CGColor::new_srgb(r, g, b, a)
}

/// Typesetting state of one text content.
pub(crate) struct Prepared {
    framesetter: CFRetained<CTFramesetter>,
    string: CFRetained<CFMutableAttributedString>,
    length: CFIndex,
    max_lines: u32,
    ellipsize: EllipsizeMode,
    align: TextAlign,
    breaks: Vec<(CFIndex, CFIndex)>,
}

const WHOLE: CFRange = CFRange {
    location: 0,
    length: 0,
};

/// A finished line and its origin (CoreText's y-up space, from the bottom
/// of the box).
pub(crate) struct LaidLine {
    pub(crate) line: CFRetained<CTLine>,
    pub(crate) origin: CGPoint,
    pub(crate) range: CFRange,
    /// Where its caret positions end: before a hard break that ends the line.
    #[allow(dead_code)]
    pub(crate) caret_end: CFIndex,
}

/// Finished lines the text view draws. A finished `CTLine` is immutable;
/// CoreText draws it from the main thread after the engine built it.
pub(crate) struct CoreTextLayout {
    pub(crate) lines: Vec<LaidLine>,
    /// The box's height (lines are placed y-up from its bottom).
    pub(crate) height: CGFloat,
}

// SAFETY: the lines are finished and immutable, and CoreText draws an
// immutable line from any thread.
unsafe impl Send for CoreTextLayout {}
// SAFETY: as above. The main thread only draws; nothing mutates a line.
unsafe impl Sync for CoreTextLayout {}

/// The platform text engine.
#[derive(Default)]
pub(crate) struct CoreTextEngine {
    fonts: FontCache,
}

impl CoreTextEngine {
    fn attributed_string(&self, text: &AttributedText) -> CFRetained<CFMutableAttributedString> {
        let string =
            CFMutableAttributedString::new(None, 0).expect("CFAttributedStringCreateMutable");
        let s = Some(&*string);
        // SAFETY: a valid mutable string and a whole-string range.
        unsafe {
            CFMutableAttributedString::replace_string(
                s,
                WHOLE,
                Some(&CFString::from_str(&text.text)),
            )
        };
        let length = text.spans.last().map_or(0, |span| span.end as CFIndex);
        CFMutableAttributedString::begin_editing(s);
        let set = |range: CFRange, key: &CFString, value: &CFType| {
            // SAFETY: values have the types CoreText requires for their keys,
            // and every range lies within the string (span offsets are UTF-16).
            unsafe { CFMutableAttributedString::set_attribute(s, range, Some(key), Some(value)) }
        };
        let paragraph = paragraph_style(text);
        set(
            CFRange {
                location: 0,
                length,
            },
            // SAFETY: an immutable CoreText constant.
            unsafe { kCTParagraphStyleAttributeName },
            paragraph.as_ref(),
        );
        for span in &text.spans {
            let range = CFRange {
                location: span.start as CFIndex,
                length: (span.end - span.start) as CFIndex,
            };
            let style = &span.style;
            set(
                range,
                // SAFETY: immutable CoreText constants, here and below.
                unsafe { kCTFontAttributeName },
                self.fonts.get(style).as_ref(),
            );
            set(
                range,
                unsafe { kCTForegroundColorAttributeName },
                cg_color(style.color).as_ref(),
            );
            if style.letter_spacing != Points::ZERO {
                let kern = CFNumber::new_cgfloat(style.letter_spacing.0 as CGFloat);
                set(range, unsafe { kCTKernAttributeName }, kern.as_ref());
            }
            if style.decoration.underline
                && style.decoration_style == nlayer_core::style::DecorationStyle::Solid
                && style.decoration_thickness.is_none()
                && style.underline_offset.is_none()
            {
                let single = CFNumber::new_i32(1);
                let decoration = cg_color(style.decoration_color.unwrap_or(style.color));
                set(
                    range,
                    unsafe { kCTUnderlineStyleAttributeName },
                    single.as_ref(),
                );
                set(
                    range,
                    unsafe { kCTUnderlineColorAttributeName },
                    decoration.as_ref(),
                );
            }
        }
        CFMutableAttributedString::end_editing(s);
        string
    }
}

fn paragraph_style(text: &AttributedText) -> CFRetained<CTParagraphStyle> {
    let alignment = match text.align {
        // React Native: the right side in right-to-left layouts.
        TextAlign::Auto if text.rtl => CTTextAlignment::Right,
        TextAlign::Auto => CTTextAlignment::Natural,
        TextAlign::Left => CTTextAlignment::Left,
        TextAlign::Center => CTTextAlignment::Center,
        TextAlign::Right => CTTextAlignment::Right,
        TextAlign::Justify => CTTextAlignment::Justified,
    };
    let line_height = text.spans.first().map_or(0.0, |s| s.style.line_height.0) as CGFloat;
    let setting = |spec, value: *const std::ffi::c_void, size| CTParagraphStyleSetting {
        spec,
        valueSize: size,
        value: NonNull::new(value.cast_mut()).expect("non-null setting"),
    };
    let direction = match text.writing_direction {
        None => CTWritingDirection::Natural,
        Some(false) => CTWritingDirection::LeftToRight,
        Some(true) => CTWritingDirection::RightToLeft,
    };
    let mut settings = vec![
        setting(
            CTParagraphStyleSpecifier::Alignment,
            (&raw const alignment).cast(),
            size_of::<CTTextAlignment>(),
        ),
        setting(
            CTParagraphStyleSpecifier::BaseWritingDirection,
            (&raw const direction).cast(),
            size_of::<CTWritingDirection>(),
        ),
    ];
    if line_height > 0.0 {
        for spec in [
            CTParagraphStyleSpecifier::MinimumLineHeight,
            CTParagraphStyleSpecifier::MaximumLineHeight,
        ] {
            settings.push(setting(
                spec,
                (&raw const line_height).cast(),
                size_of::<CGFloat>(),
            ));
        }
    }
    // SAFETY: every setting points at a live value of the declared size.
    unsafe { CTParagraphStyle::new(settings.as_ptr(), settings.len()) }
}

fn hard_breaks(text: &str) -> Vec<(CFIndex, CFIndex)> {
    let mut breaks = Vec::new();
    let mut at: CFIndex = 0;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let mut len = c.len_utf16() as CFIndex;
        if matches!(
            c,
            '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}'
        ) {
            let mut width = len;
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
                width += 1;
                len += 1;
            }
            breaks.push((at, width));
        }
        at += len;
    }
    breaks
}

impl Prepared {
    fn typesetter(&self) -> CFRetained<CTTypesetter> {
        // SAFETY: a valid framesetter.
        unsafe { self.framesetter.typesetter() }
    }

    /// Length (UTF-16 units) of the first `lines` lines at `width`.
    fn lines_length(&self, lines: u32, width: f64) -> CFIndex {
        let typesetter = self.typesetter();
        let mut end = 0;
        for _ in 0..lines {
            if end >= self.length {
                break;
            }
            // SAFETY: `end` lies within the string.
            let count = unsafe { typesetter.suggest_line_break(end, width) };
            if count <= 0 {
                break;
            }
            end += count;
        }
        end
    }

    /// `line` (starting at `start`) cut to `width` with an ellipsis, as
    /// `textOverflow` says, standing for the rest of the text.
    fn truncate(&self, start: CFIndex, width: f64) -> Option<CFRetained<CTLine>> {
        let kind = match self.ellipsize {
            EllipsizeMode::Tail => CTLineTruncationType::End,
            EllipsizeMode::Head => CTLineTruncationType::Start,
            EllipsizeMode::Middle => CTLineTruncationType::Middle,
            EllipsizeMode::Clip => {
                // SAFETY: the range lies within the string.
                return Some(unsafe {
                    self.typesetter().line(CFRange {
                        location: start,
                        length: self.length - start,
                    })
                });
            }
        };
        let last = (self.length - 1).max(start);
        // SAFETY: ranges lie within the string.
        unsafe {
            let rest = self.typesetter().line(CFRange {
                location: start,
                length: self.length - start,
            });
            let token_string =
                CFMutableAttributedString::new(None, 0).expect("CFAttributedStringCreateMutable");
            CFMutableAttributedString::replace_string(
                Some(&token_string),
                WHOLE,
                Some(&CFString::from_static_str("\u{2026}")),
            );
            let source: &CFAttributedString = &self.string;
            let attributes = CFAttributedString::attributes(source, last, std::ptr::null_mut());
            if let Some(attributes) = attributes {
                CFMutableAttributedString::set_attributes(
                    Some(&token_string),
                    CFRange {
                        location: 0,
                        length: 1,
                    },
                    Some(&attributes),
                    false,
                );
            }
            let token = CTLine::with_attributed_string(&token_string);
            rest.truncated_line(width, kind, Some(&token))
        }
    }

    fn flush_factor(&self) -> CGFloat {
        match self.align {
            TextAlign::Center => 0.5,
            TextAlign::Right => 1.0,
            TextAlign::Auto | TextAlign::Left | TextAlign::Justify => 0.0,
        }
    }
}

impl TextEngine for CoreTextEngine {
    type Prepared = Prepared;

    fn prepare(&self, text: &AttributedText) -> Prepared {
        let string = self.attributed_string(text);
        let attributed: &CFAttributedString = &string;
        // SAFETY: a valid attributed string.
        let framesetter = unsafe { CTFramesetter::with_attributed_string(attributed) };
        Prepared {
            framesetter,
            length: text.text.encode_utf16().count() as CFIndex,
            string,
            max_lines: text.max_lines,
            ellipsize: text.ellipsize,
            align: text.align,
            breaks: hard_breaks(&text.text),
        }
    }

    fn measure(&self, prepared: &Prepared, max_width: Option<Points>) -> Size<Points> {
        let width = max_width.map_or(CGFloat::MAX, |w| w.0 as CGFloat);
        let range = if prepared.max_lines > 0 {
            CFRange {
                location: 0,
                length: prepared.lines_length(prepared.max_lines, width),
            }
        } else {
            WHOLE
        };
        // SAFETY: a range within the string, no frame attributes, no fit
        // range out-param.
        let size = unsafe {
            prepared.framesetter.suggest_frame_size_with_constraints(
                range,
                None,
                CGSize::new(width, CGFloat::MAX),
                std::ptr::null_mut(),
            )
        };
        Size::new(Points(size.width as f32), Points(size.height as f32))
    }

    fn layout(&self, prepared: &Prepared, size: Size<Points>) -> TextLayout {
        let (width, height) = (size.width.0 as CGFloat, size.height.0 as CGFloat);
        let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width, height));
        // SAFETY: a valid rectangle path and a whole-string range.
        let frame = unsafe {
            let path = objc2_core_graphics::CGPath::with_rect(rect, std::ptr::null());
            prepared.framesetter.frame(WHOLE, &path, None)
        };
        // SAFETY: the frame's own line array (of CTLines) and origins.
        let mut lines = unsafe {
            let array = frame.lines();
            let count = array.count();
            let mut origins = vec![CGPoint::new(0.0, 0.0); count as usize];
            if let Some(ptr) = NonNull::new(origins.as_mut_ptr()) {
                frame.line_origins(WHOLE, ptr);
            }
            (0..count)
                .map(|i| {
                    let line = array.value_at_index(i).cast::<CTLine>();
                    let line = CFRetained::retain(NonNull::new(line.cast_mut()).expect("line"));
                    let range = line.string_range();
                    let end = range.location + range.length;
                    let caret_end = prepared
                        .breaks
                        .binary_search_by_key(&end, |&(at, len)| at + len)
                        .ok()
                        .and_then(|b| prepared.breaks.get(b))
                        .map_or(end, |&(at, _)| at.max(range.location));
                    LaidLine {
                        line,
                        origin: origins[i as usize],
                        range,
                        caret_end,
                    }
                })
                .collect::<Vec<_>>()
        };
        if prepared.max_lines > 0 {
            lines.truncate(prepared.max_lines as usize);
        }
        if let Some(last) = lines.last_mut() {
            // SAFETY: a valid line.
            let range = unsafe { last.line.string_range() };
            if range.location + range.length < prepared.length
                && let Some(truncated) = prepared.truncate(range.location, width)
            {
                // SAFETY: a valid line.
                let x = unsafe { truncated.pen_offset_for_flush(prepared.flush_factor(), width) };
                // A clipped line wider than the box starts at its start edge
                // and overflows the end (CSS), centered or not.
                last.origin.x = x.max(0.0);
                last.line = truncated;
                last.caret_end = prepared.length;
            }
        }
        TextLayout(std::sync::Arc::new(CoreTextLayout { lines, height }))
    }

    fn lines(&self, layout: &TextLayout) -> Vec<TextLine> {
        let Some(layout) = layout.downcast::<CoreTextLayout>() else {
            return Vec::new();
        };
        layout
            .lines
            .iter()
            .map(|l| {
                let (mut ascent, mut descent, mut leading) = (0.0, 0.0, 0.0);
                // SAFETY: a valid line and out pointers.
                let width = unsafe {
                    l.line
                        .typographic_bounds(&mut ascent, &mut descent, &mut leading)
                };
                TextLine {
                    text: String::new(),
                    start: l.range.location as u32,
                    end: (l.range.location + l.range.length) as u32,
                    x: Points(l.origin.x as f32),
                    y: Points((layout.height - l.origin.y - ascent) as f32),
                    width: Points(width as f32),
                    height: Points((ascent + descent + leading) as f32),
                    ascender: Points(ascent as f32),
                    descender: Points(descent as f32),
                    cap_height: Points(ascent as f32),
                    x_height: Points((ascent * 0.5) as f32),
                }
            })
            .collect()
    }
}
