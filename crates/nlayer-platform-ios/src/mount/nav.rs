//! Stacks, screens, modals and header items.

use nlayer_core::{EventKind, NodeId};
use nlayer_engine::EventPayload;
use objc2::Message;
use objc2::rc::Retained;
use objc2_ui_kit::{UIView, UIViewControllerTransitionCoordinator};

use super::*;

impl Mounter {
    pub(super) fn parent_id(&self, view: &UIView) -> Option<u32> {
        let mut parent = view.superview()?;
        loop {
            if let Some(id) = self.node_id(&parent) {
                return Some(id);
            }
            parent = parent.superview()?;
        }
    }

    /// Screens, header items and modals are not subviews of their parent:
    /// records them for [`Mounter::finish_navigation`]. Returns true if
    /// `child` is one of them.
    pub(super) fn insert_hosted(
        &mut self,
        parent: NodeId,
        child: NodeId,
        before: Option<NodeId>,
    ) -> bool {
        let Some(c) = self.slot(child) else {
            return false;
        };
        match &c.native {
            Native::Screen(_) => {
                if let Some(Slot {
                    native: Native::Stack(stack),
                    ..
                }) = self.slot_mut(parent)
                {
                    stack.screens.retain(|&s| s != child);
                    let pos = before
                        .and_then(|b| stack.screens.iter().position(|&s| s == b))
                        .unwrap_or(stack.screens.len());
                    stack.screens.insert(pos, child);
                    self.dirty_stacks.push(parent);
                }
            }
            Native::HeaderItem(_) => self.dirty_headers.push(parent),
            Native::Modal(_) => self.dirty_modals.push(child),
            _ => return false,
        }
        self.hosts.insert(child, parent);
        true
    }

    /// The counterpart of [`Mounter::insert_hosted`] for removals.
    pub(super) fn remove_hosted(&mut self, id: NodeId) -> bool {
        let Some(parent) = self.hosts.remove(&id) else {
            return false;
        };
        match self.slot(id).map(|s| &s.native) {
            Some(Native::Screen(_)) => {
                if let Some(Slot {
                    native: Native::Stack(stack),
                    ..
                }) = self.slot_mut(parent)
                {
                    stack.screens.retain(|&s| s != id);
                    stack.popped.retain(|&s| s != id);
                    self.dirty_stacks.push(parent);
                }
            }
            Some(Native::HeaderItem(view)) => {
                view.removeFromSuperview();
                self.dirty_headers.push(parent);
            }
            Some(Native::Modal(_)) => self.dirty_modals.push(id),
            _ => {}
        }
        true
    }

