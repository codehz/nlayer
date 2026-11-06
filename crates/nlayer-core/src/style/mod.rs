//! Styles: the keys a view stores, how they merge, and the Taffy layout they become.
//!
//! Extracted from `nsh_core::style`. A name [`StyleKey::parse`] knows is a
//! style; one it does not know is a warning and does not apply. There is
//! no second reject list. Shadow and gradient painting is later; those
//! fields stay so the copied apply path still type-checks. The rest of
//! the residual fields are not a commitment.

#![allow(dead_code)] // the shadow tree is the caller, and it is not extracted yet
use std::sync::Arc;

use crate::{
    css,
    effects::{
        BackdropMaterial, BlendMode, Cursor, Effects, GlassEffect, KeyframeAnimation, Outline,
        Transition,
    },
    geometry::{
        AbsoluteLength, Corners, DefiniteLength, Edges, Percentage, Point, Points, REM_SIZE, Size,
        percentage, point, px,
    },
    transform::Transform,
};

/// An sRGB color packed as `0xRRGGBBAA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Color(pub u32);

impl Color {
    /// Fully transparent.
    pub const TRANSPARENT: Color = Color(0);
    /// Opaque black.
    pub const BLACK: Color = Color(0x0000_00ff);

    /// Creates a color from 8-bit components.
    #[inline]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color((r as u32) << 24 | (g as u32) << 16 | (b as u32) << 8 | a as u32)
    }

    /// Returns `(r, g, b, a)` in `0.0..=1.0`.
    #[inline]
    pub fn to_f64(self) -> (f64, f64, f64, f64) {
        let c = self.0;
        (
            (c >> 24) as f64 / 255.0,
            ((c >> 16) & 0xff) as f64 / 255.0,
            ((c >> 8) & 0xff) as f64 / 255.0,
            (c & 0xff) as f64 / 255.0,
        )
    }

    /// Returns true if the alpha component is zero.
    #[inline]
    pub const fn is_transparent(self) -> bool {
        self.0 & 0xff == 0
    }

    /// Parses a CSS color: hex, `rgb()` / `rgba()`, `hsl()` / `hsla()`,
    /// `oklch()`, `oklab()` or a CSS named color.
    pub fn parse(s: &str) -> Option<Color> {
        crate::css::color(s)
    }
}

/// Horizontal alignment of a text block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    /// Natural alignment for the writing direction.
    #[default]
    Auto,
    /// Left aligned.
    Left,
    /// Centered.
    Center,
    /// Right aligned.
    Right,
    /// Justified.
    Justify,
}

/// `verticalAlign`: where an element inside a `<Text>` (an inline
/// attachment) sits in its line, as CSS's `vertical-align` places a
/// replaced inline box. The font it is measured against is the text's
/// around the attachment; the line grows to hold the attachment wherever
/// it reaches past that font (`TextAttachment::extent`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerticalAlign {
    /// `baseline`: its bottom on the baseline.
    #[default]
    Baseline,
    /// `text-top`: its top at the top of the font (its ascent).
    TextTop,
    /// `text-bottom`: its bottom at the bottom of the font (its descent).
    TextBottom,
    /// `middle`: its middle half the font's x height above the baseline.
    Middle,
}

impl VerticalAlign {
    /// The value named by CSS keyword `name`.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "baseline" => Self::Baseline,
            "text-top" => Self::TextTop,
            "text-bottom" => Self::TextBottom,
            "middle" => Self::Middle,
            _ => return None,
        })
    }
}

/// A font family name, interned: copying and comparing one is free, so
/// resolved text styles stay `Copy` and cheap to hash.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontFamily(u32);

static FAMILIES: std::sync::Mutex<Vec<std::sync::Arc<str>>> = std::sync::Mutex::new(Vec::new());

impl FontFamily {
    /// Interns `name`. Apps use a handful of families, so the table is a
    /// small vector scanned linearly.
    pub fn new(name: &str) -> FontFamily {
        let mut families = FAMILIES.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = families.iter().position(|f| &**f == name) {
            return FontFamily(i as u32);
        }
        families.push(name.into());
        FontFamily(families.len() as u32 - 1)
    }

    /// The family name.
    pub fn name(self) -> std::sync::Arc<str> {
        FAMILIES.lock().unwrap_or_else(|e| e.into_inner())[self.0 as usize].clone()
    }
}

impl std::fmt::Debug for FontFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FontFamily({:?})", self.name())
    }
}

/// Lines drawn through or under text (`textDecorationLine`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextDecoration {
    /// Underline.
    pub underline: bool,
    /// Strikethrough.
    pub line_through: bool,
}

impl TextDecoration {
    /// True if no line is drawn.
    #[inline]
    pub const fn is_none(self) -> bool {
        !self.underline && !self.line_through
    }
}

/// How decoration lines are drawn (`textDecorationStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DecorationStyle {
    /// One solid line (default).
    #[default]
    Solid,
    /// Two lines.
    Double,
    /// Dots.
    Dotted,
    /// Dashes.
    Dashed,
    /// A wavy line.
    Wavy,
}

