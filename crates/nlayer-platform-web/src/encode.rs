//! Mount batches as the JSON [`crate::MOUNTER_JS`] applies: one array per
//! batch, one array per item, `[op, id, ...]`.
//!
//! Visual styles become CSS here. Each distinct text is a class,
//! `nl-` and 8 hex digits of its FNV-1a hash ([`class_key`]), defined once
//! (`["css", name, text]`, before the first item that uses it). A view that
//! would define too many shared classes, or that collides with another
//! text, gets its own rule (`i<id>…`), which the page replaces in place.
//!
//! Copied from `nsh-platform-web/src/encode.rs` and `text/css.rs`. The CSS
//! is the v1 whitelist only (fill, radius, opacity, overflow, static
//! transform, transform origin, border). Text is the attributed spans, not
//! `WebLayout` glyph runs. `activeStyle`, `hoverStyle` and `focusStyle`
//! are rules on the view (`["rule", ...]`) that the page applies; they do
//! not replace the view's class.

use std::fmt::Write as _;

use nlayer_core::{
    Color, Command, EventKind, EventMask, InputConfig, Interaction, KeyboardType, MountBatch,
    MountItem, NodeId, ResizeMode, ViewKind,
    geometry::{DefiniteLength, Points, REM_SIZE, relative},
    style::{BorderStyle, TextAlign, TextStyle, TransformOrigin, ViewStyle},
    text::{AttributedText, ResolvedTextStyle},
    transform::{Transform, TransformKind},
};
use rustc_hash::FxHashMap;

/// New classes a view may define before its styles get rules of their own.
const OWN_RULES_AFTER: u8 = 16;

/// The system font stack (`fontFamily` unset). v1 has no custom fonts.
const SYSTEM_FONT: &str = "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, \
\"Helvetica Neue\", Arial, \"PingFang SC\", \"Noto Sans SC\", \"Microsoft YaHei\", sans-serif";

/// The classes defined for one page.
#[derive(Default)]
struct Classes {
    /// Class key → a second hash of its text, which tells a collision.
    defined: FxHashMap<u32, u64>,
    /// How many classes each view defined.
    count: Vec<u8>,
}

/// Encodes mount batches for one page. It remembers which classes the page
/// already has, so later batches name them and do not repeat the CSS.
#[derive(Default)]
pub struct Encoder {
    classes: Classes,
}

impl Encoder {
    /// An encoder for a page that has no generated classes yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// One batch, a JSON array. Items stay in order.
    pub fn encode(&mut self, batch: &MountBatch) -> String {
        let mut out = Out::new();
        for item in &batch.items {
            self.item(&mut out, item);
        }
        out.finish()
    }

