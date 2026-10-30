//! The mounted tree a test step names.
//!
//! The Android host fills a [`Scene`] from the views it actually shows.
//! Selectors follow the headless runner: `id:`, `text:`, a bare name, ` >> `,
//! and `nth=`. A press, a type, and a scroll then go through the view system.

use serde_json::{Value, json};

/// One view the host is showing.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub id: u32,
    pub parent: Option<u32>,
    /// Child ids, front to back as the host stacks them.
    pub children: Vec<u32>,
    pub kind: Kind,
    pub test_id: Option<String>,
    /// `role`, label, hint, state, value and whether the subtree is hidden.
    pub accessibility: nlayer_core::Accessibility,
    pub text: Option<String>,
    /// Layout frame in the parent's box, in points.
    pub frame: Rect,
    pub hidden: bool,
    pub opacity: f32,
    pub pressable: bool,
    /// This view, or a field inside it, has focus.
    pub focused: bool,
    pub input: Option<InputSnap>,
    pub runs: Vec<Run>,
    pub spans: Vec<Span>,
    pub scroll: Option<ScrollSnap>,
    pub image: Option<ImageSnap>,
    pub transform: Vec<nlayer_core::transform::TransformOp>,
}

/// What kind of view a [`Node`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    View,
    Text,
    Image,
    Scroll,
    Input,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::View => "view",
            Kind::Text => "text",
            Kind::Image => "image",
            Kind::Scroll => "scroll",
            Kind::Input => "input",
        }
    }
}

/// A rectangle in points.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn center(self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// A single-line field's shown text.
#[derive(Debug, Clone)]
pub(crate) struct InputSnap {
    pub value: String,
    pub placeholder: String,
    pub secure: bool,
    pub keyboard: String,
    pub disabled: bool,
    /// The field has focus.
    pub focused: bool,
    /// The field takes several lines.
    pub multiline: bool,
}

/// One style run, reported on the tree as `spans`.
#[derive(Debug, Clone)]
pub(crate) struct Run {
    pub text: String,
    pub font_size: f32,
    pub font_weight: u16,
    pub italic: bool,
    pub color: u32,
}

/// A nested span a selector can name. `frame` is in that view's box.
#[derive(Debug, Clone)]
pub(crate) struct Span {
    pub start: u32,
    pub end: u32,
    pub test_id: Option<String>,
    /// The span's node when it takes a press.
    pub node: Option<u32>,
    pub text: String,
    pub frame: Rect,
}

/// A scroll view's offset and content, in points.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ScrollSnap {
    pub horizontal: bool,
    pub x: f32,
    pub y: f32,
    pub content_w: f32,
    pub content_h: f32,
}

/// An image's source, fit, and painted corner radii (points, when rounded).
#[derive(Debug, Clone)]
pub(crate) struct ImageSnap {
    pub src: String,
    pub fit: String,
    pub radius: Option<[f32; 4]>,
}

#[derive(Clone, Copy)]
enum Match {
    View(u32),
    Span { view: u32, index: usize },
}

impl Match {
    fn view(self) -> u32 {
        match self {
            Match::View(id) | Match::Span { view: id, .. } => id,
        }
    }
}

/// Where a press lands. The host turns this into a touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Press {
    /// The center of view `id`.
    View(u32),
    /// The center of a span drawn by `view`.
    Span { view: u32, start: u32, end: u32 },
}

/// `query`: ready to act, still waiting, or a selector that cannot match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Query {
    Ready,
    Wait,
    Fail(String),
}

/// The host's tree for one step.
pub(crate) struct Scene {
    nodes: Vec<Option<Node>>,
    root: u32,
    width: f32,
    height: f32,
}

impl Scene {
    /// `nodes` in any order. Children left empty are filled from `parent`,
    /// in the vector's order.
    pub(crate) fn from_nodes(nodes: Vec<Node>, width: f32, height: f32) -> Self {
        let root = nodes
            .iter()
            .find(|node| node.parent.is_none())
            .map(|node| node.id)
            .unwrap_or(1);
        let mut fill = nodes.iter().all(|node| node.children.is_empty());
        if nodes.len() == 1 {
            fill = false;
        }
        let mut indexed: Vec<Option<Node>> = Vec::new();
        for node in nodes {
            let id = node.id as usize;
            if indexed.len() <= id {
                indexed.resize_with(id + 1, || None);
            }
            indexed[id] = Some(node);
        }
        if fill {
            let order: Vec<(u32, Option<u32>)> = indexed
                .iter()
                .filter_map(|node| node.as_ref().map(|node| (node.id, node.parent)))
                .collect();
            for (id, parent) in order {
                let Some(parent) = parent else { continue };
                if let Some(Some(parent)) = indexed.get_mut(parent as usize) {
                    parent.children.push(id);
                }
            }
        }
        Self {
            nodes: indexed,
            root,
            width,
            height,
        }
    }

