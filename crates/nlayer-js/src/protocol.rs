//! Decoding of JS prop values into the core's types: styles
//! ([`decode_style`]) and the v1 host props ([`decode_prop`]).

use std::sync::Arc;

use nlayer_core::{
    AccessibilityProp, AccessibilityRole, AccessibilityState, AccessibilityValue, CellSlot,
    Checked, ImageScales, ImageSource, Interaction, KeyboardDismiss, KeyboardTaps, KeyboardType,
    NodeId, Op, Prop, Tag,
    geometry::{Points, Size},
    style::{
        Style, StyleDecl, StyleError, StylePatch, StyleValue, TextStyle, TransformProperty,
        ViewStyle,
    },
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
    if name == "style" || is_state_style(name) {
        if name != "style" && cleared(&value) {
            return Ok(Some(state_style(id, name, None)));
        }
        let mut entries = Vec::new();
        style_entries(&value, &mut entries)?;
        let (style, patch) = decode_style(entries, warnings)?;
        if name == "style" {
            return Ok(Some(Op::SetStyle { id, style, patch }));
        }
        let style = *style;
        let paint = paints(&style).then(|| Box::new((style.view, style.text)));
        return Ok(Some(state_style(id, name, paint)));
    }
    if name == "testID" {
        let name = string(&value)?;
        let test_id = (!name.is_empty()).then(|| Arc::<str>::from(name));
        return Ok(Some(Op::SetProp {
            id,
            prop: Prop::TestId(test_id),
        }));
    }
    if name == "class" || name == "classList" {
        let names = if name == "class" {
            nlayer_core::class_tokens(&string(&value)?)
                .map(str::to_owned)
                .collect()
        } else {
            class_list_names(&value)?
        };
        if let Some(message) =
            nlayer_core::unknown_classes_warning(names.iter().map(String::as_str))
        {
            warnings.warn(message);
        }
        return Ok(None);
    }
    if name == "disabled" {
        return Ok(Some(Op::SetProp {
            id,
            prop: Prop::Disabled(value.as_bool().unwrap_or(false)),
        }));
    }
    if let Some(prop) = accessibility_prop(name, &value, warnings)? {
        return Ok(Some(Op::SetProp { id, prop }));
    }
    let prop = match (tag, name) {
        (Some(Tag::Image), "src") => Prop::Source(image_source(value)?),
        (Some(Tag::Scroll), "horizontal") => Prop::Horizontal(value.as_bool().unwrap_or(false)),
        (Some(Tag::Scroll), "keyboardShouldPersistTaps") => match keyboard_taps(&value) {
            Some(taps) => Prop::KeyboardTaps(taps),
            None => {
                warnings.warn(format!(
                    "unknown keyboardShouldPersistTaps `{}`",
                    string(&value)?
                ));
                return Ok(None);
            }
        },
        (Some(Tag::Scroll), "keyboardDismissMode") => {
            let name = string(&value)?;
            match KeyboardDismiss::from_name(&name) {
                Some(mode) => Prop::KeyboardDismiss(mode),
                None => {
                    warnings.warn(format!("unknown keyboardDismissMode `{name}`"));
                    return Ok(None);
                }
            }
        }
        (Some(Tag::List), "count") => Prop::Count(value.as_number().unwrap_or(0.0).max(0.0) as u32),
        (Some(Tag::List), "estimatedItemSize") => {
            Prop::EstimatedItemSize(Points(number(&value).unwrap_or(50.0)))
        }
        (Some(Tag::List), "numColumns") => {
            Prop::Columns(value.as_number().unwrap_or(1.0).max(1.0) as u32)
        }
        (Some(Tag::List), "loadMoreThreshold") => {
            Prop::LoadMoreThreshold(number(&value).unwrap_or(1.0))
        }
        (Some(Tag::List), "splice") => {
            let Some(array) = value.as_array() else {
                warnings.warn("splice is not an array".to_owned());
                return Ok(None);
            };
            let n = |i: usize| array.get::<f64>(i).unwrap_or(0.0).max(0.0) as u32;
            Prop::Splice {
                at: n(0),
                removed: n(1),
                inserted: n(2),
            }
        }
        (Some(Tag::Cell), "slot") => {
            let slot = if let Some(n) = value.as_number() {
                CellSlot::Item(n.max(0.0) as u32)
            } else {
                match string(&value)?.as_str() {
                    "header" => CellSlot::Header,
                    "footer" => CellSlot::Footer,
                    "empty" => CellSlot::Empty,
                    other => {
                        warnings.warn(format!("unknown list slot `{other}`"));
                        return Ok(None);
                    }
                }
            };
            Prop::Slot(slot)
        }
        (Some(Tag::Cell), "cellIndex") => {
            Prop::CellIndex(value.as_number().unwrap_or(0.0).max(0.0) as u32)
        }
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
        (Some(Tag::Input), "multiline") => Prop::Multiline(value.as_bool().unwrap_or(false)),
        (Some(Tag::Input), "selectedRange") => selected_range(&value)?,
        _ => {
            warnings.warn(format!("unsupported prop `{name}`"));
            return Ok(None);
        }
    };
    Ok(Some(Op::SetProp { id, prop }))
}

