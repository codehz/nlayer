//! What a test step names a view with.
//!
//! `id:<testID>` is that `testID`. `text:<text>` is a text view's content,
//! an input's value, or an empty input's placeholder, compared with the
//! ends trimmed. `label:<text>` is a view's `accessibilityLabel`.
//! `role:<role>[<label>]` is a view's `role`, and with a label the view
//! whose `accessibilityLabel` is it. Anything else is a `testID`, then that
//! text. ` >> ` looks inside the previous match. `nth=<n>` keeps the nth
//! match, from 0. None or several is an error.

use nlayer_core::{
    AccessibilityRole, NodeId, ViewKind,
    geometry::{Bounds, Points, point, px},
    text::{ApproxTextEngine, TextEngine},
};

use crate::Screen;

/// One matched view, or a nested span drawn by a text view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// A mounted view.
    View(NodeId),
    /// A nested `<Text>` span. `node` is that span when it takes presses.
    Span {
        /// The text view that draws it.
        view: NodeId,
        /// UTF-16 start, inclusive.
        start: u32,
        /// UTF-16 end, exclusive.
        end: u32,
        /// The span's node, when it listens to presses.
        node: Option<NodeId>,
    },
}

impl Target {
    /// The mounted view a hit test of this target lands on.
    pub(crate) fn view(self) -> NodeId {
        match self {
            Target::View(id) | Target::Span { view: id, .. } => id,
        }
    }
}

#[derive(Debug, Clone)]
struct Selector {
    parts: Vec<Part>,
    source: String,
}

#[derive(Debug, Clone)]
struct Part {
    matcher: Matcher,
    nth: Option<usize>,
}

#[derive(Debug, Clone)]
enum Matcher {
    TestId(String),
    Text(String),
    Label(String),
    Role {
        role: AccessibilityRole,
        name: Option<String>,
    },
    Any(String),
}

impl Selector {
    fn parse(source: &str) -> Result<Self, String> {
        let mut parts: Vec<Part> = Vec::new();
        for piece in source.split(" >> ") {
            let piece = piece.trim();
            if let Some(n) = piece.strip_prefix("nth=") {
                let n = n
                    .parse::<usize>()
                    .map_err(|e| format!("`{source}`: `{piece}`: not an index: {e}"))?;
                let last = parts
                    .last_mut()
                    .ok_or_else(|| format!("`{source}`: `nth=` follows what it picks from"))?;
                last.nth = Some(n);
                continue;
            }
            if piece.is_empty() {
                return Err(format!("`{source}`: an empty selector"));
            }
            parts.push(Part {
                matcher: Matcher::parse(piece)?,
                nth: None,
            });
        }
        if parts.is_empty() {
            return Err(format!("`{source}`: an empty selector"));
        }
        Ok(Self {
            parts,
            source: source.to_owned(),
        })
    }
}

impl Matcher {
    fn parse(piece: &str) -> Result<Self, String> {
        Ok(match piece.split_once(':') {
            Some(("id", value)) => Self::TestId(value.to_owned()),
            Some(("text", value)) => Self::Text(value.trim().to_owned()),
            Some(("label", value)) => Self::Label(value.to_owned()),
            Some(("role", value)) => {
                let (name, label) = match value.find('[') {
                    Some(open) if value.ends_with(']') => (
                        &value[..open],
                        Some(value[open + 1..value.len() - 1].to_owned()),
                    ),
                    _ => (value, None),
                };
                let Some(role) = AccessibilityRole::from_name(name) else {
                    return Err(format!("`{piece}`: unknown role `{name}`"));
                };
                Self::Role { role, name: label }
            }
            _ => Self::Any(piece.to_owned()),
        })
    }
}

/// The one view `selector` names. Several matches, or none, is an error.
pub(crate) fn resolve(screen: &Screen, selector: &str) -> Result<Target, String> {
    let selector = Selector::parse(selector)?;
    let mut scope: Option<Vec<Target>> = None;
    for part in &selector.parts {
        let mut hits = Vec::new();
        collect(screen, &part.matcher, scope.as_deref(), &mut hits);
        let hits = pick(&selector.source, &hits, part.nth)?;
        scope = Some(hits);
    }
    let hits = scope.unwrap_or_default();
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!("`{}` matches nothing", selector.source)),
        many => Err(format!(
            "`{}` matches {} views: say which (`{} >> nth=0`)",
            selector.source,
            many.len(),
            selector.source
        )),
    }
}

