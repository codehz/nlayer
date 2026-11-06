//! The test scene snapshot of mounted views.

use nlayer_core::{EventKind, NodeId, ROOT_ID, ResizeMode};
use objc2::DefinedClass;

use crate::text_view::TextView;

use super::*;

impl Mounter {
    pub(crate) fn scene(&self) -> crate::scene::Scene {
        use crate::scene::{ImageSnap, InputSnap, Kind, Node, Rect, ScrollSnap};

        let mut nodes = Vec::new();
        for id in 0..self.slots.len() as u32 {
            let Some(slot) = self.slot(id) else { continue };
            let view = slot.native.view();
            let frame = view.frame();
            let mut node = Node {
                id,
                // Screens, header items and modals are hosted by their
                // parent without being its subviews: the test tree follows
                // the app's tree, not UIKit's.
                parent: self
                    .hosts
                    .get(&id)
                    .copied()
                    .or_else(|| self.parent_id(view)),
                children: Vec::new(),
                kind: match slot.native {
                    Native::View(_) => Kind::View,
                    Native::Text(_) => Kind::Text,
                    Native::Image(_) => Kind::Image,
                    Native::Scroll(_) => Kind::Scroll,
                    Native::Input(_) | Native::Area(_) => Kind::Input,
                    Native::Stack(_)
                    | Native::Screen(_)
                    | Native::Modal(_)
                    | Native::HeaderItem(_) => Kind::View,
                },
                test_id: slot.test_id.as_ref().map(|id| id.to_string()),
                accessibility: slot.accessibility.clone(),
                text: None,
                frame: Rect {
                    x: frame.origin.x as f32,
                    y: frame.origin.y as f32,
                    w: frame.size.width as f32,
                    h: frame.size.height as f32,
                },
                hidden: slot.style.hidden,
                opacity: slot.style.opacity,
                pressable: slot.own_press,
                text_selectable: None,
                selection_color: None,
                selectable: self.areas.iter().any(|(area, _)| *area == id),
                input: None,
                runs: Vec::new(),
                spans: Vec::new(),
                scroll: None,
                image: None,
                transform: Vec::new(),
            };
            if let Native::Text(text) = &slot.native
                && let Some(attributed) = text.attributed()
            {
                node.text = Some(attributed.text.clone());
                node.text_selectable = attributed.selectable;
                node.selection_color = attributed.selection_color.map(|color| color.0);
                node.runs = style_runs(&attributed);
                node.spans = selector_spans(text, &attributed);
            }
            match &slot.native {
                Native::Input(field) => {
                    node.input = Some(InputSnap {
                        value: field.shown(),
                        placeholder: field.placeholder_text(),
                        secure: field.secure(),
                        keyboard: field.keyboard_name().to_owned(),
                        disabled: field.disabled(),
                        focused: field.isFirstResponder(),
                        multiline: false,
                    });
                }
                Native::Area(area) => {
                    node.input = Some(InputSnap {
                        value: area.shown(),
                        placeholder: area.placeholder_text(),
                        secure: area.secure(),
                        keyboard: area.keyboard_name().to_owned(),
                        disabled: area.disabled(),
                        focused: area.view().isFirstResponder(),
                        multiline: true,
                    });
                }
                _ => {}
            }
            if let Native::Scroll(scroll) = &slot.native {
                let offset = scroll.contentOffset();
                let native = scroll.contentSize();
                let (content_w, content_h) = self
                    .relay
                    .ivars()
                    .content
                    .borrow()
                    .get(&id)
                    .copied()
                    .unwrap_or((native.width as f32, native.height as f32));
                node.scroll = Some(ScrollSnap {
                    horizontal: scroll.alwaysBounceHorizontal(),
                    x: offset.x as f32,
                    y: offset.y as f32,
                    content_w,
                    content_h,
                });
            }
            if let Some(picture) = &slot.picture {
                node.image = Some(ImageSnap {
                    src: picture.uri.as_deref().unwrap_or("").to_owned(),
                    fit: fit_name(picture.mode).to_owned(),
                });
            }
            if !slot.style.transform.is_identity() {
                node.transform = slot.style.transform.ops().to_vec();
            }
            let subs = slot
                .content
                .as_ref()
                .map(|content| content.subviews())
                .unwrap_or_else(|| view.subviews());
            for index in 0..subs.count() {
                let child = subs.objectAtIndex(index);
                let child_id = child.tag() as u32;
                if child.tag() >= 0 && self.slot(child_id).is_some() {
                    node.children.push(child_id);
                }
            }
            nodes.push(node);
        }
        // Hosted children are not subviews of their parent (a screen is
        // inside its navigation controller), so link them by the app's
        // order for selectors and presses.
        let mut index = vec![None; nodes.len().max(self.slots.len())];
        for (at, node) in nodes.iter().enumerate() {
            index[node.id as usize] = Some(at);
        }
        let mut hosted: Vec<(u32, u32)> = self
            .hosts
            .iter()
            .map(|(&child, &parent)| (parent, child))
            .collect();
        hosted.sort_unstable();
        for (parent, child) in hosted {
            if let (Some(&p), Some(_)) = (index.get(parent as usize), index.get(child as usize))
                && let Some(p) = p
                && !nodes[p].children.contains(&child)
            {
                nodes[p].children.push(child);
            }
        }
        let (width, height) = self
            .view(ROOT_ID)
            .map(|view| {
                let bounds = view.bounds();
                (bounds.size.width as f32, bounds.size.height as f32)
            })
            .unwrap_or((self.viewport.width.0, self.viewport.height.0));
        crate::scene::Scene::from_nodes(nodes, width, height)
    }
}

