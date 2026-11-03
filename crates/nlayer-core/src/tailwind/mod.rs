//! Tailwind CSS classes (`class="flex-row gap-4 p-4 bg-white dark:bg-zinc-900"`),
//! resolved in Rust: each distinct class string is parsed once into rules
//! (cached), and applied to a node's style whenever its variants may have
//! changed (appearance, window width, press / hover / focus, group
//! states). JS only passes the string.
//!
//! Utilities later in the string win over earlier ones, so components can
//! take a `class` prop and append it (no `tailwind-merge`). Variant
//! utilities apply after plain ones, ordered by Tailwind's variant order
//! (`hover:` before `active:`, breakpoints ascending, `dark:` last).
//!
//! Each class string is parsed once into rules, cached in an LRU of up to
//! 4096 strings, with what the rules depend on (appearance, window width and
//! height, direction, the node's own states, a group's states, sibling
//! position, reduce motion, the `data-*` attributes it or a group shows): the core
//! (`core/classes.rs`) resolves a node again only when one of those changes. Interaction states are reported by the platforms
//! only for the bits a node's classes read (`MountItem::TrackStates`), and
//! `focus-within` propagates to ancestors in the core. A node's style is the
//! default, then its class rules, then its inline `style` replayed on top
//! (`Op::SetStyle` carries a `StylePatch` besides the merged `Style`). The
//! theme is Tailwind 4's default (the palette converted from OKLCH to sRGB).
//! A class the hosts do not draw still parses; the effect gate drops the rule.

pub(crate) mod palette;
mod theme;
mod utilities;

use rustc_hash::FxHashMap;

pub use self::theme::Theme;
pub(crate) use self::utilities::{Decl, Part};
use self::utilities::{Parts, touched};
use crate::{
    geometry::{Points, Size},
    style::{Color, EllipsizeMode, PropertyValue, Style, TextShadow},
};

/// Interaction states a class can depend on (`active:`, `hover:`, ...).
pub mod state {
    /// The view is pressed (`active:`).
    pub const PRESSED: u16 = 1;
    /// A pointer is over the view (`hover:`).
    pub const HOVERED: u16 = 2;
    /// The input has focus (`focus:`).
    pub const FOCUSED: u16 = 4;
    /// The view is disabled (`disabled:`).
    pub const DISABLED: u16 = 8;
    /// The view or a descendant has focus (`focus-within:`).
    pub const FOCUS_WITHIN: u16 = 16;
    /// The view is selected (`selected:`): its
    /// `accessibilityState.selected`, a `Button`'s `selected`. Set by the
    /// core, never reported by the platform.
    pub const SELECTED: u16 = 32;
    /// The view is checked (`checked:`): its `accessibilityState.checked`
    /// (a checkbox, a switch, a radio; not when mixed). Set by the core,
    /// never reported by the platform.
    pub const CHECKED: u16 = 64;
    /// The view is indeterminate (`indeterminate:`): its
    /// `accessibilityState.checked` is mixed (a checkbox that is neither
    /// checked nor unchecked, as CSS's `:indeterminate`). Set by the core,
    /// never reported by the platform.
    pub const INDETERMINATE: u16 = 128;
    /// The view's popup, menu or section is open (`aria-expanded:`): its
    /// `accessibilityState.expanded`, which `Popover` and `Menu` set on
    /// their trigger. Set by the core, never reported by the platform.
    pub const EXPANDED: u16 = 256;
    /// The view is a toggle button that is on (`aria-pressed:`): its
    /// `accessibilityState.pressed` is `true` (not mixed). Not
    /// [`PRESSED`], a press in progress (`active:`). Set by the core,
    /// never reported by the platform.
    pub const ARIA_PRESSED: u16 = 512;
    /// The view is busy (`aria-busy:`): its `accessibilityState.busy`.
    /// Set by the core, never reported by the platform.
    pub const BUSY: u16 = 1024;
    /// The states the core sets from the accessibility state, which the
    /// platforms never report.
    pub(crate) const ACCESSIBILITY: u16 =
        SELECTED | CHECKED | INDETERMINATE | EXPANDED | ARIA_PRESSED | BUSY;
}

/// The view's place among its siblings (`first:`, `odd:`, ...).
pub mod position {
    /// The first child.
    pub const FIRST: u8 = 1;
    /// The last child.
    pub const LAST: u8 = 2;
    /// An odd child (1st, 3rd, ...).
    pub const ODD: u8 = 4;
    /// An even child.
    pub const EVEN: u8 = 8;
    /// The only child.
    pub const ONLY: u8 = 16;
}

/// What resolved classes depend on, so the core knows when to resolve
/// them again.
pub mod deps {
    /// Light or dark appearance.
    pub const DARK: u16 = 1;
    /// The window width (breakpoints, `w-screen`).
    pub const WIDTH: u16 = 2;
    /// The window's orientation or height (`landscape:`, `h-screen`).
    pub const HEIGHT: u16 = 4;
    /// The layout direction.
    pub const RTL: u16 = 8;
    /// The view's own interaction states.
    pub const STATE: u16 = 16;
    /// The states of an ancestor `group`.
    pub const GROUP: u16 = 32;
    /// The position among siblings.
    pub const POSITION: u16 = 64;
    /// The reduce-motion setting.
    pub const MOTION: u16 = 128;
    /// The component theme's tokens (`bg-primary`): parsed again when they
    /// change.
    pub const THEME: u16 = 256;
    /// The text color it inherits (CSS's `currentColor`: a border, an
    /// outline or a ring without a color of its own).
    pub const CURRENT_COLOR: u16 = 512;
    /// The advance of its font's `0` (CSS's `ch`: `max-w-prose`).
    pub const CH: u16 = 1024;
    /// The view's own `data-*` attributes (`data-range-start:`); a group's
    /// are [`GROUP`].
    pub const DATA: u16 = 2048;
}

/// CSS's `ch` where no font is known ([`ClassList::apply`]): half a `rem`,
/// so `max-w-prose` is 520 points there.
pub(crate) const DEFAULT_CH: f32 = 8.0;