/// An accessibility prop, when `name` is one. `Role` warns and sets
/// nothing for an unknown name; a null label, hint or value clears it.
fn accessibility_prop<'js>(
    name: &str,
    value: &Value<'js>,
    warnings: &mut Warnings,
) -> rquickjs::Result<Option<Prop>> {
    let prop = match name {
        "accessibilityLabel" => AccessibilityProp::Label(optional_string(value)?),
        "accessibilityHint" => AccessibilityProp::Hint(optional_string(value)?),
        "accessibilityElementsHidden" => {
            AccessibilityProp::Hidden(value.as_bool().unwrap_or(false))
        }
        "accessibilityValue" => AccessibilityProp::Value(accessibility_value(value)?),
        "accessibilityState" => AccessibilityProp::State(accessibility_state(value)?),
        "role" => {
            let Some(name) = value.as_string().map(|s| s.to_string()).transpose()? else {
                return Ok(Some(Prop::Accessibility(AccessibilityProp::Role(
                    AccessibilityRole::None,
                ))));
            };
            match AccessibilityRole::from_name(&name) {
                Some(role) => AccessibilityProp::Role(role),
                None => {
                    warnings.warn(format!("unknown role `{name}`"));
                    return Ok(None);
                }
            }
        }
        _ => return Ok(None),
    };
    Ok(Some(Prop::Accessibility(prop)))
}

/// A label or hint: `undefined` / `null` clears it.
fn optional_string(value: &Value<'_>) -> rquickjs::Result<Option<Arc<str>>> {
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    Ok(value
        .as_string()
        .map(|s| Arc::<str>::from(s.to_string().unwrap_or_default())))
}

/// `accessibilityState`: absent fields are false / none.
fn accessibility_state(value: &Value<'_>) -> rquickjs::Result<AccessibilityState> {
    let Some(object) = value.as_object() else {
        return Ok(AccessibilityState::default());
    };
    let checked = match object.get::<_, Option<Value>>("checked")? {
        Some(value) if value.as_bool() == Some(true) => Some(Checked::True),
        Some(value) if value.as_bool() == Some(false) => Some(Checked::False),
        Some(value)
            if value
                .as_string()
                .and_then(|s| s.to_string().ok())
                .as_deref()
                == Some("mixed") =>
        {
            Some(Checked::Mixed)
        }
        _ => None,
    };
    Ok(AccessibilityState {
        disabled: object.get::<_, Option<bool>>("disabled")?.unwrap_or(false),
        selected: object.get::<_, Option<bool>>("selected")?.unwrap_or(false),
        checked,
        expanded: object.get::<_, Option<bool>>("expanded")?,
    })
}

/// `accessibilityValue`: `min` / `max` / `now` numbers, `text` a string.
fn accessibility_value(value: &Value<'_>) -> rquickjs::Result<Option<AccessibilityValue>> {
    if value.is_null() || value.is_undefined() {
        return Ok(None);
    }
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    Ok(Some(AccessibilityValue {
        min: field_f32(object, "min")?,
        max: field_f32(object, "max")?,
        now: field_f32(object, "now")?,
        text: optional_string(&object.get::<_, Value>("text")?)?,
    }))
}

fn is_state_style(name: &str) -> bool {
    matches!(name, "activeStyle" | "hoverStyle" | "focusStyle")
}