/// Case changes applied to text (`textTransform`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextTransform {
    /// As written.
    #[default]
    None,
    /// UPPER CASE.
    Uppercase,
    /// lower case.
    Lowercase,
    /// First Letter Of Each Word.
    Capitalize,
}

impl TextTransform {
    /// Applies the transform to `s`.
    pub fn apply<'a>(self, s: &'a str) -> std::borrow::Cow<'a, str> {
        match self {
            TextTransform::None => s.into(),
            TextTransform::Uppercase => s.to_uppercase().into(),
            TextTransform::Lowercase => s.to_lowercase().into(),
            TextTransform::Capitalize => {
                let mut out = String::with_capacity(s.len());
                let mut start = true;
                for c in s.chars() {
                    if start && c.is_alphanumeric() {
                        out.extend(c.to_uppercase());
                        start = false;
                    } else {
                        out.push(c);
                        start |= c.is_whitespace();
                    }
                }
                out.into()
            }
        }
    }
}

/// Where text that does not fit `numberOfLines` is cut (`textOverflow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EllipsizeMode {
    /// "Lorem ips…" (default).
    #[default]
    Tail,
    /// "…sum dolor".
    Head,
    /// "Lorem…dolor".
    Middle,
    /// Cut without an ellipsis.
    Clip,
}

/// The corner radii of a view, clockwise from the top left: a
/// [`Corners<Points>`] with the curve they are drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CornerRadii {
    /// Top left.
    pub top_left: Points,
    /// Top right.
    pub top_right: Points,
    /// Bottom right.
    pub bottom_right: Points,
    /// Bottom left.
    pub bottom_left: Points,
    /// `borderCurve: "continuous"`: corners whose curvature grows from the
    /// edges (iOS app icons) instead of circular arcs; see [`crate::shape`].
    pub continuous: bool,
}

impl CornerRadii {
    /// The same circular radius on every corner.
    pub const fn all(r: Points) -> Self {
        Self {
            top_left: r,
            top_right: r,
            bottom_right: r,
            bottom_left: r,
            continuous: false,
        }
    }

    /// Circular corners with these radii.
    pub const fn circular(corners: Corners<Points>) -> Self {
        Self {
            top_left: corners.top_left,
            top_right: corners.top_right,
            bottom_right: corners.bottom_right,
            bottom_left: corners.bottom_left,
            continuous: false,
        }
    }

    /// The radii, without the curve.
    #[inline]
    pub const fn corners(self) -> Corners<Points> {
        Corners {
            top_left: self.top_left,
            top_right: self.top_right,
            bottom_right: self.bottom_right,
            bottom_left: self.bottom_left,
        }
    }

    /// `[top_left, top_right, bottom_right, bottom_left]`.
    #[inline]
    pub const fn to_array(self) -> [Points; 4] {
        [
            self.top_left,
            self.top_right,
            self.bottom_right,
            self.bottom_left,
        ]
    }

    /// True if no corner is rounded.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.to_array().iter().all(|&r| r <= Points::ZERO)
    }

    /// The radius shared by every rounded corner, if they all share one.
    /// Platforms render such shapes with a single radius plus a mask of
    /// the rounded corners (no paths).
    pub fn shared(&self) -> Option<Points> {
        let mut shared = Points::ZERO;
        for r in self.to_array() {
            if r > Points::ZERO {
                if shared > Points::ZERO && r != shared {
                    return None;
                }
                shared = r;
            }
        }
        Some(shared)
    }

    /// Scales the radii down so adjacent corners do not overlap on a box
    /// of `size` (CSS rule).
    pub fn fit(self, size: Size<Points>) -> Self {
        let ratio = |side: Points, a: Points, b: Points| {
            if a + b > side { side / (a + b) } else { 1.0 }
        };
        let (width, height) = (size.width, size.height);
        let f = ratio(width, self.top_left, self.top_right)
            .min(ratio(width, self.bottom_left, self.bottom_right))
            .min(ratio(height, self.top_left, self.bottom_left))
            .min(ratio(height, self.top_right, self.bottom_right))
            .max(0.0);
        if f >= 1.0 {
            return self;
        }
        Self {
            top_left: self.top_left * f,
            top_right: self.top_right * f,
            bottom_right: self.bottom_right * f,
            bottom_left: self.bottom_left * f,
            ..self
        }
    }

    /// The radii of a curve `d` inside this one (e.g. the inner edge of a
    /// border).
    pub fn inset(self, d: Points) -> Self {
        Self {
            top_left: (self.top_left - d).max(Points::ZERO),
            top_right: (self.top_right - d).max(Points::ZERO),
            bottom_right: (self.bottom_right - d).max(Points::ZERO),
            bottom_left: (self.bottom_left - d).max(Points::ZERO),
            ..self
        }
    }
}

impl From<CornerRadii> for Corners<Points> {
    fn from(radii: CornerRadii) -> Self {
        radii.corners()
    }
}

/// How borders are drawn (`borderStyle`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BorderStyle {
    /// A solid line (default).
    #[default]
    Solid,
    /// Dashes.
    Dashed,
    /// Dots.
    Dotted,
}