/// What CSS's `currentColor` is for a node: its own text color (its
/// inline style's, else its classes'), else the one it inherits, else
/// black, React Native's default.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CurrentColor {
    /// The text color the node inherits.
    pub(crate) inherited: Option<Color>,
    /// The color its inline style sets.
    pub(crate) inline: Option<Color>,
}

/// The platform classes run on (`ios:`, `android:`, `web:`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Platform {
    /// iOS and Mac Catalyst.
    #[default]
    Ios,
    /// Android.
    Android,
    /// A web browser.
    Web,
}

/// The environment variants depend on, set by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ClassEnvironment {
    /// Dark appearance.
    pub dark: bool,
    /// The platform.
    pub platform: Platform,
    /// The user asked for less motion.
    pub reduce_motion: bool,
}

/// What a node's classes are matched against: the core builds it per
/// node; tools resolve classes outside a tree with one of their own.
pub struct Context<'a> {
    /// The appearance, platform and motion setting.
    pub env: ClassEnvironment,
    /// The window (root view) size.
    pub window: Size<Points>,
    /// Right-to-left layout.
    pub rtl: bool,
    /// [`state`] bits of the node.
    pub states: u16,
    /// [`position`] bits of the node.
    pub position: u8,
    /// [`state`] bits of the nearest ancestor group with this name (`""`
    /// for unnamed groups).
    pub group: &'a dyn Fn(&str) -> u16,
}

/// Whether the node (`None`) or the nearest ancestor group with this name
/// (`Some`, `""` for unnamed groups) has the boolean `data-*` attribute
/// named (without `data-`): `data-range-start:`, `group-data-range-start:`.
/// Components set them on their parts (`Interactive::project_data`); only
/// the core has them, so they stay out of the public [`Context`].
pub(crate) type Data<'a> = &'a dyn Fn(Option<&str>, &str) -> bool;

/// No `data-*` attribute: what classes resolved outside a tree see.
pub(crate) fn no_data(_: Option<&str>, _: &str) -> bool {
    false
}

/// Conditions of one rule (all must hold).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Condition {
    dark: Option<bool>,
    min_width: Option<f32>,
    max_width: Option<f32>,
    landscape: Option<bool>,
    platform: Option<Platform>,
    rtl: Option<bool>,
    reduce_motion: Option<bool>,
    /// Required [`state`] bits.
    states: u16,
    /// [`state`] bits that must be off (`enabled:`).
    not_states: u16,
    /// A group name and its required states.
    group: Option<(Box<str>, u16)>,
    /// `data-*` attributes the node must have (`data-<name>:`), by name.
    data: Vec<Box<str>>,
    /// `data-*` attributes the group of [`Condition::group`] must have
    /// (`group-data-<name>:`).
    group_data: Vec<Box<str>>,
    /// Allowed [`position`] bits (any of them).
    position: u8,
    /// Applies to an input's placeholder.
    placeholder: bool,
    /// Applies to the selection (and caret) of an input or selectable text.
    selection: bool,
}

impl Condition {
    fn matches(&self, cx: &Context<'_>, data: Data<'_>) -> bool {
        if self.dark.is_some_and(|d| d != cx.env.dark)
            || self.platform.is_some_and(|p| p != cx.env.platform)
            || self.rtl.is_some_and(|r| r != cx.rtl)
            || self
                .reduce_motion
                .is_some_and(|r| r != cx.env.reduce_motion)
            || self.min_width.is_some_and(|w| cx.window.width.0 < w)
            || self.max_width.is_some_and(|w| cx.window.width.0 >= w)
            || self
                .landscape
                .is_some_and(|l| l != (cx.window.width > cx.window.height))
            || cx.states & self.states != self.states
            || cx.states & self.not_states != 0
        {
            return false;
        }
        if self.position != 0 && cx.position & self.position == 0 {
            return false;
        }
        if !self.data.iter().all(|d| data(None, d)) {
            return false;
        }
        if let Some((name, states)) = &self.group
            && ((cx.group)(name) & states != *states
                || !self.group_data.iter().all(|d| data(Some(name), d)))
        {
            return false;
        }
        true
    }

    /// Applies to a pseudo-element (`placeholder:`, `selection:`): only
    /// its color is read.
    fn pseudo(&self) -> bool {
        self.placeholder || self.selection
    }

    /// The color a pseudo-element rule's `decls` give it: the placeholder's
    /// text color (`placeholder:text-*`), the selection's background, its
    /// highlight (`selection:bg-*`). `None`: the rule sets something else,
    /// which NSH has no native counterpart for.
    fn pseudo_color(&self, decls: &[Decl]) -> Option<Color> {
        match decls {
            [decl] if self.placeholder => decl.text_color(),
            [decl] if self.selection => decl.background_color(),
            _ => None,
        }
    }

    /// Applies only while the view (or its group) is pressed, hovered or
    /// focused: press feedback.
    fn interactive(&self) -> bool {
        const INTERACTION: u16 =
            state::PRESSED | state::HOVERED | state::FOCUSED | state::FOCUS_WITHIN;
        self.states & INTERACTION != 0
            || self
                .group
                .as_ref()
                .is_some_and(|(_, states)| states & INTERACTION != 0)
    }

    fn deps(&self) -> u16 {
        let mut d = 0;
        if self.dark.is_some() {
            d |= deps::DARK;
        }
        if self.min_width.is_some() || self.max_width.is_some() {
            d |= deps::WIDTH;
        }
        if self.landscape.is_some() {
            d |= deps::WIDTH | deps::HEIGHT;
        }
        if self.rtl.is_some() {
            d |= deps::RTL;
        }
        if self.reduce_motion.is_some() {
            d |= deps::MOTION;
        }
        if self.states != 0 || self.not_states != 0 {
            d |= deps::STATE;
        }
        if self.group.is_some() {
            d |= deps::GROUP;
        }
        if !self.data.is_empty() {
            d |= deps::DATA;
        }
        if self.position != 0 {
            d |= deps::POSITION;
        }
        d
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rule {
    cond: Condition,
    decls: Vec<Decl>,
    /// Variant order: plain utilities first, then by Tailwind's order.
    rank: Rank,
    /// `!`: applied after everything else.
    important: bool,
    /// Names theme tokens ([`deps::THEME`]).
    theme: bool,
}

/// One variant's place in Tailwind's variant order (Tailwind sorts the
/// variants it registers, the compound ones by their inner variant and
/// then by their modifier, the breakpoints by unit and then by value).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key {
    /// The variant's (or for `group-*` the group's) place.
    order: u16,
    /// A breakpoint's unit (its index in [`BREAKPOINT_UNITS`]).
    unit: u8,
    /// A breakpoint's value (thousandths, negated for `max-*`), or a
    /// group's inner variant.
    value: i64,
    /// A group's name (`group-hover/item`), `None` unnamed; a `data-*`
    /// attribute's ([`Key::data`]).
    name: Option<Box<str>>,
}

impl Key {
    const fn new(order: u16) -> Self {
        Self {
            order,
            unit: 0,
            value: 0,
            name: None,
        }
    }