/// `true` when `target`'s center is inside the viewport and every scroll
/// view around it, and neither it nor an ancestor is hidden.
pub(crate) fn visible(screen: &Screen, target: Target) -> bool {
    let Some(frame) = target_frame(screen, target) else {
        return false;
    };
    if frame.size.width.0 <= 0.0 || frame.size.height.0 <= 0.0 {
        return false;
    }
    if hidden(screen, target.view()) {
        return false;
    }
    let center = point(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    let viewport = screen.viewport();
    if center.x < px(0.0)
        || center.y < px(0.0)
        || center.x > viewport.width
        || center.y > viewport.height
    {
        return false;
    }
    let mut id = target.view();
    while let Some(parent) = screen.parent(id) {
        let Some(view) = screen.view(parent) else {
            break;
        };
        if view.kind == ViewKind::Scroll {
            let origin = absolute_origin(screen, parent);
            let x = center.x - origin.x;
            let y = center.y - origin.y;
            if x < px(0.0) || y < px(0.0) || x > view.frame.size.width || y > view.frame.size.height
            {
                return false;
            }
        }
        id = parent;
    }
    true
}

fn collect(screen: &Screen, matcher: &Matcher, scope: Option<&[Target]>, out: &mut Vec<Target>) {
    if let Matcher::Any(want) = matcher {
        collect(screen, &Matcher::TestId(want.clone()), scope, out);
        if out.is_empty() {
            collect(screen, &Matcher::Text(want.clone()), scope, out);
        }
        return;
    }
    if let Some(scope) = scope {
        for hit in scope {
            if let Target::View(id) = *hit {
                let mut own = Vec::new();
                push_matches(screen, id, matcher, &mut own);
                out.extend(
                    own.into_iter()
                        .filter(|hit| hit.view() != id || matches!(hit, Target::Span { .. })),
                );
                let children = screen
                    .view(id)
                    .map(|view| view.children.clone())
                    .unwrap_or_default();
                for child in children {
                    walk_tree(screen, child, &mut |child| {
                        push_matches(screen, child, matcher, out);
                    });
                }
            }
        }
        return;
    }
    let mut walk = |id: NodeId| push_matches(screen, id, matcher, out);
    if screen.view(nlayer_core::ROOT_ID).is_some() {
        walk_tree(screen, nlayer_core::ROOT_ID, &mut walk);
    } else {
        let mut ids: Vec<NodeId> = screen.views().map(|(id, _)| id).collect();
        ids.sort_unstable();
        for id in ids {
            if screen.parent(id).is_none() {
                walk_tree(screen, id, &mut walk);
            }
        }
    }
}

fn walk_tree(screen: &Screen, id: NodeId, f: &mut impl FnMut(NodeId)) {
    f(id);
    let children = screen
        .view(id)
        .map(|view| view.children.clone())
        .unwrap_or_default();
    for child in children {
        walk_tree(screen, child, f);
    }
}

fn push_matches(screen: &Screen, id: NodeId, matcher: &Matcher, out: &mut Vec<Target>) {
    let Some(view) = screen.view(id) else {
        return;
    };
    match matcher {
        Matcher::TestId(want) => {
            if view.test_id.as_deref() == Some(want.as_str()) {
                out.push(Target::View(id));
            }
            push_spans(view, id, want, out);
        }
        Matcher::Text(want) => {
            if text_eq(view.text.as_deref(), want) || input_eq(view, want) {
                out.push(Target::View(id));
            }
            push_span_text(view, id, want, out);
        }
        Matcher::Label(want) => {
            if view.accessibility.label.as_deref() == Some(want.as_str()) {
                out.push(Target::View(id));
            }
        }
        Matcher::Role { role, name } => {
            let accessibility = &view.accessibility;
            if accessibility.role == *role
                && name
                    .as_deref()
                    .is_none_or(|want| accessibility.label.as_deref() == Some(want))
            {
                out.push(Target::View(id));
            }
        }
        Matcher::Any(want) => {
            let mut ids = Vec::new();
            if view.test_id.as_deref() == Some(want.as_str()) {
                ids.push(Target::View(id));
            }
            push_spans(view, id, want, &mut ids);
            if !ids.is_empty() {
                out.extend(ids);
            } else {
                if text_eq(view.text.as_deref(), want) || input_eq(view, want) {
                    out.push(Target::View(id));
                }
                push_span_text(view, id, want, out);
            }
        }
    }
}

fn push_spans(view: &crate::View, id: NodeId, want: &str, out: &mut Vec<Target>) {
    for span in &view.span_ids {
        if span.test_id.as_ref() == want {
            out.push(span_target(view, id, span.start, span.end));
        }
    }
}

fn push_span_text(view: &crate::View, id: NodeId, want: &str, out: &mut Vec<Target>) {
    let Some(text) = view.text.as_deref() else {
        return;
    };
    for span in &view.span_ids {
        let slice = utf16_slice(text, span.start, span.end);
        if text_eq(Some(&slice), want) {
            out.push(span_target(view, id, span.start, span.end));
        }
    }
}

fn span_target(view: &crate::View, id: NodeId, start: u32, end: u32) -> Target {
    let node = view
        .links
        .iter()
        .find_map(|link| (link.start == start && link.end == end).then_some(link.node));
    Target::Span {
        view: id,
        start,
        end,
        node,
    }
}

fn input_eq(view: &crate::View, want: &str) -> bool {
    let Some(input) = &view.input else {
        return false;
    };
    if text_eq(Some(&input.value), want) {
        return true;
    }
    input.value.is_empty() && text_eq(Some(&input.config.placeholder), want)
}

fn text_eq(text: Option<&str>, want: &str) -> bool {
    text.is_some_and(|text| text.trim() == want.trim())
}

fn pick(source: &str, hits: &[Target], nth: Option<usize>) -> Result<Vec<Target>, String> {
    if let Some(n) = nth {
        return Ok(hits.get(n).copied().into_iter().collect());
    }
    if hits.len() > 1 {
        return Err(format!(
            "`{source}` matches {} views: say which (`{source} >> nth=0`)",
            hits.len()
        ));
    }
    Ok(hits.to_vec())
}

fn hidden(screen: &Screen, mut id: NodeId) -> bool {
    loop {
        let Some(view) = screen.view(id) else {
            return false;
        };
        if view
            .style
            .as_ref()
            .is_some_and(|style| style.hidden || style.opacity <= 0.0)
        {
            return true;
        }
        let Some(parent) = view.parent else {
            return false;
        };
        id = parent;
    }
}

fn target_frame(screen: &Screen, target: Target) -> Option<Bounds<Points>> {
    match target {
        Target::View(id) => {
            let view = screen.view(id)?;
            let origin = absolute_origin(screen, id);
            Some(Bounds::new(origin, view.frame.size))
        }
        Target::Span {
            view, start, end, ..
        } => {
            let node = screen.view(view)?;
            let origin = absolute_origin(screen, view);
            let mut rects = Vec::new();
            if let Some(layout) = &node.text_layout {
                ApproxTextEngine.range_rects(layout, start..end, &mut rects);
            }
            let rect = rects.first().copied().unwrap_or(Bounds::new(
                point(Points::ZERO, Points::ZERO),
                node.frame.size,
            ));
            Some(Bounds::new(
                point(
                    origin.x + node.text_content.origin.x + rect.origin.x,
                    origin.y + node.text_content.origin.y + rect.origin.y,
                ),
                rect.size,
            ))
        }
    }
}

fn absolute_origin(screen: &Screen, id: NodeId) -> nlayer_core::geometry::Point<Points> {
    let Some(view) = screen.view(id) else {
        return point(Points::ZERO, Points::ZERO);
    };
    let mut x = view.frame.origin.x;
    let mut y = view.frame.origin.y;
    let mut parent = view.parent;
    while let Some(pid) = parent {
        let Some(view) = screen.view(pid) else {
            break;
        };
        if view.kind == ViewKind::Scroll {
            x -= px(view.scroll_x);
            y -= px(view.scroll_y);
        }
        x += view.frame.origin.x;
        y += view.frame.origin.y;
        parent = view.parent;
    }
    point(x, y)
}

/// Moves scroll view `id` by `distance` points toward the end of its content.
pub(crate) fn scroll_by(screen: &mut Screen, id: NodeId, distance: f32) -> Result<(), String> {
    let view = screen
        .view(id)
        .filter(|view| view.kind == ViewKind::Scroll)
        .ok_or_else(|| "not a scroll view".to_owned())?;
    let horizontal = view.scroll_horizontal.unwrap_or(false);
    let content = view.scroll_content.unwrap_or(view.frame.size);
    let span = if horizontal {
        content.width.0 - view.frame.size.width.0
    } else {
        content.height.0 - view.frame.size.height.0
    };
    let span = span.max(0.0);
    if horizontal {
        let next = (view.scroll_x + distance).clamp(0.0, span);
        screen.set_scroll(id, next, view.scroll_y);
    } else {
        let next = (view.scroll_y + distance).clamp(0.0, span);
        screen.set_scroll(id, view.scroll_x, next);
    }
    Ok(())
}

/// The scroll view `target` is, or the nearest one around it.
pub(crate) fn scroll_of(screen: &Screen, target: Target) -> Option<NodeId> {
    let mut id = target.view();
    loop {
        if screen
            .view(id)
            .is_some_and(|view| view.kind == ViewKind::Scroll)
        {
            return Some(id);
        }
        id = screen.parent(id)?;
    }
}

/// An input `target` is. A span is not one.
pub(crate) fn input_of(screen: &Screen, target: Target) -> Option<NodeId> {
    match target {
        Target::View(id) if screen.view(id).is_some_and(|view| view.input.is_some()) => Some(id),
        _ => None,
    }
}

fn utf16_slice(text: &str, start: u32, end: u32) -> String {
    let units: Vec<u16> = text
        .encode_utf16()
        .skip(start as usize)
        .take(end.saturating_sub(start) as usize)
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TextLink;
    use nlayer_core::{
        EventKind, EventMask, MountBatch, MountItem, ROOT_ID,
        geometry::{px, size},
        text::{AttributedText, SpanTestId, TextLayout},
    };

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Points> {
        Bounds::new(point(px(x), px(y)), size(px(width), px(height)))
    }

    fn screen(items: Vec<MountItem>) -> Screen {
        let mut screen = Screen::default();
        screen.set_viewport(size(px(100.0), px(100.0)));
        screen.apply(&MountBatch { items });
        screen
    }

    fn text_item(
        id: NodeId,
        value: &str,
        spans: Vec<SpanTestId>,
        links: Vec<TextLink>,
    ) -> MountItem {
        MountItem::Text {
            id,
            text: std::sync::Arc::new(AttributedText {
                text: value.into(),
                test_ids: spans,
                links,
                ..AttributedText::default()
            }),
            layout: TextLayout(std::sync::Arc::new(())),
            content: rect(0.0, 0.0, 40.0, 16.0),
        }
    }

    fn span_id(start: u32, end: u32, test_id: &str) -> SpanTestId {
        let mut span = SpanTestId::default();
        span.start = start;
        span.end = end;
        span.test_id = test_id.into();
        span
    }

    #[test]
    fn an_id_beats_the_same_text_and_nth_picks_one() {
        let mut events = EventMask::default();
        events.set(EventKind::Press, true);
        let screen = screen(vec![
            MountItem::Create {
                id: 2,
                kind: ViewKind::View,
            },
            MountItem::Insert {
                parent: ROOT_ID,
                child: 2,
                before: None,
            },
            MountItem::Frame {
                id: 2,
                frame: rect(0.0, 0.0, 80.0, 40.0),
            },
            MountItem::TestId {
                id: 2,
                test_id: Some("Save".into()),
            },
            MountItem::Create {
                id: 3,
                kind: ViewKind::Text,
            },
            MountItem::Insert {
                parent: 2,
                child: 3,
                before: None,
            },
            MountItem::Frame {
                id: 3,
                frame: rect(0.0, 0.0, 40.0, 16.0),
            },
            text_item(3, "Save", vec![], vec![]),
            MountItem::Create {
                id: 4,
                kind: ViewKind::Text,
            },
            MountItem::Insert {
                parent: ROOT_ID,
                child: 4,
                before: None,
            },
            MountItem::Frame {
                id: 4,
                frame: rect(0.0, 50.0, 40.0, 16.0),
            },
            text_item(4, "Save", vec![], vec![]),
        ]);
        assert_eq!(resolve(&screen, "Save").unwrap(), Target::View(2));
        assert_eq!(
            resolve(&screen, "text:Save >> nth=1").unwrap(),
            Target::View(4)
        );
        assert!(resolve(&screen, "text:Save").unwrap_err().contains("nth=0"));
        assert!(resolve(&screen, "missing").unwrap_err().contains("nothing"));
        assert!(visible(&screen, Target::View(2)));
    }

    #[test]
    fn a_span_id_names_the_span_inside_its_block() {
        let screen = screen(vec![
            MountItem::Create {
                id: 2,
                kind: ViewKind::Text,
            },
            MountItem::Insert {
                parent: ROOT_ID,
                child: 2,
                before: None,
            },
            MountItem::Frame {
                id: 2,
                frame: rect(0.0, 0.0, 80.0, 16.0),
            },
            text_item(
                2,
                "see docs",
                vec![span_id(4, 8, "docs")],
                vec![TextLink {
                    start: 4,
                    end: 8,
                    node: 4,
                    events: {
                        let mut events = EventMask::default();
                        events.set(EventKind::Press, true);
                        events
                    },
                }],
            ),
        ]);
        assert_eq!(
            resolve(&screen, "id:docs").unwrap(),
            Target::Span {
                view: 2,
                start: 4,
                end: 8,
                node: Some(4),
            }
        );
        assert_eq!(
            resolve(&screen, "text:docs").unwrap(),
            Target::Span {
                view: 2,
                start: 4,
                end: 8,
                node: Some(4),
            }
        );
    }

    #[test]
    fn text_names_a_span_only_when_it_has_a_test_id() {
        let mut events = EventMask::default();
        events.set(EventKind::Press, true);
        let screen = screen(vec![
            MountItem::Create {
                id: 2,
                kind: ViewKind::Text,
            },
            MountItem::Insert {
                parent: ROOT_ID,
                child: 2,
                before: None,
            },
            MountItem::Frame {
                id: 2,
                frame: rect(0.0, 0.0, 80.0, 16.0),
            },
            text_item(
                2,
                "see docs link",
                vec![span_id(4, 8, "docs")],
                vec![
                    TextLink {
                        start: 4,
                        end: 8,
                        node: 4,
                        events,
                    },
                    TextLink {
                        start: 9,
                        end: 13,
                        node: 5,
                        events,
                    },
                ],
            ),
        ]);
        assert_eq!(
            resolve(&screen, "text:docs").unwrap(),
            Target::Span {
                view: 2,
                start: 4,
                end: 8,
                node: Some(4),
            }
        );
        assert!(
            resolve(&screen, "text:link")
                .unwrap_err()
                .contains("nothing")
        );
        assert!(resolve(&screen, "link").unwrap_err().contains("nothing"));
    }

    #[test]
    fn scrolling_keeps_the_offset_inside_the_content() {
        let mut screen = screen(vec![
            MountItem::Create {
                id: 2,
                kind: ViewKind::Scroll,
            },
            MountItem::Insert {
                parent: ROOT_ID,
                child: 2,
                before: None,
            },
            MountItem::Frame {
                id: 2,
                frame: rect(0.0, 0.0, 100.0, 40.0),
            },
            MountItem::ScrollContent {
                id: 2,
                size: size(px(100.0), px(140.0)),
            },
        ]);
        scroll_by(&mut screen, 2, 50.0).unwrap();
        let view = screen.view(2).unwrap();
        assert_eq!(view.scroll_y, 50.0);
        scroll_by(&mut screen, 2, 1000.0).unwrap();
        assert_eq!(screen.view(2).unwrap().scroll_y, 100.0);
        scroll_by(&mut screen, 2, -1000.0).unwrap();
        assert_eq!(screen.view(2).unwrap().scroll_y, 0.0);
    }
}
