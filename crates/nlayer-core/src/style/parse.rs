//! Property parsing and apply.
#![allow(dead_code)] // the shadow tree is the caller, and it is not extracted yet
use std::{rc::Rc, sync::Arc};

use ordered_float::OrderedFloat;
use taffy::{
    AlignContent, AlignItems, Dimension, Display, FlexDirection, FlexWrap, LengthPercentage,
    LengthPercentageAuto, Position,
};

use crate::{
    css,
    effects::{
        BlendMode, BoxShadow, Cursor, Effects, Filter, GlassEffect, Gradient, KeyframeAnimation,
        Keyframes, Outline, Transition, TransitionProps,
    },
    geometry::{DefiniteLength, Percentage, Points, REM_SIZE, percentage, point, px},
    grid,
    transform::{Transform, TransformKind},
};

use super::*;

/// The physical edges (top, right, bottom, left) a margin or padding key
/// sets.
fn edges(key: StyleKey) -> &'static [usize] {
    use StyleKey as K;
    match key {
        K::Margin | K::Padding => &[0, 1, 2, 3],
        K::MarginHorizontal | K::PaddingHorizontal => &[1, 3],
        K::MarginVertical | K::PaddingVertical => &[0, 2],
        K::MarginTop | K::PaddingTop => &[0],
        K::MarginRight | K::PaddingRight => &[1],
        K::MarginBottom | K::PaddingBottom => &[2],
        _ => &[3],
    }
}

/// The length part of an `env()` value, in points ([`env_length`]).
fn env_points(v: StyleValue<'_>) -> Points {
    match v {
        StyleValue::Str(s) => px(env_length(s).and_then(Result::ok).map_or(0.0, |(l, _)| l)),
        _ => Points::ZERO,
    }
}

/// A padding edge: its length and its `env()` term.
fn padding_edge(v: StyleValue<'_>) -> Result<(LengthPercentage, Option<EnvTerm>), StyleError> {
    if let StyleValue::Str(s) = v
        && let Some(env) = env_length(s)
    {
        let (length, var) = env?;
        // Padding is never negative.
        if length < 0.0 {
            return Err(StyleError::InvalidValue);
        }
        return Ok((LengthPercentage::length(length), var));
    }
    Ok((lp_or_zero(v)?, None))
}

/// A margin edge: its length and its `env()` term.
fn margin_edge(v: StyleValue<'_>) -> Result<(LengthPercentageAuto, Option<EnvTerm>), StyleError> {
    if let StyleValue::Str(s) = v
        && let Some(env) = env_length(s)
    {
        let (length, var) = env?;
        return Ok((LengthPercentageAuto::length(length), var));
    }
    Ok((lpa_or_zero(v)?, None))
}

/// Properties named by start and end (`marginStart`, `borderTopEndRadius`
/// ...): left and right in left-to-right layouts, the other way around in
/// right-to-left ones.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Logical {
    /// `marginStart`, `marginEnd`.
    pub margin: [Option<LengthPercentageAuto>; 2],
    /// `paddingStart`, `paddingEnd`.
    pub padding: [Option<LengthPercentage>; 2],
    /// `start`, `end`.
    pub inset: [Option<LengthPercentageAuto>; 2],
    /// `borderStartWidth`, `borderEndWidth`.
    pub border_width: [Option<Points>; 2],
    /// `borderStartColor`, `borderEndColor`.
    pub border_color: [Option<Color>; 2],
    /// `borderTopStartRadius`, `borderTopEndRadius`,
    /// `borderBottomEndRadius`, `borderBottomStartRadius`.
    pub radius: [Option<Points>; 4],
}

impl Logical {
    /// Whether no start / end property is set.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

fn set_side<T>(rect: &mut taffy::Rect<T>, left: bool, value: T) {
    if left {
        rect.left = value;
    } else {
        rect.right = value;
    }
}

/// Gives an unset `alignContent` (taffy's `normal`) its meaning for the
/// node's display: React Native's `flex-start` for flex (and block)
/// containers, where taffy's `normal` would stretch wrapped lines, and
/// CSS's `normal` for grids, whose auto tracks then stretch to fill the
/// container. (`justifyContent`'s `normal` is already `flex-start` in
/// flex containers.)
pub(crate) fn resolve_normal_alignment(layout: &mut taffy::Style) {
    if layout.display != Display::Grid && layout.align_content == AlignContent::NORMAL {
        layout.align_content = AlignContent::FLEX_START;
    }
}

/// Lays a node out in direction `rtl`: its left and right properties keep
/// their sides, as in CSS, and its start / end ones take the sides the
/// direction gives them.
pub(crate) fn apply_direction(
    layout: &mut taffy::Style,
    view: &mut ViewStyle,
    logical: &Logical,
    rtl: bool,
) {
    layout.direction = if rtl {
        taffy::Direction::Rtl
    } else {
        taffy::Direction::Ltr
    };
    if logical.is_empty() {
        return;
    }
    // Start is the left side left to right.
    for (i, left) in [!rtl, rtl].into_iter().enumerate() {
        if let Some(v) = logical.margin[i] {
            set_side(&mut layout.margin, left, v);
        }
        if let Some(v) = logical.padding[i] {
            set_side(&mut layout.padding, left, v);
        }
        if let Some(v) = logical.inset[i] {
            set_side(&mut layout.inset, left, v);
        }
        let side = if left { 3 } else { 1 };
        if let Some(w) = logical.border_width[i] {
            set_side(&mut layout.border, left, LengthPercentage::length(w.0));
            *view.borders.width_mut(side) = w;
        }
        if let Some(c) = logical.border_color[i] {
            view.borders.colors[side] = c;
        }
    }
    // Corners: top start, top end, bottom end, bottom start.
    let [top_start, top_end, bottom_end, bottom_start] = logical.radius;
    let c = &mut view.corner_radius;
    let (top_left, top_right, bottom_right, bottom_left) = if rtl {
        (top_end, top_start, bottom_start, bottom_end)
    } else {
        (top_start, top_end, bottom_end, bottom_start)
    };
    for (corner, r) in [
        (&mut c.top_left, top_left),
        (&mut c.top_right, top_right),
        (&mut c.bottom_right, bottom_right),
        (&mut c.bottom_left, bottom_left),
    ] {
        if r.is_some() {
            *corner = r;
        }
    }
}

impl Default for Style {
    /// React Native defaults, which differ from CSS (and taffy) defaults.
    fn default() -> Self {
        let layout = taffy::Style {
            display: Display::Flex,
            position: Position::Relative,
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            // Unset: `flex-start` (React Native) outside grids, CSS's
            // `normal` in grids (`resolve_normal_alignment`).
            align_content: AlignContent::NORMAL,
            // Unset: 0, as in Yoga (`min_len`).
            min_size: taffy::Size {
                width: LengthPercentageAuto::length(0.0),
                height: LengthPercentageAuto::length(0.0),
            },
            ..taffy::Style::DEFAULT
        };
        Self {
            layout,
            view: ViewStyle::default(),
            text: TextStyle::default(),
            paint: PaintStyle::default(),
            animated: Vec::new(),
            z_index: 0,
            vertical_align: VerticalAlign::Baseline,
            direction: None,
            logical: Logical::default(),
            pending: Pending::default(),
            perspective: None,
        }
    }
}

/// Which TypeScript style interface a property belongs to (each extends
/// the one before: layout, then views, then text or images).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StyleGroup {
    Layout,
    View,
    Text,
    Image,
    Input,
}

/// A style property's TypeScript declaration (`js::style`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct StyleType {
    pub(crate) name: &'static str,
    pub(crate) group: StyleGroup,
    pub(crate) ts: &'static str,
    pub(crate) doc: Option<&'static str>,
}

/// The style properties the JS bridge decodes itself (objects and lists,
/// `crates/nsh-js/src/protocol.rs`), for the TypeScript declarations.
pub(crate) const OBJECT_STYLE_TYPES: &[StyleType] = &[];

macro_rules! style_keys {
    ($($key:ident = $name:literal: $group:ident $ts:literal $($doc:literal)?),* $(,)?) => {
        /// The style properties' TypeScript declarations, in order.
        pub(crate) const STYLE_TYPES: &[StyleType] = &[$(StyleType {
            name: $name,
            group: StyleGroup::$group,
            ts: $ts,
            doc: style_keys!(@doc $($doc)?),
        }),*];

        /// A style property, by its React Native name.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum StyleKey {
            $(
                #[doc = concat!("`", $name, "`")]
                $key,
            )*
        }

        impl StyleKey {
            /// The key named `name` (React Native's camel case).
            pub fn parse(name: &str) -> Option<StyleKey> {
                Some(match name {
                    $($name => StyleKey::$key,)*
                    _ => return None,
                })
            }

            /// The React Native name.
            pub fn name(self) -> &'static str {
                match self {
                    $(StyleKey::$key => $name,)*
                }
            }
        }
    };
    (@doc) => { None };
    (@doc $doc:literal) => { Some($doc) };
}

