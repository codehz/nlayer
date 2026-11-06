//! Scroll metrics, content-size narrowing and scroll events.

use dispatch2::DispatchQueue;
use nlayer_core::{EventKind, NodeId};
use nlayer_engine::EventPayload;
use objc2::DefinedClass;
use objc2::rc::Retained;
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_ui_kit::{UIScrollView, UIView};

use super::*;

impl Mounter {
    /// Drops a deleted scroll's engine size and any width still being judged.
    pub(super) fn forget_scroll(&mut self, id: NodeId) {
        self.relay.ivars().content.borrow_mut().remove(&id);
        self.narrowed.retain(|&(n, ..)| n != id);
        self.content_sizes.retain(|&(n, _)| n != id);
    }

    /// Narrows a vertical scroll's native content width to its viewport when
    /// nothing inside reaches past it. Judged after the batch, so a child
    /// that arrived with the size counts. The engine size stays on the relay.
    pub(super) fn settle_content_sizes(&mut self) {
        let frames_applied = std::mem::take(&mut self.frames_applied);
        for (id, height) in std::mem::take(&mut self.content_sizes) {
            let mut narrowed = None;
            if let Some(Native::Scroll(scroll)) = self.native(id) {
                let viewport = scroll.bounds().size.width;
                let content = scroll.contentSize().width;
                // Content that fits cannot be narrowed; walking the subtree
                // to learn that would cost the page on every batch.
                if viewport > 0.0 && content > viewport {
                    let width = crate::scroll_extent::settled_content_width(
                        viewport,
                        content,
                        reaches_beyond(scroll, viewport + 0.5),
                    );
                    if width != content {
                        scroll.setContentSize(CGSize::new(width as CGFloat, height));
                        narrowed = Some((content, height));
                    }
                }
            }
            self.narrowed.retain(|&(n, ..)| n != id);
            if let Some((width, height)) = narrowed {
                self.narrowed.push((id, width, height));
            }
        }
        if frames_applied {
            self.rejudge_narrowed();
        }
    }

    /// Gives the engine's width back once a later batch widens what a
    /// narrowed scroll holds. Without this the tail stays out of reach.
    pub(super) fn rejudge_narrowed(&mut self) {
        let mut i = 0;
        while i < self.narrowed.len() {
            let (id, core, height) = self.narrowed[i];
            let mut done = true;
            if let Some(Native::Scroll(scroll)) = self.native(id)
                && crate::scroll_extent::still_judged(scroll.alwaysBounceHorizontal())
            {
                let viewport = scroll.bounds().size.width;
                let reaches = reaches_beyond(scroll, viewport + 0.5);
                let width = crate::scroll_extent::narrowed_width(viewport, core, reaches);
                if width != scroll.contentSize().width {
                    scroll.setContentSize(CGSize::new(width as CGFloat, height));
                }
                done = width == core;
            }
            if done {
                self.narrowed.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    pub(super) fn dispatch_scroll(&self, id: NodeId, event: EventKind, payload: EventPayload) {
        if event == EventKind::Scroll {
            let arm = self.scrolls.borrow_mut().note(id, payload);
            if arm {
                DispatchQueue::main().exec_async(move || {
                    let Some(mounter) = current() else {
                        return;
                    };
                    mounter.borrow().flush_scrolls();
                });
            }
            return;
        }
        if let Some(pending) = self.scrolls.borrow_mut().take_id(id) {
            self.send_scroll(id, EventKind::Scroll, pending);
        }
        self.send_scroll(id, event, payload);
    }

    pub(super) fn flush_scrolls(&self) {
        let pending = self.scrolls.borrow_mut().take();
        for (id, payload) in pending {
            self.send_scroll(id, EventKind::Scroll, payload);
        }
    }

    pub(super) fn send_scroll(&self, id: NodeId, event: EventKind, payload: EventPayload) {
        let listens = self
            .slot(id)
            .is_some_and(|slot| slot.events.contains(event));
        // The core follows a list's offset even without `onScroll`: only
        // the other callbacks wait for a listener.
        if event != EventKind::Scroll && !listens {
            return;
        }
        let Some(control) = &self.control else {
            return;
        };
        control.dispatch_event(id, event, payload);
    }
}

pub(super) fn scroll_metrics(scroll: &UIScrollView, content: Option<(f32, f32)>) -> EventPayload {
    let offset = scroll.contentOffset();
    let native = scroll.contentSize();
    let (content_w, content_h) = content.unwrap_or((native.width as f32, native.height as f32));
    let size = scroll.bounds().size;
    EventPayload::Scroll {
        x: offset.x as f32,
        y: offset.y as f32,
        content_w,
        content_h,
        width: size.width as f32,
        height: size.height as f32,
    }
}

/// `contentOffset` UIKit will actually keep. A target past the content is
/// clamped, and so is one above the inset: an unclamped animated `scrollTo`
/// ends wherever the animation was interrupted (nsh `command`).
pub(super) fn clamped_scroll_offset(scroll: &UIScrollView, x: f32, y: f32) -> CGPoint {
    let inset = scroll.adjustedContentInset();
    let size = scroll.contentSize();
    let bounds = scroll.bounds().size;
    let max_x = (size.width + inset.right - bounds.width).max(-inset.left);
    let max_y = (size.height + inset.bottom - bounds.height).max(-inset.top);
    CGPoint::new(
        (x as CGFloat).clamp(-inset.left, max_x),
        (y as CGFloat).clamp(-inset.top, max_y),
    )
}

/// Whether anything under `root` reaches past `limit` points from its left
/// edge, in `root`'s space. A scroll view inside scrolls on its own, so what
/// it holds does not count. Stops at the first view that reaches.
pub(super) fn reaches_beyond(root: &UIView, limit: CGFloat) -> bool {
    let mut pending = Vec::new();
    push_subviews(root, &mut pending);
    while let Some(view) = pending.pop() {
        let frame = view.convertRect_toView(view.bounds(), Some(root));
        if frame.origin.x + frame.size.width > limit {
            return true;
        }
        if view.downcast_ref::<UIScrollView>().is_none() {
            push_subviews(&view, &mut pending);
        }
    }
    false
}

pub(super) fn push_subviews(view: &UIView, out: &mut Vec<Retained<UIView>>) {
    let subs = view.subviews();
    for index in 0..subs.count() {
        out.push(subs.objectAtIndex(index));
    }
}