    pub(crate) fn texts(&self) -> Vec<String> {
        let mut out = Vec::new();
        for node in self.nodes.iter().flatten() {
            if node.kind == Kind::Text
                && let Some(text) = &node.text
            {
                out.push(text.clone());
            }
        }
        out
    }

    pub(crate) fn inputs(&self) -> Vec<String> {
        let mut out = Vec::new();
        for node in self.nodes.iter().flatten() {
            if let Some(input) = &node.input {
                out.push(input.value.clone());
            }
        }
        out
    }

    pub(crate) fn tree(&self) -> Value {
        self.node_json(self.root).unwrap_or(Value::Null)
    }

    pub(crate) fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.get(id as usize).and_then(|node| node.as_ref())
    }

    pub(crate) fn query(&self, selector: &str) -> Query {
        match self.resolve(selector) {
            Ok(hit) if self.visible(hit) => Query::Ready,
            Ok(_) => Query::Wait,
            Err(error) if error.contains("matches nothing") => Query::Wait,
            Err(error) => Query::Fail(error),
        }
    }

    pub(crate) fn press(&self, selector: &str) -> Result<Press, String> {
        let hit = self.resolve(selector)?;
        if !self.visible(hit) {
            return Err(format!("`{selector}` is not in view"));
        }
        match hit {
            Match::Span { view, index } => {
                let node = self.nodes[view as usize].as_ref().unwrap();
                let span = &node.spans[index];
                if node.accessibility.blocks_press() {
                    return Err(format!("`{selector}` does not take a press"));
                }
                if span.node.is_none() && !self.pressable(view) {
                    return Err(format!("`{selector}` does not take a press"));
                }
                Ok(Press::Span {
                    view,
                    start: span.start,
                    end: span.end,
                })
            }
            Match::View(id) => {
                if !self.pressable(id) {
                    return Err(format!("`{selector}` does not take a press"));
                }
                Ok(Press::View(id))
            }
        }
    }

    /// Center of `press`, in the root view's points. The host turns this
    /// into a touch and hit-tests it.
    pub(crate) fn point(&self, press: Press) -> Result<(f32, f32), String> {
        let hit = match press {
            Press::View(id) => Match::View(id),
            Press::Span { view, start, end } => {
                let index = self.nodes[view as usize]
                    .as_ref()
                    .and_then(|node| {
                        node.spans
                            .iter()
                            .position(|span| span.start == start && span.end == end)
                    })
                    .ok_or_else(|| "the span is gone".to_owned())?;
                Match::Span { view, index }
            }
        };
        self.absolute_frame(hit)
            .map(|frame| frame.center())
            .ok_or_else(|| "the view is gone".to_owned())
    }

    pub(crate) fn type_target(&self, selector: &str) -> Result<u32, String> {
        let hit = self.resolve(selector)?;
        if !self.visible(hit) {
            return Err(format!("`{selector}` is not in view"));
        }
        let Match::View(id) = hit else {
            return Err(format!("`{selector}` is not an input"));
        };
        let Some(input) = self.nodes[id as usize]
            .as_ref()
            .and_then(|node| node.input.as_ref())
        else {
            return Err(format!("`{selector}` is not an input"));
        };
        if input.disabled {
            return Err(format!("`{selector}` is disabled"));
        }
        Ok(id)
    }

    /// The scroll view `selector` names, or the nearest one around it.
    pub(crate) fn scroll_target(&self, selector: &str) -> Result<u32, String> {
        let hit = self.resolve(selector)?;
        if !self.visible(hit) {
            return Err(format!("`{selector}` is not in view"));
        }
        let mut id = hit.view();
        loop {
            if self
                .nodes
                .get(id as usize)
                .and_then(|node| node.as_ref())
                .is_some_and(|node| node.kind == Kind::Scroll)
            {
                return Ok(id);
            }
            id = self
                .nodes
                .get(id as usize)
                .and_then(|node| node.as_ref())
                .and_then(|node| node.parent)
                .ok_or_else(|| format!("`{selector}` is not in a scroll view"))?;
        }
    }

    fn pressable(&self, mut id: u32) -> bool {
        loop {
            let Some(node) = self.nodes.get(id as usize).and_then(|node| node.as_ref()) else {
                return false;
            };
            if node.accessibility.blocks_press() {
                return false;
            }
            if node.pressable {
                return true;
            }
            let Some(parent) = node.parent else {
                return false;
            };
            id = parent;
        }
    }

    fn visible(&self, hit: Match) -> bool {
        let Some(frame) = self.absolute_frame(hit) else {
            return false;
        };
        if frame.w <= 0.0 || frame.h <= 0.0 {
            return false;
        }
        if self.hidden(hit.view()) {
            return false;
        }
        let (x, y) = frame.center();
        if x < 0.0 || y < 0.0 || x > self.width || y > self.height {
            return false;
        }
        let mut id = hit.view();
        while let Some(parent) = self.nodes[id as usize]
            .as_ref()
            .and_then(|node| node.parent)
        {
            let Some(view) = self.nodes[parent as usize].as_ref() else {
                break;
            };
            if view.kind == Kind::Scroll {
                let origin = self
                    .absolute_frame(Match::View(parent))
                    .map(|frame| (frame.x, frame.y))
                    .unwrap_or((0.0, 0.0));
                let local_x = x - origin.0;
                let local_y = y - origin.1;
                if local_x < 0.0
                    || local_y < 0.0
                    || local_x > view.frame.w
                    || local_y > view.frame.h
                {
                    return false;
                }
            }
            id = parent;
        }
        true
    }

    fn hidden(&self, mut id: u32) -> bool {
        loop {
            let Some(node) = self.nodes.get(id as usize).and_then(|node| node.as_ref()) else {
                return false;
            };
            if node.hidden || node.opacity <= 0.0 {
                return true;
            }
            let Some(parent) = node.parent else {
                return false;
            };
            id = parent;
        }
    }

    fn absolute_frame(&self, hit: Match) -> Option<Rect> {
        let (id, local) = match hit {
            Match::View(id) => {
                let node = self.nodes.get(id as usize)?.as_ref()?;
                (id, node.frame)
            }
            Match::Span { view, index } => {
                let node = self.nodes.get(view as usize)?.as_ref()?;
                let span = node.spans.get(index)?;
                (view, span.frame)
            }
        };
        let (x, y) = self.absolute_origin(id)?;
        let (dx, dy) = match hit {
            Match::View(_) => (0.0, 0.0),
            Match::Span { .. } => (local.x, local.y),
        };
        Some(Rect {
            x: x + dx,
            y: y + dy,
            w: local.w,
            h: local.h,
        })
    }

    fn absolute_origin(&self, id: u32) -> Option<(f32, f32)> {
        let node = self.nodes.get(id as usize)?.as_ref()?;
        let mut x = node.frame.x;
        let mut y = node.frame.y;
        let mut parent = node.parent;
        while let Some(pid) = parent {
            let view = self.nodes.get(pid as usize)?.as_ref()?;
            if view.kind == Kind::Scroll
                && let Some(scroll) = view.scroll
            {
                x -= scroll.x;
                y -= scroll.y;
            }
            x += view.frame.x;
            y += view.frame.y;
            parent = view.parent;
        }
        Some((x, y))
    }

    fn node_json(&self, id: u32) -> Option<Value> {
        let node = self.nodes.get(id as usize)?.as_ref()?;
        let mut map = serde_json::Map::new();
        map.insert("kind".into(), node.kind.name().into());
        map.insert(
            "frame".into(),
            json!({
                "x": node.frame.x,
                "y": node.frame.y,
                "width": node.frame.w,
                "height": node.frame.h,
            }),
        );
        if let Some(text) = &node.text {
            map.insert("text".into(), text.clone().into());
        }
        if let Some(test_id) = &node.test_id {
            map.insert("testID".into(), test_id.clone().into());
        }
        let a = &node.accessibility;
        if a.role != nlayer_core::AccessibilityRole::None {
            map.insert("role".into(), a.role.name().into());
        }
        if let Some(label) = &a.label {
            map.insert("accessibilityLabel".into(), label.to_string().into());
        }
        if let Some(hint) = &a.hint {
            map.insert("accessibilityHint".into(), hint.to_string().into());
        }
        if a.hidden {
            map.insert("accessibilityHidden".into(), true.into());
        }
        if a.state != nlayer_core::AccessibilityState::default() {
            let mut state = serde_json::Map::new();
            state.insert("disabled".into(), a.state.disabled.into());
            state.insert("selected".into(), a.state.selected.into());
            if let Some(checked) = a.state.checked {
                state.insert(
                    "checked".into(),
                    match checked {
                        nlayer_core::Checked::False => false.into(),
                        nlayer_core::Checked::True => true.into(),
                        nlayer_core::Checked::Mixed => "mixed".into(),
                    },
                );
            }
            if let Some(expanded) = a.state.expanded {
                state.insert("expanded".into(), expanded.into());
            }
            map.insert("accessibilityState".into(), state.into());
        }
        if let Some(value) = &a.value {
            let mut json = serde_json::Map::new();
            if let Some(min) = value.min {
                json.insert("min".into(), min.into());
            }
            if let Some(max) = value.max {
                json.insert("max".into(), max.into());
            }
            if let Some(now) = value.now {
                json.insert("now".into(), now.into());
            }
            if let Some(text) = &value.text {
                json.insert("text".into(), text.to_string().into());
            }
            map.insert("accessibilityValue".into(), json.into());
        }
        if !node.runs.is_empty() {
            map.insert(
                "spans".into(),
                Value::Array(
                    node.runs
                        .iter()
                        .map(|run| {
                            json!({
                                "text": run.text,
                                "fontSize": run.font_size,
                                "fontWeight": run.font_weight,
                                "italic": run.italic,
                                "color": color_hex(run.color),
                            })
                        })
                        .collect(),
                ),
            );
        }
        if let Some(input) = &node.input {
            map.insert("value".into(), input.value.clone().into());
            map.insert("placeholder".into(), input.placeholder.clone().into());
            map.insert("secure".into(), input.secure.into());
            map.insert("keyboard".into(), input.keyboard.clone().into());
            map.insert("disabled".into(), input.disabled.into());
            map.insert("focused".into(), input.focused.into());
            map.insert("multiline".into(), input.multiline.into());
        }
        if node.focused {
            map.insert("focused".into(), true.into());
        }
        if let Some(image) = &node.image {
            map.insert("src".into(), image.src.clone().into());
            map.insert("fit".into(), image.fit.clone().into());
            if let Some(radius) = image.radius {
                map.insert("radius".into(), json!(radius));
            }
        }
        if node.kind == Kind::Scroll
            && let Some(scroll) = node.scroll
        {
            map.insert("horizontal".into(), scroll.horizontal.into());
            map.insert("offset".into(), json!({ "x": scroll.x, "y": scroll.y }));
            map.insert(
                "content".into(),
                json!({ "width": scroll.content_w, "height": scroll.content_h }),
            );
        }
        if !node.transform.is_empty() {
            map.insert(
                "transform".into(),
                Value::Array(
                    node.transform
                        .iter()
                        .map(|op| json!({ transform_name(op.kind): op.value }))
                        .collect(),
                ),
            );
        }
        map.insert(
            "children".into(),
            Value::Array(
                node.children
                    .iter()
                    .filter_map(|child| self.node_json(*child))
                    .collect(),
            ),
        );
        Some(Value::Object(map))
    }

    fn resolve(&self, source: &str) -> Result<Match, String> {
        let selector = parse(source)?;
        let mut scope: Option<Vec<Match>> = None;
        for part in &selector.parts {
            let mut hits = Vec::new();
            collect(self, part.matcher, scope.as_deref(), &mut hits);
            hits = pick(source, hits, part.nth)?;
            scope = Some(hits);
        }
        match scope.unwrap_or_default().as_slice() {
            [one] => Ok(*one),
            [] => Err(format!("`{source}` matches nothing")),
            many => Err(format!(
                "`{source}` matches {} views: say which (`{source} >> nth=0`)",
                many.len()
            )),
        }
    }
}