style_keys! {
    Display = "display": Layout "\"flex\" | \"grid\" | \"block\" | \"flow-root\" | \"none\"" "`flex` lays the children out in a row unless `flexDirection` (or a class) sets a direction, as in CSS; a view that sets no display lays them out in a column.",
    BoxSizing = "boxSizing": Layout "\"border-box\" | \"content-box\"" "The box that width and height include.",
    Position = "position": Layout "\"relative\" | \"absolute\" | \"static\" | \"fixed\" | \"sticky\"" "`static` stays in flow and lets absolute descendants use the nearest positioned ancestor as their containing block. `fixed` lays an element out in its root view and mounts it above that root view's children. `sticky`: in the flow, and while its parent shows it stays `top` (in a horizontal scroll view `left`) from the start of the viewport of the scroll view around it, as far as the end of its parent's content box; it draws above its siblings of the same `zIndex`.",
    FlexDirection = "flexDirection": Layout "\"row\" | \"column\" | \"row-reverse\" | \"column-reverse\"",
    FlexWrap = "flexWrap": Layout "\"wrap\" | \"nowrap\" | \"wrap-reverse\"",
    Flex = "flex": Layout "number | \"none\" | \"auto\" | \"initial\" | (string & {})" "CSS's shorthand: `1` is `1 1 0`, `\"none\"` is `0 0 auto`, `\"auto\"` is `1 1 auto`, or grow, shrink and basis written out (`\"2 0 50%\"`).",
    FlexGrow = "flexGrow": Layout "number",
    FlexShrink = "flexShrink": Layout "number",
    FlexBasis = "flexBasis": Layout "DimensionValue | \"min-content\" | \"max-content\" | \"fit-content\"",
    JustifyContent = "justifyContent": Layout "Exclude<Distribute, \"stretch\"> | \"stretch\"",
    AlignItems = "alignItems": Layout "Align",
    AlignSelf = "alignSelf": Layout "Align | \"auto\"",
    AlignContent = "alignContent": Layout "Distribute",
    Width = "width": Layout "DimensionValue | \"min-content\" | \"max-content\" | \"fit-content\"",
    Height = "height": Layout "DimensionValue | \"min-content\" | \"max-content\" | \"fit-content\"",
    MinWidth = "minWidth": Layout "LengthValue",
    MinHeight = "minHeight": Layout "LengthValue",
    MaxWidth = "maxWidth": Layout "LengthValue",
    MaxHeight = "maxHeight": Layout "LengthValue",
    AspectRatio = "aspectRatio": Layout "number | \"auto\" | `${number}/${number}` | `${number} / ${number}`" "Width over height: a number (`1.5`) or CSS's ratio (`\"16 / 9\"`); `\"auto\"` is none.",
    Top = "top": Layout "DimensionValue",
    Right = "right": Layout "DimensionValue",
    Bottom = "bottom": Layout "DimensionValue",
    Left = "left": Layout "DimensionValue",
    Margin = "margin": Layout "DimensionValue | EnvValue",
    MarginHorizontal = "marginHorizontal": Layout "DimensionValue | EnvValue",
    MarginVertical = "marginVertical": Layout "DimensionValue | EnvValue",
    MarginTop = "marginTop": Layout "DimensionValue | EnvValue",
    MarginRight = "marginRight": Layout "DimensionValue | EnvValue",
    MarginBottom = "marginBottom": Layout "DimensionValue | EnvValue",
    MarginLeft = "marginLeft": Layout "DimensionValue | EnvValue",
    Padding = "padding": Layout "LengthValue | EnvValue",
    PaddingHorizontal = "paddingHorizontal": Layout "LengthValue | EnvValue",
    PaddingVertical = "paddingVertical": Layout "LengthValue | EnvValue",
    PaddingTop = "paddingTop": Layout "LengthValue | EnvValue",
    PaddingRight = "paddingRight": Layout "LengthValue | EnvValue",
    PaddingBottom = "paddingBottom": Layout "LengthValue | EnvValue",
    PaddingLeft = "paddingLeft": Layout "LengthValue | EnvValue",
    Gap = "gap": Layout "LengthValue",
    RowGap = "rowGap": Layout "LengthValue",
    ColumnGap = "columnGap": Layout "LengthValue",
    BorderWidth = "borderWidth": View "number",
    BorderTopWidth = "borderTopWidth": View "number",
    BorderRightWidth = "borderRightWidth": View "number",
    BorderBottomWidth = "borderBottomWidth": View "number",
    BorderLeftWidth = "borderLeftWidth": View "number",
    ZIndex = "zIndex": Layout "number" "Order among siblings (stable); also the native stacking order.",
    BoxShadow = "boxShadow": View "string | BoxShadowLayer[]" "CSS `box-shadow`: offsets, blur, spread and color, several separated by commas, `inset` for inner shadows; or its layers (`[{ x: 0, y: 6, blur: 18, color: \"#0000004d\" }]`), which take numbers, not strings. A layer without a color is the text color (CSS's `currentColor`).",
    BackgroundImage = "backgroundImage": View "string" "CSS gradients drawn over the background color: `linear-gradient(...)`, `radial-gradient(...)`, `conic-gradient(...)`, several separated by commas.",
    Filter = "filter": View "string" "CSS filters of the view and its content: `brightness()`, `contrast()`, `grayscale()`, `hue-rotate()`, `invert()`, `saturate()`, `sepia()`, `opacity()`, `blur()`, `drop-shadow()`.",
    BackdropFilter = "backdropFilter": View "string" "CSS filters of what is behind the view (`blur(10px)`).",
    MaskImage = "maskImage": View "string" "A CSS gradient whose alpha decides how much of the view is drawn, fully where opaque and not at all where transparent: `linear-gradient(...)`, `radial-gradient(...)`, `conic-gradient(...)`. Masks everything the view draws, its backdrop filter included.",
    GlassEffect = "glassEffect": View "\"none\" | \"regular\" | \"clear\"" "A glass material behind the view's content (Apple's Liquid Glass). Drawn on iOS 26 and macOS 26 and later; elsewhere the view's `backdropFilter` is drawn as before, so give a glass view a blur to fall back to.",
    MixBlendMode = "mixBlendMode": View "\"normal\" | \"multiply\" | \"screen\" | \"overlay\" | \"darken\" | \"lighten\" | \"color-dodge\" | \"color-burn\" | \"hard-light\" | \"soft-light\" | \"difference\" | \"exclusion\" | \"hue\" | \"saturation\" | \"color\" | \"luminosity\" | \"plus-darker\" | \"plus-lighter\"" "How the view blends with what is behind it.",
    OutlineWidth = "outlineWidth": View "number" "A line outside the border that takes no space.",
    OutlineColor = "outlineColor": View "ColorValue",
    OutlineStyle = "outlineStyle": View "\"solid\" | \"dashed\" | \"dotted\" | \"none\" | \"hidden\"",
    OutlineOffset = "outlineOffset": View "number" "Distance from the border edge (negative: inside).",
    Visibility = "visibility": View "\"visible\" | \"hidden\" | \"collapse\"" "Hidden views keep their space.",
    Cursor = "cursor": View "\"auto\" | \"default\" | \"context-menu\" | \"alias\" | \"copy\" | \"cell\" | \"pointer\" | \"text\" | \"vertical-text\" | \"wait\" | \"progress\" | \"help\" | \"crosshair\" | \"move\" | \"all-scroll\" | \"grab\" | \"grabbing\" | \"not-allowed\" | \"no-drop\" | \"zoom-in\" | \"zoom-out\" | \"col-resize\" | \"e-resize\" | \"w-resize\" | \"ew-resize\" | \"row-resize\" | \"n-resize\" | \"s-resize\" | \"ns-resize\" | \"ne-resize\" | \"nw-resize\" | \"se-resize\" | \"sw-resize\" | \"nesw-resize\" | \"nwse-resize\" | \"none\"" "The pointer over the view (pointers and desktops).",
    TransitionProperty = "transitionProperty": View "string" "The properties that animate when they change (`\"opacity, transform\"`, `\"all\"`, `\"colors\"`).",
    TransitionDuration = "transitionDuration": View "number | `${number}ms` | `${number}s`" "Milliseconds as a number, or `\"150ms\"` / `\"0.2s\"`.",
    TransitionDelay = "transitionDelay": View "number | `${number}ms` | `${number}s`",
    TransitionTimingFunction = "transitionTimingFunction": View "\"linear\" | \"ease\" | \"ease-in\" | \"ease-out\" | \"ease-in-out\" | `cubic-bezier(${string})`",
    AnimationName = "animationName": View "\"none\" | \"spin\" | \"ping\" | \"pulse\" | \"bounce\"" "A built-in keyframe animation, run natively.",
    AnimationDuration = "animationDuration": View "number | `${number}ms` | `${number}s`",
    AnimationDelay = "animationDelay": View "number | `${number}ms` | `${number}s`",
    AnimationTimingFunction = "animationTimingFunction": View "\"linear\" | \"ease\" | \"ease-in\" | \"ease-out\" | \"ease-in-out\" | `cubic-bezier(${string})`",
    AnimationIterationCount = "animationIterationCount": View "number | \"infinite\"",
    TextDecorationStyle = "textDecorationStyle": Text "\"solid\" | \"double\" | \"dotted\" | \"dashed\" | \"wavy\"",
    TextDecorationThickness = "textDecorationThickness": Text "number | \"auto\" | \"from-font\"",
    TextUnderlineOffset = "textUnderlineOffset": Text "number | \"auto\"",
    GridTemplateColumns = "gridTemplateColumns": Layout "string" "CSS grid tracks: `\"repeat(3, 1fr)\"`, `\"200px 1fr\"`, `minmax()`, `auto-fill` / `auto-fit`, line names (`\"[full-start] 1fr [content-start] 2fr [content-end full-end]\"`).",
    GridTemplateRows = "gridTemplateRows": Layout "string",
    GridAutoColumns = "gridAutoColumns": Layout "string" "Tracks of implicitly created columns.",
    GridAutoRows = "gridAutoRows": Layout "string",
    GridAutoFlow = "gridAutoFlow": Layout "\"row\" | \"column\" | \"dense\" | \"row dense\" | \"column dense\"",
    GridColumn = "gridColumn": Layout "number | string" "`\"1 / 3\"`, `\"span 2\"`, a line number or name (`\"content-start / full-end\"`, `\"span col\"`).",
    GridColumnStart = "gridColumnStart": Layout "number | string",
    GridColumnEnd = "gridColumnEnd": Layout "number | string",
    GridRow = "gridRow": Layout "number | string",
    GridRowStart = "gridRowStart": Layout "number | string",
    GridRowEnd = "gridRowEnd": Layout "number | string",
    JustifyItems = "justifyItems": Layout "Align" "How grid items sit in their areas along the row.",
    JustifySelf = "justifySelf": Layout "Align | \"auto\"",
    Direction = "direction": Layout "\"inherit\" | \"ltr\" | \"rtl\"" "Layout direction; inherited by default. As in CSS, `left` and `right` properties keep their sides right to left, while the `inlineStart` / `inlineEnd` ones (`marginStart`, `end`, ...) take the sides the direction gives them.",
    MarginStart = "marginStart": Layout "DimensionValue",
    MarginEnd = "marginEnd": Layout "DimensionValue",
    PaddingStart = "paddingStart": Layout "LengthValue",
    PaddingEnd = "paddingEnd": Layout "LengthValue",
    Start = "start": Layout "DimensionValue" "Left in left-to-right layouts, right in right-to-left ones.",
    End = "end": Layout "DimensionValue",
    BorderStartWidth = "borderStartWidth": View "number",
    BorderEndWidth = "borderEndWidth": View "number",
    BorderStartColor = "borderStartColor": View "ColorValue",
    BorderEndColor = "borderEndColor": View "ColorValue",
    BorderTopStartRadius = "borderTopStartRadius": View "number",
    BorderTopEndRadius = "borderTopEndRadius": View "number",
    BorderBottomEndRadius = "borderBottomEndRadius": View "number",
    BorderBottomStartRadius = "borderBottomStartRadius": View "number",
    BackgroundColor = "backgroundColor": View "ColorValue | AnimatedNode",
    BorderRadius = "borderRadius": View "number",
    BorderTopLeftRadius = "borderTopLeftRadius": View "number",
    BorderTopRightRadius = "borderTopRightRadius": View "number",
    BorderBottomRightRadius = "borderBottomRightRadius": View "number",
    BorderBottomLeftRadius = "borderBottomLeftRadius": View "number",
    BorderColor = "borderColor": View "ColorValue | AnimatedNode",
    BorderTopColor = "borderTopColor": View "ColorValue",
    BorderRightColor = "borderRightColor": View "ColorValue",
    BorderBottomColor = "borderBottomColor": View "ColorValue",
    BorderLeftColor = "borderLeftColor": View "ColorValue",
    BorderStyle = "borderStyle": View "\"solid\" | \"dashed\" | \"dotted\" | \"none\" | \"hidden\"" "`none` and `hidden`: no border whatever the widths (they take no space).",
    BorderCurve = "borderCurve": View "\"circular\" | \"continuous\"" "`continuous`: corners whose curvature eases into the edges (iOS app icons).",
    TintColor = "tintColor": Image "ColorValue" "Recolors a template image.",
    ObjectFit = "objectFit": View "\"fill\" | \"contain\" | \"cover\" | \"none\" | \"scale-down\"" "CSS's `object-fit`: how an image or a video fits its box. Images cover it by default, videos are contained in it.",
    ObjectPosition = "objectPosition": Image "string" "Where the image sits in its box once `objectFit` has sized it, as CSS's `object-position`: keywords, percentages and lengths (`\"top\"`, `\"left bottom\"`, `\"25% 75%\"`, `\"right 10px bottom 20%\"`). A covering image is cropped around it. Default the center.",
    BackfaceVisibility = "backfaceVisibility": View "\"visible\" | \"hidden\"" "`\"hidden\"`: invisible while turned away (3D rotations).",
    Translate = "translate": View "string | number | AnimatedNode | [x: number | string | AnimatedNode, y?: number | string | AnimatedNode]" "CSS's `translate`: x, then y (`\"10px 20px\"`; percentages of the view's size, `\"-50% 0\"`); a number is points. An animated value moves it natively, along x (along both, each its own, in the `[x, y]` form).",
    Rotate = "rotate": View "string | number | AnimatedNode" "CSS's `rotate`: `\"45deg\"`, or about an axis, `\"x 90deg\"`; a number, and an animated value, is degrees about z.",
    Scale = "scale": View "string | number | AnimatedNode | [x: number | AnimatedNode, y: number | AnimatedNode]" "CSS's `scale`: `1.5`, `\"150%\"`, x and y (`\"1 2\"`, `[x, y]`).",
    Transform = "transform": View "string | TransformStep[]" "CSS's `transform`: `\"translateX(10px) rotate(45deg) scale(1.5)\"` (`translate()`, `scale()`, `rotate()` on any axis and `perspective()` functions), or `\"none\"`; or a list of steps whose amounts may be animated values (`[{ perspective: 800 }, { rotateY: angle }]`, a motion's `transform`). Applies after `translate`, `rotate` and `scale`, as in CSS.",
    TransformOrigin = "transformOrigin": View "string | (number | string)[]" "The point `transform` acts around: keywords, percentages and lengths (\"top left\", \"50% 0\", `[10, 20]`). Default the center.",
    Opacity = "opacity": View "number | AnimatedNode",
    Overflow = "overflow": View "\"visible\" | \"hidden\"",
    Color = "color": Text "ColorValue",
    FontSize = "fontSize": Text "number",
    FontWeight = "fontWeight": Text "\"normal\" | \"bold\" | number | `${number}`" "`\"normal\"` (400), `\"bold\"` (700) or 100–900.",
    LineHeight = "lineHeight": Text "number",
    FontFamily = "fontFamily": Text "string" "A system family or the name an imported font (`import Inter from \"./Inter.ttf\"`) evaluates to.",
    FontStyle = "fontStyle": Text "\"normal\" | \"italic\"",
    LetterSpacing = "letterSpacing": Text "number",
    TextDecorationLine = "textDecorationLine": Text "\"none\" | \"underline\" | \"line-through\" | \"underline line-through\"",
    TextDecorationColor = "textDecorationColor": Text "ColorValue",
    WritingDirection = "writingDirection": Text "\"auto\" | \"ltr\" | \"rtl\"" "Base direction of the text; `auto` takes it from the content.",
    TextAlignVertical = "textAlignVertical": Text "\"auto\" | \"top\" | \"center\" | \"bottom\"" "Where text shorter than its box sits (`<Text>` with a height).",
    FontVariant = "fontVariant": Text "FontVariantName[] | (string & {})" "OpenType number and letter forms: a list (`[\"tabular-nums\"]`), or CSS's names separated by spaces (`\"tabular-nums small-caps\"`).",
    TextShadow = "textShadow": Text "string | TextShadowLayer[]" "CSS's `text-shadow`: offsets, blur and color (`\"0 1px 2px rgb(0 0 0 / 0.3)\"`), several separated by commas, drawn as the widest one; or its layers (`[{ x: 0, y: 1, blur: 2, color: \"#0000004d\" }]`); a layer without a color is the text color; `\"none\"`.",
    TextTransform = "textTransform": Text "\"none\" | \"uppercase\" | \"lowercase\" | \"capitalize\"",
    TextAlign = "textAlign": Text "\"auto\" | \"left\" | \"center\" | \"right\" | \"justify\" | \"start\" | \"end\"" "`left` and `right` keep their sides in right-to-left layouts; `start` and `end` follow the direction.",
    PlaceholderColor = "placeholderColor": Input "ColorValue" "An input's placeholder (`placeholder:text-*`).",
    SelectionColor = "selectionColor": Text "ColorValue" "An input's or a selectable text's selection highlight (`selection:bg-*`); iOS draws an input's caret in it too, unless `caretColor` is set (UIKit tints both with one color).",
    CaretColor = "caretColor": Input "ColorValue" "CSS's `caret-color`: an input's caret (`caret-*`); on iOS also its selection highlight (UIKit tints both with one color).",
    Fill = "fill": View "\"none\" | \"currentColor\" | ColorValue" "What the shapes a component draws are filled with (CSS's and SVG's `fill`, inherited; `fill-*`): a chart's areas.",
    Stroke = "stroke": View "\"none\" | \"currentColor\" | ColorValue" "What the outlines of the shapes a component draws are painted with (CSS's and SVG's `stroke`, inherited; `stroke-<color>`): a chart's lines and grid lines.",
    StrokeWidth = "strokeWidth": View "number | string" "The width of those outlines, a length (CSS's and SVG's `stroke-width`, inherited, 1 by default; `stroke-<number>`).",
    StrokeDasharray = "strokeDasharray": View "number | string" "The dashes and gaps of those outlines, lengths separated by spaces or commas, an odd list repeated, or `none` (CSS's and SVG's `stroke-dasharray`, inherited).",
    NumberOfLines = "numberOfLines": Text "number" "The most lines a `<Text>` shows (CSS's `line-clamp`, class `line-clamp-*`); 0 shows them all.",
    TextOverflow = "textOverflow": Text "\"ellipsis\" | \"clip\" | \"ellipsis-start\" | \"ellipsis-middle\"" "CSS's `text-overflow`: how a clamped `<Text>` is cut, with an ellipsis at its end (the default), clipped, or with the ellipsis at its start or in its middle.",
    UserSelect = "userSelect": View "\"auto\" | \"none\" | \"text\"" "CSS's `user-select`: whether a `<Text>` can be selected (`\"text\"`) or not (`\"none\"`); a view's reaches the texts in it; `\"auto\"` follows the view around it, and is not selectable when none says so.",
    PointerEvents = "pointerEvents": View "\"auto\" | \"none\" | \"box-none\" | \"box-only\"" "Who takes a pointer. `\"none\"` takes none, the view and the views in it. `\"box-none\"` takes none on the view itself, only on the views in it. `\"box-only\"` takes them on the view itself, never on the views in it. `\"auto\"` takes them on both. Not inherited: a class `pointer-events-none` is lowered before mount.",
    VerticalAlign = "verticalAlign": View "\"baseline\" | \"text-top\" | \"text-bottom\" | \"middle\"" "Where a view or image inside a `<Text>` sits in its line (CSS `vertical-align`, `align-*`): its bottom on the baseline (the default), its top or bottom at the top or bottom of the text's font, or its middle half the font's x height above the baseline. The line grows to hold it.",
}

/// A style property value as read from JS.
#[derive(Debug, Clone, Copy)]
pub enum StyleValue<'a> {
    /// A number.
    Number(f32),
    /// A string.
    Str(&'a str),
    /// `null` / `undefined`: resets the property to its default.
    Null,
}

