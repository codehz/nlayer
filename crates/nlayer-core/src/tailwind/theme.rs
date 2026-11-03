//! Tailwind CSS v4's default theme (`theme.css`) in points (1rem = 16pt).
//! The engine uses [`Theme::default`]; there is no theme file and no design
//! tokens.

use rustc_hash::FxHashMap;

use crate::{
    css,
    effects::BoxShadow,
    style::{Color, TextShadow},
};

/// Tailwind's design tokens.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Points per spacing step (`p-1`).
    pub(crate) spacing: f32,
    /// Points per `rem` (arbitrary values in `rem`).
    pub(crate) rem: f32,
    /// Flat color names: `red-500`, `black`, `brand`.
    pub(crate) colors: FxHashMap<Box<str>, Color>,
    /// Breakpoints (`sm:`), ascending.
    /// Name, width, and whether Tailwind's is in `rem` (the default
    /// theme's, which Tailwind orders after `px` ones).
    pub(crate) breakpoints: Vec<(Box<str>, f32, bool)>,
    /// `max-w-sm` and friends.
    pub(crate) containers: FxHashMap<Box<str>, f32>,
    /// `text-lg`: size and line height (a multiple of the size).
    pub(crate) text: FxHashMap<Box<str>, (f32, Option<f32>)>,
    /// `font-bold`.
    pub(crate) weights: FxHashMap<Box<str>, u16>,
    /// `font-sans`: the family (`None` is the system font).
    pub(crate) fonts: FxHashMap<Box<str>, Option<Box<str>>>,
    /// `tracking-wide`, in ems.
    pub(crate) tracking: FxHashMap<Box<str>, f32>,
    /// `leading-snug`, multiples of the font size.
    pub(crate) leading: FxHashMap<Box<str>, f32>,
    /// `rounded-lg`.
    pub(crate) radius: FxHashMap<Box<str>, f32>,
    /// `shadow-md`.
    pub(crate) shadows: FxHashMap<Box<str>, Vec<BoxShadow>>,
    /// `inset-shadow-sm`.
    pub(crate) inset_shadows: FxHashMap<Box<str>, Vec<BoxShadow>>,
    /// `drop-shadow-md` (offset x, y, blur, color per layer).
    pub(crate) drop_shadows: FxHashMap<Box<str>, Vec<BoxShadow>>,
    /// `text-shadow-md`.
    pub(crate) text_shadows: FxHashMap<Box<str>, Vec<TextShadow>>,
    /// `blur-md`.
    pub(crate) blur: FxHashMap<Box<str>, f32>,
    /// `perspective-near`.
    pub(crate) perspective: FxHashMap<Box<str>, f32>,
    /// `aspect-video`.
    pub(crate) aspect: FxHashMap<Box<str>, f32>,
    /// `ease-out`.
    pub(crate) ease: FxHashMap<Box<str>, [f32; 4]>,
    /// Default transition duration (ms) and curve.
    pub(crate) transition: (f32, [f32; 4]),
}

fn map<V>(entries: &[(&str, V)]) -> FxHashMap<Box<str>, V>
where
    V: Clone,
{
    entries
        .iter()
        .map(|(k, v)| ((*k).into(), v.clone()))
        .collect()
}

fn shadows(s: &str) -> Vec<BoxShadow> {
    css::box_shadows(s).unwrap_or_default()
}