/// The offset after moving `distance` points toward the end of the content.
pub(crate) fn next_offset(current: f32, distance: f32, content: f32, viewport: f32) -> f32 {
    let span = (content - viewport).max(0.0);
    (current + distance).clamp(0.0, span)
}

fn transform_name(kind: nlayer_core::transform::TransformKind) -> &'static str {
    use nlayer_core::transform::TransformKind;
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

fn color_hex(packed: u32) -> String {
    let rgb = packed >> 8;
    let alpha = packed & 0xff;
    if alpha == 0xff {
        format!("#{rgb:06x}")
    } else {
        format!("#{packed:08x}")
    }
}

#[derive(Clone, Copy)]
enum Matcher<'a> {
    TestId(&'a str),
    Text(&'a str),
    Label(&'a str),
    Role {
        role: nlayer_core::AccessibilityRole,
        name: Option<&'a str>,
    },
    Any(&'a str),
}

struct Part<'a> {
    matcher: Matcher<'a>,
    nth: Option<usize>,
}

struct Selector<'a> {
    parts: Vec<Part<'a>>,
}

fn parse(source: &str) -> Result<Selector<'_>, String> {
    let mut parts: Vec<Part<'_>> = Vec::new();
    for piece in source.split(" >> ") {
        let piece = piece.trim();
        if let Some(n) = piece.strip_prefix("nth=") {
            let n = n
                .parse::<usize>()
                .map_err(|_| format!("`{source}`: `{piece}`: not an index"))?;
            let last = parts
                .last_mut()
                .ok_or_else(|| format!("`{source}`: `nth=` follows what it picks from"))?;
            last.nth = Some(n);
            continue;
        }
        if piece.is_empty() {
            return Err(format!("`{source}`: an empty selector"));
        }
        let matcher = match piece.split_once(':') {
            Some(("id", value)) => Matcher::TestId(value),
            Some(("text", value)) => Matcher::Text(value.trim()),
            Some(("label", value)) => Matcher::Label(value),
            Some(("role", value)) => {
                let (name, label) = match value.find('[') {
                    Some(open) if value.ends_with(']') => {
                        (&value[..open], Some(&value[open + 1..value.len() - 1]))
                    }
                    _ => (value, None),
                };
                let Some(role) = nlayer_core::AccessibilityRole::from_name(name) else {
                    return Err(format!("`{piece}`: unknown role `{name}`"));
                };
                Matcher::Role { role, name: label }
            }
            _ => Matcher::Any(piece),
        };
        parts.push(Part { matcher, nth: None });
    }
    if parts.is_empty() {
        return Err(format!("`{source}`: an empty selector"));
    }
    Ok(Selector { parts })
}

