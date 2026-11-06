//! Surface size, pin, keyboard and the focused field.

use std::rc::Rc;

use nlayer_core::geometry::{Points, Size};
use nlayer_core::{KeyboardTaps, MountItem, NodeId, ROOT_ID};
use nlayer_engine::EngineControl;
use objc2::rc::Retained;
use objc2::{DefinedClass, Message, msg_send};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_ui_kit::{UIScrollView, UIView};

use super::*;

impl Mounter {
    pub(crate) fn set_control(&mut self, control: EngineControl) {
        if self.pin.as_ref().is_none_or(|pin| pin.keyboard.is_none())
            && self.keyboard != Points::ZERO
        {
            control.set_keyboard_inset(self.keyboard);
        }
        self.control = Some(control);
    }

    /// The software keyboard's overlap changed. An explicit pin height wins.
    pub(crate) fn set_keyboard(&mut self, height: f32) {
        if self.pin.as_ref().is_some_and(|pin| pin.keyboard.is_some()) {
            return;
        }
        let height = Points(height);
        if self.keyboard == height {
            return;
        }
        self.keyboard = height;
        if let Some(control) = &self.control {
            control.set_keyboard_inset(height);
        }
    }

    /// Engine thread requested a frame. Unpause the display link and mark
    /// that a frame is pending.
    pub(crate) fn request_frame(&mut self) {
        self.frame_pending = true;
        if let Some(link) = self.relay.ivars().link.borrow().as_ref() {
            link.setPaused(false);
        }
    }

    /// The height `env(keyboard-inset-height)` is using. A pin replaces the
    /// live keyboard, including a pin of 0.
    pub(super) fn keyboard_inset(&self) -> f32 {
        self.pin
            .as_ref()
            .and_then(|pin| pin.keyboard)
            .unwrap_or(self.keyboard.0)
    }

    /// Resigns the focused field. A missing window still resigns the field
    /// the mounter is tracking.
    pub(super) fn end_editing(&self) {
        if let Some(window) = self.view(ROOT_ID).and_then(|view| view.window()) {
            // SAFETY: `endEditing:` walks the window's responders.
            let _: bool = unsafe { msg_send![&*window, endEditing: true] };
            return;
        }
        if let Some(id) = self.focused
            && let Some(view) = self.input_view(id)
        {
            let _: bool = unsafe { msg_send![&*view, resignFirstResponder] };
        }
    }

    pub(super) fn press_allowed(&self, view: &UIView) -> bool {
        let (taps, on_input) = self.tap_context(view);
        nlayer_core::press_allowed(Points(self.keyboard_inset()), taps, on_input)
    }

    pub(super) fn tap_dismisses(&self, view: &UIView, at: CGPoint) -> bool {
        let (taps, on_input) = self.tap_context(view);
        let handled = self.handled(view, at);
        nlayer_core::tap_dismisses(Points(self.keyboard_inset()), taps, on_input, handled)
    }

    pub(super) fn drag_dismisses(&self, scroll: &UIScrollView) -> bool {
        let id = scroll.tag() as NodeId;
        let mode = self
            .slot(id)
            .filter(|slot| std::ptr::eq(slot.native.view(), scroll as &UIView))
            .map(|slot| slot.dismiss)
            .unwrap_or_default();
        nlayer_core::drag_dismisses(Points(self.keyboard_inset()), mode)
    }

    /// The nearest scroll's tap mode, and whether the walk to it hits an input.
    pub(super) fn tap_context(&self, view: &UIView) -> (Option<KeyboardTaps>, bool) {
        let mut on_input = false;
        let mut current = Some(view.retain());
        while let Some(v) = current {
            let id = v.tag() as NodeId;
            if let Some(slot) = self
                .slot(id)
                .filter(|slot| std::ptr::eq(slot.native.view(), &*v))
            {
                if matches!(slot.native, Native::Input(_) | Native::Area(_)) {
                    on_input = true;
                }
                if matches!(slot.native, Native::Scroll(_)) {
                    return (Some(slot.taps), on_input);
                }
            }
            current = v.superview();
        }
        (None, on_input)
    }

    /// An `onPress` or an input from `view` through the nearest scroll.
    /// A pressable span counts only under `at`, which is in `view`'s coordinates.
    pub(super) fn handled(&self, view: &UIView, at: CGPoint) -> bool {
        let mut current = Some(view.retain());
        let mut point = Some(at);
        while let Some(v) = current {
            let id = v.tag() as NodeId;
            if let Some(slot) = self
                .slot(id)
                .filter(|slot| std::ptr::eq(slot.native.view(), &*v))
            {
                let on_link = if let (Native::Text(text), Some(at)) = (&slot.native, point) {
                    text.link_at(at).is_some()
                } else {
                    false
                };
                if matches!(slot.native, Native::Input(_) | Native::Area(_))
                    || slot.own_press
                    || on_link
                {
                    return true;
                }
                if matches!(slot.native, Native::Scroll(_)) {
                    return false;
                }
            }
            point = None;
            current = v.superview();
        }
        false
    }

    /// The system's appearance changed: `platform.colorScheme()` follows it.
    /// A test pin replaces the system for the run.
    pub(crate) fn set_color_scheme(&mut self, dark: bool) {
        if self
            .pin
            .as_ref()
            .is_some_and(|pin| pin.color_scheme.is_some())
        {
            return;
        }
        if let Some(control) = &self.control {
            control.set_color_scheme(nlayer_core::ColorScheme::from_dark(dark));
        }
    }