impl Default for Theme {
    fn default() -> Self {
        let rem = css::REM;
        let mut colors: FxHashMap<Box<str>, Color> = FxHashMap::default();
        for (name, shades) in super::palette::PALETTE.iter() {
            for (shade, rgba) in super::palette::SHADES.iter().zip(shades) {
                colors.insert(format!("{name}-{shade}").into(), Color(*rgba));
            }
        }
        colors.insert("black".into(), Color(0x0000_00ff));
        colors.insert("white".into(), Color(0xffff_ffff));
        colors.insert("transparent".into(), Color::TRANSPARENT);
        let text_shadow = |s: &str| -> Vec<TextShadow> {
            shadows(s)
                .into_iter()
                .map(|l| TextShadow {
                    color: l.color,
                    offset_x: l.offset_x,
                    offset_y: l.offset_y,
                    radius: l.blur,
                })
                .collect()
        };
        Theme {
            spacing: 0.25 * rem,
            rem,
            colors,
            breakpoints: vec![
                ("sm".into(), 40.0 * rem, true),
                ("md".into(), 48.0 * rem, true),
                ("lg".into(), 64.0 * rem, true),
                ("xl".into(), 80.0 * rem, true),
                ("2xl".into(), 96.0 * rem, true),
            ],
            containers: map(&[
                ("3xs", 16.0 * rem),
                ("2xs", 18.0 * rem),
                ("xs", 20.0 * rem),
                ("sm", 24.0 * rem),
                ("md", 28.0 * rem),
                ("lg", 32.0 * rem),
                ("xl", 36.0 * rem),
                ("2xl", 42.0 * rem),
                ("3xl", 48.0 * rem),
                ("4xl", 56.0 * rem),
                ("5xl", 64.0 * rem),
                ("6xl", 72.0 * rem),
                ("7xl", 80.0 * rem),
            ]),
            text: map(&[
                ("xs", (0.75 * rem, Some(1.0 / 0.75))),
                ("sm", (0.875 * rem, Some(1.25 / 0.875))),
                ("base", (rem, Some(1.5))),
                ("lg", (1.125 * rem, Some(1.75 / 1.125))),
                ("xl", (1.25 * rem, Some(1.75 / 1.25))),
                ("2xl", (1.5 * rem, Some(2.0 / 1.5))),
                ("3xl", (1.875 * rem, Some(2.25 / 1.875))),
                ("4xl", (2.25 * rem, Some(2.5 / 2.25))),
                ("5xl", (3.0 * rem, Some(1.0))),
                ("6xl", (3.75 * rem, Some(1.0))),
                ("7xl", (4.5 * rem, Some(1.0))),
                ("8xl", (6.0 * rem, Some(1.0))),
                ("9xl", (8.0 * rem, Some(1.0))),
            ]),
            weights: map(&[
                ("thin", 100),
                ("extralight", 200),
                ("light", 300),
                ("normal", 400),
                ("medium", 500),
                ("semibold", 600),
                ("bold", 700),
                ("extrabold", 800),
                ("black", 900),
            ]),
            fonts: map(&[
                ("sans", None),
                ("serif", Some("serif".into())),
                ("mono", Some("monospace".into())),
            ]),
            tracking: map(&[
                ("tighter", -0.05),
                ("tight", -0.025),
                ("normal", 0.0),
                ("wide", 0.025),
                ("wider", 0.05),
                ("widest", 0.1),
            ]),
            leading: map(&[
                ("none", 1.0),
                ("tight", 1.25),
                ("snug", 1.375),
                ("normal", 1.5),
                ("relaxed", 1.625),
                ("loose", 2.0),
            ]),
            radius: map(&[
                ("xs", 0.125 * rem),
                ("sm", 0.25 * rem),
                ("md", 0.375 * rem),
                ("lg", 0.5 * rem),
                ("xl", 0.75 * rem),
                ("2xl", rem),
                ("3xl", 1.5 * rem),
                ("4xl", 2.0 * rem),
            ]),
            shadows: map(&[
                ("2xs", shadows("0 1px rgb(0 0 0 / 0.05)")),
                ("xs", shadows("0 1px 2px 0 rgb(0 0 0 / 0.05)")),
                (
                    "sm",
                    shadows("0 1px 3px 0 rgb(0 0 0 / 0.1), 0 1px 2px -1px rgb(0 0 0 / 0.1)"),
                ),
                (
                    "md",
                    shadows("0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1)"),
                ),
                (
                    "lg",
                    shadows("0 10px 15px -3px rgb(0 0 0 / 0.1), 0 4px 6px -4px rgb(0 0 0 / 0.1)"),
                ),
                (
                    "xl",
                    shadows("0 20px 25px -5px rgb(0 0 0 / 0.1), 0 8px 10px -6px rgb(0 0 0 / 0.1)"),
                ),
                ("2xl", shadows("0 25px 50px -12px rgb(0 0 0 / 0.25)")),
                // `shadow` without a size (deprecated in v4, still common).
                (
                    "",
                    shadows("0 1px 3px 0 rgb(0 0 0 / 0.1), 0 1px 2px -1px rgb(0 0 0 / 0.1)"),
                ),
            ]),
            inset_shadows: map(&[
                ("2xs", shadows("inset 0 1px rgb(0 0 0 / 0.05)")),
                ("xs", shadows("inset 0 1px 1px rgb(0 0 0 / 0.05)")),
                ("sm", shadows("inset 0 2px 4px rgb(0 0 0 / 0.05)")),
            ]),
            drop_shadows: map(&[
                ("xs", shadows("0 1px 1px rgb(0 0 0 / 0.05)")),
                ("sm", shadows("0 1px 2px rgb(0 0 0 / 0.15)")),
                ("md", shadows("0 3px 3px rgb(0 0 0 / 0.12)")),
                ("lg", shadows("0 4px 4px rgb(0 0 0 / 0.15)")),
                ("xl", shadows("0 9px 7px rgb(0 0 0 / 0.1)")),
                ("2xl", shadows("0 25px 25px rgb(0 0 0 / 0.15)")),
                (
                    "",
                    shadows("0 1px 2px rgb(0 0 0 / 0.1), 0 1px 1px rgb(0 0 0 / 0.06)"),
                ),
            ]),
            text_shadows: map(&[
                ("2xs", text_shadow("0px 1px 0px rgb(0 0 0 / 0.15)")),
                ("xs", text_shadow("0px 1px 1px rgb(0 0 0 / 0.2)")),
                (
                    "sm",
                    text_shadow(
                        "0px 1px 0px rgb(0 0 0 / 0.075), 0px 1px 1px rgb(0 0 0 / 0.075), 0px 2px 2px rgb(0 0 0 / 0.075)",
                    ),
                ),
                (
                    "md",
                    text_shadow(
                        "0px 1px 1px rgb(0 0 0 / 0.1), 0px 1px 2px rgb(0 0 0 / 0.1), 0px 2px 4px rgb(0 0 0 / 0.1)",
                    ),
                ),
                (
                    "lg",
                    text_shadow(
                        "0px 1px 2px rgb(0 0 0 / 0.1), 0px 3px 2px rgb(0 0 0 / 0.1), 0px 4px 8px rgb(0 0 0 / 0.1)",
                    ),
                ),
            ]),
            blur: map(&[
                ("xs", 4.0),
                ("sm", 8.0),
                ("md", 12.0),
                ("lg", 16.0),
                ("xl", 24.0),
                ("2xl", 40.0),
                ("3xl", 64.0),
                ("", 8.0),
            ]),
            perspective: map(&[
                ("dramatic", 100.0),
                ("near", 300.0),
                ("normal", 500.0),
                ("midrange", 800.0),
                ("distant", 1200.0),
            ]),
            aspect: map(&[("square", 1.0), ("video", 16.0 / 9.0)]),
            ease: map(&[
                ("linear", [0.0, 0.0, 1.0, 1.0]),
                ("in", [0.4, 0.0, 1.0, 1.0]),
                ("out", [0.0, 0.0, 0.2, 1.0]),
                ("in-out", [0.4, 0.0, 0.2, 1.0]),
            ]),
            transition: (150.0, [0.4, 0.0, 0.2, 1.0]),
        }
    }
}