    fn item(&mut self, out: &mut Out, item: &MountItem) {
        match item {
            MountItem::Create { id, kind } => {
                out.item("c").num(*id).str(kind_name(*kind)).end();
            }
            MountItem::Insert {
                parent,
                child,
                before,
            } => {
                out.item("i").num(*parent).num(*child);
                match before {
                    Some(before) => out.num(*before),
                    None => out.raw("null"),
                };
                out.end();
            }
            MountItem::Remove { id } => out.item("r").num(*id).end(),
            MountItem::Delete { id } => {
                if let Some(n) = self.classes.count.get_mut(*id as usize) {
                    *n = 0;
                }
                out.item("d").num(*id).end();
            }
            MountItem::Frame { id, frame } => {
                out.item("f")
                    .num(*id)
                    .num(frame.origin.x.0)
                    .num(frame.origin.y.0)
                    .num(frame.size.width.0)
                    .num(frame.size.height.0)
                    .end();
            }
            MountItem::ViewStyle { id, style } => {
                let (css, border) = view_css(style);
                let class = self.class(out, *id, Part::View, &css);
                let border = match &border {
                    Some(border) => self.class(out, *id, Part::Border, border),
                    None => Class::None,
                };
                out.item("s")
                    .num(*id)
                    .class_arg(class)
                    .class_arg(border)
                    .end();
            }
            MountItem::Text {
                id, text, content, ..
            } => {
                let runs = text_runs(text);
                let classes: Vec<Class> = runs
                    .iter()
                    .enumerate()
                    .map(|(i, run)| self.class(out, *id, Part::Text(i), &run.css))
                    .collect();
                out.item("t")
                    .num(*id)
                    .num(content.origin.x.0)
                    .num(content.origin.y.0)
                    .num(content.size.width.0)
                    .num(content.size.height.0);
                match align_css(text.align, text.rtl) {
                    Some(align) => out.str(align),
                    None => out.raw("null"),
                };
                out.bool(text.rtl);
                out.s.push_str(",[");
                for (i, (run, class)) in runs.iter().zip(classes).enumerate() {
                    if i > 0 {
                        out.s.push(',');
                    }
                    out.s.push('[');
                    push_str(&mut out.s, &run.text);
                    out.s.push(',');
                    out.push_class(class);
                    out.s.push(']');
                }
                out.s.push(']');
                out.s.push_str(",[");
                for (i, span) in text.test_ids.iter().enumerate() {
                    if i > 0 {
                        out.s.push(',');
                    }
                    out.s.push('[');
                    push_str(&mut out.s, &span.test_id);
                    let _ = write!(out.s, ",{},{}]", span.start, span.end);
                }
                out.s.push(']');
                out.s.push_str(",[");
                for (i, link) in text.links.iter().enumerate() {
                    if i > 0 {
                        out.s.push(',');
                    }
                    let _ = write!(out.s, "[{},{},{}]", link.start, link.end, link.node);
                }
                out.s.push(']');
                out.end();
            }
            MountItem::Events { id, events } => {
                out.item("e").num(*id);
                out.s.push(',');
                push_events(&mut out.s, *events);
                out.end();
            }
            MountItem::Image {
                id,
                source,
                resize_mode,
            } => {
                out.item("img")
                    .num(*id)
                    .opt_str(source.as_ref().map(|s| &*s.uri))
                    .str(resize_mode_css(*resize_mode))
                    .end();
            }
            MountItem::ScrollConfig { id, horizontal } => {
                out.item("sc").num(*id).bool(*horizontal).end();
            }
            MountItem::ScrollContent { id, size } => {
                out.item("scs")
                    .num(*id)
                    .num(size.width.0)
                    .num(size.height.0)
                    .end();
            }
            MountItem::ScrollBy { id, dx, dy } => {
                out.item("sb").num(*id).num(dx.0).num(dy.0).end();
            }
            MountItem::Input { id, config } => {
                let css = field_css(config);
                let class = self.class(out, *id, Part::Field, &css);
                out.item("in").num(*id).class_arg(class);
                out.s.push_str(",{");
                out.s.push_str("\"placeholder\":");
                push_str(&mut out.s, &config.placeholder);
                let _ = write!(
                    out.s,
                    ",\"secure\":{},\"keyboard\":",
                    if config.secure { "true" } else { "false" }
                );
                push_str(&mut out.s, keyboard_name(config.keyboard));
                let _ = write!(
                    out.s,
                    ",\"disabled\":{},\"rtl\":{}",
                    if config.disabled { "true" } else { "false" },
                    if config.rtl { "true" } else { "false" }
                );
                out.s.push('}');
                out.end();
            }
            MountItem::InputValue { id, text, count } => {
                out.item("iv").num(*id).str(text).num(*count).end();
            }
            MountItem::InputContent { id, content } => {
                out.item("ic")
                    .num(*id)
                    .num(content.origin.x.0)
                    .num(content.origin.y.0)
                    .num(content.size.width.0)
                    .num(content.size.height.0)
                    .end();
            }
            MountItem::Command { id, command } => {
                let Command::ScrollTo { x, y, animated } = command;
                out.item("cmd")
                    .num(*id)
                    .str("scrollTo")
                    .num(x.0)
                    .num(y.0)
                    .bool(*animated)
                    .end();
            }
            MountItem::TestId { id, test_id } => {
                out.item("tid").num(*id).opt_str(test_id.as_deref()).end();
            }
            MountItem::StateStyle {
                id,
                interaction,
                view,
                text,
            } => self.state(out, *id, *interaction, view, text),
        }
    }

    /// The class of `css`, style `part` of view `id`, defining it first
    /// when the page does not have it.
    fn class(&mut self, out: &mut Out, id: NodeId, part: Part, css: &str) -> Class {
        if css.is_empty() {
            return Class::None;
        }
        let key = class_key(css);
        let hash = text_hash(css);
        let shared = match self.classes.defined.get(&key) {
            Some(&existing) if existing == hash => Some(false),
            Some(_) => None,
            None => {
                let i = id as usize;
                if self.classes.count.len() <= i {
                    self.classes.count.resize(i + 1, 0);
                }
                if self.classes.count[i] >= OWN_RULES_AFTER {
                    None
                } else {
                    self.classes.count[i] += 1;
                    self.classes.defined.insert(key, hash);
                    Some(true)
                }
            }
        };
        let class = match shared {
            Some(_) => Class::Shared(key),
            None => Class::Own(id, part),
        };
        if shared != Some(false) {
            out.item("css");
            out.class_arg(class).str(css).end();
        }
        class
    }