    /// `data-<name>:`'s place: [`rank::DATA`], then by name.
    fn data(name: &str) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::new(rank::DATA)
        }
    }
}

/// Breakpoint units as Tailwind orders them: by their name.
const BREAKPOINT_UNITS: [&str; 4] = ["", "pt", "px", "rem"];

/// Where a rule's variants put it: Tailwind sorts rules by the bit set of
/// their variants' places, so the highest variant decides, then the next
/// highest, and a rule with one more variant comes later. Plain
/// utilities have none and come first.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Rank(Vec<Key>);

impl Rank {
    /// Adds `key`, keeping the keys highest first.
    pub(crate) fn add(&mut self, key: Key) {
        if let Err(i) = self.0.binary_search_by(|k| key.cmp(k)) {
            self.0.insert(i, key);
        }
    }

    /// The group variant of `inner` (a [`rank`] key) on group `name`.
    pub(crate) fn add_group(&mut self, inner: &Key, name: &str) {
        self.add(Key {
            order: rank::GROUP,
            unit: 0,
            value: inner.order.into(),
            name: (!name.is_empty()).then(|| name.into()),
        });
    }
}

impl From<Key> for Rank {
    fn from(key: Key) -> Self {
        Rank(vec![key])
    }
}

/// The places of the variants (Tailwind's order, as [`variant`] gives
/// them).
pub(crate) mod rank {
    use super::Key;

    /// `group-*`: before the pseudo-elements and `first:` ... `even:`.
    pub(crate) const GROUP: u16 = 1;
    pub(crate) const SELECTION: Key = Key::new(10);
    pub(crate) const PLACEHOLDER: Key = Key::new(11);
    pub(crate) const FIRST: Key = Key::new(20);
    pub(crate) const LAST: Key = Key::new(21);
    pub(crate) const ONLY: Key = Key::new(22);
    pub(crate) const ODD: Key = Key::new(23);
    pub(crate) const EVEN: Key = Key::new(24);
    pub(crate) const CHECKED: Key = Key::new(30);
    pub(crate) const INDETERMINATE: Key = Key::new(31);
    /// NSH's (Tailwind has `aria-selected:`).
    pub(crate) const SELECTED: Key = Key::new(32);
    pub(crate) const FOCUS_WITHIN: Key = Key::new(33);
    pub(crate) const HOVER: Key = Key::new(34);
    pub(crate) const FOCUS: Key = Key::new(35);
    pub(crate) const FOCUS_VISIBLE: Key = Key::new(36);
    pub(crate) const ACTIVE: Key = Key::new(37);
    pub(crate) const ENABLED: Key = Key::new(38);
    pub(crate) const DISABLED: Key = Key::new(39);
    /// Tailwind's `aria-*` variants come after the pseudo-classes, in the
    /// order of their names.
    pub(crate) const ARIA_BUSY: Key = Key::new(40);
    pub(crate) const ARIA_CHECKED: Key = Key::new(41);
    pub(crate) const EXPANDED: Key = Key::new(42);
    pub(crate) const ARIA_PRESSED: Key = Key::new(43);
    pub(crate) const ARIA_SELECTED: Key = Key::new(44);
    /// `data-*`, after `aria-*` as Tailwind registers them, in the order
    /// of their names ([`Key::data`]).
    pub(crate) const DATA: u16 = 45;
    pub(crate) const MOTION_SAFE: Key = Key::new(46);
    pub(crate) const MOTION_REDUCE: Key = Key::new(47);
    /// `max-*` breakpoints, widest first.
    pub(crate) const MAX: u16 = 50;
    /// Breakpoints and `min-*`, narrowest first.
    pub(crate) const MIN: u16 = 51;
    pub(crate) const PORTRAIT: Key = Key::new(60);
    pub(crate) const LANDSCAPE: Key = Key::new(61);
    pub(crate) const LTR: Key = Key::new(62);
    pub(crate) const RTL: Key = Key::new(63);
    pub(crate) const DARK: Key = Key::new(64);
    /// NSH's platform variants, after Tailwind's.
    pub(crate) const IOS: Key = Key::new(70);
    pub(crate) const ANDROID: Key = Key::new(71);
    pub(crate) const WEB: Key = Key::new(72);
}

impl Rule {
    /// A rule applying `decls` while the node is in all of [`state`]
    /// `states`, ordered by `rank`.
    pub(crate) fn new(states: u16, rank: impl Into<Rank>, decls: Vec<Decl>) -> Self {
        Self {
            cond: Condition {
                states,
                ..Condition::default()
            },
            decls,
            rank: rank.into(),
            important: false,
            theme: false,
        }
    }

    /// A state style (`activeStyle`, ...): `decls` while the node is in
    /// `states` (and, unless `disabled`, not disabled), applied over the
    /// inline style as `!` utilities are, ordered by `rank`.
    pub(crate) fn state_style(
        states: u16,
        rank: impl Into<Rank>,
        decls: Vec<crate::style::StyleDecl>,
    ) -> Self {
        // A style's defaults and its `currentColor` are the classes' own
        // kinds, applied before the rules and read with the text color.
        let decls = decls
            .into_iter()
            .map(|d| match d {
                crate::style::StyleDecl::Default(key, value) => Decl::Default(key, value),
                crate::style::StyleDecl::CurrentColor(key) => Decl::CurrentColor(key),
                d => Decl::Style(d),
            })
            .collect();
        let mut rule = Self::new(states, rank, decls);
        if states & state::DISABLED == 0 {
            rule.cond.not_states = state::DISABLED;
        }
        rule.important = true;
        rule
    }

