//! Decoding of JS prop values into the core's types: styles
//! ([`decode_style`]) and the v1 host props ([`decode_prop`]).

use nlayer_core::{
    ImageScales, ImageSource, KeyboardType, NodeId, Op, Prop, Tag,
    geometry::{Points, Size},
    style::{Style, StyleDecl, StyleError, StylePatch, StyleValue, TransformProperty},
    transform::TransformKind,
};
use rquickjs::{Array, Object, Value};
use rustc_hash::FxHashSet;

/// Reports each distinct problem once, so a bad style in a list does not
/// flood the log.
#[derive(Default)]
pub(crate) struct Warnings {
    seen: FxHashSet<String>,
    deferred: Option<Vec<String>>,
}

impl Warnings {
    /// Warnings that are only collected, to be merged into the VM's
    /// (when decoding runs JS that may itself decode).
    pub(crate) fn deferred() -> Warnings {
        Warnings {
            seen: FxHashSet::default(),
            deferred: Some(Vec::new()),
        }
    }

    pub(crate) fn warn(&mut self, message: String) {
        if let Some(pending) = &mut self.deferred {
            pending.push(message);
        } else if self.seen.len() < 1024 && self.seen.insert(message.clone()) {
            log::warn!("{message}");
        }
    }

    /// Logs what `other` collected and was not reported yet.
    pub(crate) fn merge(&mut self, other: Warnings) {
        for message in other.deferred.unwrap_or_default() {
            self.warn(message);
        }
    }
}

fn number(value: &Value<'_>) -> Option<f32> {
    value.as_number().map(|n| n as f32)
}

fn field_f32(obj: &Object<'_>, key: &str) -> rquickjs::Result<Option<f32>> {
    Ok(obj.get::<_, Option<f64>>(key)?.map(|v| v as f32))
}

/// The op setting prop `name` to `value`, if the prop is one v1 has.
pub(crate) fn decode_prop<'js>(
    id: NodeId,
    tag: Option<Tag>,
    name: &str,
    value: Value<'js>,
    warnings: &mut Warnings,
) -> rquickjs::Result<Option<Op>> {
    if name == "style" {
        let mut entries = Vec::new();
        style_entries(&value, &mut entries)?;
        let (style, patch) = decode_style(entries, warnings)?;
        return Ok(Some(Op::SetStyle { id, style, patch }));
    }
    let prop = match (tag, name) {
        (Some(Tag::Image), "src") => Prop::Source(image_source(value)?),
        (Some(Tag::Scroll), "horizontal") => Prop::Horizontal(value.as_bool().unwrap_or(false)),
        (Some(Tag::Input), "value") => Prop::Value(string(&value)?),
        (Some(Tag::Input), "placeholder") => Prop::Placeholder(string(&value)?),
        (Some(Tag::Input), "secureTextEntry") => Prop::Secure(value.as_bool().unwrap_or(false)),
        (Some(Tag::Input), "keyboardType") => {
            let name = string(&value)?;
            match KeyboardType::from_name(&name) {
                Some(keyboard) => Prop::Keyboard(keyboard),
                None => {
                    warnings.warn(format!("unknown keyboardType `{name}`"));
                    return Ok(None);
                }
            }
        }
        (Some(Tag::Input), "disabled") => Prop::Disabled(value.as_bool().unwrap_or(false)),
        _ => {
            warnings.warn(format!("unsupported prop `{name}`"));
            return Ok(None);
        }
    };
    Ok(Some(Op::SetProp { id, prop }))
}

/// A string prop; anything else (null, undefined) is the empty string.
fn string(value: &Value<'_>) -> rquickjs::Result<String> {
    match value.as_string() {
        Some(s) => s.to_string(),
        None => Ok(String::new()),
    }
}

/// `src`: a URL or a bundled asset (`{ url, size, scales }`).
fn image_source(value: Value<'_>) -> rquickjs::Result<Option<ImageSource>> {
    if let Some(url) = value.as_string() {
        let url = url.to_string()?;
        return Ok((!url.is_empty()).then(|| ImageSource {
            uri: url.into(),
            size: None,
            scales: ImageScales::NONE,
        }));
    }
    let Some(obj) = value.as_object() else {
        return Ok(None);
    };
    let Some(url) = obj.get::<_, Option<String>>("url")? else {
        return Ok(None);
    };
    let size = match obj.get::<_, Option<Object>>("size")? {
        Some(size) => match (field_f32(&size, "width")?, field_f32(&size, "height")?) {
            (Some(w), Some(h)) => Some(Size::new(Points(w), Points(h))),
            _ => None,
        },
        None => None,
    };
    let scales = match obj.get::<_, Option<Array>>("scales")? {
        Some(list) => list
            .iter::<f64>()
            .filter_map(|v| v.ok())
            .map(|v| v as f32)
            .collect(),
        None => ImageScales::NONE,
    };
    Ok(Some(ImageSource {
        uri: url.into(),
        size,
        scales,
    }))
}