/// CSS's `line-height` (`lineHeight`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum LineHeight {
    /// The font's own (`normal`).
    #[default]
    Normal,
    /// Points (`"20px"`).
    Points(Points),
    /// A multiple of the font size, which nested texts take with their own
    /// size (a number: `1.5`).
    Multiple(f32),
    /// A multiple of the element's own font size, which nested texts take
    /// as points (`"1.5em"`, `"150%"`).
    Em(f32),
}

impl From<Points> for LineHeight {
    fn from(points: Points) -> Self {
        LineHeight::Points(points)
    }
}

impl From<crate::geometry::Rems> for LineHeight {
    fn from(rems: crate::geometry::Rems) -> Self {
        LineHeight::Points(rems.to_points(crate::geometry::REM_SIZE))
    }
}

impl From<crate::geometry::AbsoluteLength> for LineHeight {
    fn from(length: crate::geometry::AbsoluteLength) -> Self {
        LineHeight::Points(length.to_points(crate::geometry::REM_SIZE))
    }
}

/// A grid template as a style holds it (`gridTemplateColumns`).
#[derive(Debug, Clone, PartialEq)]
pub struct GridTemplateValue(Rc<grid::Template>);

/// A style property's value, parsed ([`StyleKey::parse_value`]): what
/// [`Style::apply_value`] sets without parsing. TSX styles are parsed into
/// it once; classes and Rust's typed styles make it directly.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyValue {
    /// The property's default (`null`).
    Default,
    /// `display`.
    Display(Display),
    /// `boxSizing`.
    BoxSizing(taffy::BoxSizing),
    /// `position`: Taffy's, then whether it is `fixed` and `sticky`.
    Position(Position, bool, bool),
    /// `flexDirection`.
    FlexDirection(FlexDirection),
    /// `flexWrap`.
    FlexWrap(FlexWrap),
    /// `flex`: grow, shrink and basis.
    Flex(OrderedFloat<f32>, OrderedFloat<f32>, Dimension),
    /// A number (`flexGrow`, `zIndex`, `opacity`).
    Number(OrderedFloat<f32>),
    /// A number or none (`aspectRatio`).
    OptNumber(Option<OrderedFloat<f32>>),
    /// A size (`width`, `flexBasis`).
    Dimension(Dimension),
    /// A length, a percentage or `auto` (`margin`, `top`, `minWidth`).
    Lpa(LengthPercentageAuto),
    /// A length or a percentage (`padding`, `gap`).
    Lp(LengthPercentage),
    /// [`PropertyValue::Lpa`] or unset (`marginStart`).
    OptLpa(Option<LengthPercentageAuto>),
    /// [`PropertyValue::Lp`] or unset (`paddingStart`).
    OptLp(Option<LengthPercentage>),
    /// Points (`borderWidth`, `borderRadius`, `outlineWidth`).
    Points(Points),
    /// Points or unset (`fontSize`, a corner's radius).
    OptPoints(Option<Points>),
    /// `alignContent`, `justifyContent`.
    AlignContent(AlignContent),
    /// `alignItems`, `alignSelf` (`None`: `auto`), `justifyItems`.
    AlignItems(Option<AlignItems>),
    /// `verticalAlign`.
    VerticalAlign(VerticalAlign),
    /// Right to left (`Some(true)`), left to right, or unset (`direction`,
    /// `writingDirection`, `fontStyle`'s italic).
    OptBool(Option<bool>),
    /// `pointerEvents`.
    PointerEvents(PointerEvents),
    /// A flag (`overflow: hidden`, `visibility: hidden`, `borderCurve:
    /// continuous`, `backfaceVisibility: hidden`).
    Bool(bool),
    /// A color or unset.
    Color(Option<Color>),
    /// CSS's `currentColor`, written out: the node's text color.
    CurrentColor,
    /// A value that writes `currentColor` inside it, parsed with a
    /// stand-in color, and its text, parsed again with the node's color.
    Deferred(Box<str>, Box<PropertyValue>),
    /// `borderStyle`, `outlineStyle`: the style and whether it hides the
    /// line (`none`).
    BorderStyle(BorderStyle, bool),
    /// `objectFit` (`None`: the element's default).
    ObjectFit(Option<crate::ResizeMode>),
    /// `objectPosition`.
    ObjectPosition(ObjectPosition),
    /// A transform property's steps, each with its animated value if any.
    Transform(Box<[(TransformKind, f32, Option<crate::animation::NodeId>)]>),
    /// `transformOrigin`.
    TransformOrigin(TransformOrigin),
    /// `fontWeight`.
    FontWeight(Option<u16>),
    /// `lineHeight`.
    LineHeight(LineHeight),
    /// `fontFamily`.
    FontFamily(Option<FontFamily>),
    /// `letterSpacing`: points, or ems of the font size.
    LetterSpacing(Option<Points>, Option<OrderedFloat<f32>>),
    /// `textDecorationLine`.
    TextDecoration(Option<TextDecoration>),
    /// `fontVariant`.
    FontVariant(Option<FontVariant>),
    /// `textShadow`: the widest layer and whether it names its color.
    TextShadow(Option<(TextShadow, bool)>),
    /// `textTransform`.
    TextTransform(Option<TextTransform>),
    /// `textAlign`, and whether it is physical (`left` / `right`).
    TextAlign(Option<TextAlign>, bool),
    /// `boxShadow`'s layers, each with whether it names its color.
    Shadows(Rc<[(BoxShadow, bool)]>),
    /// `backgroundImage`, `maskImage`.
    Gradients(Rc<[Gradient]>),
    /// `filter`, `backdropFilter`, each with whether a `drop-shadow()`
    /// names its color.
    Filters(Rc<[(Filter, bool)]>),
    /// `glassEffect`.
    GlassEffect(GlassEffect),
    /// `mixBlendMode`.
    BlendMode(BlendMode),
    /// `cursor`.
    Cursor(Cursor),
    /// `transitionProperty`.
    TransitionProps(TransitionProps),
    /// Milliseconds (durations, delays).
    Ms(OrderedFloat<f32>),
    /// A timing function's cubic Bézier control points.
    Easing([OrderedFloat<f32>; 4]),
    /// `animationName`.
    Keyframes(Option<Keyframes>),
    /// `animationIterationCount` (`None`: forever).
    Iterations(Option<u32>),
    /// `fill`, `stroke` (`None`: unset, inherited).
    Paint(Option<SvgPaint>),
    /// `strokeDasharray` (empty: `none`).
    Dashes(Option<Arc<[Points]>>),
    /// A margin or padding of a length (points) plus an environment
    /// term (`calc(env(safe-area-inset-top) + 8px)`, or `max` of two).
    EnvLength(Points, EnvTerm),
    /// `textAlignVertical`: 0% the top, 50% the middle, 100% the bottom.
    AlignVertical(Option<Percentage>),
    /// `numberOfLines` (`Some(0)`: no clamp).
    NumberOfLines(Option<u32>),
    /// `textOverflow`.
    TextOverflow(Option<EllipsizeMode>),
    /// `textDecorationStyle`.
    DecorationStyle(Option<DecorationStyle>),
    /// `gridTemplateColumns`, `gridTemplateRows`.
    GridTemplate(GridTemplateValue),
    /// `gridAutoColumns`, `gridAutoRows`.
    GridTracks(Rc<[taffy::TrackSizingFunction]>),
    /// `gridAutoFlow`.
    GridAutoFlow(taffy::GridAutoFlow),
    /// `gridColumn`, `gridRow`.
    GridLine(taffy::Line<taffy::GridPlacement<String>>),
    /// `gridColumnStart` and the other lines.
    GridPlacement(taffy::GridPlacement<String>),
}

impl PropertyValue {
    /// A grid template's value.
    pub(crate) fn grid_template(template: grid::Template) -> Self {
        PropertyValue::GridTemplate(GridTemplateValue(Rc::new(template)))
    }
}

impl Eq for PropertyValue {}

impl std::hash::Hash for PropertyValue {
    /// The variant and its plain payloads: values that compare equal hash
    /// alike (list payloads are only compared).
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            PropertyValue::Number(n) | PropertyValue::Ms(n) => n.hash(state),
            PropertyValue::Points(p) => OrderedFloat(p.0).hash(state),
            PropertyValue::OptPoints(p) => p.map(|p| OrderedFloat(p.0)).hash(state),
            PropertyValue::Color(c) => c.hash(state),
            PropertyValue::Bool(b) => b.hash(state),
            PropertyValue::OptBool(b) => b.hash(state),
            PropertyValue::PointerEvents(p) => p.hash(state),
            PropertyValue::FontWeight(w) => w.hash(state),
            PropertyValue::Deferred(s, _) => s.hash(state),
            _ => {}
        }
    }
}

/// One declaration of a [`StylePatch`].
#[derive(Debug, Clone, PartialEq)]
pub enum StyleDecl {
    /// A property.
    Set(StyleKey, PropertyValue),
    /// Sets a transform property to its steps, each driven by an animated
    /// value when it has one (the amount is then the identity until it is
    /// bound): the steps of other properties stay.
    SetTransform(
        TransformProperty,
        Box<[(TransformKind, f32, Option<crate::animation::NodeId>)]>,
    ),
    /// Appends a transform step after every property's, driven by an
    /// animated value when given (a Rust component's `bind_transform`).
    Transform(TransformKind, f32, Option<crate::animation::NodeId>),
    /// A property driven by a native animation node.
    Animated(crate::animation::AnimationProp, crate::animation::NodeId),
    /// A value the property takes unless a class or the style itself sets
    /// it (`display: "flex"`'s row, as CSS's): applied before the classes
    /// ([`StylePatch::apply_defaults`]), never by [`StyleDecl::apply`].
    Default(StyleKey, PropertyValue),
    /// The color property is the node's text color unless a class or the
    /// style sets it: CSS's `currentColor`, the color of a border or an
    /// outline whose width the style sets. Applied before the classes, as
    /// [`StyleDecl::Default`].
    CurrentColor(StyleKey),
}

impl StyleDecl {
    /// Applies the declaration to `style`.
    pub fn apply(&self, style: &mut Style) -> Result<(), StyleError> {
        match self {
            StyleDecl::Set(key, value) => style.apply_value(*key, value),
            StyleDecl::SetTransform(property, steps) => style.set_transform(*property, steps),
            StyleDecl::Transform(kind, amount, node) => {
                // The bound steps are the list's last part: they go at its end.
                let index = style
                    .view
                    .transform
                    .push(*kind, *amount)
                    .ok_or(StyleError::InvalidValue)?;
                style.pending.transform_counts[TRANSFORM_PARTS - 1] += 1;
                if let Some(node) = node {
                    style
                        .animated
                        .push((crate::animation::AnimationProp::Transform(index), *node));
                }
                Ok(())
            }
            StyleDecl::Animated(prop, node) => {
                style.animated.push((*prop, *node));
                Ok(())
            }
            StyleDecl::Default(..) | StyleDecl::CurrentColor(_) => Ok(()),
        }
    }
}

/// A style as written (`style={...}`): its declarations in order (the
/// shorthands of [`StylePatch::set`] before their longhands), kept so the
/// core can apply them again over class styles when those change.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StylePatch {
    decls: Vec<StyleDecl>,
}

impl StylePatch {
    /// Applies `decl` to `style` and records it if it applied. A default
    /// ([`StyleDecl::Default`]) applies unless the patch sets the property.
    pub fn push(&mut self, style: &mut Style, decl: StyleDecl) -> Result<(), StyleError> {
        match &decl {
            StyleDecl::Default(key, value) if !self.sets(*key) => {
                style.apply_value(*key, value)?;
            }
            decl => decl.apply(style)?,
        }
        self.decls.push(decl);
        Ok(())
    }

    /// Sets property `key` of the style object this patch records on
    /// `style`, and records it. As in React Native, a longhand
    /// (`borderTopWidth`, `marginHorizontal`, `flexGrow`, ...) wins over
    /// its shorthand (`borderWidth`, `margin`, `flex`) in one object
    /// whatever their order: a shorthand set after its longhands is
    /// recorded before them, and they apply again; a `null` longhand is
    /// unset, so its shorthand shows.
    pub fn set(
        &mut self,
        style: &mut Style,
        key: &str,
        value: StyleValue<'_>,
    ) -> Result<(), StyleError> {
        let key = StyleKey::parse(key).ok_or(StyleError::UnknownProperty)?;
        if matches!(value, StyleValue::Null) && self.has_shorthand_of(key) {
            return Ok(());
        }
        let parsed = key.parse_value(value)?;
        style.apply_value(key, &parsed)?;
        if key == StyleKey::Display && matches!(value, StyleValue::Str("flex")) {
            // A CSS flex container is a row unless a direction is set
            // (by a class or this style, before or after).
            let row = PropertyValue::FlexDirection(FlexDirection::Row);
            if !self.sets(StyleKey::FlexDirection) {
                let _ = style.apply_value(StyleKey::FlexDirection, &row);
            }
            self.decls
                .push(StyleDecl::Default(StyleKey::FlexDirection, row));
        }
        if matches!(
            key,
            StyleKey::BoxShadow | StyleKey::TextShadow | StyleKey::Filter
        ) {
            // Its layers (`drop-shadow()`s) without a color are the text
            // color's: as a width's.
            let colorless = match key {
                StyleKey::BoxShadow => style.pending.current_shadows != 0,
                StyleKey::Filter => style.pending.current_drop_shadows != 0,
                _ => style.pending.current_text_shadow,
            };
            if colorless {
                self.insert_current_color(key);
            } else {
                self.decls.retain(|d| *d != StyleDecl::CurrentColor(key));
            }
        }
        if let Some(color) = key.current_color()
            && !matches!(value, StyleValue::Null)
        {
            self.insert_current_color(color);
        }
        let decl = StyleDecl::Set(key, parsed);
        let longhands = key.longhands();
        let Some(at) = self
            .decls
            .iter()
            .position(|d| matches!(d, StyleDecl::Set(k, _) if longhands.contains(k)))
        else {
            self.decls.push(decl);
            return Ok(());
        };
        // Nulls set before are dropped (the shorthand shows); the others
        // apply again over it.
        self.decls.retain(
            |d| !matches!(d, StyleDecl::Set(k, PropertyValue::Default) if longhands.contains(k)),
        );
        let at = at.min(self.decls.len());
        self.decls.insert(at, decl);
        for d in &self.decls[at + 1..] {
            if let StyleDecl::Set(k, v) = d
                && longhands.contains(k)
            {
                let _ = style.apply_value(*k, v);
            }
        }
        Ok(())
    }

    /// Whether a shorthand of `key` is recorded.
    fn has_shorthand_of(&self, key: StyleKey) -> bool {
        self.decls
            .iter()
            .any(|d| matches!(d, StyleDecl::Set(k, _) if k.longhands().contains(&key)))
    }