    /// [`Rule::new`], also requiring the nearest ancestor group `group`
    /// (`""`: unnamed) in its states, and dark appearance when `dark`.
    pub(crate) fn with(
        states: u16,
        group: Option<(&str, u16)>,
        dark: bool,
        rank: Rank,
        decls: Vec<Decl>,
    ) -> Self {
        let mut rule = Self::new(states, rank, decls);
        rule.cond.group = group.map(|(name, states)| (name.into(), states));
        rule.cond.dark = dark.then_some(true);
        rule
    }
}

/// Node properties classes and styles set besides layout and paint.
/// The core reads `numberOfLines`, `textOverflow` and `userSelect` from
/// the style; classes fill the rest.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClassProps {
    /// `numberOfLines` (`line-clamp-*`, `truncate`); `Some(0)` removes a clamp.
    pub number_of_lines: Option<u32>,
    /// `textOverflow` (`text-ellipsis`, `text-clip`, `truncate`).
    pub ellipsize: Option<EllipsizeMode>,
    /// `whitespace-nowrap`, `text-nowrap`, `truncate`: lines never wrap.
    /// Inherited, as CSS's `white-space`: a view's reaches the texts in it.
    pub no_wrap: Option<bool>,
    /// `userSelect` (`select-none` / `select-text`; `auto`: none). A
    /// view's reach the texts in it that say none, as CSS's `user-select:
    /// auto` takes its parent's.
    pub selectable: Option<bool>,
    /// `placeholder:text-*` (inputs).
    pub placeholder_color: Option<Color>,
    /// `selection:bg-*`: the highlight of a selection (inputs, selectable
    /// texts).
    pub selection_color: Option<Color>,
    /// `caret-*` (inputs).
    pub caret_color: Option<Color>,
    /// `text-shadow-*` layers beyond the first (drawn as one shadow).
    pub text_shadows: Option<Vec<TextShadow>>,
    /// `space-x-*` / `space-y-*`: margins the view gives its children.
    pub space: Option<Space>,
}

/// `space-x-*` / `space-y-*` (and `-reverse`): the margins a view gives
/// each of its children but the last, as Tailwind CSS's `:where(& >
/// :not(:last-child))`: under the child's own margins, which win.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct Space {
    /// `space-x-*`: the end margin (`margin-inline-end`; the start one
    /// with `space-x-reverse`).
    pub x: Option<PropertyValue>,
    /// `space-y-*`: the bottom margin (`margin-block-end`; the top one
    /// with `space-y-reverse`).
    pub y: Option<PropertyValue>,
    /// `space-x-reverse`.
    pub reverse_x: bool,
    /// `space-y-reverse`.
    pub reverse_y: bool,
    /// The horizontal margins override normal child declarations.
    pub important_x: bool,
    /// The vertical margins override normal child declarations.
    pub important_y: bool,
}

impl Space {
    /// The margins of a child laid out in direction `rtl`: Tailwind's
    /// `margin-inline-end`, the right side left to right and the left side
    /// right to left (the start side when reversed).
    pub(crate) fn apply(&self, style: &mut Style, important: bool, rtl: bool) {
        use crate::style::StyleKey as K;
        if let Some(v) = &self.x
            && self.important_x == important
        {
            let (end, start) = if rtl {
                (K::MarginLeft, K::MarginRight)
            } else {
                (K::MarginRight, K::MarginLeft)
            };
            let (key, other) = if self.reverse_x {
                (start, end)
            } else {
                (end, start)
            };
            let _ = style.set_key(other, crate::style::StyleValue::Number(0.0));
            let _ = style.apply_value(key, v);
        }
        if let Some(v) = &self.y
            && self.important_y == important
        {
            let key = if self.reverse_y {
                K::MarginTop
            } else {
                K::MarginBottom
            };
            let other = if self.reverse_y {
                K::MarginBottom
            } else {
                K::MarginTop
            };
            let _ = style.set_key(other, crate::style::StyleValue::Number(0.0));
            let _ = style.apply_value(key, v);
        }
    }
}

/// A parsed class string.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClassList {
    rules: Vec<Rule>,
    /// [`deps`] bits.
    deps: u16,
    /// [`state`] bits the platform must report for this view.
    tracked: u16,
    /// The view is a `group` (`""`) or named groups (`group/item`).
    groups: Vec<Box<str>>,
    /// Classes that are not known utilities.
    unknown: Vec<Box<str>>,
    /// Classes that parsed and the effect gate refused.
    unsupported: Vec<Box<str>>,
}

impl ClassList {
    /// [`deps`] bits.
    #[inline]
    pub fn deps(&self) -> u16 {
        self.deps
    }

    /// [`state`] bits whose changes the platform must report.
    #[inline]
    pub fn tracked_states(&self) -> u16 {
        self.tracked
    }

    /// Group names this view defines (`""`: an unnamed `group`).
    pub fn groups(&self) -> &[Box<str>] {
        &self.groups
    }

    /// Classes that are not known utilities or variants.
    pub fn unknown(&self) -> &[Box<str>] {
        &self.unknown
    }

    /// Classes the effect gate refused. The rules were not kept.
    pub fn unsupported(&self) -> &[Box<str>] {
        &self.unsupported
    }

    /// Whether nothing applies.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Applies the rules matching `cx` to `style` and `props`, then
    /// `inline` (the node's inline style, which only `!` utilities win
    /// over). `currentColor` (a border without a color) is the node's own
    /// text color, else black.
    pub fn apply(
        &self,
        cx: &Context<'_>,
        theme: &Theme,
        style: &mut Style,
        props: &mut ClassProps,
        inline: impl FnOnce(&mut Style),
    ) {
        self.apply_with(
            cx,
            &no_data,
            theme,
            CurrentColor::default(),
            DEFAULT_CH,
            style,
            props,
            inline,
        );
    }