    /// `activeStyle` / `hoverStyle` / `focusStyle`. The class stays on the
    /// view; the page paints the rules while that state holds. Active beats
    /// hover, which beats focus, whatever order the rules were defined in.
    fn state(
        &mut self,
        out: &mut Out,
        id: NodeId,
        interaction: Interaction,
        view: &ViewStyle,
        text: &TextStyle,
    ) {
        let (token, pseudo, weight) = match interaction {
            Interaction::Hover => ("h", ":hover", 3),
            Interaction::Active => ("a", ":active", 4),
            Interaction::Focus => ("f", ":focus-within", 2),
        };
        let name = format!("i{id}{token}");
        let (view_css, border) = view_css(view);
        let text_css = state_text_css(text);
        let empty = view_css.is_empty() && border.is_none() && text_css.is_empty();
        rule(out, &name, weight, pseudo, "", &view_css);
        rule(
            out,
            &name,
            weight,
            pseudo,
            " > .nl-border",
            border.as_deref().unwrap_or(""),
        );
        rule(out, &name, weight, pseudo, " .nl-text span", &text_css);
        rule(out, &name, weight, pseudo, " > .nl-field", &text_css);
        out.item("st").num(id).str(interaction.name());
        if empty {
            out.raw("null");
        } else {
            out.str(&name);
        }
        out.end();
    }
}

/// The class of a style.
#[derive(Clone, Copy)]
enum Class {
    /// No CSS.
    None,
    /// By content: `nl-` and 8 hex digits.
    Shared(u32),
    /// A view's own rule. The page puts its prefix in front.
    Own(NodeId, Part),
}

/// Which of a view's styles.
#[derive(Clone, Copy)]
enum Part {
    View,
    Border,
    Text(usize),
    Field,
}

/// JSON output.
struct Out {
    s: String,
    first: bool,
}

impl Out {
    fn new() -> Self {
        Self {
            s: String::from("["),
            first: true,
        }
    }

    fn item(&mut self, op: &str) -> &mut Self {
        if !self.first {
            self.s.push(',');
        }
        self.first = false;
        self.s.push_str("[\"");
        self.s.push_str(op);
        self.s.push('"');
        self
    }

    fn end(&mut self) {
        self.s.push(']');
    }

    fn num(&mut self, n: impl Into<f64>) -> &mut Self {
        let n = n.into();
        self.s.push(',');
        if n.is_finite() {
            let r = (n * 1000.0).round() / 1000.0;
            let _ = write!(self.s, "{r}");
        } else {
            self.s.push_str("null");
        }
        self
    }

    fn str(&mut self, v: &str) -> &mut Self {
        self.s.push(',');
        push_str(&mut self.s, v);
        self
    }

    fn opt_str(&mut self, v: Option<&str>) -> &mut Self {
        match v {
            Some(v) => self.str(v),
            None => self.raw("null"),
        }
    }

    fn bool(&mut self, v: bool) -> &mut Self {
        self.raw(if v { "true" } else { "false" })
    }

    fn raw(&mut self, json: &str) -> &mut Self {
        self.s.push(',');
        self.s.push_str(json);
        self
    }

    fn push_class(&mut self, class: Class) {
        let _ = match class {
            Class::None => write!(self.s, "null"),
            Class::Shared(key) => write!(self.s, "\"nl-{key:08x}\""),
            Class::Own(id, Part::View) => write!(self.s, "\"i{id}\""),
            Class::Own(id, Part::Border) => write!(self.s, "\"i{id}b\""),
            Class::Own(id, Part::Text(i)) => write!(self.s, "\"i{id}t{i}\""),
            Class::Own(id, Part::Field) => write!(self.s, "\"i{id}f\""),
        };
    }

    fn class_arg(&mut self, class: Class) -> &mut Self {
        self.s.push(',');
        self.push_class(class);
        self
    }

    fn finish(mut self) -> String {
        self.s.push(']');
        self.s
    }
}

