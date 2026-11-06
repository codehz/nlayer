//! Slot lookup and the focus ancestor chain.

use nlayer_core::tailwind::state::FOCUSED;
use nlayer_core::{NodeId, ROOT_ID};
use objc2::Message;
use objc2::rc::Retained;
use objc2_ui_kit::{UIScrollView, UIView};

use crate::input::Field;
use crate::text_view::TextView;

use super::*;

impl Mounter {
    pub(super) fn chain(&self, id: NodeId) -> Vec<NodeId> {
        let Some(view) = self.view(id).map(|view| view.retain()) else {
            return vec![id];
        };
        let mut ids = Vec::new();
        let mut current = Some(view);
        while let Some(view) = current {
            if let Some(tag) = self.node_id(&view) {
                ids.push(tag);
                if tag == ROOT_ID {
                    break;
                }
            }
            current = view.superview();
        }
        if ids.is_empty() {
            ids.push(id);
        }
        ids
    }

    /// The node id of `view`, when it is that node's own native view.
    /// A content clip and a shadow host share no node.
    pub(super) fn node_id(&self, view: &UIView) -> Option<u32> {
        let tag = view.tag();
        if tag < 0 {
            return None;
        }
        let id = tag as u32;
        let ours = self.view(id)?;
        std::ptr::eq(ours, view).then_some(id)
    }

    pub(super) fn mark_focused(&mut self, ids: &[NodeId], on: bool) {
        for &id in ids {
            if let Some(slot) = self.slot_mut(id) {
                slot.focused = on;
            }
            self.repaint(id);
        }
        // `focused` paints the whole chain. The engine hears only this view
        // and computes focus-within itself.
        if let Some(&id) = ids.first() {
            self.report_state(id, FOCUSED, on);
        }
    }

    pub(super) fn native(&self, id: NodeId) -> Option<&Native> {
        self.slot(id).map(|s| &s.native)
    }

    pub(super) fn view(&self, id: NodeId) -> Option<&UIView> {
        self.native(id).map(|n| n.view())
    }

    pub(super) fn slot(&self, id: NodeId) -> Option<&Slot> {
        self.slots.get(id as usize)?.as_ref()
    }

    pub(super) fn slot_mut(&mut self, id: NodeId) -> Option<&mut Slot> {
        self.slots.get_mut(id as usize)?.as_mut()
    }

    pub(super) fn take(&mut self, id: NodeId) -> Option<Slot> {
        self.slots.get_mut(id as usize)?.take()
    }

    pub(super) fn ensure(&mut self, id: NodeId) {
        if self.slots.len() <= id as usize {
            self.slots.resize_with(id as usize + 1, || None);
        }
    }

    #[allow(dead_code)] // the host tests read the field; a surface does not.
    pub(crate) fn field(&self, id: NodeId) -> Option<Retained<Field>> {
        match self.native(id)? {
            Native::Input(field) => Some(field.retain()),
            _ => None,
        }
    }

    /// The input's native view, one line or several.
    pub(crate) fn input_view(&self, id: NodeId) -> Option<Retained<UIView>> {
        match self.native(id)? {
            Native::Input(field) => {
                let view: &UIView = field;
                Some(view.retain())
            }
            Native::Area(area) => {
                let view: &UIView = area.view();
                Some(view.retain())
            }
            _ => None,
        }
    }

    pub(crate) fn scroll(&self, id: NodeId) -> Option<Retained<UIScrollView>> {
        match self.native(id)? {
            Native::Scroll(scroll) => Some(scroll.retain()),
            _ => None,
        }
    }

    pub(crate) fn text(&self, id: NodeId) -> Option<Retained<TextView>> {
        match self.native(id)? {
            Native::Text(view) => Some(view.retain()),
            _ => None,
        }
    }

    #[cfg(feature = "host-tests")]
    pub(crate) fn stack(&self, id: NodeId) -> Option<&crate::navigation::StackHost> {
        match self.native(id)? {
            Native::Stack(stack) => Some(stack),
            _ => None,
        }
    }

    #[cfg(feature = "host-tests")]
    pub(crate) fn screen(&self, id: NodeId) -> Option<&crate::navigation::ScreenHost> {
        match self.native(id)? {
            Native::Screen(screen) => Some(screen),
            _ => None,
        }
    }

    #[cfg(feature = "host-tests")]
    pub(crate) fn modal(&self, id: NodeId) -> Option<&crate::navigation::ModalHost> {
        match self.native(id)? {
            Native::Modal(modal) => Some(modal),
            _ => None,
        }
    }
}