    /// Records that `key` is the node's text color where nothing sets it
    /// ([`StyleDecl::CurrentColor`]), once. These are kept first and in key
    /// order: they apply before the classes in any order, so styles that
    /// set the same keys compare equal.
    fn insert_current_color(&mut self, key: StyleKey) {
        let decl = StyleDecl::CurrentColor(key);
        if self.decls.contains(&decl) {
            return;
        }
        let at = self
            .decls
            .iter()
            .take_while(
                |d| matches!(d, StyleDecl::CurrentColor(k) if (*k as usize) < (key as usize)),
            )
            .count();
        self.decls.insert(at, decl);
    }

    /// Whether a declaration sets `key`.
    fn sets(&self, key: StyleKey) -> bool {
        self.decls
            .iter()
            .any(|d| matches!(d, StyleDecl::Set(k, _) if *k == key))
    }

    /// Applies every declaration to `style`, in order.
    pub fn apply(&self, style: &mut Style) {
        for decl in &self.decls {
            // Declarations were checked when they were recorded.
            let _ = decl.apply(style);
        }
    }

    /// Applies the values the style gives properties that nothing sets
    /// ([`StyleDecl::Default`], [`StyleDecl::CurrentColor`] with `current`,
    /// the node's text color): to the style before its classes, which
    /// [`StylePatch::apply`] then follows.
    pub fn apply_defaults(&self, style: &mut Style, current: Color) {
        for decl in &self.decls {
            match decl {
                StyleDecl::Default(key, value) => {
                    let _ = style.apply_value(*key, value);
                }
                // A shadow's colorless layers take the color once the
                // style is in place ([`Style::apply_current_color`]).
                StyleDecl::CurrentColor(
                    StyleKey::BoxShadow | StyleKey::TextShadow | StyleKey::Filter,
                ) => {}
                StyleDecl::CurrentColor(key) => style.set_color(*key, current),
                _ => {}
            }
        }
    }

    /// Whether a color of the style is the node's text color
    /// ([`StyleDecl::CurrentColor`]): it is styled again when the color
    /// it inherits changes.
    pub fn reads_current_color(&self) -> bool {
        // A side's color is set by its own key or by `borderColor`.
        let set = |k: StyleKey| match k {
            // A colorless shadow layer reads it for as long as it is there.
            StyleKey::BoxShadow | StyleKey::TextShadow | StyleKey::Filter => false,
            StyleKey::OutlineColor | StyleKey::BorderColor => self.sets(k),
            _ => self.sets(k) || self.sets(StyleKey::BorderColor),
        };
        self.decls.iter().any(|d| match d {
            StyleDecl::CurrentColor(k) => !set(*k),
            // `currentColor` written out, or inside a value.
            StyleDecl::Set(_, PropertyValue::CurrentColor | PropertyValue::Deferred(..)) => true,
            _ => false,
        })
    }

    /// Whether there are no declarations.
    pub fn is_empty(&self) -> bool {
        self.decls.is_empty()
    }

    /// The declarations.
    pub fn decls(&self) -> &[StyleDecl] {
        &self.decls
    }
}

impl StyleKey {
    /// The color a width property draws in, CSS's `currentColor` unless
    /// something sets it.
    fn current_color(self) -> Option<StyleKey> {
        use StyleKey as K;
        Some(match self {
            K::BorderWidth => K::BorderColor,
            K::BorderTopWidth => K::BorderTopColor,
            K::BorderRightWidth => K::BorderRightColor,
            K::BorderBottomWidth => K::BorderBottomColor,
            K::BorderLeftWidth => K::BorderLeftColor,
            K::BorderStartWidth => K::BorderStartColor,
            K::BorderEndWidth => K::BorderEndColor,
            K::OutlineWidth => K::OutlineColor,
            _ => return None,
        })
    }

    /// The properties that override this shorthand in a style object
    /// whatever their order (Yoga takes an edge's own value before its
    /// axis' and that of all edges, `flexGrow` before `flex`).
    fn longhands(self) -> &'static [StyleKey] {
        use StyleKey as K;
        match self {
            K::Margin => &[
                K::MarginHorizontal,
                K::MarginVertical,
                K::MarginTop,
                K::MarginRight,
                K::MarginBottom,
                K::MarginLeft,
            ],
            K::MarginHorizontal => &[K::MarginLeft, K::MarginRight],
            K::MarginVertical => &[K::MarginTop, K::MarginBottom],
            K::Padding => &[
                K::PaddingHorizontal,
                K::PaddingVertical,
                K::PaddingTop,
                K::PaddingRight,
                K::PaddingBottom,
                K::PaddingLeft,
            ],
            K::PaddingHorizontal => &[K::PaddingLeft, K::PaddingRight],
            K::PaddingVertical => &[K::PaddingTop, K::PaddingBottom],
            K::BorderWidth => &[
                K::BorderTopWidth,
                K::BorderRightWidth,
                K::BorderBottomWidth,
                K::BorderLeftWidth,
            ],
            K::BorderColor => &[
                K::BorderTopColor,
                K::BorderRightColor,
                K::BorderBottomColor,
                K::BorderLeftColor,
            ],
            K::Gap => &[K::RowGap, K::ColumnGap],
            K::Flex => &[K::FlexGrow, K::FlexShrink, K::FlexBasis],
            K::GridColumn => &[K::GridColumnStart, K::GridColumnEnd],
            K::GridRow => &[K::GridRowStart, K::GridRowEnd],
            _ => &[],
        }
    }
}

/// Why a style property was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleError {
    /// The property name is not supported.
    UnknownProperty,
    /// The value has the wrong type or format.
    InvalidValue,
}

enum Len {
    Px(f32),
    Pct(f32),
    Auto,
}

impl Len {
    fn parse(v: StyleValue<'_>) -> Result<Len, StyleError> {
        match v {
            StyleValue::Number(n) => Ok(Len::Px(n)),
            StyleValue::Str("auto") => Ok(Len::Auto),
            StyleValue::Str(s) => s
                .strip_suffix('%')
                .and_then(|p| p.trim().parse::<f32>().ok())
                .map(|p| Len::Pct(p / 100.0))
                .ok_or(StyleError::InvalidValue),
            StyleValue::Null => Ok(Len::Auto),
        }
    }

    fn dim(self) -> Dimension {
        match self {
            Len::Px(v) => Dimension::length(v),
            Len::Pct(v) => Dimension::percent(v),
            Len::Auto => Dimension::auto(),
        }
    }

    fn lpa(self) -> LengthPercentageAuto {
        match self {
            Len::Px(v) => LengthPercentageAuto::length(v),
            Len::Pct(v) => LengthPercentageAuto::percent(v),
            Len::Auto => LengthPercentageAuto::auto(),
        }
    }

    fn lp(self) -> Result<LengthPercentage, StyleError> {
        match self {
            Len::Px(v) => Ok(LengthPercentage::length(v)),
            Len::Pct(v) => Ok(LengthPercentage::percent(v)),
            Len::Auto => Err(StyleError::InvalidValue),
        }
    }
}

/// Intrinsic sizes are delegated to Taffy's sizing algorithms.
fn dimension(value: StyleValue<'_>) -> Result<Dimension, StyleError> {
    Ok(match value {
        StyleValue::Str("min-content") => Dimension::min_content(),
        StyleValue::Str("max-content") => Dimension::max_content(),
        StyleValue::Str("fit-content") => Dimension::fit_content(),
        value => Len::parse(value)?.dim(),
    })
}

/// A zero-default length: `null` resets to 0 rather than `auto`.
fn lp_or_zero(v: StyleValue<'_>) -> Result<LengthPercentage, StyleError> {
    match v {
        StyleValue::Null => Ok(LengthPercentage::length(0.0)),
        v => Len::parse(v)?.lp(),
    }
}

fn opt_lpa(v: StyleValue<'_>) -> Result<Option<LengthPercentageAuto>, StyleError> {
    match v {
        StyleValue::Null => Ok(None),
        v => Ok(Some(Len::parse(v)?.lpa())),
    }
}

fn opt_lp(v: StyleValue<'_>) -> Result<Option<LengthPercentage>, StyleError> {
    match v {
        StyleValue::Null => Ok(None),
        v => Ok(Some(Len::parse(v)?.lp()?)),
    }
}

/// Whether a value of `key` may write `currentColor` inside it (CSS's
/// properties that take a color, alone or in a gradient, a shadow or a
/// filter; `color` itself takes it alone: the inherited color).
fn defers_current_color(key: StyleKey) -> bool {
    matches!(
        key,
        StyleKey::BackgroundImage
            | StyleKey::MaskImage
            | StyleKey::Filter
            | StyleKey::BackdropFilter
            | StyleKey::BoxShadow
            | StyleKey::TextShadow
    ) || current_bits(key).is_some()
}

/// The color properties `currentColor` fills, by bit of
/// `Pending::current_colors` ([`current_bits`]).
const CURRENT_KEYS: [StyleKey; 10] = [
    StyleKey::BorderTopColor,
    StyleKey::BorderRightColor,
    StyleKey::BorderBottomColor,
    StyleKey::BorderLeftColor,
    StyleKey::BorderStartColor,
    StyleKey::BorderEndColor,
    StyleKey::OutlineColor,
    StyleKey::BackgroundColor,
    StyleKey::TextDecorationColor,
    StyleKey::CaretColor,
];

/// The bits of [`CURRENT_KEYS`] color property `key` sets (`borderColor`:
/// every side's), if it takes `currentColor`.
fn current_bits(key: StyleKey) -> Option<u16> {
    if key == StyleKey::BorderColor {
        return Some(0b1111);
    }
    let bit = CURRENT_KEYS.iter().position(|k| *k == key)?;
    Some(1 << bit)
}

/// A minimum size: unset is 0, as in Yoga, so that flex items shrink
/// without measuring their content (React Native's layout, NSH's
/// default); `auto` is CSS's automatic minimum, which keeps a flex item
/// from shrinking below its content's min-content size.
fn min_len(v: StyleValue<'_>) -> Result<LengthPercentageAuto, StyleError> {
    match v {
        StyleValue::Null => Ok(LengthPercentageAuto::length(0.0)),
        v => Ok(Len::parse(v)?.lpa()),
    }
}

fn lpa_or_zero(v: StyleValue<'_>) -> Result<LengthPercentageAuto, StyleError> {
    match v {
        StyleValue::Null => Ok(LengthPercentageAuto::length(0.0)),
        v => Ok(Len::parse(v)?.lpa()),
    }
}

fn number(v: StyleValue<'_>, default: f32) -> Result<f32, StyleError> {
    match v {
        StyleValue::Number(n) => Ok(n),
        StyleValue::Null => Ok(default),
        StyleValue::Str(_) => Err(StyleError::InvalidValue),
    }
}

fn opt_number(v: StyleValue<'_>) -> Result<Option<f32>, StyleError> {
    match v {
        StyleValue::Number(n) => Ok(Some(n)),
        StyleValue::Null => Ok(None),
        StyleValue::Str(_) => Err(StyleError::InvalidValue),
    }
}

fn opt_points(v: StyleValue<'_>) -> Result<Option<Points>, StyleError> {
    Ok(opt_number(v)?.map(px))
}

fn color(v: StyleValue<'_>) -> Result<Option<Color>, StyleError> {
    match v {
        StyleValue::Str(s) => Color::parse(s).map(Some).ok_or(StyleError::InvalidValue),
        StyleValue::Null => Ok(None),
        StyleValue::Number(_) => Err(StyleError::InvalidValue),
    }
}

/// A `fill` / `stroke` value; `inherit` is unset, since they inherit.
fn svg_paint(v: StyleValue<'_>) -> Result<Option<SvgPaint>, StyleError> {
    let StyleValue::Str(s) = v else {
        return match v {
            StyleValue::Null => Ok(None),
            _ => Err(StyleError::InvalidValue),
        };
    };
    let s = s.trim();
    Ok(if s.eq_ignore_ascii_case("none") {
        Some(SvgPaint::None)
    } else if s.eq_ignore_ascii_case("currentcolor") {
        Some(SvgPaint::CurrentColor)
    } else if s.eq_ignore_ascii_case("inherit") {
        None
    } else {
        Some(SvgPaint::Color(
            Color::parse(s).ok_or(StyleError::InvalidValue)?,
        ))
    })
}

/// A `strokeDasharray` value: lengths (no negatives) separated by commas
/// and / or spaces, or `none` (empty); one whose lengths add up to zero is
/// a solid line, as in SVG.
fn dasharray(v: StyleValue<'_>) -> Result<Option<Arc<[Points]>>, StyleError> {
    let lengths: Vec<Points> = match v {
        StyleValue::Null => return Ok(None),
        StyleValue::Number(n) if n >= 0.0 => vec![px(n)],
        StyleValue::Number(_) => return Err(StyleError::InvalidValue),
        StyleValue::Str(s) if s.trim().eq_ignore_ascii_case("none") => Vec::new(),
        StyleValue::Str(s) => s
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .map(|part| match css::length(part) {
                Some(n) if n >= 0.0 => Ok(px(n)),
                _ => Err(StyleError::InvalidValue),
            })
            .collect::<Result<_, _>>()?,
    };
    if lengths.iter().all(|l| l.0 == 0.0) {
        return Ok(Some(Arc::from([])));
    }
    Ok(Some(lengths.into()))
}

fn keyword<'a>(v: StyleValue<'a>) -> Result<Option<&'a str>, StyleError> {
    match v {
        StyleValue::Str(s) => Ok(Some(s)),
        StyleValue::Null => Ok(None),
        StyleValue::Number(_) => Err(StyleError::InvalidValue),
    }
}

fn align_items(v: StyleValue<'_>) -> Result<Option<AlignItems>, StyleError> {
    Ok(match keyword(v)? {
        None => None,
        Some("flex-start") => Some(AlignItems::FLEX_START),
        Some("flex-end") => Some(AlignItems::FLEX_END),
        // CSS's logical keywords: the writing direction's start and end
        // (right to left, `start` is the right), not the flex direction's.
        Some("start") => Some(AlignItems::START),
        Some("end") => Some(AlignItems::END),
        Some("self-start") => Some(AlignItems::SELF_START),
        Some("self-end") => Some(AlignItems::SELF_END),
        Some("center") => Some(AlignItems::CENTER),
        Some("stretch") => Some(AlignItems::STRETCH),
        Some("baseline") => Some(AlignItems::BASELINE),
        // CSS's `normal`: `stretch` for items without an aspect ratio.
        Some("normal") => Some(AlignItems::NORMAL),
        Some(_) => return Err(StyleError::InvalidValue),
    })
}