/// Border widths and colors, per side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Borders {
    /// Widths.
    pub widths: Edges<Points>,
    /// Colors: top, right, bottom, left.
    pub colors: [Color; 4],
    /// Line style.
    pub style: BorderStyle,
}

impl Borders {
    /// No border.
    pub const NONE: Borders = Borders {
        widths: Edges {
            top: Points::ZERO,
            right: Points::ZERO,
            bottom: Points::ZERO,
            left: Points::ZERO,
        },
        colors: [Color::BLACK; 4],
        style: BorderStyle::Solid,
    };

    /// True if some side draws something.
    pub fn is_visible(&self) -> bool {
        let widths = self.widths.to_array();
        (0..4).any(|i| widths[i] > Points::ZERO && !self.colors[i].is_transparent())
    }

    /// `(width, color)` when every side is the same.
    pub fn uniform(&self) -> Option<(Points, Color)> {
        let widths = self.widths.to_array();
        let [w, ..] = widths;
        let [c, ..] = self.colors;
        (widths.iter().all(|&x| x == w) && self.colors.iter().all(|&x| x == c)).then_some((w, c))
    }

    /// The width of side `side`: 0 top, 1 right, 2 bottom, 3 left (the
    /// order of `colors`).
    pub fn width(&self, side: usize) -> Points {
        match side {
            0 => self.widths.top,
            1 => self.widths.right,
            2 => self.widths.bottom,
            _ => self.widths.left,
        }
    }

    fn width_mut(&mut self, side: usize) -> &mut Points {
        match side {
            0 => &mut self.widths.top,
            1 => &mut self.widths.right,
            2 => &mut self.widths.bottom,
            _ => &mut self.widths.left,
        }
    }
}

impl Default for Borders {
    fn default() -> Self {
        Self::NONE
    }
}

/// Visual (non-layout) properties of a native view.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewStyle {
    /// Background fill.
    pub background_color: Color,
    /// `borderRadius`: the radius of corners without their own.
    pub border_radius: Points,
    /// `borderTopLeftRadius` etc.; they override `border_radius`.
    pub corner_radius: Corners<Option<Points>>,
    /// `borderCurve: "continuous"`.
    pub continuous_corners: bool,
    /// Borders (their widths also affect layout).
    pub borders: Borders,
    /// Opacity in `0.0..=1.0`.
    pub opacity: f32,
    /// Clip children to the (rounded) bounds.
    pub overflow_hidden: bool,
    /// Static transform, around the view's center.
    pub transform: Transform,
    /// `tintColor`: recolors an image's opaque pixels.
    pub tint_color: Option<Color>,
    /// Android's material shadow: the node's `android={{ elevation }}` prop,
    /// not a style.
    pub elevation: Points,
    /// `transformOrigin`: the point transforms act around.
    pub transform_origin: TransformOrigin,
    /// `objectPosition`: where an image sits in its box (image views; the
    /// platforms take it from [`crate::mount::ImageOptions::position`]).
    pub object_position: ObjectPosition,
    /// `backfaceVisibility: "hidden"`: invisible when turned away.
    pub backface_hidden: bool,
    /// Layered / inset shadows, gradients and filters.
    pub effects: Option<Arc<Effects>>,
    /// `mixBlendMode`.
    pub blend_mode: BlendMode,
    /// The iOS material from `ios={{ backdropMaterial }}`, projected at mount time.
    pub backdrop_material: BackdropMaterial,
    /// `glassEffect`.
    pub glass_effect: GlassEffect,
    /// `outline*`.
    pub outline: Outline,
    /// `visibility: "hidden"`: not drawn and not touchable, still laid out.
    pub hidden: bool,
    /// `cursor`.
    pub cursor: Cursor,
    /// `transition*`: style changes animate.
    pub transition: Option<Transition>,
    /// `animation*`: a keyframe animation runs.
    pub animation: Option<KeyframeAnimation>,
}

/// `transformOrigin`: the point transforms rotate and scale around.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformOrigin {
    /// Horizontal position: from the left edge, or a fraction of the width.
    pub x: DefiniteLength,
    /// Vertical position: from the top edge, or a fraction of the height.
    pub y: DefiniteLength,
    /// Depth.
    pub z: Points,
}

impl Default for TransformOrigin {
    fn default() -> Self {
        Self::CENTER
    }
}

impl TransformOrigin {
    /// The center (the default).
    pub const CENTER: Self = Self {
        x: DefiniteLength::Fraction(Percentage(0.5)),
        y: DefiniteLength::Fraction(Percentage(0.5)),
        z: Points::ZERO,
    };

    /// Offset of the origin from the center of a box of `size`, in the
    /// plane, and its depth.
    pub fn offset(&self, size: Size<Points>) -> (Point<Points>, Points) {
        let at = |len: DefiniteLength, side: Points| {
            len.to_points(AbsoluteLength::Points(side), REM_SIZE) - side / 2.0
        };
        (
            point(at(self.x, size.width), at(self.y, size.height)),
            self.z,
        )
    }