fn pick(source: &str, hits: Vec<Match>, nth: Option<usize>) -> Result<Vec<Match>, String> {
    if let Some(n) = nth {
        return Ok(hits.get(n).copied().into_iter().collect());
    }
    if hits.len() > 1 {
        return Err(format!(
            "`{source}` matches {} views: say which (`{source} >> nth=0`)",
            hits.len()
        ));
    }
    Ok(hits)
}

fn collect(scene: &Scene, matcher: Matcher<'_>, scope: Option<&[Match]>, out: &mut Vec<Match>) {
    if let Matcher::Any(want) = matcher {
        collect(scene, Matcher::TestId(want), scope, out);
        if out.is_empty() {
            collect(scene, Matcher::Text(want), scope, out);
        }
        return;
    }
    if let Some(scope) = scope {
        for hit in scope {
            if let Match::View(id) = *hit {
                let mut own = Vec::new();
                push(scene, id, matcher, &mut own);
                out.extend(
                    own.into_iter()
                        .filter(|hit| hit.view() != id || matches!(hit, Match::Span { .. })),
                );
                let children = scene.nodes[id as usize]
                    .as_ref()
                    .map(|node| node.children.clone())
                    .unwrap_or_default();
                for child in children {
                    walk(scene, child, &mut |child| push(scene, child, matcher, out));
                }
            }
        }
        return;
    }
    walk(scene, scene.root, &mut |id| push(scene, id, matcher, out));
}