    pub(crate) fn set_pin(&mut self, pin: crate::pin::Pin) {
        self.viewport = Size::new(Points(pin.width), Points(pin.height));
        self.insets = nlayer_core::geometry::Edges {
            top: Points(pin.safe_area.top),
            right: Points(pin.safe_area.right),
            bottom: Points(pin.safe_area.bottom),
            left: Points(pin.safe_area.left),
        };
        self.pin = Some(pin);
    }

    pub(crate) fn pin_size(&self) -> Option<(f32, f32)> {
        self.pin.as_ref().map(|pin| (pin.width, pin.height))
    }

    /// The safe area the test pin replaced the system with, in points:
    /// top, right, bottom, left. `None` follows the window.
    pub(crate) fn pin_safe_area(&self) -> Option<(f32, f32, f32, f32)> {
        self.pin.as_ref().map(|pin| {
            (
                pin.safe_area.top,
                pin.safe_area.right,
                pin.safe_area.bottom,
                pin.safe_area.left,
            )
        })
    }

    /// Drops every view except the root. The next engine's ids start over,
    /// so a slot left behind would be that id's new view.
    pub(crate) fn clear_app(&mut self) {
        if let Some(id) = self.focused {
            self.end_focus(id);
        }
        let ids: Vec<NodeId> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let id = index as NodeId;
                (slot.is_some() && id != ROOT_ID).then_some(id)
            })
            .collect();
        for id in &ids {
            if let Some(slot) = self.slot_mut(*id)
                && let Native::Modal(modal) = &mut slot.native
            {
                modal.discard();
            }
        }
        for id in ids.into_iter().rev() {
            self.apply_item(MountItem::Delete { id });
        }
        if let Some(root) = self.view(ROOT_ID) {
            let subs = root.subviews();
            let children: Vec<Retained<UIView>> = (0..subs.count())
                .map(|index| subs.objectAtIndex(index))
                .collect();
            for child in children {
                child.removeFromSuperview();
            }
        }
        self.control = None;
        self.focused = None;
        let _ = self.scrolls.borrow_mut().take();
        self.content_sizes.clear();
        self.narrowed.clear();
        self.frames_applied = false;
        self.animator = crate::animation::Animator::default();
        self.frame_pending = false;
        if let Some(link) = self.relay.ivars().link.borrow().as_ref() {
            link.setPaused(true);
        }
        self.hosts.clear();
        self.dirty_stacks.clear();
        self.dirty_headers.clear();
        self.dirty_modals.clear();
        self.deferred_stacks.clear();
        self.pending_pan.clear();
        self.pending_pinch.clear();
        self.pending_rotation.clear();
        self.pending_bindings.clear();
        self.areas.clear();
        self.opened_urls.clear();
        self.relay.ivars().content.borrow_mut().clear();
        abandon_loads();
    }

    /// Tells the engine the surface size and safe area, when either changed.
    pub(crate) fn publish(&mut self, size: CGSize, insets: objc2_ui_kit::UIEdgeInsets) {
        let (size, insets) = if let Some(pin) = &self.pin {
            (
                CGSize::new(pin.width as CGFloat, pin.height as CGFloat),
                objc2_ui_kit::UIEdgeInsets {
                    top: pin.safe_area.top as CGFloat,
                    left: pin.safe_area.left as CGFloat,
                    bottom: pin.safe_area.bottom as CGFloat,
                    right: pin.safe_area.right as CGFloat,
                },
            )
        } else {
            (size, insets)
        };
        let keyboard = self.pin.as_ref().and_then(|pin| pin.keyboard);
        let size = Size::new(Points(size.width as f32), Points(size.height as f32));
        let insets = nlayer_core::geometry::Edges {
            top: Points(insets.top as f32),
            right: Points(insets.right as f32),
            bottom: Points(insets.bottom as f32),
            left: Points(insets.left as f32),
        };
        let Some(control) = self.control.clone() else {
            self.viewport = size;
            self.insets = insets;
            return;
        };
        if size != self.viewport {
            self.viewport = size;
            control.resize(size);
        }
        if insets != self.insets {
            self.insets = insets;
            control.set_safe_area(insets);
        }
        if let Some(height) = keyboard {
            control.set_keyboard_inset(Points(height));
        }
    }

    pub(crate) fn view_of(&self, id: NodeId) -> Option<Retained<UIView>> {
        self.view(id).map(|view| view.retain())
    }
}

/// Resigns the focused field without holding the mounter. `endEditing:`
/// posts the keyboard notification and `editingEnded:` before it returns.
pub(super) fn resign_keyboard(shared: &Rc<std::cell::RefCell<Mounter>>) {
    let window = {
        let mounter = shared.borrow();
        mounter.view(ROOT_ID).and_then(|view| view.window())
    };
    if let Some(window) = window {
        // SAFETY: `endEditing:` walks the window's responders.
        let _: bool = unsafe { msg_send![&*window, endEditing: true] };
        return;
    }
    let field = {
        let mounter = shared.borrow();
        mounter.focused.and_then(|id| mounter.input_view(id))
    };
    if let Some(field) = field {
        let _: bool = unsafe { msg_send![&*field, resignFirstResponder] };
    }
}
