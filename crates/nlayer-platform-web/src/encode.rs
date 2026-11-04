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
//! is the drawn whitelist (fill, radius, opacity, overflow, static
//! transform, transform origin, border, `box-shadow`, gradient
//! `background-image`). Text is glyph runs from
//! [`crate::text::WebLayout`]: each run is placed where layout put it.
//! `activeStyle`, `hoverStyle` and `focusStyle` are rules on the view
//! (`["rule", ...]`) that the page applies; they do not replace the view's
//! class.
//! A view style is `["s", id, class, border, layer]`. `layer` is the content
//! clip class when a visible outer shadow must stay outside `overflow: clip`;
//! otherwise null. A scroll is never split. A view style also sends
//! `["xf", id, steps]` (`{ translateX: 12 }`, angles in degrees), including
//! an empty list that clears a previous one. The page paints the CSS and
//! keeps the steps for the test tree.
//!

use std::fmt::Write as _;

use nlayer_core::{
    Color, Command, EventKind, EventMask, InputConfig, Interaction, KeyboardType, MountBatch,
    MountItem, NodeId, ResizeMode, ViewKind,
    animation::AnimationProp,
    effects::{BoxShadow, Gradient, GradientKind, GradientSize},
    geometry::{DefiniteLength, Points, REM_SIZE, Radians, Size, relative},
    style::{BorderStyle, DecorationStyle, TextAlign, TextStyle, TransformOrigin, ViewStyle},
    text::ResolvedTextStyle,
    transform::{Transform, TransformKind},
};
use rustc_hash::FxHashMap;