    /// Parses CSS's syntax: one to three of keywords (`left`, `center`,
    /// `right`, `top`, `bottom`), percentages and lengths (`10px` or a
    /// bare number), the third being a depth.
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        let fraction = |f: f32| DefiniteLength::Fraction(percentage(f));
        let len = |p: &str| -> Option<DefiniteLength> {
            if let Some(pct) = p.strip_suffix('%') {
                return pct.trim().parse::<f32>().ok().map(|v| fraction(v / 100.0));
            }
            p.strip_suffix("px")
                .unwrap_or(p)
                .parse::<f32>()
                .ok()
                .map(|v| px(v).into())
        };
        let mut x = None;
        let mut y = None;
        let mut free = Vec::new();
        for &p in parts.iter().take(2) {
            match p {
                "left" => x = Some(fraction(0.0)),
                "right" => x = Some(fraction(1.0)),
                "top" => y = Some(fraction(0.0)),
                "bottom" => y = Some(fraction(1.0)),
                "center" => free.push(fraction(0.5)),
                p => free.push(len(p)?),
            }
        }
        // Unnamed values fill x, then y.
        for v in free {
            if x.is_none() {
                x = Some(v);
            } else if y.is_none() {
                y = Some(v);
            } else {
                return None;
            }
        }
        let z = match parts.get(2) {
            Some(p) => px(p.strip_suffix("px").unwrap_or(p).parse::<f32>().ok()?),
            None => Points::ZERO,
        };
        Some(Self {
            x: x.unwrap_or(fraction(0.5)),
            y: y.unwrap_or(fraction(0.5)),
            z,
        })
    }
}

/// One axis of a CSS `<position>`: a distance from the start edge (left,
/// top) or the end edge (right, bottom). Percentages are fractions of the
/// free space: the box's size less the content's (negative when the
/// content overflows), as CSS's `object-position` resolves them.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PositionAxis {
    /// Measured from the end edge (right, bottom) instead of the start.
    pub from_end: bool,
    /// The distance from that edge.
    pub offset: DefiniteLength,
}

impl Default for PositionAxis {
    /// The center.
    fn default() -> Self {
        Self::CENTER
    }
}

impl PositionAxis {
    /// The start edge (`left`, `top`, `0%`).
    pub const START: Self = Self::start(DefiniteLength::Fraction(Percentage(0.0)));
    /// The center (`center`, `50%`).
    pub const CENTER: Self = Self::start(DefiniteLength::Fraction(Percentage(0.5)));
    /// The end edge (`right`, `bottom`, `100%`).
    pub const END: Self = Self::start(DefiniteLength::Fraction(Percentage(1.0)));

    /// `offset` from the start edge.
    pub const fn start(offset: DefiniteLength) -> Self {
        Self {
            from_end: false,
            offset,
        }
    }

    /// `offset` from the end edge.
    pub const fn end(offset: DefiniteLength) -> Self {
        Self {
            from_end: true,
            offset,
        }
    }

    /// Where content starts along this axis, from the box's start edge,
    /// given the `free` space (the box's size less the content's).
    pub fn resolve(&self, free: Points) -> Points {
        let d = self
            .offset
            .to_points(AbsoluteLength::Points(free), REM_SIZE);
        if self.from_end { free - d } else { d }
    }
}

/// `objectPosition`: where an image sits in its box once
/// `objectFit` / `object-*` has sized it, as CSS's `object-position`
/// places it (the center by default). The parts of a covering image
/// outside the box are cropped.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct ObjectPosition {
    /// Horizontal.
    pub x: PositionAxis,
    /// Vertical.
    pub y: PositionAxis,
}

impl ObjectPosition {
    /// The center (the default).
    pub const CENTER: Self = Self {
        x: PositionAxis::CENTER,
        y: PositionAxis::CENTER,
    };

    /// A position of two axes.
    pub const fn new(x: PositionAxis, y: PositionAxis) -> Self {
        Self { x, y }
    }

    /// Parses CSS's `<position>` as `object-position` takes it: one or two
    /// keywords, percentages or lengths (`top`, `left bottom`, `25% 75%`,
    /// `10px 0`), or edges with offsets (`right 10px bottom 20%`).
    pub fn parse(s: &str) -> Option<Self> {
        css::object_position(s)
    }

    /// The top left corner of content of `content` size placed in a box of
    /// `size`.
    pub fn place(&self, size: Size<Points>, content: Size<Points>) -> Point<Points> {
        point(
            self.x.resolve(size.width - content.width),
            self.y.resolve(size.height - content.height),
        )
    }
}

impl std::fmt::Display for ObjectPosition {
    /// CSS's four-value form (`left 50% top 10px`), which every position
    /// can be written in.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let length = |l: DefiniteLength| match l {
            DefiniteLength::Absolute(a) => format!("{}px", a.to_points(REM_SIZE).0),
            DefiniteLength::Fraction(p) => format!("{}%", p.0 * 100.0),
        };
        write!(
            f,
            "{} {} {} {}",
            if self.x.from_end { "right" } else { "left" },
            length(self.x.offset),
            if self.y.from_end { "bottom" } else { "top" },
            length(self.y.offset),
        )
    }
}

/// `fontVariant`: OpenType features, as bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontVariant(pub u8);