/// The key of the class of CSS text `css`: its 32-bit FNV-1a hash (as
/// `nlayer.js` computes the names of the classes it makes itself).
fn class_key(css: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in css.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// The 64-bit FNV-1a hash of `css`.
fn text_hash(css: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in css.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn push_str(out: &mut String, v: &str) {
    out.push('"');
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `events` as the names of their props.
fn push_events(s: &mut String, events: EventMask) {
    const KINDS: [EventKind; 8] = [
        EventKind::Press,
        EventKind::Scroll,
        EventKind::ScrollEnd,
        EventKind::ScrollDragStart,
        EventKind::ScrollDragEnd,
        EventKind::MomentumStart,
        EventKind::MomentumEnd,
        EventKind::Change,
    ];
    s.push('[');
    let mut first = true;
    for kind in KINDS {
        if events.contains(kind) {
            if !first {
                s.push(',');
            }
            first = false;
            push_str(s, kind.prop_name());
        }
    }
    s.push(']');
}

fn kind_name(kind: ViewKind) -> &'static str {
    match kind {
        ViewKind::View => "view",
        ViewKind::Text => "text",
        ViewKind::Image => "image",
        ViewKind::Scroll => "scroll",
        ViewKind::Input => "input",
    }
}

fn resize_mode_css(mode: ResizeMode) -> &'static str {
    match mode {
        ResizeMode::Cover => "cover",
        ResizeMode::Contain => "contain",
        ResizeMode::Stretch => "fill",
        ResizeMode::Center => "none",
        ResizeMode::ScaleDown => "scale-down",
    }
}

fn keyboard_name(keyboard: KeyboardType) -> &'static str {
    match keyboard {
        KeyboardType::Text => "text",
        KeyboardType::Email => "email",
        KeyboardType::Number => "number",
        KeyboardType::Phone => "phone",
    }
}

/// A CSS color.
fn css_color(c: Color) -> String {
    let v = c.0;
    let (r, g, b, a) = (v >> 24, (v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff);
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("rgba({r},{g},{b},{:.3})", a as f32 / 255.0)
    }
}

/// The CSS of a view's visual style, and of its border overlay (drawn over
/// the children, as UIKit draws layer borders; also keeps borders out of
/// the content box, which layout already placed).
fn view_css(s: &ViewStyle) -> (String, Option<String>) {
    let mut css = String::new();
    if !s.background_color.is_transparent() {
        let _ = write!(css, "background-color:{};", css_color(s.background_color));
    }
    let radii: [f32; 4] = s
        .corner_radius
        .to_array()
        .map(|r| r.unwrap_or(s.border_radius).0);
    let radius = if radii.iter().any(|&r| r > 0.0) {
        format!(
            "border-radius:{}px {}px {}px {}px;",
            radii[0], radii[1], radii[2], radii[3]
        )
    } else {
        String::new()
    };
    css.push_str(&radius);
    if s.opacity < 1.0 {
        let _ = write!(css, "opacity:{};", s.opacity);
    }
    // `clip`, not `hidden`: a hidden box is still a scroll container, which
    // the browser scrolls to bring a focused field into view.
    if s.overflow_hidden {
        css.push_str("overflow:clip;");
    }
    if !s.transform.is_identity() {
        let transform = css_transform(&s.transform);
        let _ = write!(css, "transform:{transform};");
    }
    if let Some(origin) = origin_css(s.transform_origin) {
        let _ = write!(css, "transform-origin:{origin};");
    }
    let b = &s.borders;
    let border = if b.widths.any(|&w| w > Points::ZERO) {
        let style = border_style(b.style);
        let sides = ["top", "right", "bottom", "left"];
        let mut out = String::new();
        for ((side, width), color) in sides.iter().zip(b.widths.to_array()).zip(b.colors) {
            let width = width.0;
            let _ = write!(
                out,
                "border-{side}-width:{width}px;border-{side}-style:{style};border-{side}-color:{};",
                css_color(color)
            );
        }
        out.push_str(&radius);
        Some(out)
    } else {
        None
    };
    (css, border)
}

fn rule(out: &mut Out, name: &str, weight: u8, pseudo: &str, tail: &str, text: &str) {
    out.item("rule")
        .str(name)
        .num(weight)
        .str(pseudo)
        .str(tail)
        .str(text)
        .end();
}

/// Text paint a state style set. Unset properties stay the view's.
fn state_text_css(text: &TextStyle) -> String {
    let mut css = String::new();
    if let Some(color) = text.color {
        let _ = write!(css, "color:{};", css_color(color));
    }
    if let Some(size) = text.font_size {
        let _ = write!(css, "font-size:{}px;", size.0);
    }
    if let Some(weight) = text.font_weight {
        let _ = write!(css, "font-weight:{weight};");
    }
    if let Some(italic) = text.italic {
        css.push_str(if italic {
            "font-style:italic;"
        } else {
            "font-style:normal;"
        });
    }
    if let Some(height) = text.line_height {
        let _ = write!(css, "line-height:{}px;", height.0);
    } else if let Some(multiple) = text.line_height_multiple {
        let _ = write!(css, "line-height:{multiple};");
    }
    if let Some(align) = text.text_align
        && let Some(align) = align_css(align, false)
    {
        let _ = write!(css, "text-align:{align};");
    }
    css
}

