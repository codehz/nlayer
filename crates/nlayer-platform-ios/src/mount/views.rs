//! Creating views and inserting them into their parent.

use nlayer_core::{NodeId, ViewKind};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{MainThreadOnly, Message};
use objc2_core_foundation::CGRect;
use objc2_ui_kit::{UIScrollView, UIScrollViewContentInsetAdjustmentBehavior, UIView};

use crate::input::{AreaInput, Field};
use crate::text_view::TextView;

use super::*;

impl Mounter {
    /// Replaces the editor when `multiline` changes. The new view takes the
    /// old one's place in its superview, text, focus and content box.
    pub(super) fn set_input_multiline(&mut self, id: NodeId, multiline: bool) {
        let area = matches!(self.native(id), Some(Native::Area(_)));
        if area == multiline {
            return;
        }
        let Some(old) = self.input_view(id) else {
            return;
        };
        let content = self.slot(id).and_then(|slot| slot.input_content);
        let text = match self.native(id) {
            Some(Native::Input(field)) => field.shown(),
            Some(Native::Area(area)) => area.shown(),
            _ => return,
        };
        let focused = old.isFirstResponder();
        let relay = ProtocolObject::from_ref(&*self.relay);
        let native = if multiline {
            Native::Area(AreaInput::new(self.mtm, relay))
        } else {
            Native::Input(Field::new(self.mtm, ProtocolObject::from_ref(&*self.relay)))
        };
        let new = native.view().retain();
        new.setTag(id as isize);
        new.setOpaque(false);
        new.setBackgroundColor(Some(&objc2_ui_kit::UIColor::clearColor()));
        new.setFrame(old.frame());
        if let Some(parent) = old.superview() {
            let subs = parent.subviews();
            let index = (0..subs.count()).find(|&index| {
                Retained::as_ptr(&subs.objectAtIndex(index)) == Retained::as_ptr(&old)
            });
            if let Some(index) = index {
                parent.insertSubview_atIndex(&new, index as isize);
            }
        }
        old.removeFromSuperview();
        match &native {
            Native::Input(field) => field.set_value(&text, 0),
            Native::Area(area) => area.set_value(&text, 0),
            _ => {}
        }
        let accessibility = self.slot(id).map(|slot| slot.accessibility.clone());
        if let Some(accessibility) = &accessibility {
            apply_accessibility(&new, accessibility, self.mtm);
        }
        if let Some(slot) = self.slot_mut(id) {
            slot.native = native;
        }
        if let Some(content) = content {
            match self.native(id) {
                Some(Native::Input(field)) => field.set_content(content),
                Some(Native::Area(area)) => area.set_content(content),
                _ => {}
            }
        }
        self.repaint(id);
        if focused {
            new.becomeFirstResponder();
        }
    }

    pub(super) fn create(&mut self, id: NodeId, kind: ViewKind) {
        let (native, picture) = match kind {
            ViewKind::View => (
                Native::View(Retained::into_super(BoxView::new(self.mtm))),
                None,
            ),
            ViewKind::Text => (Native::Text(TextView::new(self.mtm)), None),
            ViewKind::Scroll => {
                let scroll =
                    UIScrollView::initWithFrame(UIScrollView::alloc(self.mtm), CGRect::ZERO);
                scroll.setContentInsetAdjustmentBehavior(
                    UIScrollViewContentInsetAdjustmentBehavior::Never,
                );
                scroll.setDelaysContentTouches(false);
                // SAFETY: `Relay` implements `UIScrollViewDelegate`. UIKit
                // holds the delegate weakly; the mounter keeps the relay.
                unsafe {
                    scroll.setDelegate(Some(ProtocolObject::from_ref(&*self.relay)));
                }
                (Native::Scroll(scroll), None)
            }
            ViewKind::Image => {
                let (view, picture) = image_layers(self.mtm);
                (Native::Image(view), Some(picture))
            }
            ViewKind::Input => (
                Native::Input(Field::new(self.mtm, ProtocolObject::from_ref(&*self.relay))),
                None,
            ),
            ViewKind::Stack => {
                let stack = crate::navigation::StackHost::new(self.mtm, id);
                (Native::Stack(Box::new(stack)), None)
            }
            ViewKind::Screen => {
                let control = self.control.clone();
                let screen =
                    crate::navigation::ScreenHost::new(self.mtm, id, control, self.pin_safe_area());
                (Native::Screen(Box::new(screen)), None)
            }
            ViewKind::Modal => {
                let control = self.control.clone();
                let modal = crate::navigation::ModalHost::new(self.mtm, id, control);
                (Native::Modal(Box::new(modal)), None)
            }
            ViewKind::HeaderItem => (Native::HeaderItem(UIView::new(self.mtm)), None),
        };
        let view = native.view().retain();
        view.setTag(id as isize);
        view.setOpaque(false);
        view.setBackgroundColor(Some(&objc2_ui_kit::UIColor::clearColor()));
        self.ensure(id);
        let mut slot = Slot::plain(native);
        slot.picture = picture;
        if kind == ViewKind::Scroll {
            slot.keyboard_tap = Some(keyboard_tap(self.mtm, &self.relay, &view));
        }
        self.slots[id as usize] = Some(slot);
        // Bindings may arrive before the view (the core sets a prop before
        // the batch creates it). Apply what was kept.
        if let Some((x, y)) = self.pending_pan.remove(&id)
            && let Some(slot) = self.slot_mut(id)
        {
            slot.pan_nodes = (x, y);
        }
        if let Some(scale) = self.pending_pinch.remove(&id)
            && let Some(slot) = self.slot_mut(id)
        {
            slot.pinch_node = scale;
        }
        if let Some(angle) = self.pending_rotation.remove(&id)
            && let Some(slot) = self.slot_mut(id)
        {
            slot.rotation_node = angle;
        }
        if let Some(bindings) = self.pending_bindings.remove(&id)
            && let Some(slot) = self.slot_mut(id)
        {
            slot.bindings = bindings;
        }
        self.sync_gestures(id);
        self.apply_animations();
    }

    pub(super) fn insert(&mut self, parent: NodeId, child: NodeId, before: Option<NodeId>) {
        if self.insert_hosted(parent, child, before) {
            return;
        }
        let Some(child_view) = self.view(child).map(|v| v.retain()) else {
            return;
        };
        // A plain view that clips around an outer shadow keeps its children
        // in `content`. The shadow host stays a sibling of the view itself.
        let container = self
            .slot(parent)
            .and_then(|slot| slot.content.clone())
            .or_else(|| self.view(parent).map(|view| view.retain()));
        let Some(container) = container else {
            return;
        };
        match before.and_then(|id| self.view(id)).map(|v| v.retain()) {
            Some(before_view) => {
                container.insertSubview_belowSubview(&child_view, &before_view);
            }
            None => container.addSubview(&child_view),
        }
        // A new sibling sits between a view and the host that was under it.
        self.sync_shadow_host(child);
        if let Some(before) = before {
            self.sync_shadow_host(before);
        }
    }

    pub(super) fn sync_shadow_host(&self, id: NodeId) {
        let Some(slot) = self.slot(id) else {
            return;
        };
        let view = slot.native.view().retain();
        place_shadow_host(&view, slot);
    }

    pub(super) fn detach_shadow_host(&self, id: NodeId) {
        if let Some(host) = self.slot(id).and_then(|slot| slot.shadow_host.clone()) {
            host.removeFromSuperview();
        }
    }
}