impl FontVariant {
    /// `small-caps`.
    pub const SMALL_CAPS: u8 = 1;
    /// `oldstyle-nums`.
    pub const OLDSTYLE_NUMS: u8 = 2;
    /// `lining-nums`.
    pub const LINING_NUMS: u8 = 4;
    /// `tabular-nums`.
    pub const TABULAR_NUMS: u8 = 8;
    /// `proportional-nums`.
    pub const PROPORTIONAL_NUMS: u8 = 16;

    /// Parses space-separated variant names.
    pub fn parse(s: &str) -> Option<Self> {
        let mut bits = 0;
        for word in s.split_whitespace() {
            bits |= match word {
                "small-caps" => Self::SMALL_CAPS,
                "oldstyle-nums" => Self::OLDSTYLE_NUMS,
                "lining-nums" => Self::LINING_NUMS,
                "tabular-nums" => Self::TABULAR_NUMS,
                "proportional-nums" => Self::PROPORTIONAL_NUMS,
                "normal" => 0,
                _ => return None,
            };
        }
        Some(Self(bits))
    }

    /// Whether `bit` is set.
    pub fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    /// The OpenType feature tags.
    pub fn tags(self) -> impl Iterator<Item = &'static str> {
        [
            (Self::SMALL_CAPS, "smcp"),
            (Self::OLDSTYLE_NUMS, "onum"),
            (Self::LINING_NUMS, "lnum"),
            (Self::TABULAR_NUMS, "tnum"),
            (Self::PROPORTIONAL_NUMS, "pnum"),
        ]
        .into_iter()
        .filter(move |&(bit, _)| self.has(bit))
        .map(|(_, tag)| tag)
    }
}

impl ViewStyle {
    /// Drops the gradient backgrounds, so the background color shows: what
    /// press feedback and animated background colors do (a gradient is
    /// drawn over the color and would hide them).
    pub(crate) fn clear_background_images(&mut self) {
        if let Some(e) = self
            .effects
            .as_deref()
            .filter(|e| !e.background_images.is_empty())
        {
            let mut effects = e.clone();
            effects.background_images.clear();
            self.effects = crate::effects::shared(effects);
        }
    }

    /// The radius of each corner (before fitting them to a size with
    /// [`CornerRadii::fit`]).
    pub fn radii(&self) -> CornerRadii {
        let radii = self
            .corner_radius
            .map(|r| r.unwrap_or(self.border_radius).max(Points::ZERO));
        CornerRadii {
            continuous: self.continuous_corners,
            ..CornerRadii::circular(radii)
        }
    }
}

impl Default for ViewStyle {
    fn default() -> Self {
        Self {
            background_color: Color::TRANSPARENT,
            border_radius: Points::ZERO,
            corner_radius: Corners {
                top_left: None,
                top_right: None,
                bottom_right: None,
                bottom_left: None,
            },
            continuous_corners: false,
            borders: Borders::NONE,
            opacity: 1.0,
            overflow_hidden: false,
            transform: Transform::IDENTITY,
            tint_color: None,
            elevation: Points::ZERO,
            transform_origin: TransformOrigin::CENTER,
            object_position: ObjectPosition::CENTER,
            backface_hidden: false,
            effects: None,
            blend_mode: BlendMode::Normal,
            backdrop_material: BackdropMaterial::UltraThin,
            glass_effect: GlassEffect::None,
            outline: Outline::NONE,
            hidden: false,
            cursor: Cursor::Auto,
            transition: None,
            animation: None,
        }
    }
}

/// A shadow behind text (`textShadow`, `text-shadow-*`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextShadow {
    /// Color.
    pub color: Color,
    /// Horizontal offset.
    pub offset_x: Points,
    /// Vertical offset.
    pub offset_y: Points,
    /// Blur radius.
    pub radius: Points,
}