pub(super) fn style_runs(text: &nlayer_core::text::AttributedText) -> Vec<crate::scene::Run> {
    text.spans
        .iter()
        .map(|span| crate::scene::Run {
            text: crate::scene::utf16_slice(&text.text, span.start, span.end),
            font_size: span.style.font_size.0,
            font_weight: span.style.font_weight,
            italic: span.style.italic,
            color: span.style.color.0,
        })
        .collect()
}

pub(super) fn selector_spans(
    view: &TextView,
    text: &nlayer_core::text::AttributedText,
) -> Vec<crate::scene::Span> {
    let mut spans = Vec::new();
    for id in &text.test_ids {
        let node = text.links.iter().find_map(|link| {
            (link.start == id.start && link.end == id.end && link.events.contains(EventKind::Press))
                .then_some(link.node)
        });
        spans.push(span_snap(
            view,
            text,
            id.start,
            id.end,
            Some(id.test_id.to_string()),
            node,
        ));
    }
    for link in &text.links {
        if !link.events.contains(EventKind::Press) {
            continue;
        }
        if spans
            .iter()
            .any(|span| span.start == link.start && span.end == link.end)
        {
            continue;
        }
        spans.push(span_snap(
            view,
            text,
            link.start,
            link.end,
            None,
            Some(link.node),
        ));
    }
    spans
}

pub(super) fn span_snap(
    view: &TextView,
    text: &nlayer_core::text::AttributedText,
    start: u32,
    end: u32,
    test_id: Option<String>,
    node: Option<NodeId>,
) -> crate::scene::Span {
    let (x, y, w, h) = view.span_frame(start, end).unwrap_or((0.0, 0.0, 0.0, 0.0));
    crate::scene::Span {
        start,
        end,
        test_id,
        node,
        text: crate::scene::utf16_slice(&text.text, start, end),
        frame: crate::scene::Rect { x, y, w, h },
    }
}

pub(super) fn fit_name(mode: ResizeMode) -> &'static str {
    match mode {
        ResizeMode::Cover => "cover",
        ResizeMode::Contain => "contain",
        ResizeMode::Stretch => "fill",
        ResizeMode::Center => "none",
        ResizeMode::ScaleDown => "scale-down",
    }
}
