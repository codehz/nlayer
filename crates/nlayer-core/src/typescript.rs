//! TypeScript for the v1 style interfaces, taken from [`STYLE_TYPES`].
//!
//! The table still names properties the public subset does not (shadows,
//! gradients, filters, animation). Those stay out of the declarations.
//! `AnimatedNode` is stripped: v1 transforms are static.

use crate::style::{STYLE_TYPES, StyleGroup, StyleType};

/// Style properties `build` declares. Each name is a [`STYLE_TYPES`] entry;
/// the TypeScript comes from that entry, not from this list.
const PUBLIC: &[&str] = &[
    "alignContent",
    "alignItems",
    "alignSelf",
    "aspectRatio",
    "backgroundColor",
    "borderBottomColor",
    "borderBottomLeftRadius",
    "borderBottomRightRadius",
    "borderBottomWidth",
    "borderColor",
    "borderLeftColor",
    "borderLeftWidth",
    "borderRadius",
    "borderRightColor",
    "borderRightWidth",
    "borderStyle",
    "borderTopColor",
    "borderTopLeftRadius",
    "borderTopRightRadius",
    "borderTopWidth",
    "borderWidth",
    "bottom",
    "boxSizing",
    "color",
    "columnGap",
    "display",
    "flex",
    "flexBasis",
    "flexDirection",
    "flexGrow",
    "flexShrink",
    "flexWrap",
    "fontSize",
    "fontStyle",
    "fontWeight",
    "gap",
    "gridAutoColumns",
    "gridAutoFlow",
    "gridAutoRows",
    "gridColumn",
    "gridColumnEnd",
    "gridColumnStart",
    "gridRow",
    "gridRowEnd",
    "gridRowStart",
    "gridTemplateColumns",
    "gridTemplateRows",
    "height",
    "justifyContent",
    "justifyItems",
    "justifySelf",
    "left",
    "lineHeight",
    "margin",
    "marginBottom",
    "marginHorizontal",
    "marginLeft",
    "marginRight",
    "marginTop",
    "marginVertical",
    "maxHeight",
    "maxWidth",
    "minHeight",
    "minWidth",
    "objectFit",
    "opacity",
    "overflow",
    "padding",
    "paddingBottom",
    "paddingHorizontal",
    "paddingLeft",
    "paddingRight",
    "paddingTop",
    "paddingVertical",
    "position",
    "right",
    "rotate",
    "rowGap",
    "scale",
    "textAlign",
    "top",
    "transform",
    "transformOrigin",
    "translate",
    "width",
    "zIndex",
];

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
 * or one of them plus a length (`calc(env(name) + 8px)`). */
export type EnvValue = `env(${string})` | `calc(${string})`;

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
        "Styles of text, inherited by nested `<Text>` spans, and of inputs.",
    ),
];

/// `styles.d.ts`: the v1 style interfaces.
pub fn style_declarations() -> String {
    use std::fmt::Write as _;
    let mut out = String::from(PRELUDE);
    for (group, head, doc) in INTERFACES {
        let props: Vec<&StyleType> = STYLE_TYPES
            .iter()
            .filter(|t| t.group == *group && is_public(t.name))
            .collect();
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

fn is_public(name: &str) -> bool {
    PUBLIC.binary_search(&name).is_ok()
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
    fn the_public_list_is_sorted_and_every_name_is_in_the_table() {
        assert!(PUBLIC.windows(2).all(|w| w[0] < w[1]));
        for name in PUBLIC {
            assert!(
                STYLE_TYPES.iter().any(|t| t.name == *name),
                "{name} is not a style key"
            );
        }
    }

    #[test]
    fn declarations_follow_the_table_and_leave_out_the_rest() {
        let out = style_declarations();
        for name in PUBLIC {
            let t = STYLE_TYPES.iter().find(|t| t.name == *name).unwrap();
            let line = format!("  {name}?: {};", public_ts(t.ts));
            assert!(out.contains(&line), "missing {line}\n{out}");
        }
        for name in [
            "boxShadow",
            "backgroundImage",
            "filter",
            "backdropFilter",
            "cursor",
            "animationName",
            "transitionProperty",
            "fontFamily",
            "textShadow",
            "userSelect",
            "pointerEvents",
            "placeholderColor",
        ] {
            assert!(
                STYLE_TYPES.iter().any(|t| t.name == name),
                "{name} left the table; the exclusion test no longer proves a filter"
            );
            assert!(!out.contains(&format!("  {name}?:")), "{name} is not v1");
        }
        assert!(!out.contains("AnimatedNode"), "{out}");
        assert!(out.contains("export interface TextStyle extends ViewStyle"));
        assert!(out.contains("EnvValue"));
    }
}