/// Text properties as written on a node; unset values are inherited from
/// the enclosing text.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextStyle {
    /// Foreground color.
    pub color: Option<Color>,
    /// Font size.
    pub font_size: Option<Points>,
    /// Font weight, `100..=900`.
    pub font_weight: Option<u16>,
    /// Line height.
    pub line_height: Option<Points>,
    /// Block alignment (only meaningful on a text block).
    pub text_align: Option<TextAlign>,
    /// Whether `text_align`'s left or right is a side (`left`, `right`),
    /// kept right to left, rather than the start or the end (`start`,
    /// `end`), which right to left is the other side.
    pub text_align_physical: bool,
    /// Font family; `None` inherits (the system font at the root).
    pub font_family: Option<FontFamily>,
    /// Italic (`fontStyle`).
    pub italic: Option<bool>,
    /// Extra space between characters.
    pub letter_spacing: Option<Points>,
    /// Extra space between characters in ems of this node's font size
    /// (`"0.05em"`); overrides `letter_spacing` when set after it.
    pub letter_spacing_em: Option<f32>,
    /// A line-height length relative to this node's font size (`em` or
    /// a percentage), resolved before inheritance.
    pub line_height_em: Option<f32>,
    /// Line height as a multiple of the font size (`"1.5"`),
    /// inherited as a multiple.
    pub line_height_multiple: Option<f32>,
    /// Underline / strikethrough.
    pub decoration: Option<TextDecoration>,
    /// `textDecorationStyle`.
    pub decoration_style: Option<DecorationStyle>,
    /// `textDecorationThickness`.
    pub decoration_thickness: Option<Points>,
    /// `textUnderlineOffset`: below the baseline.
    pub underline_offset: Option<Points>,
    /// Color of the decoration lines; the text color by default.
    pub decoration_color: Option<Color>,
    /// Case transform.
    pub transform: Option<TextTransform>,
    /// The text shadow's color (`textShadow`, `text-shadow-*`: its widest
    /// layer's).
    pub shadow_color: Option<Color>,
    /// The text shadow's offset.
    pub shadow_offset: Option<Point<Points>>,
    /// The text shadow's blur radius.
    pub shadow_radius: Option<Points>,
    /// `fontVariant`.
    pub font_variant: Option<FontVariant>,
    /// `writingDirection`: right to left (`Some(true)`), left to right, or
    /// from the content (`None`, `auto`).
    pub writing_direction: Option<bool>,
    /// `textAlignVertical`: where text shorter than its box sits, 0% top
    /// (`auto`, `top`), 50% `center`, 100% `bottom`.
    pub align_vertical: Option<Percentage>,
    /// `placeholderColor`: an input's placeholder (not inherited).
    pub placeholder_color: Option<Color>,
    /// `selectionColor`: an input's or a selectable text's selection
    /// highlight (not inherited).
    pub selection_color: Option<Color>,
    /// `caretColor`: an input's caret (not inherited).
    pub caret_color: Option<Color>,
}

impl TextStyle {
    /// The text shadow these properties describe, if any. Like NSShadow, an
    /// offset or radius without a color uses black at a third of opacity.
    pub fn shadow(&self) -> Option<TextShadow> {
        if self.shadow_color.is_none()
            && self.shadow_offset.is_none()
            && self.shadow_radius.is_none()
        {
            return None;
        }
        let color = self.shadow_color.unwrap_or(Color(0x0000_0055));
        if color.is_transparent() {
            return None;
        }
        let offset = self.shadow_offset.unwrap_or_default();
        Some(TextShadow {
            color,
            offset_x: offset.x,
            offset_y: offset.y,
            radius: self.shadow_radius.unwrap_or_default().max(Points::ZERO),
        })
    }
}

/// What `fill` or `stroke` paints with (CSS's and SVG's `<paint>`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SvgPaint {
    /// `none`: nothing.
    None,
    /// `currentColor`: the element's text color (`color`, inherited).
    CurrentColor,
    /// A color.
    Color(Color),
}

/// SVG's paint properties, as CSS has them on any element and inherits
/// them (`fill`, `stroke`, `strokeWidth`, `strokeDasharray`; `fill-*` /
/// `stroke-*`): what the shapes a Rust component draws paint with (the
/// marks of `nsh/base/plot`), read through a drawing's
/// [`crate::canvas::ViewPaint`]s. Unset values inherit; at the root they
/// are CSS's initial values, which the drawing names.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct PaintStyle {
    /// `fill`.
    pub fill: Option<SvgPaint>,
    /// `stroke`.
    pub stroke: Option<SvgPaint>,
    /// `strokeWidth`.
    pub stroke_width: Option<Points>,
    /// `strokeDasharray`: the dashes and gaps, empty for `none`.
    pub stroke_dasharray: Option<Arc<[Points]>>,
}

impl PaintStyle {
    /// These properties where set, `parent`'s (the inherited ones) where
    /// not.
    pub fn inherit(&self, parent: &PaintStyle) -> PaintStyle {
        PaintStyle {
            fill: self.fill.or(parent.fill),
            stroke: self.stroke.or(parent.stroke),
            stroke_width: self.stroke_width.or(parent.stroke_width),
            stroke_dasharray: self
                .stroke_dasharray
                .clone()
                .or_else(|| parent.stroke_dasharray.clone()),
        }
    }
}

/// A complete node style, split by consumer.
#[derive(Debug, Clone)]
pub struct Style {
    /// Layout properties, consumed by taffy.
    pub layout: taffy::Style,
    /// Visual properties of the native view.
    pub view: ViewStyle,
    /// Text properties.
    pub text: TextStyle,
    /// SVG's paint properties (inherited, as text properties are).
    pub paint: PaintStyle,
    /// Visual properties driven by native animation nodes; they override
    /// the static values while bound.
    pub animated: Vec<(crate::animation::AnimationProp, crate::animation::NodeId)>,
    /// `zIndex`: stacking order among siblings (higher on top).
    pub z_index: i32,
    /// `verticalAlign`: where the element sits in its line when it is
    /// inside a `<Text>`.
    pub vertical_align: VerticalAlign,
    /// `direction`: right to left (`Some(true)`), left to right, or
    /// inherited (`None`).
    pub direction: Option<bool>,
    /// Start / end properties, placed by the resolved direction.
    pub logical: Logical,
    /// Values that only show together with another property.
    pub(crate) pending: Pending,
    /// CSS's `perspective` (`perspective-*`, `Styled::perspective`): the
    /// depth the view gives its children's 3D transforms, seen from its
    /// center; the core composes it into their transforms.
    pub perspective: Option<Points>,
}