fn cleared(value: &Value<'_>) -> bool {
    value.is_null() || value.is_undefined() || value.as_bool() == Some(false)
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

/// A string prop; anything else (null, undefined) is the empty string.
fn string(value: &Value<'_>) -> rquickjs::Result<String> {
    match value.as_string() {
        Some(s) => s.to_string(),
        None => Ok(String::new()),
    }
}

/// `true` is `always`, `false` is `never`. A string is the named mode.
fn keyboard_taps(value: &Value<'_>) -> Option<KeyboardTaps> {
    if let Some(on) = value.as_bool() {
        return Some(if on {
            KeyboardTaps::Always
        } else {
            KeyboardTaps::Never
        });
    }
    let name = value.as_string()?.to_string().ok()?;
    KeyboardTaps::from_name(&name)
}

/// `selectedRange`: `{ start, end? }` in UTF-16 units. A missing or
/// negative end is the start (a caret).
fn selected_range(value: &Value<'_>) -> rquickjs::Result<Prop> {
    let Some(object) = value.as_object() else {
        return Ok(Prop::Selection { start: 0, end: 0 });
    };
    let start = field_f32(object, "start")?.unwrap_or(0.0).max(0.0) as u32;
    let end = field_f32(object, "end")?.unwrap_or(start as f32).max(0.0) as u32;
    Ok(Prop::Selection { start, end })
}

fn truthy(value: &Value<'_>) -> rquickjs::Result<bool> {
    if value.is_null() || value.is_undefined() {
        return Ok(false);
    }
    if let Some(on) = value.as_bool() {
        return Ok(on);
    }
    if let Some(n) = value.as_number() {
        return Ok(n != 0.0 && !n.is_nan());
    }
    if let Some(s) = value.as_string() {
        return Ok(!s.to_string()?.is_empty());
    }
    Ok(true)
}

/// `classList`: the names whose value is truthy.
fn class_list_names(value: &Value<'_>) -> rquickjs::Result<Vec<String>> {
    let Some(obj) = value.as_object() else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    for prop in obj.props::<String, Value>() {
        let (key, value) = prop?;
        if truthy(&value)? {
            names.push(key);
        }
    }
    Ok(names)
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
        if let Some(property) = axis_property(&key)
            && let Some(items) = value.as_array()
        {
            // `[x, y]`: numbers or CSS strings. Animated values are not v1.
            if let Some(steps) = transform_axes(&key, items, warnings)? {
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

/// `translate` / `scale`, the properties that take `[x, y]`.
fn axis_property(key: &str) -> Option<TransformProperty> {
    match key {
        "translate" => Some(TransformProperty::Translate),
        "scale" => Some(TransformProperty::Scale),
        _ => None,
    }
}

/// `translate: [x, y]` / `scale: [x, y]`. `translate: [x]` is x alone;
/// `scale: [k]` is both axes. An invalid value drops the property.
fn transform_axes(
    key: &str,
    items: &Array<'_>,
    warnings: &mut Warnings,
) -> rquickjs::Result<Option<TransformSteps>> {
    let translate = key == "translate";
    let len = items.len();
    if !(1..=2).contains(&len) {
        warnings.warn(format!("style `{key}` takes one value, or `[x, y]`"));
        return Ok(None);
    }
    let mut steps = Vec::with_capacity(len);
    for (i, item) in items.iter::<Value>().enumerate() {
        let item = item?;
        let kind = match (translate, i, len) {
            (true, 0, _) => TransformKind::TranslateX,
            (true, _, _) => TransformKind::TranslateY,
            (false, 0, 1) => TransformKind::Scale,
            (false, 0, _) => TransformKind::ScaleX,
            (false, _, _) => TransformKind::ScaleY,
        };
        let step = if let Some(n) = item.as_number() {
            Some((kind, n as f32))
        } else if let Some(text) = item.as_string() {
            let text = text.to_string()?;
            if translate {
                nlayer_core::transform::translation(i == 1, &text).map(|op| (op.kind, op.value))
            } else {
                scale_amount(&text).map(|value| (kind, value))
            }
        } else {
            None
        };
        match step {
            Some((kind, value)) => steps.push((kind, value, None)),
            None => {
                warnings.warn(format!("invalid value in style `{key}`"));
                return Ok(None);
            }
        }
    }
    Ok(Some(steps.into()))
}

/// CSS's `scale` amount: `1.5` or `150%`.
fn scale_amount(text: &str) -> Option<f32> {
    let text = text.trim();
    match text.strip_suffix('%') {
        Some(percent) => percent.trim().parse::<f32>().ok().map(|n| n / 100.0),
        None => text.parse().ok(),
    }
}