    /// [`ClassList::apply`] in a tree, where `current` says what the
    /// node's `currentColor` comes from and `ch` is its font's `0` advance.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn apply_with(
        &self,
        cx: &Context<'_>,
        data: Data<'_>,
        theme: &Theme,
        current: CurrentColor,
        ch: f32,
        style: &mut Style,
        props: &mut ClassProps,
        inline: impl FnOnce(&mut Style),
    ) {
        let mut parts = Parts::default();
        parts.ch = ch;
        let mut important = false;
        let current = if self.deps & deps::CURRENT_COLOR != 0 {
            self.current_color(cx, data, current)
        } else {
            Color::BLACK
        };
        parts.current = current;
        for rule in &self.rules {
            if rule.cond.pseudo() || !rule.cond.matches(cx, data) {
                continue;
            }
            for decl in &rule.decls {
                match decl {
                    Decl::Default(key, value) => {
                        let _ = style.apply_value(*key, value);
                    }
                    Decl::CurrentColor(key) => style.set_color(*key, current),
                    _ => {}
                }
            }
        }
        // A background color of an interaction state (`active:bg-*`) hides
        // the gradients unless that state sets one too: drawn over the
        // color, they would hide the feedback (CSS would keep them).
        let mut state_background = false;
        let mut state_image = false;
        for rule in &self.rules {
            if rule.cond.pseudo() || !rule.cond.matches(cx, data) {
                continue;
            }
            important |= rule.important;
            for decl in &rule.decls {
                decl.apply(cx.env.dark, rule.important, style, props, &mut parts);
            }
            let background = rule.decls.iter().any(Decl::sets_background_color);
            if rule.cond.interactive() {
                if background {
                    state_background = true;
                    state_image = false;
                }
                state_image |= rule.decls.iter().any(Decl::sets_background_image);
            } else if background {
                // A later rule's color (`dark:bg-*`) is not feedback.
                state_background = false;
            }
        }
        let state_color = style.view.background_color;
        // `placeholder:` and `selection:` rules only carry a color.
        for rule in self.rules.iter().filter(|r| r.cond.pseudo()) {
            if !rule.cond.matches(cx, data) {
                continue;
            }
            if let Some(c) = rule.cond.pseudo_color(&rule.decls) {
                if rule.cond.placeholder {
                    props.placeholder_color = Some(c);
                } else {
                    props.selection_color = Some(c);
                }
            }
        }
        parts.finish(style, props, theme, cx.window, touched::ALL);
        inline(style);
        if self.deps & deps::CURRENT_COLOR != 0 {
            // What the classes set to `currentColor` inside a value.
            style.apply_current_color(current);
        }
        if important {
            // `!` utilities win over the inline style too, as in CSS. The
            // parts keep every rule's values (a `!scale-*` still composes
            // with a plain `translate-*`); only the groups the important
            // rules touch are written again.
            parts.touched = 0;
            parts.transform_parts = 0;
            parts.screen.clear();
            for rule in self
                .rules
                .iter()
                .filter(|r| r.important && !r.cond.pseudo())
            {
                if rule.cond.matches(cx, data) {
                    for decl in &rule.decls {
                        decl.apply(cx.env.dark, rule.important, style, props, &mut parts);
                    }
                }
            }
            parts.finish(style, props, theme, cx.window, parts.touched);
            if self.deps & deps::CURRENT_COLOR != 0 {
                style.apply_current_color(current);
            }
        }
        // Unless the inline style replaced the state's color.
        if state_background && !state_image && style.view.background_color == state_color {
            style.view.clear_background_images();
        }
    }
}

impl ClassList {
    /// CSS's `currentColor` of a node with these classes in `cx`: the text
    /// color of its `!` classes, its inline style, its classes, else the
    /// one it inherits, else black.
    pub(crate) fn current_color(
        &self,
        cx: &Context<'_>,
        data: Data<'_>,
        current: CurrentColor,
    ) -> Color {
        let [plain, important] = self.text_colors(cx, data);
        important
            .or(current.inline)
            .or(plain)
            .or(current.inherited)
            .unwrap_or(Color::BLACK)
    }

    /// The text colors the rules matching `cx` set: the plain ones' and
    /// the `!` ones' (which win over the inline style).
    fn text_colors(&self, cx: &Context<'_>, data: Data<'_>) -> [Option<Color>; 2] {
        let mut out = [None; 2];
        for rule in &self.rules {
            if rule.cond.pseudo() || !rule.cond.matches(cx, data) {
                continue;
            }
            for decl in &rule.decls {
                if let Some(c) = decl.sets_text_color(cx.env.dark) {
                    out[usize::from(rule.important)] = c;
                }
            }
        }
        out
    }

    /// The rules of a Rust component's styles (already in order), on a
    /// view that is the groups `groups` (`""`: unnamed).
    pub(crate) fn from_rules(rules: Vec<Rule>, groups: Vec<Box<str>>) -> Self {
        let mut list = ClassList::default();
        for rule in rules {
            list.add(rule);
        }
        if !groups.is_empty() {
            list.tracked |= state::PRESSED | state::HOVERED | state::FOCUS_WITHIN;
            list.groups = groups;
        }
        list.rules
            .sort_by(|a, b| (a.important, &a.rank).cmp(&(b.important, &b.rank)));
        list
    }

    fn add(&mut self, rule: Rule) {
        self.deps |= rule.cond.deps();
        self.tracked |= rule.cond.states & !(state::DISABLED | state::ACCESSIBILITY);
        if rule.cond.states & state::FOCUS_WITHIN != 0 {
            self.tracked |= state::FOCUS_WITHIN;
        }
        for d in &rule.decls {
            self.deps |= d.deps();
        }
        if rule.theme {
            self.deps |= deps::THEME;
        }
        self.rules.push(rule);
    }

    /// `self`, then `over`: at equal rank `over`'s rules win (the app's
    /// classes on a Rust component).
    pub(crate) fn merged(&self, over: &ClassList) -> ClassList {
        let mut list = self.clone();
        for rule in &over.rules {
            list.add(rule.clone());
        }
        list.groups.extend(over.groups.iter().cloned());
        list.unknown.extend(over.unknown.iter().cloned());
        list.unsupported.extend(over.unsupported.iter().cloned());
        list.tracked |= over.tracked;
        list.rules
            .sort_by(|a, b| (a.important, &a.rank).cmp(&(b.important, &b.rank)));
        list
    }
}

/// The class cache and theme of one engine.
#[derive(Debug, Default)]
pub struct Tailwind {
    theme: Theme,
    cache: FxHashMap<Box<str>, std::rc::Rc<ClassList>>,
}