/// Who takes a pointer, as the host mounts it. Not inherited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PointerEvents {
    /// The view and the views in it.
    #[default]
    Auto,
    /// Neither the view nor the views in it.
    None,
    /// The views in it, not the view itself.
    BoxNone,
    /// The view itself, never the views in it.
    BoxOnly,
}

/// Values of properties that take effect only together with another one,
/// kept so that the result does not depend on the order they are set in:
/// `outlineWidth` under `outlineStyle: "none"` and the border widths under
/// `borderStyle: "none"` (no outline, no border, as in CSS),
/// `animationDuration` and the other `animation*` keys before
/// `animationName`.
#[derive(Clone, Default)]
pub(crate) struct Pending {
    /// `outlineWidth` as set.
    outline_width: Points,
    /// `outlineStyle: "none"` / `"hidden"`.
    outline_hidden: bool,
    /// The border widths as set: top, right, bottom, left.
    border_widths: [Points; 4],
    /// `borderStartWidth`, `borderEndWidth` as set.
    border_logical: [Option<Points>; 2],
    /// `borderStyle: "none"` / `"hidden"`: every used width is 0.
    border_hidden: bool,
    /// `position: "fixed"`: laid out and mounted in its root view's box.
    pub(crate) fixed_position: bool,
    /// `position: "sticky"`: laid out as `relative`, its insets where it
    /// sticks in the viewport of the scroll view around it.
    pub(crate) sticky_position: bool,
    /// `animationDuration`.
    animation_duration: Option<f32>,
    /// `animationDelay`.
    animation_delay: Option<f32>,
    /// `animationTimingFunction`.
    animation_easing: Option<[f32; 4]>,
    /// `animationIterationCount` (`None` inside: forever).
    animation_iterations: Option<Option<u32>>,
    /// The `env()` terms of the padding and margin, added by the core to
    /// the lengths their declarations set.
    pub(crate) env: EnvEdges,
    /// `objectFit`: how an image or a video fits its box (`None`: the
    /// element's default).
    pub(crate) object_fit: Option<crate::ResizeMode>,
    /// `numberOfLines`: the most lines of a text (`Some(0)`, `none`: no clamp
    /// over a class's).
    pub(crate) number_of_lines: Option<u32>,
    /// `textOverflow`: where a clamped text is cut.
    pub(crate) text_overflow: Option<EllipsizeMode>,
    /// `userSelect`: whether a text can be selected (`None`, `auto`: the
    /// view's around it, else the platform's).
    pub(crate) user_select: Option<bool>,
    /// `pointerEvents`, when the style sets it. `None` is not set (`auto`,
    /// so a class can still decide).
    pub(crate) pointer_events: Option<PointerEvents>,
    /// The `boxShadow` layers (by index) that name no color: CSS's
    /// `currentColor`, filled in by [`Style::apply_current_color`].
    current_shadows: u64,
    /// The `textShadow` names no color.
    current_text_shadow: bool,
    /// The color properties set to `currentColor` last ([`current_bits`]),
    /// filled in by [`Style::apply_current_color`].
    current_colors: u16,
    /// The `filter` list's `drop-shadow()`s (by index) that name no color:
    /// CSS's `currentColor`.
    current_drop_shadows: u64,
    /// Values that write `currentColor` inside them (a gradient's stop, a
    /// `color-mix()`, a shadow's color), by property: parsed again with
    /// the color by [`Style::apply_current_color`].
    current_values: Vec<(StyleKey, Box<str>)>,
    /// How many entries of the transform list each [`TransformProperty`]
    /// holds, in their order, then the steps Rust components bind after
    /// them ([`StyleDecl::Transform`]).
    transform_counts: [u8; TRANSFORM_PARTS],
}

/// [`TransformProperty`]'s parts of a transform list, and the bound steps
/// after them.
const TRANSFORM_PARTS: usize = 5;

/// The properties a view's transform is made of, in the order CSS
/// composes them: `translate`, `rotate`, `scale`, then `transform` (CSS's
/// string, or a list of steps whose amounts may be animated values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransformProperty {
    /// `translate`.
    Translate,
    /// `rotate`.
    Rotate,
    /// `scale`.
    Scale,
    /// `transform`.
    Transform,
}

/// Bookkeeping, not what the style shows: styles that look the same print
/// the same (tests compare typed styles and classes so).
impl std::fmt::Debug for Pending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Pending")
    }
}

impl Pending {
    /// `animation` with the `animation*` values set.
    fn animation(&self, mut animation: KeyframeAnimation) -> KeyframeAnimation {
        if let Some(ms) = self.animation_duration {
            animation.duration = ms;
        }
        if let Some(ms) = self.animation_delay {
            animation.delay = ms;
        }
        if let Some(easing) = self.animation_easing {
            animation.easing = easing;
        }
        if let Some(count) = self.animation_iterations {
            animation.iterations = count;
        }
        animation
    }
}