fn walk(scene: &Scene, id: u32, f: &mut impl FnMut(u32)) {
    f(id);
    let children = scene
        .nodes
        .get(id as usize)
        .and_then(|node| node.as_ref())
        .map(|node| node.children.clone())
        .unwrap_or_default();
    for child in children {
        walk(scene, child, f);
    }
}

fn push(scene: &Scene, id: u32, matcher: Matcher<'_>, out: &mut Vec<Match>) {
    let Some(node) = scene.nodes.get(id as usize).and_then(|node| node.as_ref()) else {
        return;
    };
    match matcher {
        Matcher::TestId(want) => {
            if node.test_id.as_deref() == Some(want) {
                out.push(Match::View(id));
            }
            push_span_ids(node, id, want, out);
        }
        Matcher::Text(want) => {
            if text_eq(node.text.as_deref(), want) || input_eq(node, want) {
                out.push(Match::View(id));
            }
            push_span_text(node, id, want, out);
        }
        Matcher::Label(want) => {
            if node.accessibility.label.as_deref() == Some(want) {
                out.push(Match::View(id));
            }
        }
        Matcher::Role { role, name } => {
            let a = &node.accessibility;
            if a.role == role && name.is_none_or(|want| a.label.as_deref() == Some(want)) {
                out.push(Match::View(id));
            }
        }
        Matcher::Any(_) => {}
    }
}