/// New classes a view may define before its styles get rules of their own.
const OWN_RULES_AFTER: u8 = 16;

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
    /// Kind from [`MountItem::Create`], so a scroll is never split.
    kinds: Vec<Option<ViewKind>>,
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
                self.remember(*id, *kind);
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
                if let Some(kind) = self.kinds.get_mut(*id as usize) {
                    *kind = None;
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
                let (css, border, layer) = view_css(style, self.is_scroll(*id));
                let class = self.class(out, *id, Part::View, &css);
                let border = match &border {
                    Some(border) => self.class(out, *id, Part::Border, border),
                    None => Class::None,
                };
                let layer = match &layer {
                    Some(layer) => self.class(out, *id, Part::Layer, layer),
                    None => Class::None,
                };
                out.item("s")
                    .num(*id)
                    .class_arg(class)
                    .class_arg(border)
                    .class_arg(layer)
                    .end();
                push_transform(out, *id, &style.transform);
            }
            MountItem::Animation(_) => {
                // The page's animation graph takes the ops directly.
            }
            MountItem::AnimationBindings { id, bindings } => {
                let mut json = String::from("[");
                for (i, (prop, node)) in bindings.iter().enumerate() {
                    if i > 0 {
                        json.push(',');
                    }
                    json.push('[');
                    json.push('"');
                    json.push_str(&animation_prop_name(*prop));
                    json.push('"');
                    json.push(',');
                    json.push_str(&node.to_string());
                    json.push(']');
                }
                json.push(']');
                out.item("ab").num(*id).raw(&json).end();
            }
            MountItem::Text {
                id,
                text,
                layout,
                content,
            } => {
                let layout = layout.downcast::<crate::text::WebLayout>();
                // Defined before the item that uses them. A run's line box is
                // the font's own (ascent + descent): the extra leading is in
                // the run's y, so the glyphs sit on the baseline attachments
                // were placed against.
                let mut classes = Vec::new();
                if let Some(layout) = layout.as_ref() {
                    for (i, style) in layout.styles.iter().enumerate() {
                        classes.push(self.class(out, *id, Part::Text(i), &run_css(style)));
                    }
                }
                out.item("t")
                    .num(*id)
                    .num(content.origin.x.0)
                    .num(content.origin.y.0)
                    .num(content.size.width.0)
                    .num(content.size.height.0);
                out.s.push_str(",[");
                if let Some(layout) = layout.as_ref() {
                    for (i, run) in layout.runs.iter().enumerate() {
                        if i > 0 {
                            out.s.push(',');
                        }
                        let _ = write!(out.s, "[{},{},", run.x, run.y);
                        push_str(&mut out.s, &run.text);
                        let _ = write!(out.s, ",{},", run.style);
                        match run.start {
                            Some(start) => {
                                let _ = write!(out.s, "{start}");
                            }
                            None => out.s.push_str("null"),
                        }
                        if !run.after.is_empty() {
                            out.s.push(',');
                            push_str(&mut out.s, &run.after);
                        }
                        out.s.push(']');
                    }
                }
                out.s.push_str("],[");
                for (i, class) in classes.into_iter().enumerate() {
                    if i > 0 {
                        out.s.push(',');
                    }
                    out.push_class(class);
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
                // `selectable`: `null` follows the view around it (`auto`),
                // `true` is `userSelect: "text"`, `false` `"none"`.
                match text.selectable {
                    Some(selectable) => out.bool(selectable),
                    None => out.raw("null"),
                };
                match text.selection_color {
                    Some(color) => out.str(&css_color(color)),
                    None => out.raw("null"),
                };
                out.s.push_str(",[");
                for (i, href) in text.hrefs.iter().enumerate() {
                    if i > 0 {
                        out.s.push(',');
                    }
                    let _ = write!(out.s, "[{},{},{}]", href.start, href.end, href.node);
                }
                out.s.push(']');
                // Span backgrounds, in the content box: `[x, y, w, h, radius, color]`.
                out.s.push_str(",[");
                if let Some(layout) = layout.as_ref() {
                    for (i, b) in layout.backgrounds.iter().enumerate() {
                        if i > 0 {
                            out.s.push(',');
                        }
                        let _ = write!(
                            out.s,
                            "[{},{},{},{},{},",
                            b.x, b.y, b.width, b.height, b.radius
                        );
                        push_str(&mut out.s, &css_color(b.color));
                        out.s.push(']');
                    }
                }
                out.s.push_str("],");
                push_str(&mut out.s, &text.text);
                out.end();
            }
            MountItem::LongPressDelay { id, delay } => {
                out.item("lp").num(*id);
                match delay {
                    Some(delay) => out.num(*delay),
                    None => out.raw("null"),
                };
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
            MountItem::ScrollConfig {
                id,
                horizontal,
                taps,
                dismiss,
            } => {
                out.item("sc")
                    .num(*id)
                    .bool(*horizontal)
                    .str(taps.name())
                    .str(dismiss.name())
                    .end();
            }
            MountItem::ScrollContent {
                id,
                size,
                refresh_indicator,
                paging,
                snap_interval,
                snap_offsets,
                snap_alignment,
                disable_interval_momentum,
                deceleration,
            } => {
                out.item("scs")
                    .num(*id)
                    .num(size.width.0)
                    .num(size.height.0)
                    .opt_num(refresh_indicator.as_ref().map(|h| h.0))
                    .bool(*paging)
                    .opt_num(*snap_interval);
                out.s.push(',');
                if let Some(offsets) = snap_offsets {
                    out.s.push('[');
                    for (i, off) in offsets.iter().enumerate() {
                        if i > 0 {
                            out.s.push(',');
                        }
                        write!(out.s, "{}", off).unwrap();
                    }
                    out.s.push(']');
                } else {
                    out.s.push_str("null");
                }
                out.num(*snap_alignment)
                    .bool(*disable_interval_momentum)
                    .num(*deceleration)
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
                    ",\"disabled\":{},\"rtl\":{},\"multiline\":{}",
                    if config.disabled { "true" } else { "false" },
                    if config.rtl { "true" } else { "false" },
                    if config.multiline { "true" } else { "false" }
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
            MountItem::Command { id, command } => match command {
                Command::ScrollTo { x, y, animated } => {
                    out.item("cmd")
                        .num(*id)
                        .str("scrollTo")
                        .num(x.0)
                        .num(y.0)
                        .bool(*animated)
                        .end();
                }
                Command::Blur => {
                    out.item("cmd").num(*id).str("blur").end();
                }
                Command::SetSelection { start, end } => {
                    out.item("cmd")
                        .num(*id)
                        .str("setSelection")
                        .num(*start)
                        .num(*end)
                        .end();
                }
                Command::ScrollToIndex { index, animated } => {
                    out.item("cmd")
                        .num(*id)
                        .str("scrollToIndex")
                        .num(*index)
                        .bool(*animated)
                        .end();
                }
                Command::ScrollToEnd { animated } => {
                    out.item("cmd")
                        .num(*id)
                        .str("scrollToEnd")
                        .bool(*animated)
                        .end();
                }
                Command::Back | Command::GoBack => {
                    // Back routing handled by engine/page layer
                }
                Command::OpenUrl { url } => {
                    out.item("cmd").num(*id).str("openUrl").str(url).end();
                }
            },
            MountItem::Selectable { id, selectable } => {
                out.item("sel").num(*id).bool(*selectable).end();
            }
            MountItem::ScreenConfig { id, config } => {
                out.item("scr").num(*id);
                out.s.push_str(",{");
                out.s.push_str("\"headerHeight\":");
                push_num(&mut out.s, config.header_height.0);
                out.s.push_str(",\"headerVisible\":");
                out.s.push_str(if config.header_visible {
                    "true"
                } else {
                    "false"
                });
                out.s.push_str(",\"headerTransparent\":");
                out.s.push_str(if config.header_transparent {
                    "true"
                } else {
                    "false"
                });
                out.s.push_str(",\"preventBack\":");
                out.s
                    .push_str(if config.prevent_back { "true" } else { "false" });
                out.s.push_str(",\"gestureEnabled\":");
                out.s.push_str(if config.gesture_enabled {
                    "true"
                } else {
                    "false"
                });
                out.s.push_str(",\"backVisible\":");
                out.s
                    .push_str(if config.back_visible { "true" } else { "false" });
                out.s.push_str(",\"animation\":");
                push_str(
                    &mut out.s,
                    match config.animation {
                        nlayer_core::ScreenAnimation::Default => "default",
                        nlayer_core::ScreenAnimation::Fade => "fade",
                        nlayer_core::ScreenAnimation::None => "none",
                    },
                );
                if let Some(title) = &config.title {
                    out.s.push_str(",\"title\":");
                    push_str(&mut out.s, title);
                }
                if let Some(color) = config.tint_color {
                    out.s.push_str(",\"tintColor\":");
                    push_str(&mut out.s, &css_color(color));
                }
                if let Some(color) = config.background_color {
                    out.s.push_str(",\"backgroundColor\":");
                    push_str(&mut out.s, &css_color(color));
                }
                if let Some(color) = config.title_color {
                    out.s.push_str(",\"titleColor\":");
                    push_str(&mut out.s, &css_color(color));
                }
                if let Some(node) = config.left {
                    out.s.push_str(",\"left\":");
                    push_num(&mut out.s, node);
                }
                if let Some(node) = config.right {
                    out.s.push_str(",\"right\":");
                    push_num(&mut out.s, node);
                }
                if let Some(node) = config.title_view {
                    out.s.push_str(",\"titleView\":");
                    push_num(&mut out.s, node);
                }
                if let Some(node) = config.header_view {
                    out.s.push_str(",\"headerView\":");
                    push_num(&mut out.s, node);
                }
                out.s.push('}');
                out.end();
            }
            MountItem::ModalConfig { id, config } => {
                out.item("mc").num(*id);
                out.s.push_str(",{");
                out.s.push_str("\"animation\":");
                push_str(
                    &mut out.s,
                    match config.animation {
                        nlayer_core::ModalAnimation::Slide => "slide",
                        nlayer_core::ModalAnimation::Fade => "fade",
                        nlayer_core::ModalAnimation::None => "none",
                    },
                );
                out.s.push_str(",\"presentation\":");
                push_str(
                    &mut out.s,
                    match config.presentation {
                        nlayer_core::ModalPresentation::FullScreen => "fullscreen",
                        nlayer_core::ModalPresentation::PageSheet => "pagesheet",
                        nlayer_core::ModalPresentation::FormSheet => "formsheet",
                    },
                );
                out.s.push_str(",\"closeOnEscape\":");
                out.s.push_str(if config.close_on_escape {
                    "true"
                } else {
                    "false"
                });
                out.s.push_str(",\"closeOnBackdropPress\":");
                out.s.push_str(if config.close_on_backdrop_press {
                    "true"
                } else {
                    "false"
                });
                out.s.push_str(",\"closeOnSwipe\":");
                out.s.push_str(if config.close_on_swipe {
                    "true"
                } else {
                    "false"
                });
                out.s.push('}');
                out.end();
            }
            MountItem::ListContent {
                id,
                sticky_indices,
                inverted,
                viewport_parent,
                viewability,
                maintain_position,
            } => {
                out.item("lc").num(*id);
                out.s.push_str(",{");
                out.s.push_str("\"stickyIndices\":[");
                for (i, idx) in sticky_indices.iter().enumerate() {
                    if i > 0 {
                        out.s.push(',');
                    }
                    write!(out.s, "{}", idx).unwrap();
                }
                out.s.push_str("],\"inverted\":");
                out.s.push_str(if *inverted { "true" } else { "false" });
                out.s.push_str(",\"viewportParent\":");
                out.s
                    .push_str(if *viewport_parent { "true" } else { "false" });
                if let Some(v) = viewability {
                    out.s.push_str(",\"viewability\":{");
                    if let Some(coverage) = v.view_area_coverage_threshold {
                        out.s.push_str("\"viewAreaCoverage\":");
                        write!(out.s, "{}", coverage).unwrap();
                    }
                    if let Some(item_visible) = v.item_visible_threshold {
                        if v.view_area_coverage_threshold.is_some() {
                            out.s.push(',');
                        }
                        out.s.push_str("\"itemVisible\":");
                        write!(out.s, "{}", item_visible).unwrap();
                    }
                    out.s.push('}');
                }
                if let Some(m) = maintain_position {
                    out.s.push_str(",\"maintainPosition\":{\"minIndex\":");
                    write!(out.s, "{}", m.min_index_for_visible).unwrap();
                    if let Some(threshold) = m.autoscroll_to_bottom_threshold {
                        out.s.push_str(",\"autoscrollThreshold\":");
                        write!(out.s, "{}", threshold).unwrap();
                    }
                    out.s.push('}');
                }
                out.s.push('}');
                out.end();
            }
            MountItem::TestId { id, test_id } => {
                out.item("tid").num(*id).opt_str(test_id.as_deref()).end();
            }
            MountItem::Accessibility { id, accessibility } => {
                let a = accessibility.as_ref();
                let checked = match a.state.checked {
                    None => 0,
                    Some(nlayer_core::Checked::False) => 1,
                    Some(nlayer_core::Checked::True) => 2,
                    Some(nlayer_core::Checked::Mixed) => 3,
                };
                let expanded = match a.state.expanded {
                    None => 0,
                    Some(false) => 1,
                    Some(true) => 2,
                };
                let value = a.value.as_ref();
                out.item("a")
                    .num(*id)
                    .str(a.role.name())
                    .opt_str(a.label.as_deref())
                    .opt_str(a.hint.as_deref())
                    .bool(a.hidden)
                    .bool(a.state.disabled)
                    .bool(a.state.selected)
                    .num(checked)
                    .num(expanded)
                    .opt_num(value.and_then(|v| v.min))
                    .opt_num(value.and_then(|v| v.max))
                    .opt_num(value.and_then(|v| v.now))
                    .opt_str(value.and_then(|v| v.text.as_deref()))
                    .end();
            }
            MountItem::PanBinding { id, x, y } => {
                out.item("pb").num(*id);
                match x {
                    Some(node) => out.num(*node),
                    None => out.raw("null"),
                };
                match y {
                    Some(node) => out.num(*node),
                    None => out.raw("null"),
                };
                out.end();
            }
            MountItem::PinchBinding { id, scale } => {
                out.item("pinchb").num(*id);
                match scale {
                    Some(node) => out.num(*node),
                    None => out.raw("null"),
                };
                out.end();
            }
            MountItem::RotationBinding { id, angle } => {
                out.item("rotb").num(*id);
                match angle {
                    Some(node) => out.num(*node),
                    None => out.raw("null"),
                };
                out.end();
            }
            MountItem::StateStyle {
                id,
                interaction,
                view,
                text,
            } => self.state(out, *id, *interaction, view, text),
            MountItem::TrackStates { id, states } => {
                out.item("ts").num(*id).num(*states).end();
            }
            MountItem::Refreshing { id, refreshing } => {
                out.item("ref").num(*id).bool(*refreshing).end();
            }
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

    fn remember(&mut self, id: NodeId, kind: ViewKind) {
        let i = id as usize;
        if self.kinds.len() <= i {
            self.kinds.resize(i + 1, None);
        }
        self.kinds[i] = Some(kind);
    }

    /// A scroll, including a list that scrolls itself, clips its own shadow.
    fn is_scroll(&self, id: NodeId) -> bool {
        self.kinds.get(id as usize).copied().flatten() == Some(ViewKind::Scroll)
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
        let (view_css, border, _) = view_css(view, self.is_scroll(id));
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
        rule(
            out,
            &name,
            weight,
            pseudo,
            " > :is(.nl-field, .nl-layer > .nl-field)",
            &text_css,
        );
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
    Layer,
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
        self.s.push(',');
        push_num(&mut self.s, n);
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

    fn opt_num(&mut self, n: Option<f32>) -> &mut Self {
        match n {
            Some(n) => self.num(n),
            None => self.raw("null"),
        }
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
            Class::Own(id, Part::Layer) => write!(self.s, "\"i{id}l\""),
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

fn push_num(out: &mut String, n: impl Into<f64>) {
    let n = n.into();
    if n.is_finite() {
        let r = (n * 1000.0).round() / 1000.0;
        let _ = write!(out, "{r}");
    } else {
        out.push_str("null");
    }
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
    const KINDS: [EventKind; 9] = [
        EventKind::Press,
        EventKind::Scroll,
        EventKind::ScrollEnd,
        EventKind::ScrollDragStart,
        EventKind::ScrollDragEnd,
        EventKind::MomentumStart,
        EventKind::MomentumEnd,
        EventKind::Change,
        EventKind::LongPress,
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
        ViewKind::Stack => "stack",
        ViewKind::Screen => "screen",
        ViewKind::Modal => "modal",
        ViewKind::HeaderItem => "headeritem",
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

/// Whether clipping moves to a content layer so a visible outer shadow is
/// not cut by the view's own overflow. A scroll never splits: it clips
/// itself, shadow included.
fn content_layer(s: &ViewStyle) -> bool {
    s.overflow_hidden
        && s.effects.as_ref().is_some_and(|effects| {
            effects
                .box_shadows
                .iter()
                .any(|shadow| !shadow.inset && shadow.is_visible())
        })
}

/// `box-shadow` text, top layer first. An empty list emits nothing (`none`
/// would clear a base shadow a state slot must leave alone).
fn shadow_css(layers: &[BoxShadow]) -> Option<String> {
    if layers.is_empty() {
        return None;
    }
    Some(
        layers
            .iter()
            .map(|b| {
                format!(
                    "{}{}px {}px {}px {}px {}",
                    if b.inset { "inset " } else { "" },
                    b.offset_x.0,
                    b.offset_y.0,
                    b.blur.0,
                    b.spread.0,
                    css_color(b.color)
                )
            })
            .collect::<Vec<_>>()
            .join(","),
    )
}

/// The CSS of `g`. Without lengths, the stops the core resolves, so the
/// browser places them as every platform does (a negative stop at 0);
/// with lengths, the stops as written (lengths in pixels), which only the
/// box resolves.
fn gradient(g: &Gradient) -> String {
    let stops: Vec<String> = if g.has_lengths() {
        g.stops
            .iter()
            .map(|s| match s.position {
                Some(DefiniteLength::Fraction(p)) => {
                    format!("{} {}%", css_color(s.color), p.0 * 100.0)
                }
                Some(DefiniteLength::Absolute(a)) => {
                    format!("{} {}px", css_color(s.color), a.to_points(REM_SIZE).0)
                }
                None => css_color(s.color),
            })
            .collect()
    } else {
        g.resolved_stops(Size::default())
            .iter()
            .map(|(c, p)| format!("{} {}%", css_color(*c), p.0 * 100.0))
            .collect()
    };
    let stops = stops.join(", ");
    let repeating = if g.repeating { "repeating-" } else { "" };
    match g.kind {
        GradientKind::Linear { angle } => {
            format!("{repeating}linear-gradient({}deg, {stops})", degrees(angle))
        }
        GradientKind::LinearCorner { right, bottom } => format!(
            "{repeating}linear-gradient(to {} {}, {stops})",
            if bottom { "bottom" } else { "top" },
            if right { "right" } else { "left" }
        ),
        GradientKind::Radial { circle, size, x, y } => {
            let size = match size {
                GradientSize::ClosestSide => "closest-side",
                GradientSize::ClosestCorner => "closest-corner",
                GradientSize::FarthestSide => "farthest-side",
                GradientSize::FarthestCorner => "farthest-corner",
            };
            format!(
                "{repeating}radial-gradient({} {size} at {}% {}%, {stops})",
                if circle { "circle" } else { "ellipse" },
                x.0 * 100.0,
                y.0 * 100.0
            )
        }
        GradientKind::Conic { from, x, y } => format!(
            "{repeating}conic-gradient(from {}deg at {}% {}%, {stops})",
            degrees(from),
            x.0 * 100.0,
            y.0 * 100.0
        ),
    }
}

/// An angle in CSS degrees, rounded to 1e-4 so that whole degrees are
/// written as the style wrote them (radians to degrees in `f32` can be an
/// ulp off).
fn degrees(a: Radians) -> f32 {
    (a.to_degrees() * 1e4).round() / 1e4
}

/// The CSS of a view's visual style, of its border overlay (drawn over
/// the children, as UIKit draws layer borders; also keeps borders out of
/// the content box, which layout already placed), and of its content layer
/// when an outer shadow must not be clipped. Filters, masks, outlines and
/// text shadows are not drawn.
fn view_css(s: &ViewStyle, scroll: bool) -> (String, Option<String>, Option<String>) {
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
    // the browser scrolls to bring a focused field into view. A visible
    // outer shadow moves that clip onto a content layer so the shadow stays
    // outside the box. A scroll always clips itself.
    let split = !scroll && content_layer(s);
    if s.overflow_hidden && !split {
        css.push_str("overflow:clip;");
    }
    if let Some(effects) = &s.effects {
        if !effects.background_images.is_empty() {
            let images: Vec<String> = effects.background_images.iter().map(gradient).collect();
            let _ = write!(css, "background-image:{};", images.join(","));
        }
    }
    if let Some(shadows) = s
        .effects
        .as_ref()
        .and_then(|effects| shadow_css(&effects.box_shadows))
    {
        let _ = write!(css, "box-shadow:{shadows};");
    }
    // Laid out, not drawn, and out of the page's hit testing.
    if s.hidden {
        css.push_str("visibility:hidden;");
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
    let layer = split.then(|| "overflow:clip;".to_owned());
    (css, border, layer)
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
            TransformKind::Perspective => write!(out, "perspective({v}px) "),
        };
    }
    out.pop();
    out
}

/// Static transform steps for the test tree. CSS still paints the transform.
fn push_transform(out: &mut Out, id: NodeId, transform: &Transform) {
    out.item("xf").num(id);
    let mut steps = String::from("[");
    for (i, op) in transform.ops().iter().enumerate() {
        if i > 0 {
            steps.push(',');
        }
        let value = (f64::from(op.value) * 1000.0).round() / 1000.0;
        let _ = write!(steps, "{{\"{}\":{value}}}", transform_name(op.kind));
    }
    steps.push(']');
    out.raw(&steps).end();
}

fn animation_prop_name(prop: AnimationProp) -> String {
    match prop {
        AnimationProp::Opacity => "opacity".to_owned(),
        AnimationProp::BackgroundColor => "backgroundColor".to_owned(),
        AnimationProp::BorderColor => "borderColor".to_owned(),
        AnimationProp::Transform(i) => format!("transform:{i}"),
    }
}

fn transform_name(kind: TransformKind) -> &'static str {
    match kind {
        TransformKind::TranslateX | TransformKind::TranslateXFraction => "translateX",
        TransformKind::TranslateY | TransformKind::TranslateYFraction => "translateY",
        TransformKind::Scale => "scale",
        TransformKind::ScaleX => "scaleX",
        TransformKind::ScaleY => "scaleY",
        TransformKind::Rotate => "rotate",
        TransformKind::RotateX => "rotateX",
        TransformKind::RotateY => "rotateY",
        TransformKind::Perspective => "perspective",
    }
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

/// The CSS `font` shorthand. The family is the system stack, or the one
/// the style names in front of it. `font` resets `line-height`.
fn css_font(style: &ResolvedTextStyle) -> String {
    crate::text::css_font(style)
}

/// An explicit `lineHeight`, otherwise `1.2` times the font size. Fields
/// use this; text runs use the font's own ascent and descent instead.
fn css_line_height(style: &ResolvedTextStyle) -> String {
    if style.line_height.0 > 0.0 {
        format!("line-height:{}px;", style.line_height.0)
    } else {
        "line-height:1.2;".to_owned()
    }
}

/// The CSS of one laid-out run. The line box is the font's metrics, not
/// the styled line height: leading is already in the run's position.
fn run_css(style: &crate::text::RunStyle) -> String {
    let s = &style.style;
    let mut css = format!(
        "font:{};line-height:{}px;color:{};",
        css_font(s),
        style.ascent + style.descent,
        css_color(s.color)
    );
    if s.letter_spacing.0 != 0.0 {
        let _ = write!(css, "letter-spacing:{}px;", s.letter_spacing.0);
    }
    let d = s.decoration;
    if !d.is_none() {
        let lines = match (d.underline, d.line_through) {
            (true, true) => "underline line-through",
            (true, false) => "underline",
            _ => "line-through",
        };
        let _ = write!(
            css,
            "text-decoration:{lines} {};",
            decoration_css(s.decoration_style)
        );
        if let Some(c) = s.decoration_color {
            let _ = write!(css, "text-decoration-color:{};", css_color(c));
        }
        if let Some(t) = s.decoration_thickness {
            let _ = write!(css, "text-decoration-thickness:{}px;", t.0);
        }
        if let Some(o) = s.underline_offset {
            let _ = write!(css, "text-underline-offset:{}px;", o.0);
        }
    }
    if let Some(numeric) = crate::text::font_variant_numeric(s.font_variant) {
        let _ = write!(css, "font-variant-numeric:{numeric};");
    }
    if s.font_variant
        .has(nlayer_core::style::FontVariant::SMALL_CAPS)
    {
        css.push_str("font-variant-caps:small-caps;");
    }
    css
}

fn decoration_css(style: DecorationStyle) -> &'static str {
    match style {
        DecorationStyle::Solid => "solid",
        DecorationStyle::Double => "double",
        DecorationStyle::Dotted => "dotted",
        DecorationStyle::Dashed => "dashed",
        DecorationStyle::Wavy => "wavy",
    }
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