/// A CSS environment variable (`env()`) a padding or margin may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvVar {
    /// `safe-area-inset-top`: the top inset of the root view the node
    /// renders in.
    SafeAreaInsetTop,
    /// `safe-area-inset-right`.
    SafeAreaInsetRight,
    /// `safe-area-inset-bottom`.
    SafeAreaInsetBottom,
    /// `safe-area-inset-left`.
    SafeAreaInsetLeft,
    /// `keyboard-inset-height` (the VirtualKeyboard API's): the software
    /// keyboard's height over the surface, 0 while it is hidden.
    KeyboardInsetHeight,
}

/// One `env()` use on a padding or margin edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvTerm {
    /// A single variable.
    Var(EnvVar),
    /// `max(env(a), env(b))`: the larger of the two live insets. A length
    /// stays outside, in `calc()`.
    Max(EnvVar, EnvVar),
}

impl EnvVar {
    /// The variable named `name`, if NSH has it.
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "safe-area-inset-top" => Self::SafeAreaInsetTop,
            "safe-area-inset-right" => Self::SafeAreaInsetRight,
            "safe-area-inset-bottom" => Self::SafeAreaInsetBottom,
            "safe-area-inset-left" => Self::SafeAreaInsetLeft,
            "keyboard-inset-height" => Self::KeyboardInsetHeight,
            _ => return None,
        })
    }
}

/// The `env()` terms of a node's padding and margin by physical edge (top,
/// right, bottom, left).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct EnvEdges {
    /// The padding's.
    pub(crate) padding: [Option<EnvTerm>; 4],
    /// The margin's.
    pub(crate) margin: [Option<EnvTerm>; 4],
}

impl EnvEdges {
    /// Whether no edge has a term.
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A padding or margin value using `env()`: `env(name)`, `env(name,
/// <length>)`, `calc(env(name) + <length>)`, `calc(<length> + env(name))`,
/// `max(env(a), env(b))`, or that `max` plus a length inside `calc()`.
/// `None` when `s` uses no `env()`; else the length part (in points) and
/// the term (none when an unknown name's fallback stands in for a single
/// `env()`), or an error for anything else. `max` takes two known names
/// and no fallback.
pub(crate) fn env_length(s: &str) -> Option<Result<(f32, Option<EnvTerm>), StyleError>> {
    if !s.contains("env(") {
        return None;
    }
    /// `env(name)` with a known name and no fallback.
    fn bare_env(s: &str) -> Option<EnvVar> {
        let args = s.trim().strip_prefix("env(")?.strip_suffix(')')?;
        if args.contains(',') {
            return None;
        }
        EnvVar::parse(args.trim())
    }
    // One term: `env(...)`, `max(env(...), env(...))`, or a length.
    fn term(s: &str) -> Option<(f32, Option<EnvTerm>)> {
        let s = s.trim();
        if let Some(args) = s.strip_prefix("max(").and_then(|r| r.strip_suffix(')')) {
            let parts = css::split_top(args, ',');
            let [a, b] = parts.as_slice() else {
                return None;
            };
            return Some((0.0, Some(EnvTerm::Max(bare_env(a)?, bare_env(b)?))));
        }
        let Some(args) = s.strip_prefix("env(").and_then(|r| r.strip_suffix(')')) else {
            return Some((css::length(s)?, None));
        };
        let (name, fallback) = match args.split_once(',') {
            Some((name, fallback)) => (name.trim(), Some(fallback.trim())),
            None => (args.trim(), None),
        };
        match EnvVar::parse(name) {
            Some(var) => Some((0.0, Some(EnvTerm::Var(var)))),
            // CSS: an unknown variable is its fallback.
            None => Some((css::length(fallback?)?, None)),
        }
    }
    let parse = |s: &str| {
        let s = s.trim();
        let Some(inner) = s.strip_prefix("calc(").and_then(|r| r.strip_suffix(')')) else {
            return term(s);
        };
        // `a + b`, the `+` outside parentheses and between white space, as
        // CSS requires.
        let mut depth = 0i32;
        let mut plus = None;
        for (i, c) in inner.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                '+' if depth == 0 => {
                    let spaced = inner[..i].ends_with(char::is_whitespace)
                        && inner[i + 1..].starts_with(char::is_whitespace);
                    if !spaced || plus.is_some() {
                        return None;
                    }
                    plus = Some(i);
                }
                _ => {}
            }
        }
        let at = plus?;
        let (a, b) = (term(inner[..at].trim())?, term(inner[at + 1..].trim())?);
        if a.1.is_some() && b.1.is_some() {
            return None;
        }
        Some((a.0 + b.0, a.1.or(b.1)))
    };
    Some(parse(s).ok_or(StyleError::InvalidValue))
}

mod parse;

#[cfg(test)]
mod tests;

pub use parse::{
    GridTemplateValue, LineHeight, Logical, PropertyValue, StyleDecl, StyleError, StyleKey,
    StylePatch, StyleValue,
};
#[allow(unused_imports)]
pub(crate) use parse::{
    OBJECT_STYLE_TYPES, STYLE_TYPES, StyleGroup, StyleType, apply_direction,
    resolve_normal_alignment,
};