impl Tailwind {
    /// Classes with the default theme.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the theme (cached classes are parsed again).
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.cache.clear();
    }

    /// The theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// The parsed form of `classes` (cached by string).
    pub fn classes(&mut self, classes: &str) -> std::rc::Rc<ClassList> {
        self.get(classes).0
    }

    /// Like [`Tailwind::classes`], and whether `classes` was parsed just
    /// now (to report its unknown classes once).
    pub fn get(&mut self, classes: &str) -> (std::rc::Rc<ClassList>, bool) {
        if let Some(c) = self.cache.get(classes) {
            return (c.clone(), false);
        }
        let parsed = std::rc::Rc::new(parse(&self.theme, classes));
        // Class strings are static in practice; a bound keeps generated
        // ones (`w-[${n}px]`) from growing the cache forever.
        if self.cache.len() >= 4096 {
            self.cache.clear();
        }
        self.cache.insert(classes.into(), parsed.clone());
        (parsed, true)
    }
}

/// Parses a class string.
pub fn parse(theme: &Theme, classes: &str) -> ClassList {
    let mut list = ClassList::default();
    for token in classes.split_ascii_whitespace() {
        match parse_token(theme, token) {
            Some(Token::Rule(rule)) if effect_allowed(&rule) => list.add(rule),
            Some(Token::Rule(_) | Token::Group(_)) => list.unsupported.push(token.into()),
            Some(Token::Ignored) => {}
            None => list.unknown.push(token.into()),
        }
    }
    // Stable: equal ranks keep the order of the string.
    list.rules
        .sort_by(|a, b| (a.important, &a.rank).cmp(&(b.important, &b.rank)));
    list
}

enum Token {
    Rule(Rule),
    Group(Box<str>),
    /// Accepted, but has no effect natively (`antialiased`; [`ignored`]).
    Ignored,
}

/// Splits `token` at `:` outside brackets: variants, then the utility.
fn split_variants(token: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in token.char_indices() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth -= 1,
            ':' if depth == 0 => {
                parts.push(&token[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&token[start..]);
    parts
}

fn parse_token(theme: &Theme, token: &str) -> Option<Token> {
    let parts = split_variants(token);
    let (utility, variants) = parts.split_last()?;
    let mut utility = *utility;
    // `!p-4` (v3) or `p-4!` (v4): important.
    let mut important = false;
    if let Some(u) = utility.strip_prefix('!') {
        utility = u;
        important = true;
    } else if let Some(u) = utility.strip_suffix('!') {
        utility = u;
        important = true;
    }
    if variants.is_empty() {
        if utility == "group" {
            return Some(Token::Group("".into()));
        }
        if let Some(name) = utility.strip_prefix("group/") {
            return Some(Token::Group(name.into()));
        }
    }
    let mut cond = Condition::default();
    let mut rank = Rank::default();
    for v in variants {
        rank.add(variant(theme, v, &mut cond)?);
    }
    if ignored(utility) {
        return Some(Token::Ignored);
    }
    let Some(decls) = utilities::parse(theme, utility) else {
        return None;
    };
    if cond.pseudo() && cond.pseudo_color(&decls).is_none() {
        return None;
    }
    Some(Token::Rule(Rule {
        cond,
        decls,
        rank,
        important,
        theme: false,
    }))
}

/// Utilities whose CSS is a rendering hint or what every native view
/// already is: accepted, and they do nothing (`box-border` is preflight's
/// `box-sizing`, NSH's only one).
fn ignored(utility: &str) -> bool {
    matches!(
        utility,
        "antialiased"
            | "subpixel-antialiased"
            | "isolation-auto"
            | "appearance-none"
            | "appearance-auto"
            | "will-change-auto"
            | "will-change-scroll"
            | "will-change-contents"
            | "will-change-transform"
    ) || utility
        .strip_prefix("will-change-[")
        .is_some_and(|v| v.len() > 1 && v.ends_with(']'))
}

/// Names in a `class` string.
pub fn class_tokens(classes: &str) -> impl Iterator<Item = &str> {
    classes.split_whitespace().filter(|name| !name.is_empty())
}

/// `unknown classes: a b`, or `None` when there are no names. Same text
/// nsh logs once a class string is first parsed.
pub fn unknown_classes_warning<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    class_list_warning("unknown classes", names)
}

/// `unsupported classes: a b`, or `None` when there are no names.
pub fn unsupported_classes_warning<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    class_list_warning("unsupported classes", names)
}

fn class_list_warning<'a>(label: &str, names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut names = names.into_iter().filter(|name| !name.is_empty());
    let first = names.next()?;
    let mut out = format!("{label}: {first}");
    for name in names {
        out.push(' ');
        out.push_str(name);
    }
    Some(out)
}

/// The hosts draw this rule. Otherwise the whole token is unsupported.
fn effect_allowed(rule: &Rule) -> bool {
    condition_allowed(&rule.cond) && rule.decls.iter().all(decl_allowed)
}

fn condition_allowed(cond: &Condition) -> bool {
    if cond.dark.is_some()
        || cond.min_width.is_some()
        || cond.max_width.is_some()
        || cond.landscape.is_some()
        || cond.platform.is_some()
        || cond.rtl.is_some()
        || cond.reduce_motion.is_some()
        || cond.group.is_some()
        || !cond.data.is_empty()
        || !cond.group_data.is_empty()
        || cond.position != 0
        || cond.placeholder
    {
        return false;
    }
    const ALLOWED: u16 =
        state::PRESSED | state::HOVERED | state::FOCUSED | state::FOCUS_WITHIN | state::DISABLED;
    cond.states & !ALLOWED == 0 && cond.not_states & !state::DISABLED == 0
}