fn border_style(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::Solid => "solid",
        BorderStyle::Dashed => "dashed",
        BorderStyle::Dotted => "dotted",
    }
}

/// The CSS transform functions of a list. Fractions stay percentages; the
/// core resolves them to points before a batch when it knows the size.
fn css_transform(t: &Transform) -> String {
    let mut out = String::new();
    for op in t.ops() {
        let v = op.value;
        let _ = match op.kind {
            TransformKind::TranslateX => write!(out, "translateX({v}px) "),
            TransformKind::TranslateY => write!(out, "translateY({v}px) "),
            TransformKind::TranslateXFraction => write!(out, "translateX({}%) ", v * 100.0),
            TransformKind::TranslateYFraction => write!(out, "translateY({}%) ", v * 100.0),
            TransformKind::Scale => write!(out, "scale({v}) "),
            TransformKind::ScaleX => write!(out, "scaleX({v}) "),
            TransformKind::ScaleY => write!(out, "scaleY({v}) "),
            TransformKind::Rotate => write!(out, "rotate({v}deg) "),
            TransformKind::RotateX => write!(out, "rotateX({v}deg) "),
            TransformKind::RotateY => write!(out, "rotateY({v}deg) "),
            TransformKind::SkewX => write!(out, "skewX({v}deg) "),
            TransformKind::SkewY => write!(out, "skewY({v}deg) "),
            TransformKind::Perspective => write!(out, "perspective({v}px) "),
        };
    }
    out.pop();
    out
}

fn origin_css(o: TransformOrigin) -> Option<String> {
    if o.x == relative(0.5) && o.y == relative(0.5) && o.z == Points::ZERO {
        return None;
    }
    let z = if o.z == Points::ZERO {
        String::new()
    } else {
        format!(" {}px", o.z.0)
    };
    Some(format!("{} {}{z}", origin(o.x), origin(o.y)))
}

fn origin(o: DefiniteLength) -> String {
    match o {
        DefiniteLength::Absolute(v) => format!("{}px", v.to_points(REM_SIZE).0),
        DefiniteLength::Fraction(f) => format!("{}%", f.0 * 100.0),
    }
}

/// The CSS `font` shorthand. The family is the system stack. `font`
/// resets `line-height`, so callers write [`css_line_height`] after it.
fn css_font(style: &ResolvedTextStyle) -> String {
    format!(
        "{}{} {}px {SYSTEM_FONT}",
        if style.italic { "italic " } else { "" },
        style.font_weight,
        style.font_size.0
    )
}

/// Matches [`nlayer_core::text::ApproxTextEngine`]: an explicit
/// `lineHeight`, otherwise `1.2` times the font size. The page's default
/// line box is taller than that, and the text box clips what hangs out.
fn css_line_height(style: &ResolvedTextStyle) -> String {
    if style.line_height.0 > 0.0 {
        format!("line-height:{}px;", style.line_height.0)
    } else {
        "line-height:1.2;".to_owned()
    }
}

struct Run {
    text: String,
    css: String,
}

/// One run per attributed span: color, weight, size and italic. The page
/// lays them out in the content box (no glyph positions).
fn text_runs(text: &AttributedText) -> Vec<Run> {
    text.spans
        .iter()
        .map(|span| Run {
            text: utf16_slice(&text.text, span.start, span.end),
            css: format!(
                "font:{};{}color:{};",
                css_font(&span.style),
                css_line_height(&span.style),
                css_color(span.style.color)
            ),
        })
        .filter(|run| !run.text.is_empty())
        .collect()
}

fn utf16_slice(s: &str, start: u32, end: u32) -> String {
    let mut out = String::new();
    let mut i = 0u32;
    for ch in s.chars() {
        let next = i + ch.len_utf16() as u32;
        if next > start && i < end {
            out.push(ch);
        }
        i = next;
        if i >= end {
            break;
        }
    }
    out
}

fn align_css(align: TextAlign, rtl: bool) -> Option<&'static str> {
    Some(match align {
        TextAlign::Auto if rtl => "right",
        TextAlign::Auto => return None,
        TextAlign::Left => "left",
        TextAlign::Center => "center",
        TextAlign::Right => "right",
        TextAlign::Justify => "justify",
    })
}

fn field_css(config: &InputConfig) -> String {
    let mut css = format!(
        "font:{};{}color:{};",
        css_font(&config.style),
        css_line_height(&config.style),
        css_color(config.style.color)
    );
    if let Some(align) = align_css(config.align, config.rtl) {
        let _ = write!(css, "text-align:{align};");
    }
    if config.rtl {
        css.push_str("direction:rtl;");
    }
    css
}