/// `alignContent` / `justifyContent`; unset (null, `normal`) is CSS's
/// `normal`, which [`resolve_normal_alignment`] makes React Native's
/// `flex-start` outside grids.
fn align_content(v: StyleValue<'_>) -> Result<AlignContent, StyleError> {
    Ok(match keyword(v)? {
        None | Some("normal") => AlignContent::NORMAL,
        Some("flex-start") => AlignContent::FLEX_START,
        Some("flex-end") => AlignContent::FLEX_END,
        Some("start") => AlignContent::START,
        Some("end") => AlignContent::END,
        Some("center") => AlignContent::CENTER,
        Some("stretch") => AlignContent::STRETCH,
        Some("space-between") => AlignContent::SPACE_BETWEEN,
        Some("space-around") => AlignContent::SPACE_AROUND,
        Some("space-evenly") => AlignContent::SPACE_EVENLY,
        Some(_) => return Err(StyleError::InvalidValue),
    })
}

fn font_weight(v: StyleValue<'_>) -> Result<Option<u16>, StyleError> {
    Ok(match v {
        StyleValue::Null => None,
        StyleValue::Number(n) => Some((n as u16).clamp(100, 900)),
        StyleValue::Str("normal") => Some(400),
        StyleValue::Str("bold") => Some(700),
        StyleValue::Str(s) => Some(
            s.parse::<u16>()
                .map_err(|_| StyleError::InvalidValue)?
                .clamp(100, 900),
        ),
    })
}

/// A length in points from a number or a string (`"2px"`, `"0.5rem"`).
fn length_value(v: StyleValue<'_>, default: f32) -> Result<f32, StyleError> {
    match v {
        StyleValue::Number(n) => Ok(n),
        StyleValue::Str(s) => css::length(s).ok_or(StyleError::InvalidValue),
        StyleValue::Null => Ok(default),
    }
}

/// Milliseconds from a number or `"150ms"` / `"0.2s"`.
fn duration(v: StyleValue<'_>) -> Result<f32, StyleError> {
    match v {
        StyleValue::Number(n) => Ok(n.max(0.0)),
        StyleValue::Null => Ok(0.0),
        StyleValue::Str(s) => {
            let s = s.trim();
            let ms = if let Some(n) = s.strip_suffix("ms") {
                css::number(n)
            } else if let Some(n) = s.strip_suffix('s') {
                css::number(n).map(|n| n * 1000.0)
            } else {
                css::number(s)
            };
            ms.map(|n| n.max(0.0)).ok_or(StyleError::InvalidValue)
        }
    }
}

/// A CSS timing function as cubic Bézier control points.
fn timing_function(v: StyleValue<'_>) -> Result<[f32; 4], StyleError> {
    let Some(s) = keyword(v)? else {
        return Ok(Transition::EASE);
    };
    Ok(match s.trim() {
        "linear" => [0.0, 0.0, 1.0, 1.0],
        "ease" => Transition::EASE,
        "ease-in" => [0.42, 0.0, 1.0, 1.0],
        "ease-out" => [0.0, 0.0, 0.58, 1.0],
        "ease-in-out" => [0.42, 0.0, 0.58, 1.0],
        s => {
            let args = s
                .strip_prefix("cubic-bezier(")
                .and_then(|a| a.strip_suffix(')'))
                .ok_or(StyleError::InvalidValue)?;
            let n: Vec<f32> = args.split(',').filter_map(css::number).collect();
            let [a, b, c, d] = n.as_slice() else {
                return Err(StyleError::InvalidValue);
            };
            [*a, *b, *c, *d]
        }
    })
}

/// `transitionProperty`: CSS property names, mapped to the groups that
/// transition natively.
fn transition_properties(s: &str) -> Option<TransitionProps> {
    let mut bits = 0;
    for name in s.split(',').map(str::trim) {
        bits |= match name {
            "all" => TransitionProps::ALL,
            "none" => 0,
            "color"
            | "colors"
            | "background-color"
            | "backgroundColor"
            | "border-color"
            | "borderColor"
            | "outline-color"
            | "text-decoration-color"
            | "fill"
            | "stroke"
            | "--tw-gradient-from"
            | "--tw-gradient-via"
            | "--tw-gradient-to" => TransitionProps::COLORS,
            "opacity" => TransitionProps::OPACITY,
            "transform" | "translate" | "scale" | "rotate" => TransitionProps::TRANSFORM,
            "box-shadow" | "boxShadow" | "shadow" => TransitionProps::SHADOW,
            "border-radius" | "borderRadius" | "border-width" | "borderWidth" => {
                TransitionProps::SHAPE
            }
            // Properties that do not animate natively.
            "filter" | "backdrop-filter" | "display" | "visibility" | "content-visibility"
            | "overlay" | "pointer-events" => 0,
            _ => return None,
        };
    }
    Some(TransitionProps(bits))
}

fn grid_value<T>(
    v: StyleValue<'_>,
    parse: impl Fn(&str) -> Option<Vec<T>>,
) -> Result<Vec<T>, StyleError> {
    match keyword(v)? {
        None => Ok(Vec::new()),
        Some(s) => parse(s).ok_or(StyleError::InvalidValue),
    }
}

fn grid_template(v: StyleValue<'_>) -> Result<grid::Template, StyleError> {
    match keyword(v)? {
        None => Ok((Vec::new(), Vec::new())),
        Some(s) => grid::template(s).ok_or(StyleError::InvalidValue),
    }
}

fn grid_placement(v: StyleValue<'_>) -> Result<taffy::GridPlacement<String>, StyleError> {
    match v {
        StyleValue::Null => Ok(taffy::GridPlacement::Auto),
        StyleValue::Number(n) => line_index(n),
        StyleValue::Str(s) => grid::placement(s).ok_or(StyleError::InvalidValue),
    }
}

/// CSS's `aspect-ratio`: `16 / 9`, `1.5`, `auto` (none).
fn aspect_ratio(s: &str) -> Result<Option<f32>, StyleError> {
    let s = s.trim();
    if s == "auto" {
        return Ok(None);
    }
    let ratio = match s.split_once('/') {
        Some((w, h)) => css::number(w.trim())
            .zip(css::number(h.trim()))
            .map(|(w, h)| w / h),
        None => css::number(s),
    };
    match ratio {
        Some(r) if r.is_finite() && r > 0.0 => Ok(Some(r)),
        _ => Err(StyleError::InvalidValue),
    }
}

/// A grid line by its number (CSS has no line 0).
fn line_index(n: f32) -> Result<taffy::GridPlacement<String>, StyleError> {
    match n as i16 {
        0 => Err(StyleError::InvalidValue),
        n => Ok(grid::line_number(n)),
    }
}

fn grid_line(v: StyleValue<'_>) -> Result<taffy::Line<taffy::GridPlacement<String>>, StyleError> {
    match v {
        StyleValue::Null => Ok(taffy::Line {
            start: taffy::GridPlacement::Auto,
            end: taffy::GridPlacement::Auto,
        }),
        StyleValue::Number(n) => Ok(grid::line_from(line_index(n)?)),
        StyleValue::Str(s) => grid::line(s).ok_or(StyleError::InvalidValue),
    }
}

impl StyleKey {
    /// Parses `value` for this property: what [`Style::apply_value`] sets.
    /// Only a TSX style's values (and class strings, once) come here.
    pub fn parse_value(self, value: StyleValue<'_>) -> Result<PropertyValue, StyleError> {
        if let StyleValue::Str(s) = value {
            if css::is_current_color(s) && (current_bits(self).is_some() || self == StyleKey::Color)
            {
                return Ok(PropertyValue::CurrentColor);
            }
            if defers_current_color(self) && css::mentions_current_color(s) {
                // Checked now with a stand-in color, parsed again with the
                // node's once the style is in place.
                let probe = css::with_current_color(s, Color::BLACK);
                let probe = self.parse_inner(StyleValue::Str(&probe))?;
                return Ok(PropertyValue::Deferred(s.into(), Box::new(probe)));
            }
        }
        match value {
            StyleValue::Null => Ok(PropertyValue::Default),
            value => self.parse_inner(value),
        }
    }

    /// `n` as this property's value, as a number in a TSX style is
    /// (points for lengths, a multiple for `lineHeight`).
    pub fn number_value(self, n: f32) -> Result<PropertyValue, StyleError> {
        self.parse_inner(StyleValue::Number(n))
    }

    /// `length` as this property's value: points, a fraction of the
    /// containing block or `auto`, where the property takes them.
    pub fn length_value(
        self,
        length: crate::geometry::Length,
    ) -> Result<PropertyValue, StyleError> {
        use StyleKey as K;

        use crate::geometry::Length;
        let len = match length {
            Length::Definite(DefiniteLength::Absolute(a)) => Len::Px(a.to_points(REM_SIZE).0),
            Length::Definite(DefiniteLength::Fraction(f)) => Len::Pct(f.0),
            Length::Auto => Len::Auto,
        };
        Ok(match (self, len) {
            (K::FlexBasis | K::Width | K::Height, len) => PropertyValue::Dimension(len.dim()),
            (
                K::MinWidth
                | K::MinHeight
                | K::MaxWidth
                | K::MaxHeight
                | K::Top
                | K::Right
                | K::Bottom
                | K::Left
                | K::Margin
                | K::MarginHorizontal
                | K::MarginVertical
                | K::MarginTop
                | K::MarginRight
                | K::MarginBottom
                | K::MarginLeft,
                len,
            ) => PropertyValue::Lpa(len.lpa()),
            (
                K::Padding
                | K::PaddingHorizontal
                | K::PaddingVertical
                | K::PaddingTop
                | K::PaddingRight
                | K::PaddingBottom
                | K::PaddingLeft
                | K::Gap
                | K::RowGap
                | K::ColumnGap,
                len,
            ) => PropertyValue::Lp(len.lp()?),
            (K::MarginStart | K::MarginEnd | K::Start | K::End, len) => {
                PropertyValue::OptLpa(Some(len.lpa()))
            }
            (K::PaddingStart | K::PaddingEnd, len) => PropertyValue::OptLp(Some(len.lp()?)),
            (K::LineHeight, Len::Px(n)) => PropertyValue::LineHeight(LineHeight::Points(px(n))),
            (K::LineHeight, Len::Pct(f)) => PropertyValue::LineHeight(LineHeight::Em(f)),
            (key, Len::Px(n)) => key.number_value(n)?,
            _ => return Err(StyleError::InvalidValue),
        })
    }

    /// The value `null` gives the property.
    fn default_value(self) -> PropertyValue {
        self.parse_inner(StyleValue::Null)
            .expect("every property takes null")
    }

