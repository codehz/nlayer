//! TypeScript for the style interfaces, taken from [`STYLE_TYPES`].
//!
//! Every key the engine parses is declared. A name it does not know is a
//! warning at run time, not a second reject list. `AnimatedNode` is
//! stripped: v1 transforms are static.

use crate::style::{STYLE_TYPES, StyleGroup, StyleType};

const PRELUDE: &str = r#"/** A CSS-like color: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`, `rgb()`,
 * `rgba()` or a named color (`black`, `white`, `transparent`, …). */
export type ColorValue = string;

/** Points, a percentage of the parent, or `"auto"`. */
export type DimensionValue = number | `${number}%` | "auto";

/** Points or a percentage of the parent. */
export type LengthValue = number | `${number}%`;

/** A length from a CSS environment variable, on padding and margin:
 * `env(safe-area-inset-top)` (`-right`, `-bottom`, `-left`), `env(keyboard-inset-height)`
 * (0 while the keyboard is hidden), with an optional fallback (`env(name, 8px)`),
 * one of them plus a length (`calc(env(name) + 8px)`), or the larger of two
 * known variables (`max(env(a), env(b))`, also inside that calc). */
export type EnvValue = `env(${string})` | `calc(${string})` | `max(${string})`;

/** One static transform step. Angles are degrees (`"45deg"`, `"0.5rad"` or a number). */
export type TransformStep =
  | { translateX: number }
  | { translateY: number }
  | { scale: number }
  | { scaleX: number }
  | { scaleY: number }
  | { rotate: number | string }
  | { rotateZ: number | string }
  | { rotateX: number | string }
  | { rotateY: number | string }
  | { skewX: number | string }
  | { skewY: number | string }
  | { perspective: number };

/** `start` / `end` follow the writing direction; `flex-start` / `flex-end` follow the flex direction. */
export type Align = "flex-start" | "flex-end" | "center" | "stretch" | "baseline" | "start" | "end" | "self-start" | "self-end" | "normal";

/** `normal` is `flex-start` in flex containers; grid tracks sized `auto` stretch. */
export type Distribute = "flex-start" | "flex-end" | "center" | "stretch" | "space-between" | "space-around" | "space-evenly" | "start" | "end" | "normal";

/** One layer of `boxShadow`: offsets, blur and spread in points; `color`
 * the text color when unset; `inset` draws it inside the box. */
export type BoxShadowLayer = {
  x: number;
  y: number;
  blur?: number;
  spread?: number;
  color?: ColorValue;
  inset?: boolean;
};

/** One layer of `textShadow`: offsets and blur in points; `color` the text
 * color when unset. */
export type TextShadowLayer = { x: number; y: number; blur?: number; color?: ColorValue };

/** An OpenType number or letter form (`fontVariant`). */
export type FontVariantName = "small-caps" | "oldstyle-nums" | "lining-nums" | "tabular-nums" | "proportional-nums";
"#;

const INTERFACES: &[(StyleGroup, &str, &str)] = &[
    (
        StyleGroup::Layout,
        "LayoutStyle",
        "Layout (flexbox and grid). A view that sets no display lays its children out in a column, and nothing shrinks unless `flexShrink` says so.",
    ),
    (
        StyleGroup::View,
        "ViewStyle extends LayoutStyle",
        "Styles of views, and of text, images and inputs, which are views.",
    ),
    (
        StyleGroup::Text,
        "TextStyle extends ViewStyle",
        "Styles of text, inherited by nested `<Text>` spans.",
    ),
    (
        StyleGroup::Input,
        "InputStyle extends TextStyle",
        "Styles of inputs.",
    ),
    (
        StyleGroup::Image,
        "ImageStyle extends ViewStyle",
        "Styles of images.",
    ),
];

/// `styles.d.ts`: the v1 style interfaces.
pub fn style_declarations() -> String {
    use std::fmt::Write as _;
    let mut out = String::from(PRELUDE);
    for (group, head, doc) in INTERFACES {
        let props: Vec<&StyleType> = STYLE_TYPES.iter().filter(|t| t.group == *group).collect();
        if props.is_empty() {
            continue;
        }
        let _ = write!(out, "\n/** {doc} */\nexport interface {head} {{\n");
        for t in props {
            if let Some(doc) = t.doc {
                let doc = doc.replace("*/", "* /");
                let _ = writeln!(out, "  /** {doc} */");
            }
            let _ = writeln!(out, "  {}?: {};", t.name, public_ts(t.ts));
        }
        out.push_str("}\n");
    }
    out
}

/// Drops animated-node unions. v1 does not expose them.
fn public_ts(ts: &str) -> String {
    ts.replace(" | AnimatedNode", "")
        .replace("AnimatedNode | ", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_follow_the_table() {
        let out = style_declarations();
        for t in STYLE_TYPES {
            let line = format!("  {}?: {};", t.name, public_ts(t.ts));
            assert!(out.contains(&line), "missing {line}\n{out}");
        }
        assert!(out.contains("export type BoxShadowLayer"));
        assert!(out.contains("export type TextShadowLayer"));
        assert!(out.contains("export type FontVariantName"));
        assert!(out.contains("export interface ImageStyle extends ViewStyle"));
        assert!(out.contains("export interface InputStyle extends TextStyle"));
        assert!(!out.contains("AnimatedNode"), "{out}");
        assert!(out.contains("export interface TextStyle extends ViewStyle"));
        assert!(out.contains("EnvValue"));
    }
}