/// The properties of a style: an object, or (React Native style) an array
/// of styles, possibly nested, merged left to right; falsy entries are
/// skipped. A property set by a later style replaces it whole.
fn style_entries<'js>(
    value: &Value<'js>,
    out: &mut Vec<(String, Value<'js>)>,
) -> rquickjs::Result<()> {
    if let Some(items) = value.as_array() {
        for item in items.iter::<Value>() {
            style_entries(&item?, out)?;
        }
    } else if let Some(obj) = value.as_object() {
        for prop in obj.props::<String, Value>() {
            let (key, value) = prop?;
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value,
                None => out.push((key, value)),
            }
        }
    }
    Ok(())
}

fn decode_style<'js>(
    entries: Vec<(String, Value<'js>)>,
    warnings: &mut Warnings,
) -> rquickjs::Result<(Box<Style>, StylePatch)> {
    let mut style = Box::<Style>::default();
    let mut patch = StylePatch::default();
    for (key, value) in entries {
        // `undefined` sets nothing (`color: active ? c : undefined`): the
        // default stays, as in a spread object.
        if value.is_undefined() {
            continue;
        }
        if key == "transform"
            && let Some(items) = value.as_array()
        {
            // The list of steps, which CSS's string cannot hold.
            if let Some((property, steps)) = transform_list(items)? {
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
        if key == "transformOrigin"
            && let Some(items) = value.as_array()
        {
            // An array of coordinates: the string form.
            let mut words = Vec::new();
            for item in items.iter::<Value>() {
                let item = item?;
                if let Some(n) = item.as_number() {
                    words.push(format!("{n}px"));
                } else if let Some(s) = item.as_string() {
                    words.push(s.to_string()?);
                }
            }
            let joined = words.join(" ");
            if let Err(e) = patch.set(&mut style, &key, StyleValue::Str(&joined)) {
                warnings.warn(format!("style `{key}`: {e:?}"));
            }
            continue;
        }
        let string;
        let v = match value.type_of() {
            rquickjs::Type::Int | rquickjs::Type::Float => {
                StyleValue::Number(value.as_number().unwrap_or_default() as f32)
            }
            rquickjs::Type::String => {
                string = value.as_string().expect("string").to_string()?;
                StyleValue::Str(&string)
            }
            rquickjs::Type::Null | rquickjs::Type::Undefined => StyleValue::Null,
            _ => {
                warnings.warn(format!("style `{key}`: unsupported value type"));
                continue;
            }
        };
        match patch.set(&mut style, &key, v) {
            Ok(()) => {}
            Err(StyleError::UnknownProperty) => {
                warnings.warn(format!("unsupported style property `{key}`"))
            }
            Err(StyleError::InvalidValue) => {
                warnings.warn(format!("invalid value for style `{key}`: {v:?}"))
            }
        }
    }
    Ok((style, patch))
}

/// The steps of a `transform` list.
type TransformSteps = Box<[(TransformKind, f32, Option<u32>)]>;

/// A `transform` list: its steps, in order.
fn transform_list(
    items: &Array<'_>,
) -> rquickjs::Result<Option<(TransformProperty, TransformSteps)>> {
    let mut steps: Vec<(TransformKind, f32, Option<u32>)> = Vec::new();
    for item in items.iter::<Value>() {
        let item = item?;
        let Some(obj) = item.as_object() else {
            continue;
        };
        for entry in obj.props::<String, Value>() {
            let (key, value) = entry?;
            let Some(kind) = TransformKind::parse(&key) else {
                continue;
            };
            if let Some(amount) = transform_amount(kind, &value) {
                steps.push((kind, amount, None));
            }
        }
    }
    Ok((!steps.is_empty()).then(|| (TransformProperty::Transform, steps.into())))
}

/// The amount of one transform step: numbers are degrees for an angle and
/// points for a translation.
fn transform_amount(_kind: TransformKind, value: &Value<'_>) -> Option<f32> {
    if let Some(n) = number(value) {
        return Some(n);
    }
    let text = value.as_string()?.to_string().ok()?;
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
