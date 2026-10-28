//! Decoding of page values into host props and styles. The rules match
//! `nlayer-js`'s decoder; the values are slots instead of QuickJS values.

use std::cell::RefCell;

use nlayer_core::{
    ImageScales, ImageSource, Interaction, KeyboardType, NodeId, Op, Prop, Tag,
    geometry::{Points, Size},
    style::{
        Style, StyleDecl, StyleError, StylePatch, StyleValue, TextStyle, TransformProperty,
        ViewStyle,
    },
    transform::TransformKind,
};
use rustc_hash::FxHashSet;

use super::{PropError, values::Value};

#[derive(Default)]
pub(super) struct Warnings {
    seen: RefCell<FxHashSet<String>>,
}

impl Warnings {
    pub(super) fn warn(&self, message: impl Into<String>) {
        let message = message.into();
        if self.seen.borrow_mut().insert(message.clone()) {
            log::warn!("{message}");
        }
    }
}

/// The op setting prop `name` to `value`, when v1 has that prop.
pub(super) fn decode_prop(
    id: NodeId,
    tag: Option<Tag>,
    name: &str,
    value: &Value,
    warnings: &Warnings,
) -> Result<Option<Op>, PropError> {
    if name == "style" || is_state_style(name) {
        if name != "style" && cleared(value) {
            return Ok(Some(state_style(id, name, None)));
        }
        let entries = style_entries(value)?;
        let (style, patch) = decode_style(entries, warnings)?;
        if name == "style" {
            return Ok(Some(Op::SetStyle { id, style, patch }));
        }
        let style = *style;
        let paint = paints(&style).then(|| Box::new((style.view, style.text)));
        return Ok(Some(state_style(id, name, paint)));
    }
    if name == "testID" {
        let name = string(value);
        let test_id = (!name.is_empty()).then(|| std::sync::Arc::<str>::from(name));
        return Ok(Some(Op::SetProp {
            id,
            prop: Prop::TestId(test_id),
        }));
    }
    let prop = match (tag, name) {
        (Some(Tag::Image), "src") => Prop::Source(image_source(value)?),
        (Some(Tag::Scroll), "horizontal") => Prop::Horizontal(value.bool()),
        (Some(Tag::Input), "value") => Prop::Value(string(value)),
        (Some(Tag::Input), "placeholder") => Prop::Placeholder(string(value)),
        (Some(Tag::Input), "secureTextEntry") => Prop::Secure(value.bool()),
        (Some(Tag::Input), "keyboardType") => {
            let name = string(value);
            match KeyboardType::from_name(&name) {
                Some(keyboard) => Prop::Keyboard(keyboard),
                None => {
                    warnings.warn(format!("unknown keyboardType `{name}`"));
                    return Ok(None);
                }
            }
        }
        (Some(Tag::Input), "disabled") => Prop::Disabled(value.bool()),
        _ => {
            warnings.warn(format!("unsupported prop `{name}`"));
            return Ok(None);
        }
    };
    Ok(Some(Op::SetProp { id, prop }))
}

fn is_state_style(name: &str) -> bool {
    matches!(name, "activeStyle" | "hoverStyle" | "focusStyle")
}

fn cleared(value: &Value) -> bool {
    matches!(value.kind(), 0 | 1) || (value.kind() == 2 && !value.bool())
}

fn paints(style: &Style) -> bool {
    style.view != ViewStyle::default() || style.text != TextStyle::default()
}

fn state_style(id: NodeId, name: &str, paint: Option<Box<(ViewStyle, TextStyle)>>) -> Op {
    let interaction = match name {
        "hoverStyle" => Interaction::Hover,
        "focusStyle" => Interaction::Focus,
        _ => Interaction::Active,
    };
    Op::SetProp {
        id,
        prop: Prop::StateStyle { interaction, paint },
    }
}

fn string(value: &Value) -> String {
    value.as_string().unwrap_or_default()
}

fn image_source(value: &Value) -> Result<Option<ImageSource>, PropError> {
    if value.kind() == 4 {
        let url = string(value);
        return Ok((!url.is_empty()).then(|| ImageSource {
            uri: url.into(),
            size: None,
            scales: ImageScales::NONE,
        }));
    }
    if value.kind() != 7 {
        return Ok(None);
    }
    let url = value.get("url")?;
    if url.kind() == 0 || url.kind() == 1 {
        return Ok(None);
    }
    let url = string(&url);
    let size_value = value.get("size")?;
    let size = if size_value.kind() == 7 {
        match (
            size_value.get("width")?.number_value(),
            size_value.get("height")?.number_value(),
        ) {
            (Some(w), Some(h)) => Some(Size::new(Points(w as f32), Points(h as f32))),
            _ => None,
        }
    } else {
        None
    };
    let scales_value = value.get("scales")?;
    let scales = if scales_value.kind() == 5 {
        let n = scales_value.length().unwrap_or(0);
        let mut list = Vec::with_capacity(n);
        for i in 0..n {
            if let Some(v) = scales_value.index(i)?.number_value() {
                list.push(v as f32);
            }
        }
        list.into_iter().collect()
    } else {
        ImageScales::NONE
    };
    Ok(Some(ImageSource {
        uri: url.into(),
        size,
        scales,
    }))
}