    /// The popup is the view after the backdrop and often fills the dialog
    /// (`flex-1`) to center a smaller card. That box would take every press
    /// that should hit the backdrop. Its own box is not a hit; the views
    /// inside it still are. With no backdrop, the popup takes presses again.
    pub(super) fn pass_modal_presses(&self) {
        let modals: Vec<NodeId> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                slot.as_ref()
                    .is_some_and(|slot| matches!(slot.native, Native::Modal(_)))
                    .then_some(index as NodeId)
            })
            .collect();
        for id in modals {
            let Some(view) = self.view(id).map(|view| view.retain()) else {
                continue;
            };
            let container = self
                .slot(id)
                .and_then(|slot| slot.content.clone())
                .unwrap_or(view);
            let subs = container.subviews();
            let nodes: Vec<Retained<UIView>> = (0..subs.count())
                .map(|index| subs.objectAtIndex(index))
                .filter(|child| child.tag() > 0)
                .collect();
            let pass = nodes.len() > 1;
            for (index, node) in nodes.iter().enumerate() {
                if let Some(box_view) = node.downcast_ref::<BoxView>() {
                    box_view.set_pass_box(pass && index > 0);
                }
            }
        }
    }

    /// Applies the navigation changes of a batch once all its views exist
    /// and are laid out: pushes and pops screens, updates headers, presents
    /// and dismisses modals.
    pub(super) fn finish_navigation(&mut self) {
        let mut stacks = std::mem::take(&mut self.dirty_stacks);
        stacks.sort_unstable();
        stacks.dedup();
        for id in stacks {
            let Some(Slot {
                native: Native::Stack(stack),
                ..
            }) = self.slot(id)
            else {
                continue;
            };
            if let Some(coordinator) = stack.nav.transitionCoordinator() {
                if !self.deferred_stacks.contains(&id) {
                    self.deferred_stacks.push(id);
                    let done = block2::RcBlock::new(
                        move |_: std::ptr::NonNull<
                            objc2::runtime::ProtocolObject<
                                dyn objc2_ui_kit::UIViewControllerTransitionCoordinatorContext,
                            >,
                        >| {
                            Self::finish_deferred(id);
                        },
                    );
                    // Refused when the transition is already ending.
                    if !coordinator.animateAlongsideTransition_completion(None, Some(&done)) {
                        Self::finish_deferred(id);
                    }
                }
                continue;
            }
            let screens: Vec<_> = stack
                .screens
                .iter()
                .filter(|s| !stack.popped.contains(s))
                .filter_map(|&s| match self.slot(s).map(|s| &s.native) {
                    Some(Native::Screen(screen)) => {
                        Some((s, screen.controller.clone(), screen.config.animation))
                    }
                    _ => None,
                })
                .collect();
            let leaving = stack
                .nav
                .topViewController()
                .and_then(|c| c.downcast::<crate::navigation::ScreenController>().ok())
                .and_then(|c| match self.slot(c.screen_id()).map(|s| &s.native) {
                    Some(Native::Screen(screen)) => Some(screen.config.animation),
                    _ => None,
                });
            stack.layout();
            stack.reconcile(&screens, leaving);
            // A screen shown without its `willShow` callback (the delegate
            // can arrive while this batch holds the mounter) or one whose
            // header options changed while it was on top still gets the bar
            // the config asks for.
            if let Some(&top) = stack
                .screens
                .iter()
                .rev()
                .find(|s| !stack.popped.contains(s))
                && let Some(Native::Screen(screen)) = self.slot(top).map(|s| &s.native)
                && stack.nav.isNavigationBarHidden() != !screen.config.header_visible
            {
                stack
                    .nav
                    .setNavigationBarHidden_animated(!screen.config.header_visible, false);
            }
            if let Some(Slot {
                native: Native::Stack(stack),
                ..
            }) = self.slot_mut(id)
            {
                stack.contain();
            }
        }
        let mut headers = std::mem::take(&mut self.dirty_headers);
        headers.sort_unstable();
        headers.dedup();
        for id in headers {
            let Some(Slot {
                native: Native::Screen(screen),
                ..
            }) = self.slot(id)
            else {
                continue;
            };
            screen.apply_header(
                |item| match self.slot(item).map(|s| &s.native) {
                    Some(Native::HeaderItem(v)) => Some(v.retain()),
                    _ => None,
                },
                self.mtm,
            );
        }
        let modals = std::mem::take(&mut self.dirty_modals);
        let root = self
            .slot(nlayer_core::ROOT_ID)
            .map(|s| s.native.view().retain());
        for id in modals {
            let shown = self.hosts.contains_key(&id);
            let control = self.control.clone();
            let Some(Slot {
                native: Native::Modal(modal),
                ..
            }) = self.slot_mut(id)
            else {
                continue;
            };
            if shown {
                if let Some(root) = &root {
                    modal.present(root);
                }
            } else {
                modal.dismiss(move || {
                    if let Some(control) = control.as_ref() {
                        control.dispatch_event(id, EventKind::Dismiss, EventPayload::None);
                    }
                });
            }
        }
    }

    /// A stack is about to show a screen: its header options apply.
    pub(crate) fn stack_will_show(
        &mut self,
        nav: &objc2_ui_kit::UINavigationController,
        controller: &objc2_ui_kit::UIViewController,
        animated: bool,
    ) {
        let Some(screen) = controller.downcast_ref::<crate::navigation::ScreenController>() else {
            return;
        };
        let Some(Slot {
            native: Native::Screen(screen),
            ..
        }) = self.slot(screen.screen_id())
        else {
            return;
        };
        let c = &screen.config;
        nav.setNavigationBarHidden_animated(!c.header_visible, animated);
        crate::navigation::style_bar(&nav.navigationBar(), c);
    }

    /// A stack finished a transition: screens the user popped (back button,
    /// swipe) are reported so the app removes them.
    pub(crate) fn stack_did_show(&mut self, id: NodeId) {
        self.report_popped(id);
        if self.deferred_stacks.contains(&id) {
            Self::finish_deferred(id);
        }
    }

    /// Applies the changes stack `id` deferred during a transition, on the
    /// next main-queue turn (the transition coordinator outlives the
    /// callbacks that end it), unless another callback already did.
    pub(super) fn finish_deferred(id: NodeId) {
        dispatch2::DispatchQueue::main().exec_async(move || {
            if let Some(m) = current()
                && let Ok(mut m) = m.try_borrow_mut()
                && let Some(i) = m.deferred_stacks.iter().position(|&s| s == id)
            {
                m.deferred_stacks.swap_remove(i);
                m.dirty_stacks.push(id);
                m.finish_navigation();
            }
        });
    }

    /// Reports the screens of stack `id` that the user popped natively.
    pub(super) fn report_popped(&mut self, id: NodeId) {
        let Some(Slot {
            native: Native::Stack(stack),
            ..
        }) = self.slot(id)
        else {
            return;
        };
        let shown = stack.nav.viewControllers();
        // Screens the controller had and lost; not those still waiting for
        // a transition to end.
        let gone: Vec<NodeId> = stack
            .applied
            .borrow()
            .iter()
            .copied()
            .filter(|s| stack.screens.contains(s))
            .filter(|s| !stack.popped.contains(s))
            .filter(|&s| match self.slot(s).map(|s| &s.native) {
                Some(Native::Screen(screen)) => !shown.iter().any(|c| {
                    std::ptr::eq::<objc2_ui_kit::UIViewController>(&*c, &**screen.controller)
                }),
                _ => false,
            })
            .collect();
        if gone.is_empty() {
            return;
        }
        if let Some(Slot {
            native: Native::Stack(stack),
            ..
        }) = self.slot_mut(id)
        {
            stack.popped.extend(&gone);
        }
        for screen in gone {
            if let Some(control) = &self.control {
                control.dispatch_event(screen, EventKind::Dismiss, EventPayload::None);
            }
        }
    }

    /// The top screen of stack `id`, whether it listens to `onBackPress`
    /// and whether it keeps the user (`prevent_back`).
    pub(crate) fn stack_back_press(&self, id: NodeId) -> Option<(NodeId, bool, bool)> {
        let Some(Slot {
            native: Native::Stack(stack),
            ..
        }) = self.slot(id)
        else {
            return None;
        };
        let top = *stack
            .screens
            .iter()
            .rev()
            .find(|s| !stack.popped.contains(s))?;
        match self.slot(top) {
            Some(Slot {
                native: Native::Screen(screen),
                events,
                ..
            }) => Some((
                top,
                events.contains(EventKind::BackRequest),
                screen.config.prevent_back,
            )),
            _ => None,
        }
    }

    /// The platform's back: the engine pops the top screen (or a screen that
    /// keeps the user hears `onBackPress` first) and the mounter follows the
    /// ops.
    pub(crate) fn back(&self) {
        if let Some(control) = &self.control {
            control.back();
        }
    }

    /// Sends a payload-less event to the engine.
    pub(crate) fn dispatch(&self, id: NodeId, kind: EventKind) {
        if let Some(control) = &self.control {
            control.dispatch_event(id, kind, EventPayload::None);
        }
    }

    /// Whether the user may pop the top screen of stack `id`.
    pub(crate) fn stack_can_pop(&self, id: NodeId) -> bool {
        let Some(Slot {
            native: Native::Stack(stack),
            ..
        }) = self.slot(id)
        else {
            return false;
        };
        let visible = stack
            .screens
            .iter()
            .filter(|s| !stack.popped.contains(s))
            .count();
        let top = stack
            .screens
            .iter()
            .rev()
            .find(|s| !stack.popped.contains(s))
            .and_then(|&s| match self.slot(s).map(|s| &s.native) {
                Some(Native::Screen(screen)) if visible > 1 => {
                    Some((screen.config.gesture_enabled, screen.config.prevent_back))
                }
                _ => None,
            });
        match top {
            Some((gesture, prevent)) => {
                gesture && !prevent && stack.nav.transitionCoordinator().is_none()
            }
            None => false,
        }
    }
}