fn decl_allowed(decl: &Decl) -> bool {
    use crate::style::StyleDecl;
    use utilities::PropDecl;
    match decl {
        Decl::Screen(..)
        | Decl::Adaptive(..)
        | Decl::AdaptivePart(_)
        | Decl::CurrentPart(..)
        | Decl::Rejected => false,
        Decl::Prop(PropDecl::Caret(_)) => false,
        Decl::Prop(_) | Decl::Ch(..) => true,
        Decl::Part(part) => part_allowed(part),
        Decl::Set(key, _)
        | Decl::Default(key, _)
        | Decl::CurrentColor(key)
        | Decl::SetCurrentColor(key, _) => key_allowed(*key),
        Decl::Style(
            StyleDecl::Set(key, _) | StyleDecl::Default(key, _) | StyleDecl::CurrentColor(key),
        ) => key_allowed(*key),
        Decl::Style(_) => false,
    }
}

fn part_allowed(part: &Part) -> bool {
    use Part::*;
    !matches!(
        part,
        Shadow(_)
            | ShadowColor(_)
            | ShadowAlpha(_)
            | InsetShadow(_)
            | InsetShadowColor(_)
            | InsetShadowAlpha(_)
            | Ring(_)
            | RingInset
            | RingColor(_)
            | RingOffset(_)
            | RingOffsetColor(_)
            | InsetRing(_)
            | InsetRingColor(_)
            | Gradient(_)
            | ArbitraryGradient(_)
            | GradientNone
            | From(..)
            | Via(..)
            | To(..)
            | FromPos(_)
            | ViaPos(_)
            | ToPos(_)
            | Filter { .. }
            | FilterNone { .. }
            | DropShadows(_)
            | DropShadowColor(_)
            | DropShadowAlpha(_)
            | TransitionProps(_)
            | Duration(_)
            | Ease(_)
            | Delay(_)
            | TransitionNone
            | Animate(_)
            | Animation(_)
            | TextShadow(_)
            | TextShadowColor(_)
            | TextShadowAlpha(_)
            | SkewX(_)
            | SkewY(_)
    )
}

/// Style keys the three hosts already paint. Text decoration, text
/// transform, font variant, vertical text alignment, object position and
/// tint are omitted: at least one host does not read the field.
fn key_allowed(key: crate::style::StyleKey) -> bool {
    use crate::style::StyleKey as K;
    matches!(
        key,
        K::Display
            | K::BoxSizing
            | K::Position
            | K::FlexDirection
            | K::FlexWrap
            | K::Flex
            | K::FlexGrow
            | K::FlexShrink
            | K::FlexBasis
            | K::JustifyContent
            | K::AlignItems
            | K::AlignSelf
            | K::AlignContent
            | K::Width
            | K::Height
            | K::MinWidth
            | K::MinHeight
            | K::MaxWidth
            | K::MaxHeight
            | K::AspectRatio
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
            | K::MarginLeft
            | K::Padding
            | K::PaddingHorizontal
            | K::PaddingVertical
            | K::PaddingTop
            | K::PaddingRight
            | K::PaddingBottom
            | K::PaddingLeft
            | K::Gap
            | K::RowGap
            | K::ColumnGap
            | K::ZIndex
            | K::GridTemplateColumns
            | K::GridTemplateRows
            | K::GridAutoColumns
            | K::GridAutoRows
            | K::GridAutoFlow
            | K::GridColumn
            | K::GridColumnStart
            | K::GridColumnEnd
            | K::GridRow
            | K::GridRowStart
            | K::GridRowEnd
            | K::JustifyItems
            | K::JustifySelf
            | K::Direction
            | K::MarginStart
            | K::MarginEnd
            | K::PaddingStart
            | K::PaddingEnd
            | K::Start
            | K::End
            | K::BorderWidth
            | K::BorderTopWidth
            | K::BorderRightWidth
            | K::BorderBottomWidth
            | K::BorderLeftWidth
            | K::BorderStartWidth
            | K::BorderEndWidth
            | K::BackgroundColor
            | K::BorderRadius
            | K::BorderTopLeftRadius
            | K::BorderTopRightRadius
            | K::BorderBottomRightRadius
            | K::BorderBottomLeftRadius
            | K::BorderTopStartRadius
            | K::BorderTopEndRadius
            | K::BorderBottomEndRadius
            | K::BorderBottomStartRadius
            | K::BorderColor
            | K::BorderTopColor
            | K::BorderRightColor
            | K::BorderBottomColor
            | K::BorderLeftColor
            | K::BorderStartColor
            | K::BorderEndColor
            | K::BorderStyle
            | K::Opacity
            | K::Overflow
            | K::Color
            | K::FontSize
            | K::FontWeight
            | K::LineHeight
            | K::FontStyle
            | K::LetterSpacing
            | K::TextAlign
            | K::FontFamily
            | K::Translate
            | K::Rotate
            | K::Scale
            | K::Transform
            | K::TransformOrigin
            | K::ObjectFit
            | K::NumberOfLines
            | K::TextOverflow
            | K::UserSelect
            | K::SelectionColor
            | K::WritingDirection
            | K::VerticalAlign
    )
}