fn style_entries(value: &Value) -> Result<Vec<(String, Value)>, PropError> {
    let mut out = Vec::new();
    flatten_style(value, &mut out)?;
    Ok(out)
}

fn flatten_style(value: &Value, out: &mut Vec<(String, Value)>) -> Result<(), PropError> {
    if value.kind() == 5 {
        let n = value.length().unwrap_or(0);
        for i in 0..n {
            let item = value.index(i)?;
            if item.truthy() {
                flatten_style(&item, out)?;
            }
        }
        return Ok(());
    }
    if value.kind() != 7 {
        return Ok(());
    }
    let keys = value.keys()?;
    let n = keys.length().unwrap_or(0);
    for i in 0..n {
        let key = keys.index(i)?.as_string().unwrap_or_default();
        let child = value.get(&key)?;
        if let Some(entry) = out.iter_mut().find(|(k, _)| *k == key) {
            entry.1 = child;
        } else {
            out.push((key, child));
        }
    }
    Ok(())
}

fn decode_style(
    entries: Vec<(String, Value)>,
    warnings: &Warnings,
) -> Result<(Box<Style>, StylePatch), PropError> {
    let mut style = Box::<Style>::default();
    let mut patch = StylePatch::default();
    for (key, value) in entries {
        if value.kind() == 0 {
            continue;
        }
        if key == "transform" && value.kind() == 5 {
            if let Some((property, steps)) = transform_list(&value)? {
                let decl = StyleDecl::SetTransform(property, steps);
                if patch.push(&mut style, decl).is_err() {
                    warnings.warn(format!(
                        "too many transforms (at most {})",
                        nlayer_core::MAX_TRANSFORMS
                    ));
                }
            }
            continue;
        }
        if key == "transformOrigin" && value.kind() == 5 {
            let mut words = Vec::new();
            let n = value.length().unwrap_or(0);
            for i in 0..n {
                let item = value.index(i)?;
                if let Some(number) = item.number_value() {
                    words.push(format!("{number}px"));
                } else if item.kind() == 4 {
                    words.push(string(&item));
                }
            }
            let joined = words.join(" ");
            if let Err(error) = patch.set(&mut style, &key, StyleValue::Str(&joined)) {
                warnings.warn(format!("style `{key}`: {error:?}"));
            }
            continue;
        }
        let owned;
        let decoded = match value.kind() {
            3 => StyleValue::Number(value.number_value().unwrap_or(0.0) as f32),
            4 => {
                owned = string(&value);
                StyleValue::Str(&owned)
            }
            0 | 1 => StyleValue::Null,
            _ => {
                warnings.warn(format!("style `{key}`: unsupported value type"));
                continue;
            }
        };
        match patch.set(&mut style, &key, decoded) {
            Ok(()) => {}
            Err(StyleError::UnknownProperty) => {
                warnings.warn(format!("unsupported style property `{key}`"))
            }
            Err(StyleError::InvalidValue) => {
                warnings.warn(format!("invalid value for style `{key}`"))
            }
        }
    }
    Ok((style, patch))
}

fn transform_list(
    items: &Value,
) -> Result<Option<(TransformProperty, Box<[(TransformKind, f32, Option<u32>)]>)>, PropError> {
    let mut steps = Vec::new();
    let n = items.length().unwrap_or(0);
    for i in 0..n {
        let item = items.index(i)?;
        if item.kind() != 7 {
            continue;
        }
        let keys = item.keys()?;
        let kn = keys.length().unwrap_or(0);
        for k in 0..kn {
            let key = keys.index(k)?.as_string().unwrap_or_default();
            let Some(kind) = TransformKind::parse(&key) else {
                continue;
            };
            let amount = item.get(&key)?;
            if let Some(amount) = transform_amount(&amount) {
                steps.push((kind, amount, None));
            }
        }
    }
    Ok((!steps.is_empty()).then(|| (TransformProperty::Transform, steps.into())))
}

fn transform_amount(value: &Value) -> Option<f32> {
    if let Some(n) = value.number_value() {
        return Some(n as f32);
    }
    let text = value.as_string()?;
    let text = text.trim();
    if let Some(deg) = text.strip_suffix("deg") {
        return deg.trim().parse().ok();
    }
    if let Some(rad) = text.strip_suffix("rad") {
        return rad.trim().parse::<f32>().ok().map(f32::to_degrees);
    }
    if let Some(px) = text.strip_suffix("px") {
        return px.trim().parse().ok();
    }
    text.parse().ok()
}