impl Theme {
    /// A color by name (`red-500`, `brand`).
    pub fn color(&self, name: &str) -> Option<Color> {
        self.colors.get(name).copied()
    }

    /// The names of every scale's entries (`red-500`, `lg`, `video`,
    /// `snappy`, ...), sorted and without repeats: what utilities and
    /// breakpoint variants take by name. Tools enumerate the classes a
    /// theme offers with them (the engine's record of Tailwind classes).
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.breakpoints.iter().map(|(n, ..)| &**n).collect();
        let keys = [
            self.colors.keys().collect::<Vec<_>>(),
            self.containers.keys().collect(),
            self.text.keys().collect(),
            self.weights.keys().collect(),
            self.fonts.keys().collect(),
            self.tracking.keys().collect(),
            self.leading.keys().collect(),
            self.radius.keys().collect(),
            self.shadows.keys().collect(),
            self.inset_shadows.keys().collect(),
            self.drop_shadows.keys().collect(),
            self.text_shadows.keys().collect(),
            self.blur.keys().collect(),
            self.perspective.keys().collect(),
            self.aspect.keys().collect(),
            self.ease.keys().collect(),
        ];
        names.extend(keys.into_iter().flatten().map(|n| &**n));
        names.sort_unstable();
        names.dedup();
        names
    }
}