/// Whether `name` names a boolean `data-*` attribute (`data-<name>:`):
/// lower case letters, digits and `-`. Tailwind's `data-[state=open]:`,
/// an attribute's value, is not supported.
fn data_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Applies variant `v` to `cond`; returns its place in Tailwind's order.
fn variant(theme: &Theme, v: &str, cond: &mut Condition) -> Option<Key> {
    use state::*;
    let states = |cond: &mut Condition, s: u16| {
        cond.states |= s;
    };
    // Tailwind registers `group-*` first, then the pseudo-elements, then
    // the structural pseudo-classes: later ones win over earlier ones.
    Some(match v {
        "selection" => {
            cond.selection = true;
            rank::SELECTION
        }
        "placeholder" => {
            cond.placeholder = true;
            rank::PLACEHOLDER
        }
        "first" => {
            cond.position |= position::FIRST;
            rank::FIRST
        }
        "last" => {
            cond.position |= position::LAST;
            rank::LAST
        }
        "only" => {
            cond.position |= position::ONLY;
            rank::ONLY
        }
        "odd" => {
            cond.position |= position::ODD;
            rank::ODD
        }
        "even" => {
            cond.position |= position::EVEN;
            rank::EVEN
        }
        "checked" => {
            states(cond, CHECKED);
            rank::CHECKED
        }
        "indeterminate" => {
            states(cond, INDETERMINATE);
            rank::INDETERMINATE
        }
        "selected" => {
            states(cond, SELECTED);
            rank::SELECTED
        }
        "focus-within" => {
            states(cond, FOCUS_WITHIN);
            rank::FOCUS_WITHIN
        }
        "hover" => {
            states(cond, HOVERED);
            rank::HOVER
        }
        "focus" => {
            states(cond, FOCUSED);
            rank::FOCUS
        }
        "focus-visible" => {
            states(cond, FOCUSED);
            rank::FOCUS_VISIBLE
        }
        "active" => {
            states(cond, PRESSED);
            rank::ACTIVE
        }
        "enabled" => {
            cond.not_states |= DISABLED;
            rank::ENABLED
        }
        "disabled" => {
            states(cond, DISABLED);
            rank::DISABLED
        }
        "aria-busy" => {
            states(cond, BUSY);
            rank::ARIA_BUSY
        }
        "aria-checked" => {
            states(cond, CHECKED);
            rank::ARIA_CHECKED
        }
        "aria-expanded" => {
            states(cond, EXPANDED);
            rank::EXPANDED
        }
        "aria-pressed" => {
            states(cond, ARIA_PRESSED);
            rank::ARIA_PRESSED
        }
        "aria-selected" => {
            states(cond, SELECTED);
            rank::ARIA_SELECTED
        }
        "motion-safe" => {
            cond.reduce_motion = Some(false);
            rank::MOTION_SAFE
        }
        "motion-reduce" => {
            cond.reduce_motion = Some(true);
            rank::MOTION_REDUCE
        }
        "portrait" => {
            cond.landscape = Some(false);
            rank::PORTRAIT
        }
        "landscape" => {
            cond.landscape = Some(true);
            rank::LANDSCAPE
        }
        "ltr" => {
            cond.rtl = Some(false);
            rank::LTR
        }
        "rtl" => {
            cond.rtl = Some(true);
            rank::RTL
        }
        "dark" => {
            cond.dark = Some(true);
            rank::DARK
        }
        "ios" => {
            cond.platform = Some(Platform::Ios);
            rank::IOS
        }
        "android" => {
            cond.platform = Some(Platform::Android);
            rank::ANDROID
        }
        "web" => {
            cond.platform = Some(Platform::Web);
            rank::WEB
        }
        v => {
            // `data-range-start`: Tailwind 4's boolean form, `[data-range-start]`.
            if let Some(name) = v.strip_prefix("data-").filter(|n| data_name(n)) {
                if !cond.data.iter().any(|d| **d == *name) {
                    cond.data.push(name.into());
                }
                return Some(Key::data(name));
            }
            if let Some(rest) = v.strip_prefix("group-") {
                // `group-hover`, `group-hover/item`.
                let (s, name) = match rest.split_once('/') {
                    Some((s, n)) => (s, n),
                    None => (rest, ""),
                };
                if let Some(data) = s.strip_prefix("data-").filter(|n| data_name(n)) {
                    let entry = cond.group.get_or_insert_with(|| (name.into(), 0));
                    if *entry.0 != *name {
                        return None;
                    }
                    if !cond.group_data.iter().any(|d| **d == *data) {
                        cond.group_data.push(data.into());
                    }
                    let mut group = Rank::default();
                    group.add_group(&Key::data(data), name);
                    return group.0.pop();
                }
                let (bit, inner) = match s {
                    "hover" => (HOVERED, rank::HOVER),
                    "active" => (PRESSED, rank::ACTIVE),
                    "focus" => (FOCUSED, rank::FOCUS),
                    "focus-visible" => (FOCUSED, rank::FOCUS_VISIBLE),
                    "focus-within" => (FOCUS_WITHIN, rank::FOCUS_WITHIN),
                    "disabled" => (DISABLED, rank::DISABLED),
                    "selected" => (SELECTED, rank::SELECTED),
                    "checked" => (CHECKED, rank::CHECKED),
                    "indeterminate" => (INDETERMINATE, rank::INDETERMINATE),
                    "aria-busy" => (BUSY, rank::ARIA_BUSY),
                    "aria-checked" => (CHECKED, rank::ARIA_CHECKED),
                    "aria-expanded" => (EXPANDED, rank::EXPANDED),
                    "aria-pressed" => (ARIA_PRESSED, rank::ARIA_PRESSED),
                    "aria-selected" => (SELECTED, rank::ARIA_SELECTED),
                    _ => return None,
                };
                let entry = cond.group.get_or_insert_with(|| (name.into(), 0));
                if *entry.0 != *name {
                    return None;
                }
                entry.1 |= bit;
                let mut group = Rank::default();
                group.add_group(&inner, name);
                return group.0.pop();
            }
            // Breakpoints: `md:`, `max-md:`, `min-[600px]:`, `max-[600px]:`.
            let (max, name) = match v.strip_prefix("max-") {
                Some(n) => (true, n),
                None => (false, v.strip_prefix("min-").unwrap_or(v)),
            };
            // Tailwind orders breakpoints by their unit's name, then by
            // their value (`max-*` widest first).
            let (width, unit, value) =
                match name.strip_prefix('[').and_then(|n| n.strip_suffix(']')) {
                    Some(arbitrary) => {
                        let arbitrary = arbitrary.trim();
                        let digits = arbitrary
                            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                            .unwrap_or(arbitrary.len());
                        let unit = BREAKPOINT_UNITS
                            .iter()
                            .position(|u| *u == &arbitrary[digits..])?;
                        let value = crate::css::number(&arbitrary[..digits])?;
                        (crate::css::length(arbitrary)?, unit, value)
                    }
                    None => {
                        let b = theme.breakpoints.iter().find(|b| *b.0 == *name)?;
                        match b.2 {
                            true => (b.1, 3, b.1 / theme.rem),
                            false => (b.1, 2, b.1),
                        }
                    }
                };
            let thousandths = (f64::from(value) * 1000.0).round() as i64;
            let key = |order, value| Key {
                order,
                unit: unit as u8,
                value,
                name: None,
            };
            if max {
                cond.max_width = Some(cond.max_width.map_or(width, |w| w.min(width)));
                key(rank::MAX, -thousandths)
            } else {
                cond.min_width = Some(cond.min_width.map_or(width, |w| w.max(width)));
                key(rank::MIN, thousandths)
            }
        }
    })
}

#[cfg(test)]
mod tests;