fn push_span_ids(node: &Node, id: u32, want: &str, out: &mut Vec<Match>) {
    for (index, span) in node.spans.iter().enumerate() {
        if span.test_id.as_deref() == Some(want) {
            out.push(Match::Span { view: id, index });
        }
    }
}

fn push_span_text(node: &Node, id: u32, want: &str, out: &mut Vec<Match>) {
    // A nested span is named by its `testID`. `text:` matches that span's
    // text only when it has one. A pressable span without a `testID` is
    // not a match (headless and web do the same).
    for (index, span) in node.spans.iter().enumerate() {
        if span.test_id.is_none() {
            continue;
        }
        if text_eq(Some(&span.text), want) {
            out.push(Match::Span { view: id, index });
        }
    }
}

fn input_eq(node: &Node, want: &str) -> bool {
    let Some(input) = &node.input else {
        return false;
    };
    if text_eq(Some(&input.value), want) {
        return true;
    }
    input.value.is_empty() && text_eq(Some(&input.placeholder), want)
}

fn text_eq(text: Option<&str>, want: &str) -> bool {
    text.is_some_and(|text| text.trim() == want.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    fn view(id: u32, parent: Option<u32>, frame: Rect) -> Node {
        Node {
            id,
            parent,
            children: Vec::new(),
            kind: Kind::View,
            test_id: None,
            accessibility: nlayer_core::Accessibility::default(),
            text: None,
            frame,
            hidden: false,
            opacity: 1.0,
            pressable: false,
            focused: false,
            input: None,
            runs: Vec::new(),
            spans: Vec::new(),
            scroll: None,
            image: None,
            transform: Vec::new(),
        }
    }

    #[test]
    fn a_bare_name_is_the_test_id_before_the_text() {
        let mut named = view(2, Some(1), rect(0.0, 0.0, 10.0, 10.0));
        named.test_id = Some("pad".into());
        named.pressable = true;
        let mut labeled = view(3, Some(1), rect(0.0, 20.0, 40.0, 10.0));
        labeled.kind = Kind::Text;
        labeled.text = Some("pad".into());
        let mut root = view(1, None, rect(0.0, 0.0, 100.0, 100.0));
        root.pressable = true;
        let scene = Scene::from_nodes(vec![root, named, labeled], 100.0, 100.0);
        assert!(matches!(scene.press("pad"), Ok(Press::View(2))));
        assert!(matches!(scene.press("text:pad"), Ok(Press::View(3))));
        assert!(
            scene
                .press("missing")
                .unwrap_err()
                .contains("matches nothing")
        );
    }

    #[test]
    fn two_matches_need_an_index_and_a_span_presses_itself() {
        let mut block = view(2, Some(1), rect(0.0, 0.0, 80.0, 20.0));
        block.kind = Kind::Text;
        block.text = Some("see docs".into());
        block.runs.push(Run {
            text: "docs".into(),
            font_size: 14.0,
            font_weight: 400,
            italic: false,
            color: 0xff00_00ff,
        });
        block.spans.push(Span {
            start: 4,
            end: 8,
            test_id: Some("docs".into()),
            node: Some(9),
            text: "docs".into(),
            frame: rect(20.0, 0.0, 24.0, 12.0),
        });
        let mut other = view(3, Some(1), rect(0.0, 30.0, 10.0, 10.0));
        other.test_id = Some("docs".into());
        let scene = Scene::from_nodes(
            vec![view(1, None, rect(0.0, 0.0, 100.0, 100.0)), block, other],
            100.0,
            100.0,
        );
        assert!(scene.press("id:docs").is_err());
        assert_eq!(
            scene.press("id:docs >> nth=0").unwrap(),
            Press::Span {
                view: 2,
                start: 4,
                end: 8
            }
        );
        let tree = scene.tree();
        assert_eq!(tree["children"][0]["spans"][0]["text"], "docs");
        assert_eq!(tree["children"][0]["spans"][0]["color"], "#ff0000");
    }

    #[test]
    fn a_scrolled_off_view_is_not_in_view_and_the_offset_clamps() {
        let mut scroll = view(2, Some(1), rect(0.0, 0.0, 100.0, 40.0));
        scroll.kind = Kind::Scroll;
        scroll.scroll = Some(ScrollSnap {
            horizontal: false,
            x: 0.0,
            y: 50.0,
            content_w: 100.0,
            content_h: 200.0,
        });
        let mut row = view(3, Some(2), rect(0.0, 0.0, 100.0, 20.0));
        row.test_id = Some("row".into());
        row.pressable = true;
        let scene = Scene::from_nodes(
            vec![view(1, None, rect(0.0, 0.0, 100.0, 100.0)), scroll, row],
            100.0,
            100.0,
        );
        assert!(scene.press("row").unwrap_err().contains("is not in view"));
        assert_eq!(next_offset(0.0, 30.0, 200.0, 40.0), 30.0);
        assert_eq!(next_offset(150.0, 30.0, 200.0, 40.0), 160.0);
        assert_eq!(next_offset(0.0, -10.0, 200.0, 40.0), 0.0);
    }

    #[test]
    fn typing_names_the_field_and_not_a_disabled_one() {
        let mut field = view(2, Some(1), rect(0.0, 0.0, 80.0, 24.0));
        field.kind = Kind::Input;
        field.test_id = Some("name".into());
        field.input = Some(InputSnap {
            value: String::new(),
            placeholder: "Name".into(),
            secure: false,
            keyboard: "text".into(),
            disabled: false,
            focused: false,
            multiline: false,
        });
        let mut disabled = view(3, Some(1), rect(0.0, 30.0, 80.0, 24.0));
        disabled.kind = Kind::Input;
        disabled.test_id = Some("lock".into());
        disabled.input = Some(InputSnap {
            value: String::new(),
            placeholder: String::new(),
            secure: false,
            keyboard: "text".into(),
            disabled: true,
            focused: false,
            multiline: false,
        });
        let scene = Scene::from_nodes(
            vec![view(1, None, rect(0.0, 0.0, 100.0, 100.0)), field, disabled],
            100.0,
            100.0,
        );
        assert_eq!(scene.type_target("Name").unwrap(), 2);
        assert!(scene.type_target("lock").unwrap_err().contains("disabled"));
        assert_eq!(scene.inputs(), ["", ""]);
    }

    #[test]
    fn text_names_a_span_only_when_it_has_a_test_id() {
        let mut block = view(2, Some(1), rect(0.0, 0.0, 80.0, 20.0));
        block.kind = Kind::Text;
        block.text = Some("see docs link".into());
        block.spans.push(Span {
            start: 4,
            end: 8,
            test_id: Some("docs".into()),
            node: Some(4),
            text: "docs".into(),
            frame: rect(20.0, 0.0, 24.0, 12.0),
        });
        block.spans.push(Span {
            start: 9,
            end: 13,
            test_id: None,
            node: Some(5),
            text: "link".into(),
            frame: rect(48.0, 0.0, 24.0, 12.0),
        });
        let scene = Scene::from_nodes(
            vec![view(1, None, rect(0.0, 0.0, 100.0, 100.0)), block],
            100.0,
            100.0,
        );
        assert_eq!(
            scene.press("text:docs").unwrap(),
            Press::Span {
                view: 2,
                start: 4,
                end: 8
            }
        );
        assert!(scene.press("text:link").unwrap_err().contains("nothing"));
        assert!(scene.press("link").unwrap_err().contains("nothing"));
    }
}