    fn parse_inner(self, value: StyleValue<'_>) -> Result<PropertyValue, StyleError> {
        use StyleKey as K;
        let of = OrderedFloat;
        Ok(match self {
            // Layout
            K::Display => PropertyValue::Display(match keyword(value)? {
                None | Some("flex") => Display::Flex,
                Some("grid") => Display::Grid,
                Some("block") => Display::Block,
                Some("flow-root") => Display::FlowRoot,
                Some("none") => Display::None,
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::BoxSizing => PropertyValue::BoxSizing(match keyword(value)? {
                None | Some("border-box") => taffy::BoxSizing::BorderBox,
                Some("content-box") => taffy::BoxSizing::ContentBox,
                _ => return Err(StyleError::InvalidValue),
            }),
            K::Position => {
                let (position, fixed, sticky) = match keyword(value)? {
                    None | Some("relative") => (Position::Relative, false, false),
                    Some("absolute") => (Position::Absolute, false, false),
                    Some("fixed") => (Position::Fixed, true, false),
                    Some("static") => (Position::Static, false, false),
                    Some("sticky") => (Position::Relative, false, true),
                    Some(_) => return Err(StyleError::InvalidValue),
                };
                PropertyValue::Position(position, fixed, sticky)
            }
            K::FlexDirection => PropertyValue::FlexDirection(match keyword(value)? {
                None | Some("column") => FlexDirection::Column,
                Some("row") => FlexDirection::Row,
                Some("column-reverse") => FlexDirection::ColumnReverse,
                Some("row-reverse") => FlexDirection::RowReverse,
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::FlexWrap => PropertyValue::FlexWrap(match keyword(value)? {
                None | Some("nowrap") => FlexWrap::NoWrap,
                Some("wrap") => FlexWrap::Wrap,
                Some("wrap-reverse") => FlexWrap::WrapReverse,
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::Flex => {
                // CSS's shorthand. Unset: the defaults (React Native's
                // `flexShrink: 0`).
                let (grow, shrink, basis) = match value {
                    StyleValue::Null => (0.0, 0.0, Dimension::auto()),
                    StyleValue::Number(n) if n >= 0.0 => (n, 1.0, Dimension::length(0.0)),
                    StyleValue::Number(_) => return Err(StyleError::InvalidValue),
                    StyleValue::Str(s) => {
                        let (grow, shrink, basis) = css::flex(s).ok_or(StyleError::InvalidValue)?;
                        let basis = match basis.map(|b| (b, css::length_or_percent(b))) {
                            None => Dimension::length(0.0),
                            // A CSS length (`30px`, `2rem`) or a percentage.
                            Some((_, Some(DefiniteLength::Absolute(a)))) => {
                                Dimension::length(a.to_points(crate::geometry::REM_SIZE).0)
                            }
                            Some((_, Some(DefiniteLength::Fraction(f)))) => Dimension::percent(f.0),
                            // `auto`, `min-content`, ...
                            Some((b, None)) => dimension(StyleValue::Str(b))?,
                        };
                        (grow, shrink, basis)
                    }
                };
                PropertyValue::Flex(of(grow), of(shrink), basis)
            }
            K::FlexGrow | K::FlexShrink | K::ZIndex => {
                PropertyValue::Number(of(number(value, 0.0)?))
            }
            K::Opacity => PropertyValue::Number(of(number(value, 1.0)?.clamp(0.0, 1.0))),
            K::FlexBasis | K::Width | K::Height => PropertyValue::Dimension(dimension(value)?),
            K::JustifyContent | K::AlignContent => {
                PropertyValue::AlignContent(align_content(value)?)
            }
            K::AlignItems | K::JustifyItems => PropertyValue::AlignItems(align_items(value)?),
            K::AlignSelf | K::JustifySelf => PropertyValue::AlignItems(match value {
                StyleValue::Str("auto") => None,
                v => align_items(v)?,
            }),
            K::MinWidth | K::MinHeight => PropertyValue::Lpa(min_len(value)?),
            K::MaxWidth | K::MaxHeight | K::Top | K::Right | K::Bottom | K::Left => {
                PropertyValue::Lpa(Len::parse(value)?.lpa())
            }
            K::AspectRatio => PropertyValue::OptNumber(match value {
                StyleValue::Str(s) => aspect_ratio(s)?.map(of),
                v => opt_number(v)?.map(of),
            }),
            K::Margin
            | K::MarginHorizontal
            | K::MarginVertical
            | K::MarginTop
            | K::MarginRight
            | K::MarginBottom
            | K::MarginLeft => match margin_edge(value)? {
                (_, Some(var)) => PropertyValue::EnvLength(env_points(value), var),
                (v, None) => PropertyValue::Lpa(v),
            },
            K::Padding
            | K::PaddingHorizontal
            | K::PaddingVertical
            | K::PaddingTop
            | K::PaddingRight
            | K::PaddingBottom
            | K::PaddingLeft => match padding_edge(value)? {
                (_, Some(var)) => PropertyValue::EnvLength(env_points(value), var),
                (v, None) => PropertyValue::Lp(v),
            },
            K::Gap | K::RowGap | K::ColumnGap => PropertyValue::Lp(lp_or_zero(value)?),
            K::BorderWidth
            | K::BorderTopWidth
            | K::BorderRightWidth
            | K::BorderBottomWidth
            | K::BorderLeftWidth => PropertyValue::Points(px(number(value, 0.0)?.max(0.0))),
            K::VerticalAlign => PropertyValue::VerticalAlign(match keyword(value)? {
                None => VerticalAlign::Baseline,
                Some(k) => VerticalAlign::parse(k).ok_or(StyleError::InvalidValue)?,
            }),
            K::Direction => PropertyValue::OptBool(match keyword(value)? {
                None | Some("inherit") => None,
                Some("ltr") => Some(false),
                Some("rtl") => Some(true),
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::MarginStart | K::MarginEnd | K::Start | K::End => {
                PropertyValue::OptLpa(opt_lpa(value)?)
            }
            K::PaddingStart | K::PaddingEnd => PropertyValue::OptLp(opt_lp(value)?),
            K::BorderStartWidth | K::BorderEndWidth => {
                PropertyValue::OptPoints(opt_points(value)?.map(|w| w.max(Points::ZERO)))
            }
            K::BorderStartColor
            | K::BorderEndColor
            | K::BackgroundColor
            | K::BorderColor
            | K::BorderTopColor
            | K::BorderRightColor
            | K::BorderBottomColor
            | K::BorderLeftColor
            | K::TintColor
            | K::OutlineColor
            | K::Color
            | K::TextDecorationColor
            | K::PlaceholderColor
            | K::SelectionColor
            | K::CaretColor => PropertyValue::Color(color(value)?),
            K::BorderTopStartRadius
            | K::BorderTopEndRadius
            | K::BorderBottomEndRadius
            | K::BorderBottomStartRadius
            | K::BorderTopLeftRadius
            | K::BorderTopRightRadius
            | K::BorderBottomRightRadius
            | K::BorderBottomLeftRadius
            | K::FontSize => PropertyValue::OptPoints(opt_points(value)?),
            K::BorderRadius => PropertyValue::Points(px(number(value, 0.0)?)),
            K::BorderStyle | K::OutlineStyle => {
                // `none` (and `hidden`) draws and lays out no line whatever
                // the width, set before or after it (CSS).
                let (style, hidden) = match keyword(value)? {
                    None | Some("solid") => (BorderStyle::Solid, false),
                    Some("dashed") => (BorderStyle::Dashed, false),
                    Some("dotted") => (BorderStyle::Dotted, false),
                    Some("none") | Some("hidden") => (BorderStyle::Solid, true),
                    Some(_) => return Err(StyleError::InvalidValue),
                };
                PropertyValue::BorderStyle(style, hidden)
            }
            K::BorderCurve => PropertyValue::Bool(match keyword(value)? {
                None | Some("circular") => false,
                Some("continuous") => true,
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::BackfaceVisibility | K::Overflow => PropertyValue::Bool(match keyword(value)? {
                None | Some("visible") => false,
                Some("hidden") => true,
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::Visibility => PropertyValue::Bool(match keyword(value)? {
                None | Some("visible") => false,
                Some("hidden") | Some("collapse") => true,
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::ObjectFit => PropertyValue::ObjectFit(match keyword(value)? {
                None => None,
                Some("fill") => Some(crate::ResizeMode::Stretch),
                Some("contain") => Some(crate::ResizeMode::Contain),
                Some("cover") => Some(crate::ResizeMode::Cover),
                Some("none") => Some(crate::ResizeMode::Center),
                Some("scale-down") => Some(crate::ResizeMode::ScaleDown),
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::ObjectPosition => PropertyValue::ObjectPosition(match keyword(value)? {
                None => ObjectPosition::CENTER,
                Some(s) => ObjectPosition::parse(s).ok_or(StyleError::InvalidValue)?,
            }),
            K::Transform => transform_value(value, crate::transform::parse_css, None)?,
            K::Translate => transform_value(
                value,
                crate::transform::parse_translate,
                Some(TransformKind::TranslateX),
            )?,
            K::Rotate => transform_value(
                value,
                crate::transform::parse_rotate,
                Some(TransformKind::Rotate),
            )?,
            K::Scale => transform_value(
                value,
                crate::transform::parse_scale,
                Some(TransformKind::Scale),
            )?,
            K::TransformOrigin => PropertyValue::TransformOrigin(match keyword(value)? {
                None => TransformOrigin::CENTER,
                Some(s) => TransformOrigin::parse(s).ok_or(StyleError::InvalidValue)?,
            }),

            // Text
            K::Fill | K::Stroke => PropertyValue::Paint(svg_paint(value)?),
            K::StrokeWidth => PropertyValue::OptPoints(match value {
                StyleValue::Null => None,
                StyleValue::Number(n) if n >= 0.0 => Some(px(n)),
                StyleValue::Str(s) => match css::length(s) {
                    Some(n) if n >= 0.0 => Some(px(n)),
                    _ => return Err(StyleError::InvalidValue),
                },
                StyleValue::Number(_) => return Err(StyleError::InvalidValue),
            }),
            K::StrokeDasharray => PropertyValue::Dashes(dasharray(value)?),
            K::FontWeight => PropertyValue::FontWeight(font_weight(value)?),
            K::LineHeight => PropertyValue::LineHeight(line_height(value)?),
            K::FontFamily => {
                PropertyValue::FontFamily(keyword(value)?.map(|name| FontFamily::new(name.trim())))
            }
            K::FontStyle => PropertyValue::OptBool(match keyword(value)? {
                None => None,
                Some("normal") => Some(false),
                Some("italic") => Some(true),
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::LetterSpacing => {
                let (points, em) = match value {
                    StyleValue::Str(s) if s.ends_with("em") && !s.ends_with("rem") => {
                        let n = css::number(&s[..s.len() - 2]).ok_or(StyleError::InvalidValue)?;
                        (None, Some(of(n)))
                    }
                    StyleValue::Str(s) => (
                        Some(px(css::length(s).ok_or(StyleError::InvalidValue)?)),
                        None,
                    ),
                    v => (opt_points(v)?, None),
                };
                PropertyValue::LetterSpacing(points, em)
            }
            K::TextDecorationLine => PropertyValue::TextDecoration(match keyword(value)? {
                None => None,
                Some(words) => {
                    let mut d = TextDecoration::default();
                    for word in words.split_whitespace() {
                        match word {
                            "none" => {}
                            "underline" => d.underline = true,
                            "line-through" => d.line_through = true,
                            _ => return Err(StyleError::InvalidValue),
                        }
                    }
                    Some(d)
                }
            }),
            K::WritingDirection => PropertyValue::OptBool(match keyword(value)? {
                None | Some("auto") => None,
                Some("ltr") => Some(false),
                Some("rtl") => Some(true),
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::FontVariant => PropertyValue::FontVariant(match keyword(value)? {
                None => None,
                Some(s) => Some(FontVariant::parse(s).ok_or(StyleError::InvalidValue)?),
            }),
            K::TextShadow => {
                let layers = match keyword(value)? {
                    None => Vec::new(),
                    Some(s) => crate::css::text_shadows(s).ok_or(StyleError::InvalidValue)?,
                };
                // One text shadow natively: the widest layer stands for all,
                // as the classes' does.
                PropertyValue::TextShadow(
                    layers
                        .into_iter()
                        .max_by(|a, b| a.0.radius.0.total_cmp(&b.0.radius.0)),
                )
            }
            K::NumberOfLines => PropertyValue::NumberOfLines(match value {
                StyleValue::Null => None,
                StyleValue::Number(n) if n >= 0.0 => Some(n as u32),
                _ => return Err(StyleError::InvalidValue),
            }),
            K::TextOverflow => PropertyValue::TextOverflow(match keyword(value)? {
                None => None,
                Some("ellipsis") => Some(EllipsizeMode::Tail),
                Some("clip") => Some(EllipsizeMode::Clip),
                Some("ellipsis-start") => Some(EllipsizeMode::Head),
                Some("ellipsis-middle") => Some(EllipsizeMode::Middle),
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::UserSelect => PropertyValue::OptBool(match keyword(value)? {
                None | Some("auto") => None,
                Some("none") => Some(false),
                Some("text") => Some(true),
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::PointerEvents => PropertyValue::PointerEvents(match keyword(value)? {
                None | Some("auto") => PointerEvents::Auto,
                Some("none") => PointerEvents::None,
                Some("box-none") => PointerEvents::BoxNone,
                Some("box-only") => PointerEvents::BoxOnly,
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::TextAlignVertical => PropertyValue::AlignVertical(match keyword(value)? {
                None => None,
                Some("auto" | "top") => Some(percentage(0.0)),
                Some("center") => Some(percentage(0.5)),
                Some("bottom") => Some(percentage(1.0)),
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::TextTransform => PropertyValue::TextTransform(match keyword(value)? {
                None => None,
                Some("none") => Some(TextTransform::None),
                Some("uppercase") => Some(TextTransform::Uppercase),
                Some("lowercase") => Some(TextTransform::Lowercase),
                Some("capitalize") => Some(TextTransform::Capitalize),
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::TextAlign => {
                // `left` and `right` keep their sides right to left, as in
                // CSS; `start` and `end` are left and right there.
                let (align, physical) = match keyword(value)? {
                    None => (None, false),
                    Some("auto") => (Some(TextAlign::Auto), false),
                    Some("left") => (Some(TextAlign::Left), true),
                    Some("center") => (Some(TextAlign::Center), false),
                    Some("right") => (Some(TextAlign::Right), true),
                    Some("justify") => (Some(TextAlign::Justify), false),
                    Some("start") => (Some(TextAlign::Left), false),
                    Some("end") => (Some(TextAlign::Right), false),
                    Some(_) => return Err(StyleError::InvalidValue),
                };
                PropertyValue::TextAlign(align, physical)
            }

            // Effects
            K::BoxShadow => PropertyValue::Shadows(match keyword(value)? {
                None => Rc::from([]),
                Some(s) => css::shadow_layers(s)
                    .ok_or(StyleError::InvalidValue)?
                    .into(),
            }),
            K::BackgroundImage => PropertyValue::Gradients(match keyword(value)? {
                None => Rc::from([]),
                Some(s) => css::gradients(s).ok_or(StyleError::InvalidValue)?.into(),
            }),
            K::MaskImage => PropertyValue::Gradients(match keyword(value)? {
                None | Some("none") => Rc::from([]),
                Some(s) => css::gradients(s).ok_or(StyleError::InvalidValue)?.into(),
            }),
            K::Filter => PropertyValue::Filters(match keyword(value)? {
                None => Rc::from([]),
                Some(s) => css::filter_list(s).ok_or(StyleError::InvalidValue)?.into(),
            }),
            K::BackdropFilter => PropertyValue::Filters(match keyword(value)? {
                None => Rc::from([]),
                Some(s) => css::filters(s)
                    .ok_or(StyleError::InvalidValue)?
                    .into_iter()
                    .map(|f| (f, true))
                    .collect(),
            }),
            K::GlassEffect => PropertyValue::GlassEffect(match keyword(value)? {
                None => GlassEffect::None,
                Some(s) => GlassEffect::parse(s).ok_or(StyleError::InvalidValue)?,
            }),
            K::MixBlendMode => PropertyValue::BlendMode(match keyword(value)? {
                None => BlendMode::Normal,
                Some(s) => BlendMode::parse(s).ok_or(StyleError::InvalidValue)?,
            }),
            K::OutlineWidth => PropertyValue::Points(px(length_value(value, 0.0)?.max(0.0))),
            K::OutlineOffset => PropertyValue::Points(px(length_value(value, 0.0)?)),
            K::Cursor => PropertyValue::Cursor(match keyword(value)? {
                None => Cursor::Auto,
                Some(s) => Cursor::parse(s).ok_or(StyleError::InvalidValue)?,
            }),
            K::TransitionProperty => PropertyValue::TransitionProps(match keyword(value)? {
                None => TransitionProps(TransitionProps::ALL),
                Some(s) => transition_properties(s).ok_or(StyleError::InvalidValue)?,
            }),
            K::TransitionDuration
            | K::TransitionDelay
            | K::AnimationDuration
            | K::AnimationDelay => PropertyValue::Ms(of(duration(value)?)),
            K::TransitionTimingFunction | K::AnimationTimingFunction => {
                PropertyValue::Easing(timing_function(value)?.map(of))
            }
            K::AnimationName => PropertyValue::Keyframes(match keyword(value)? {
                None | Some("none") => None,
                Some("spin") => Some(Keyframes::Spin),
                Some("ping") => Some(Keyframes::Ping),
                Some("pulse") => Some(Keyframes::Pulse),
                Some("bounce") => Some(Keyframes::Bounce),
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::AnimationIterationCount => PropertyValue::Iterations(match value {
                StyleValue::Str("infinite") | StyleValue::Null => None,
                v => Some(number(v, 1.0)?.max(0.0) as u32),
            }),
            K::TextDecorationStyle => PropertyValue::DecorationStyle(match keyword(value)? {
                None => None,
                Some("solid") => Some(DecorationStyle::Solid),
                Some("double") => Some(DecorationStyle::Double),
                Some("dotted") => Some(DecorationStyle::Dotted),
                Some("dashed") => Some(DecorationStyle::Dashed),
                Some("wavy") => Some(DecorationStyle::Wavy),
                Some(_) => return Err(StyleError::InvalidValue),
            }),
            K::TextDecorationThickness => PropertyValue::OptPoints(match value {
                StyleValue::Null | StyleValue::Str("auto") | StyleValue::Str("from-font") => None,
                v => Some(px(length_value(v, 1.0)?.max(0.0))),
            }),
            K::TextUnderlineOffset => PropertyValue::OptPoints(match value {
                StyleValue::Null | StyleValue::Str("auto") => None,
                v => Some(px(length_value(v, 0.0)?)),
            }),

            // Grid
            K::GridTemplateColumns | K::GridTemplateRows => {
                PropertyValue::GridTemplate(GridTemplateValue(Rc::new(grid_template(value)?)))
            }
            K::GridAutoColumns | K::GridAutoRows => {
                PropertyValue::GridTracks(grid_value(value, grid::auto_tracks)?.into())
            }
            K::GridAutoFlow => PropertyValue::GridAutoFlow(match keyword(value)? {
                None => taffy::GridAutoFlow::Row,
                Some(s) => grid::auto_flow(s).ok_or(StyleError::InvalidValue)?,
            }),
            K::GridColumn | K::GridRow => PropertyValue::GridLine(grid_line(value)?),
            K::GridColumnStart | K::GridColumnEnd | K::GridRowStart | K::GridRowEnd => {
                PropertyValue::GridPlacement(grid_placement(value)?)
            }
        })
    }
}

/// A transform property's value: CSS's syntax as `parse` reads it, a
/// number as one `number` step, `null` as none.
fn transform_value(
    value: StyleValue<'_>,
    parse: fn(&str) -> Option<Vec<crate::TransformOp>>,
    number: Option<TransformKind>,
) -> Result<PropertyValue, StyleError> {
    let ops = match value {
        StyleValue::Null => Vec::new(),
        StyleValue::Number(n) => {
            let kind = number.ok_or(StyleError::InvalidValue)?;
            vec![crate::TransformOp { kind, value: n }]
        }
        StyleValue::Str(s) => parse(s).ok_or(StyleError::InvalidValue)?,
    };
    Ok(PropertyValue::Transform(
        ops.iter().map(|op| (op.kind, op.value, None)).collect(),
    ))
}

/// `lineHeight`: a number is points (React Native); strings, which
/// classes write, are a multiple of the font size (`"1.5"`), a multiple
/// of the element's own font size (`"1.5em"`, `"150%"`), points
/// (`"20px"`) or the font's own (`"normal"`).
fn line_height(value: StyleValue<'_>) -> Result<LineHeight, StyleError> {
    Ok(match value {
        StyleValue::Null | StyleValue::Str("normal") => LineHeight::Normal,
        StyleValue::Number(n) => LineHeight::Points(px(n)),
        StyleValue::Str(s) if s.ends_with("px") || s.ends_with("pt") || s.ends_with("rem") => {
            LineHeight::Points(px(css::length(s).ok_or(StyleError::InvalidValue)?))
        }
        StyleValue::Str(s) if s.ends_with('%') || s.ends_with("em") => {
            let n = if let Some(n) = s.strip_suffix('%') {
                css::number(n).map(|n| n / 100.0)
            } else {
                css::number(s.strip_suffix("em").unwrap_or(s))
            }
            .filter(|n| *n >= 0.0)
            .ok_or(StyleError::InvalidValue)?;
            LineHeight::Em(n)
        }
        StyleValue::Str(s) => LineHeight::Multiple(
            css::number(s)
                .filter(|n| *n >= 0.0)
                .ok_or(StyleError::InvalidValue)?,
        ),
    })
}

impl Style {
    /// Gives the shadow layers a style wrote without a color the node's
    /// text color, `current` (CSS's `currentColor`).
    pub(crate) fn apply_current_color(&mut self, current: Color) {
        if !self.pending.current_values.is_empty() {
            // Taken out while they apply: setting a key drops its entry.
            let values = std::mem::take(&mut self.pending.current_values);
            for (key, value) in &values {
                let value = css::with_current_color(value, current);
                let _ = self.set_key(*key, StyleValue::Str(&value));
            }
            self.pending.current_values = values;
        }
        let mask = self.pending.current_drop_shadows;
        if mask != 0 {
            self.edit_effects(|e| {
                for (i, filter) in e.filters.iter_mut().enumerate().take(64) {
                    if mask & 1 << i != 0
                        && let Filter::DropShadow { color, .. } = filter
                    {
                        *color = current;
                    }
                }
            });
        }
        let mask = self.pending.current_shadows;
        if mask != 0 {
            self.edit_effects(|e| {
                for (i, layer) in e.box_shadows.iter_mut().enumerate().take(64) {
                    if mask & 1 << i != 0 {
                        layer.color = current;
                    }
                }
            });
        }
        if self.pending.current_text_shadow {
            self.text.shadow_color = Some(current);
        }
        let colors = self.pending.current_colors;
        for (bit, key) in CURRENT_KEYS.iter().enumerate() {
            if colors & 1 << bit != 0 {
                self.put_color(*key, current);
            }
        }
    }

    /// What the style says of the node's own props: `numberOfLines`,
    /// `textOverflow` and `userSelect`, into `props`.
    pub(crate) fn node_props(&self, props: &mut crate::tailwind::ClassProps) {
        let p = &self.pending;
        props.number_of_lines = p.number_of_lines;
        props.ellipsize = p.text_overflow;
        props.selectable = p.user_select;
    }

    /// `objectFit` (the `object-*` classes set it too): how an image or a
    /// video fits its box; `None` leaves the element's default.
    pub fn object_fit(&self) -> Option<crate::ResizeMode> {
        self.pending.object_fit
    }

    /// Sets transform property `property` to `steps` in its place in the
    /// transform list, the other properties' steps around it; a step with
    /// an animated value is bound to it.
    pub fn set_transform(
        &mut self,
        property: TransformProperty,
        steps: &[(TransformKind, f32, Option<crate::animation::NodeId>)],
    ) -> Result<(), StyleError> {
        use crate::animation::AnimationProp;
        let part = property as usize;
        let counts = self.pending.transform_counts;
        let start: usize = counts[..part].iter().map(|&c| usize::from(c)).sum();
        let old = usize::from(counts[part]);
        let ops = self.view.transform.ops();
        if ops.len() - old + steps.len() > crate::MAX_TRANSFORMS {
            return Err(StyleError::InvalidValue);
        }
        let mut list = Transform::IDENTITY;
        let kept = ops[..start].iter().map(|op| (op.kind, op.value));
        let after = ops[start + old..].iter().map(|op| (op.kind, op.value));
        for (kind, value) in kept
            .chain(steps.iter().map(|&(kind, value, _)| (kind, value)))
            .chain(after)
        {
            let _ = list.push(kind, value);
        }
        self.view.transform = list;
        // The bindings of the steps after it move with them.
        let moved = steps.len() as isize - old as isize;
        self.animated.retain_mut(|(prop, _)| match prop {
            AnimationProp::Transform(i) if usize::from(*i) >= start + old => {
                *i = (*i as isize + moved) as u8;
                true
            }
            AnimationProp::Transform(i) => usize::from(*i) < start,
            _ => true,
        });
        for (k, &(_, _, node)) in steps.iter().enumerate() {
            if let Some(node) = node {
                self.animated
                    .push((AnimationProp::Transform((start + k) as u8), node));
            }
        }
        self.pending.transform_counts[part] = steps.len() as u8;
        Ok(())
    }

    /// Changes the effects lists (copying them if shared).
    pub fn edit_effects(&mut self, f: impl FnOnce(&mut Effects)) {
        let mut effects = self
            .view
            .effects
            .take()
            .map(|e| (*e).clone())
            .unwrap_or_default();
        f(&mut effects);
        self.view.effects = crate::effects::shared(effects);
    }

    /// Sets the box shadows (top layer first).
    pub fn set_box_shadows(&mut self, shadows: Vec<BoxShadow>) {
        self.edit_effects(|e| e.box_shadows = shadows);
    }

    /// Sets the gradient backgrounds (top layer first).
    pub fn set_background_images(&mut self, images: Vec<Gradient>) {
        self.edit_effects(|e| e.background_images = images);
    }

    /// Sets the filters.
    pub fn set_filters(&mut self, filters: Vec<Filter>, backdrop: bool) {
        self.edit_effects(|e| {
            if backdrop {
                e.backdrop_filters = filters;
            } else {
                e.filters = filters;
            }
        });
    }

    fn transition_mut(&mut self) -> &mut Transition {
        self.view.transition.get_or_insert(Transition {
            properties: TransitionProps(TransitionProps::ALL),
            duration: 0.0,
            delay: 0.0,
            easing: Transition::EASE,
        })
    }

    fn set_border_width(&mut self, sides: &[usize], w: Points) {
        for &side in sides {
            self.pending.border_widths[side] = w;
            if !self.pending.border_hidden {
                self.set_used_border_width(side, w);
            }
        }
    }

    /// The width side `side` lays out and draws with.
    fn set_used_border_width(&mut self, side: usize, w: Points) {
        *self.view.borders.width_mut(side) = w;
        let v = LengthPercentage::length(w.0);
        let b = &mut self.layout.border;
        match side {
            0 => b.top = v,
            1 => b.right = v,
            2 => b.bottom = v,
            _ => b.left = v,
        }
    }

    fn set_logical_border_width(&mut self, i: usize, w: Option<Points>) {
        self.pending.border_logical[i] = w;
        if !self.pending.border_hidden {
            self.logical.border_width[i] = w;
        }
    }

    fn set_border_color(&mut self, sides: &[usize], c: Option<Color>) {
        let c = c.unwrap_or(Color::BLACK);
        for &side in sides {
            self.view.borders.colors[side] = c;
        }
    }

    /// Sets color property `key` (`borderColor`, `outlineColor`) to `color`;
    /// other keys are left as they are.
    pub(crate) fn set_color(&mut self, key: StyleKey, color: Color) {
        if let Some(bits) = current_bits(key) {
            self.pending.current_colors &= !bits;
        }
        self.put_color(key, color);
    }

    /// [`Style::set_color`], leaving what `currentColor` fills alone.
    fn put_color(&mut self, key: StyleKey, color: Color) {
        match key {
            StyleKey::BorderColor => self.view.borders.colors = [color; 4],
            StyleKey::OutlineColor => self.view.outline.color = color,
            StyleKey::BackgroundColor => self.view.background_color = color,
            StyleKey::BorderTopColor => self.view.borders.colors[0] = color,
            StyleKey::BorderRightColor => self.view.borders.colors[1] = color,
            StyleKey::BorderBottomColor => self.view.borders.colors[2] = color,
            StyleKey::BorderLeftColor => self.view.borders.colors[3] = color,
            StyleKey::BorderStartColor => self.logical.border_color[0] = Some(color),
            StyleKey::BorderEndColor => self.logical.border_color[1] = Some(color),
            StyleKey::TextDecorationColor => self.text.decoration_color = Some(color),
            StyleKey::CaretColor => self.text.caret_color = Some(color),
            _ => debug_assert!(false, "set_color({key:?})"),
        }
    }

    /// Sets one property using its React Native name.
    pub fn set(&mut self, key: &str, value: StyleValue<'_>) -> Result<(), StyleError> {
        self.set_key(
            StyleKey::parse(key).ok_or(StyleError::UnknownProperty)?,
            value,
        )
    }

    /// Sets one property.
    pub fn set_key(&mut self, key: StyleKey, value: StyleValue<'_>) -> Result<(), StyleError> {
        let value = key.parse_value(value)?;
        self.apply_value(key, &value)
    }

    /// Sets property `key` to `value`, parsed before ([`StyleKey::parse_value`])
    /// or made typed (classes, Rust's styles). A value of another property's
    /// kind is rejected.
    pub fn apply_value(&mut self, key: StyleKey, value: &PropertyValue) -> Result<(), StyleError> {
        use PropertyValue as V;
        use StyleKey as K;
        if !self.pending.current_values.is_empty() {
            self.pending.current_values.retain(|(k, _)| *k != key);
        }
        match value {
            V::Default => return self.apply_value(key, &key.default_value()),
            V::Deferred(text, probe) => {
                self.apply_value(key, probe)?;
                self.pending.current_values.push((key, text.clone()));
                return Ok(());
            }
            _ => {}
        }
        let current = matches!(value, V::CurrentColor);
        if let Some(bits) = current_bits(key) {
            // The last value wins: `currentColor` is filled in once the
            // style is in place, another value takes its place.
            if current {
                self.pending.current_colors |= bits;
                return Ok(());
            }
            self.pending.current_colors &= !bits;
        } else if current {
            if key != K::Color {
                return Err(StyleError::InvalidValue);
            }
            // CSS: `color: currentColor` is the inherited color.
            self.text.color = None;
            return Ok(());
        }
        let l = &mut self.layout;
        match (key, value) {
            // Layout
            (K::Display, V::Display(v)) => l.display = *v,
            (K::BoxSizing, V::BoxSizing(v)) => l.box_sizing = *v,
            (K::Position, V::Position(position, fixed, sticky)) => {
                l.position = *position;
                self.pending.fixed_position = *fixed;
                self.pending.sticky_position = *sticky;
            }
            (K::FlexDirection, V::FlexDirection(v)) => l.flex_direction = *v,
            (K::FlexWrap, V::FlexWrap(v)) => l.flex_wrap = *v,
            (K::Flex, V::Flex(grow, shrink, basis)) => {
                (l.flex_grow, l.flex_shrink, l.flex_basis) = (grow.0, shrink.0, *basis);
            }
            (K::FlexGrow, V::Number(n)) => l.flex_grow = n.0,
            (K::FlexShrink, V::Number(n)) => l.flex_shrink = n.0,
            (K::FlexBasis, V::Dimension(v)) => l.flex_basis = *v,
            (K::JustifyContent, V::AlignContent(v)) => l.justify_content = *v,
            (K::AlignItems, V::AlignItems(v)) => l.align_items = v.unwrap_or(AlignItems::STRETCH),
            (K::AlignSelf, V::AlignItems(v)) => l.align_self = *v,
            (K::AlignContent, V::AlignContent(v)) => l.align_content = *v,
            (K::Width, V::Dimension(v)) => l.size.width = *v,
            (K::Height, V::Dimension(v)) => l.size.height = *v,
            (K::MinWidth, V::Lpa(v)) => l.min_size.width = *v,
            (K::MinHeight, V::Lpa(v)) => l.min_size.height = *v,
            (K::MaxWidth, V::Lpa(v)) => l.max_size.width = *v,
            (K::MaxHeight, V::Lpa(v)) => l.max_size.height = *v,
            (K::AspectRatio, V::OptNumber(v)) => l.aspect_ratio = v.map(|n| n.0),
            (K::Top, V::Lpa(v)) => l.inset.top = *v,
            (K::Right, V::Lpa(v)) => l.inset.right = *v,
            (K::Bottom, V::Lpa(v)) => l.inset.bottom = *v,
            (K::Left, V::Lpa(v)) => l.inset.left = *v,
            (
                K::Margin
                | K::MarginHorizontal
                | K::MarginVertical
                | K::MarginTop
                | K::MarginRight
                | K::MarginBottom
                | K::MarginLeft,
                v,
            ) => {
                let (length, var) = match v {
                    V::Lpa(v) => (*v, None),
                    V::EnvLength(p, var) => (LengthPercentageAuto::length(p.0), Some(*var)),
                    _ => return Err(StyleError::InvalidValue),
                };
                let m = &mut l.margin;
                for &side in edges(key) {
                    *[&mut m.top, &mut m.right, &mut m.bottom, &mut m.left][side] = length;
                    self.pending.env.margin[side] = var;
                }
            }
            (
                K::Padding
                | K::PaddingHorizontal
                | K::PaddingVertical
                | K::PaddingTop
                | K::PaddingRight
                | K::PaddingBottom
                | K::PaddingLeft,
                v,
            ) => {
                let (length, var) = match v {
                    V::Lp(v) => (*v, None),
                    V::EnvLength(p, var) => (LengthPercentage::length(p.0), Some(*var)),
                    _ => return Err(StyleError::InvalidValue),
                };
                let p = &mut l.padding;
                for &side in edges(key) {
                    *[&mut p.top, &mut p.right, &mut p.bottom, &mut p.left][side] = length;
                    self.pending.env.padding[side] = var;
                }
            }
            (K::Gap, V::Lp(v)) => {
                l.gap = taffy::Size {
                    width: *v,
                    height: *v,
                };
            }
            (K::RowGap, V::Lp(v)) => l.gap.height = *v,
            (K::ColumnGap, V::Lp(v)) => l.gap.width = *v,
            (K::BorderWidth, V::Points(w)) => self.set_border_width(&[0, 1, 2, 3], *w),
            (K::BorderTopWidth, V::Points(w)) => self.set_border_width(&[0], *w),
            (K::BorderRightWidth, V::Points(w)) => self.set_border_width(&[1], *w),
            (K::BorderBottomWidth, V::Points(w)) => self.set_border_width(&[2], *w),
            (K::BorderLeftWidth, V::Points(w)) => self.set_border_width(&[3], *w),
            (K::ZIndex, V::Number(n)) => self.z_index = n.0 as i32,
            (K::VerticalAlign, V::VerticalAlign(v)) => self.vertical_align = *v,
            (K::Direction, V::OptBool(v)) => self.direction = *v,
            (K::MarginStart, V::OptLpa(v)) => self.logical.margin[0] = *v,
            (K::MarginEnd, V::OptLpa(v)) => self.logical.margin[1] = *v,
            (K::PaddingStart, V::OptLp(v)) => self.logical.padding[0] = *v,
            (K::PaddingEnd, V::OptLp(v)) => self.logical.padding[1] = *v,
            (K::Start, V::OptLpa(v)) => self.logical.inset[0] = *v,
            (K::End, V::OptLpa(v)) => self.logical.inset[1] = *v,
            (K::BorderStartWidth, V::OptPoints(w)) => self.set_logical_border_width(0, *w),
            (K::BorderEndWidth, V::OptPoints(w)) => self.set_logical_border_width(1, *w),
            (K::BorderStartColor, V::Color(c)) => self.logical.border_color[0] = *c,
            (K::BorderEndColor, V::Color(c)) => self.logical.border_color[1] = *c,
            (K::BorderTopStartRadius, V::OptPoints(r)) => self.logical.radius[0] = *r,
            (K::BorderTopEndRadius, V::OptPoints(r)) => self.logical.radius[1] = *r,
            (K::BorderBottomEndRadius, V::OptPoints(r)) => self.logical.radius[2] = *r,
            (K::BorderBottomStartRadius, V::OptPoints(r)) => self.logical.radius[3] = *r,

            // View
            (K::BackgroundColor, V::Color(c)) => {
                self.view.background_color = c.unwrap_or(Color::TRANSPARENT)
            }
            (K::BorderRadius, V::Points(r)) => self.view.border_radius = *r,
            (K::BorderTopLeftRadius, V::OptPoints(r)) => self.view.corner_radius.top_left = *r,
            (K::BorderTopRightRadius, V::OptPoints(r)) => self.view.corner_radius.top_right = *r,
            (K::BorderBottomRightRadius, V::OptPoints(r)) => {
                self.view.corner_radius.bottom_right = *r
            }
            (K::BorderBottomLeftRadius, V::OptPoints(r)) => {
                self.view.corner_radius.bottom_left = *r
            }
            (K::BorderColor, V::Color(c)) => self.set_border_color(&[0, 1, 2, 3], *c),
            (K::BorderTopColor, V::Color(c)) => self.set_border_color(&[0], *c),
            (K::BorderRightColor, V::Color(c)) => self.set_border_color(&[1], *c),
            (K::BorderBottomColor, V::Color(c)) => self.set_border_color(&[2], *c),
            (K::BorderLeftColor, V::Color(c)) => self.set_border_color(&[3], *c),
            (K::BorderStyle, V::BorderStyle(style, hidden)) => {
                // `none` (and `hidden`, the same outside tables) draws and
                // lays out no border whatever the widths, set before or
                // after it (CSS).
                let hidden = *hidden;
                self.view.borders.style = *style;
                if hidden != self.pending.border_hidden {
                    self.pending.border_hidden = hidden;
                    for side in 0..4 {
                        let w = if hidden {
                            Points::ZERO
                        } else {
                            self.pending.border_widths[side]
                        };
                        self.set_used_border_width(side, w);
                    }
                    self.logical.border_width = if hidden {
                        [None; 2]
                    } else {
                        self.pending.border_logical
                    };
                }
            }
            (K::BorderCurve, V::Bool(v)) => self.view.continuous_corners = *v,
            (K::TintColor, V::Color(c)) => self.view.tint_color = *c,
            (K::ObjectFit, V::ObjectFit(v)) => self.pending.object_fit = *v,
            (K::ObjectPosition, V::ObjectPosition(v)) => self.view.object_position = *v,
            (K::BackfaceVisibility, V::Bool(v)) => self.view.backface_hidden = *v,
            (K::Transform, V::Transform(steps)) => {
                self.set_transform(TransformProperty::Transform, steps)?
            }
            (K::Translate, V::Transform(steps)) => {
                self.set_transform(TransformProperty::Translate, steps)?
            }
            (K::Rotate, V::Transform(steps)) => {
                self.set_transform(TransformProperty::Rotate, steps)?
            }
            (K::Scale, V::Transform(steps)) => {
                self.set_transform(TransformProperty::Scale, steps)?
            }
            (K::TransformOrigin, V::TransformOrigin(v)) => self.view.transform_origin = *v,
            (K::Opacity, V::Number(n)) => self.view.opacity = n.0.clamp(0.0, 1.0),
            (K::Overflow, V::Bool(v)) => self.view.overflow_hidden = *v,

            // Text
            (K::Color, V::Color(c)) => self.text.color = *c,
            (K::Fill, V::Paint(v)) => self.paint.fill = *v,
            (K::Stroke, V::Paint(v)) => self.paint.stroke = *v,
            (K::StrokeWidth, V::OptPoints(v)) => self.paint.stroke_width = *v,
            (K::StrokeDasharray, V::Dashes(v)) => self.paint.stroke_dasharray = v.clone(),
            (K::FontSize, V::OptPoints(v)) => self.text.font_size = *v,
            (K::FontWeight, V::FontWeight(v)) => self.text.font_weight = *v,
            (K::LineHeight, V::LineHeight(v)) => {
                let t = &mut self.text;
                (t.line_height, t.line_height_multiple, t.line_height_em) = match *v {
                    LineHeight::Normal => (None, None, None),
                    LineHeight::Points(p) => (Some(p), None, None),
                    LineHeight::Multiple(n) => (None, Some(n), None),
                    LineHeight::Em(n) => (None, None, Some(n)),
                };
            }
            (K::FontFamily, V::FontFamily(v)) => self.text.font_family = *v,
            (K::FontStyle, V::OptBool(v)) => self.text.italic = *v,
            (K::LetterSpacing, V::LetterSpacing(points, em)) => {
                self.text.letter_spacing = *points;
                self.text.letter_spacing_em = em.map(|n| n.0);
            }
            (K::TextDecorationLine, V::TextDecoration(v)) => self.text.decoration = *v,
            (K::TextDecorationColor, V::Color(c)) => self.text.decoration_color = *c,
            (K::PlaceholderColor, V::Color(c)) => self.text.placeholder_color = *c,
            (K::SelectionColor, V::Color(c)) => self.text.selection_color = *c,
            (K::CaretColor, V::Color(c)) => self.text.caret_color = *c,
            (K::WritingDirection, V::OptBool(v)) => self.text.writing_direction = *v,
            (K::TextAlignVertical, V::AlignVertical(v)) => self.text.align_vertical = *v,
            (K::NumberOfLines, V::NumberOfLines(v)) => self.pending.number_of_lines = *v,
            (K::TextOverflow, V::TextOverflow(v)) => self.pending.text_overflow = *v,
            (K::UserSelect, V::OptBool(v)) => self.pending.user_select = *v,
            (K::PointerEvents, V::PointerEvents(v)) => {
                self.pending.pointer_events = Some(*v);
            }
            (K::FontVariant, V::FontVariant(v)) => self.text.font_variant = *v,
            (K::TextShadow, V::TextShadow(v)) => {
                self.pending.current_text_shadow = v.is_some_and(|(_, colored)| !colored);
                let widest = v.map(|(s, _)| s);
                self.text.shadow_color = widest.map(|s| s.color);
                self.text.shadow_offset = widest.map(|s| point(s.offset_x, s.offset_y));
                self.text.shadow_radius = widest.map(|s| s.radius);
            }
            (K::TextTransform, V::TextTransform(v)) => self.text.transform = *v,
            (K::TextAlign, V::TextAlign(align, physical)) => {
                self.text.text_align = *align;
                self.text.text_align_physical = *physical;
            }

            // Effects
            (K::BoxShadow, V::Shadows(layers)) => {
                self.pending.current_shadows = layers
                    .iter()
                    .enumerate()
                    .filter(|(i, (_, colored))| *i < 64 && !colored)
                    .fold(0, |mask, (i, _)| mask | 1 << i);
                let list = layers.iter().map(|(l, _)| *l).collect();
                self.edit_effects(|e| e.box_shadows = list);
            }
            (K::BackgroundImage, V::Gradients(list)) => {
                let list = list.to_vec();
                self.edit_effects(|e| e.background_images = list);
            }
            (K::MaskImage, V::Gradients(list)) => {
                let list = list.to_vec();
                self.edit_effects(|e| e.mask_images = list);
            }
            (K::Filter, V::Filters(list)) => {
                // A `drop-shadow()` without a color is the text color's.
                self.pending.current_drop_shadows = list
                    .iter()
                    .take(64)
                    .enumerate()
                    .filter(|(_, (_, named))| !named)
                    .fold(0, |m, (i, _)| m | 1 << i);
                let list = list.iter().map(|(f, _)| *f).collect();
                self.edit_effects(|e| e.filters = list);
            }
            (K::BackdropFilter, V::Filters(list)) => {
                let list = list.iter().map(|(f, _)| *f).collect();
                self.edit_effects(|e| e.backdrop_filters = list);
            }
            (K::GlassEffect, V::GlassEffect(v)) => self.view.glass_effect = *v,
            (K::MixBlendMode, V::BlendMode(v)) => self.view.blend_mode = *v,
            (K::OutlineWidth, V::Points(width)) => {
                self.pending.outline_width = *width;
                if !self.pending.outline_hidden {
                    self.view.outline.width = *width;
                }
            }
            (K::OutlineColor, V::Color(c)) => {
                self.view.outline.color = c.unwrap_or(Outline::NONE.color)
            }
            (K::OutlineOffset, V::Points(v)) => self.view.outline.offset = *v,
            (K::OutlineStyle, V::BorderStyle(style, hidden)) => {
                // `none` draws no outline whatever the width, set before
                // or after it (CSS).
                self.view.outline.style = *style;
                self.pending.outline_hidden = *hidden;
                self.view.outline.width = if *hidden {
                    Points::ZERO
                } else {
                    self.pending.outline_width
                };
            }
            (K::Visibility, V::Bool(v)) => self.view.hidden = *v,
            (K::Cursor, V::Cursor(v)) => self.view.cursor = *v,
            (K::TransitionProperty, V::TransitionProps(v)) => self.transition_mut().properties = *v,
            (K::TransitionDuration, V::Ms(ms)) => self.transition_mut().duration = ms.0,
            (K::TransitionDelay, V::Ms(ms)) => self.transition_mut().delay = ms.0,
            (K::TransitionTimingFunction, V::Easing(e)) => {
                self.transition_mut().easing = e.map(|n| n.0)
            }
            (K::AnimationName, V::Keyframes(keyframes)) => {
                // An animation already running (a class's) keeps its
                // values; a new one takes those set before its name.
                let pending = &self.pending;
                self.view.animation = keyframes.map(|keyframes| {
                    let base = self
                        .view
                        .animation
                        .unwrap_or_else(|| pending.animation(KeyframeAnimation::new(keyframes)));
                    KeyframeAnimation { keyframes, ..base }
                });
            }
            (K::AnimationDuration, V::Ms(ms)) => {
                self.pending.animation_duration = Some(ms.0);
                if let Some(a) = self.view.animation.as_mut() {
                    a.duration = ms.0;
                }
            }
            (K::AnimationDelay, V::Ms(ms)) => {
                self.pending.animation_delay = Some(ms.0);
                if let Some(a) = self.view.animation.as_mut() {
                    a.delay = ms.0;
                }
            }
            (K::AnimationTimingFunction, V::Easing(e)) => {
                let easing = e.map(|n| n.0);
                self.pending.animation_easing = Some(easing);
                if let Some(a) = self.view.animation.as_mut() {
                    a.easing = easing;
                }
            }
            (K::AnimationIterationCount, V::Iterations(count)) => {
                self.pending.animation_iterations = Some(*count);
                if let Some(a) = self.view.animation.as_mut() {
                    a.iterations = *count;
                }
            }
            (K::TextDecorationStyle, V::DecorationStyle(v)) => self.text.decoration_style = *v,
            (K::TextDecorationThickness, V::OptPoints(v)) => self.text.decoration_thickness = *v,
            (K::TextUnderlineOffset, V::OptPoints(v)) => self.text.underline_offset = *v,

            // Grid
            (K::GridTemplateColumns, V::GridTemplate(t)) => {
                (l.grid_template_columns, l.grid_template_column_names) = (*t.0).clone();
            }
            (K::GridTemplateRows, V::GridTemplate(t)) => {
                (l.grid_template_rows, l.grid_template_row_names) = (*t.0).clone();
            }
            (K::GridAutoColumns, V::GridTracks(v)) => l.grid_auto_columns = v.to_vec(),
            (K::GridAutoRows, V::GridTracks(v)) => l.grid_auto_rows = v.to_vec(),
            (K::GridAutoFlow, V::GridAutoFlow(v)) => l.grid_auto_flow = *v,
            (K::GridColumn, V::GridLine(v)) => l.grid_column = v.clone(),
            (K::GridRow, V::GridLine(v)) => l.grid_row = v.clone(),
            (K::GridColumnStart, V::GridPlacement(v)) => l.grid_column.start = v.clone(),
            (K::GridColumnEnd, V::GridPlacement(v)) => l.grid_column.end = v.clone(),
            (K::GridRowStart, V::GridPlacement(v)) => l.grid_row.start = v.clone(),
            (K::GridRowEnd, V::GridPlacement(v)) => l.grid_row.end = v.clone(),
            (K::JustifyItems, V::AlignItems(v)) => {
                l.justify_items = v.unwrap_or(AlignItems::STRETCH)
            }
            (K::JustifySelf, V::AlignItems(v)) => l.justify_self = *v,
            _ => return Err(StyleError::InvalidValue),
        }
        Ok(())
    }
}
