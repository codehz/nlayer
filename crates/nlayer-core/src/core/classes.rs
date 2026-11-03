//! Class styles on the shadow tree.
//!
//! Cut from `nsh_core::core::classes`: `class` / `classList`, parent
//! `space-*` margins, and `focus-within`. Dark, window size, group, sibling
//! position, motion and `ch` are not restyled (those utilities are already
//! unsupported). State paint slots stay paint-only.

use std::rc::Rc;

use crate::{
    ops::NodeId,
    style::{Color, Style, StylePatch},
    tailwind::{ClassEnvironment, ClassList, ClassProps, Context, deps, state},
    text::TextEngine,
};

use super::{Core, Key, Kind, NO_KEY};

/// `class` and the truthy `classList` keys, in order. `classList` is applied
/// after `class`.
#[derive(Debug, Default, Clone)]
pub(super) struct ClassNames {
    class: Box<str>,
    list: Vec<Box<str>>,
}

/// Bits the platform (or a test) reports. `disabled:` reads the prop.
/// `focus-within` is maintained here.
const REPORTED: u16 = state::PRESSED | state::HOVERED | state::FOCUSED;

impl<M: TextEngine> Core<M> {
    /// The platform, or a headless test, reports interaction states of view
    /// `id`. Only [`REPORTED`] bits are replaced. A disabled view is not
    /// pressed, so `active:` does not match.
    pub fn set_states(&mut self, id: NodeId, states: u16) {
        let Ok(key) = self.key(id) else { return };
        self.set_node_states(key, states);
    }

    fn set_node_states(&mut self, key: Key, states: u16) {
        let node = &mut self.nodes[key as usize];
        let mut new = node.states & !REPORTED | states & REPORTED;
        if new & state::DISABLED != 0 {
            new &= !state::PRESSED;
        }
        if new == node.states {
            return;
        }
        let focus_changed = (new ^ node.states) & state::FOCUSED != 0;
        node.states = new;
        self.states_changed(key);
        if focus_changed {
            self.focus_reported(key, new & state::FOCUSED != 0);
        }
    }

    /// View `key` took or lost the keyboard focus. One focus per surface: a
    /// view taking it replaces the previous one, and a blur of a view that
    /// is not the focus changes nothing. The root is never the focus. The
    /// chain is resolved at the next [`Core::commit`], so a blur then a
    /// focus leaves the views both chains share untouched.
    fn focus_reported(&mut self, key: Key, focused: bool) {
        let id = self.nodes[key as usize].id;
        if focused {
            self.focus = (key != self.root).then_some((key, id));
        } else if self.focus == Some((key, id)) {
            self.focus = None;
        } else {
            return;
        }
        self.focus_moved = true;
    }

    /// Puts `focus-within` on the focus and its parents, and takes it off
    /// the views it left. Ancestors that stay on both chains are not
    /// restyled. No overlay or portal: the chain is the parent links.
    pub(super) fn update_focus_within(&mut self) {
        self.focus_moved = false;
        let focus = self
            .focus
            .filter(|&(k, id)| self.nodes.get(k as usize).is_some_and(|n| n.id == id));
        if focus.is_none() {
            self.focus = None;
        }
        let mut chain = std::mem::take(&mut self.focus_scratch);
        chain.clear();
        if let Some((mut k, _)) = focus {
            while k != NO_KEY {
                let node = &self.nodes[k as usize];
                chain.push((k, node.id));
                k = node.parent;
            }
        }
        if chain == self.focus_chain {
            self.focus_scratch = chain;
            return;
        }
        let old = std::mem::replace(&mut self.focus_chain, chain);
        for &(k, id) in &old {
            if self.focus_chain.contains(&(k, id)) {
                continue;
            }
            let Some(node) = self.nodes.get_mut(k as usize).filter(|n| n.id == id) else {
                continue;
            };
            if node.states & state::FOCUS_WITHIN != 0 {
                node.states &= !state::FOCUS_WITHIN;
                self.states_changed(k);
            }
        }
        for i in 0..self.focus_chain.len() {
            let k = self.focus_chain[i].0;
            let node = &mut self.nodes[k as usize];
            if node.states & state::FOCUS_WITHIN == 0 {
                node.states |= state::FOCUS_WITHIN;
                self.states_changed(k);
            }
        }
        self.focus_scratch = old;
    }

    /// `disabled` on a host element: the `disabled:` / `enabled:` variants.
    /// The input and accessibility updates stay with the prop; this is the
    /// class bit. Disabled clears a press, so `active:` does not match.
    pub(super) fn set_disabled(&mut self, key: Key, disabled: bool) {
        let node = &mut self.nodes[key as usize];
        let before = node.states;
        if disabled {
            node.states |= state::DISABLED;
            node.states &= !state::PRESSED;
        } else {
            node.states &= !state::DISABLED;
        }
        if node.states != before {
            self.states_changed(key);
        }
    }

    /// Restyles `key` when its classes read its own states.
    fn states_changed(&mut self, key: Key) {
        let restyle = self.nodes[key as usize]
            .classes
            .as_ref()
            .is_some_and(|c| c.deps() & deps::STATE != 0);
        if restyle {
            self.restyle(key);
        }
    }

    /// Sets `class`. An empty string clears it and leaves `classList`.
    pub(super) fn set_class_name(&mut self, key: Key, name: Box<str>) {
        self.set_classes(key, Some(name), None);
    }

    /// Replaces `classList` with the truthy keys, in order.
    pub(super) fn set_class_list_names(&mut self, key: Key, list: Vec<Box<str>>) {
        self.set_classes(key, None, Some(list));
    }

    fn set_classes(&mut self, key: Key, names: Option<Box<str>>, list: Option<Vec<Box<str>>>) {
        let (class, listed) = {
            let current = self.nodes[key as usize]
                .class_names
                .get_or_insert_with(Default::default);
            if let Some(n) = names {
                current.class = n;
            }
            if let Some(l) = list {
                current.list = l;
            }
            (current.class.clone(), current.list.clone())
        };
        if class.trim().is_empty() && listed.is_empty() {
            self.nodes[key as usize].class_names = None;
            self.set_resolved_classes(key, None);
            return;
        }
        let mut combined = String::with_capacity(class.len() + 16 * listed.len());
        combined.push_str(&class);
        for name in &listed {
            combined.push(' ');
            combined.push_str(name);
        }
        let classes = if combined.trim().is_empty() {
            None
        } else {
            let (classes, fresh) = self.tailwind.get(&combined);
            if fresh {
                // Once per distinct string. A cache hit does not warn.
                if let Some(message) =
                    crate::unknown_classes_warning(classes.unknown().iter().map(|s| &**s))
                {
                    log::warn!("{message}");
                }
                if let Some(message) =
                    crate::unsupported_classes_warning(classes.unsupported().iter().map(|s| &**s))
                {
                    log::warn!("{message}");
                }
            }
            Some(classes)
        };
        self.set_resolved_classes(key, classes);
    }

    fn set_resolved_classes(&mut self, key: Key, classes: Option<Rc<ClassList>>) {
        self.nodes[key as usize].classes = classes;
        self.restyle(key);
    }

    /// Stores the `style` prop. With no classes and no parent `space-*`,
    /// `style` is applied as given. Otherwise the node is rebuilt from its
    /// classes and this patch; the stored style is what clearing classes
    /// restores.
    pub(super) fn apply_authored_style(
        &mut self,
        id: NodeId,
        style: Style,
        patch: StylePatch,
    ) -> Result<(), super::ApplyError> {
        let key = self.key(id)?;
        let rebuild = self.nodes[key as usize].classes.is_some() || self.space_of(key).is_some();
        let node = &mut self.nodes[key as usize];
        node.inline_style = patch;
        node.authored = Some(Box::new(style));
        if rebuild {
            self.restyle(key);
            Ok(())
        } else {
            let style = self.nodes[key as usize]
                .authored
                .as_deref()
                .expect("style was just stored")
                .clone();
            self.set_style(id, style)
        }
    }

    /// The margins `key`'s parent gives it (`space-*`): an element child,
    /// not inline, and not the last element child.
    fn space_of(&self, key: Key) -> Option<&crate::tailwind::Space> {
        let node = &self.nodes[key as usize];
        let parent = node.parent;
        if parent == NO_KEY || node.inline {
            return None;
        }
        let space = self.nodes[parent as usize]
            .class_props
            .as_ref()?
            .space
            .as_ref()?;
        let last = self.nodes[parent as usize]
            .children
            .iter()
            .rev()
            .find(|&&c| self.is_element_child(c))?;
        (*last != key && self.is_element_child(key)).then_some(space)
    }

    /// A child that counts for `space-*` and sibling position. Not raw text.
    /// Inline is already excluded by [`Self::space_of`].
    fn is_element_child(&self, key: Key) -> bool {
        self.nodes[key as usize].kind != Kind::RawText
    }

    /// `parent`'s children changed (`moved` joined or left it). Restyle the
    /// child that moved and the last two element children, whose last-ness
    /// is what `space-*` reads. A child that left has no parent, so the
    /// margin it still carries clears.
    pub(super) fn spacing_changed(&mut self, parent: Key, moved: Key) {
        let spacing = self.nodes[parent as usize]
            .class_props
            .as_ref()
            .is_some_and(|p| p.space.is_some());
        if !spacing && (moved == NO_KEY || !self.nodes[moved as usize].spaced) {
            return;
        }
        let mut keys = [moved, NO_KEY, NO_KEY];
        let mut elements = self.nodes[parent as usize]
            .children
            .iter()
            .rev()
            .filter(|&&c| self.is_element_child(c));
        keys[1] = elements.next().copied().unwrap_or(NO_KEY);
        keys[2] = elements.next().copied().unwrap_or(NO_KEY);
        for (i, k) in keys.into_iter().enumerate() {
            if k == NO_KEY || keys[..i].contains(&k) {
                continue;
            }
            let (now, was) = (self.space_of(k).is_some(), self.nodes[k as usize].spaced);
            // The one that moved may have left another `space-*` parent,
            // whose margin it still has.
            if now != was || i == 0 && now {
                self.restyle(k);
            }
        }
    }

    /// Resolves `key`'s style from its classes and inline style, then runs
    /// the normal style commit. The stored `style` prop is not stacked on
    /// top of classes (that would double padding); the patch is replayed
    /// inside [`ClassList::apply`], so `!` still beats it per key.
    pub(super) fn restyle(&mut self, key: Key) {
        let rtl = self.resolved_rtl(key);
        let space = self.space_of(key).cloned();
        self.nodes[key as usize].spaced = space.is_some();
        let has_classes = self.nodes[key as usize].classes.is_some();
        let mut props = ClassProps::default();
        let style = if has_classes {
            let mut style = Style::default();
            if let Some(space) = &space {
                space.apply(&mut style, false, rtl);
            }
            self.resolve_classes(key, &mut style, &mut props);
            style
        } else if space.is_some() && !self.nodes[key as usize].inline_style.is_empty() {
            // Parent `space-*` under the inline declarations, which win
            // unless the space utility is important.
            let mut style = Style::default();
            if let Some(space) = &space {
                space.apply(&mut style, false, rtl);
            }
            let patch = self.nodes[key as usize].inline_style.clone();
            patch.apply_defaults(&mut style, Color::BLACK);
            patch.apply(&mut style);
            if let Some(space) = &space {
                space.apply(&mut style, true, rtl);
            }
            style
        } else {
            // No classes: the last `style` prop, not a rebuild from a
            // partial patch. Space, when the patch recorded nothing, is
            // applied on that style.
            let mut style = self.nodes[key as usize]
                .authored
                .as_deref()
                .cloned()
                .unwrap_or_default();
            if let Some(space) = &space {
                space.apply(&mut style, false, rtl);
                space.apply(&mut style, true, rtl);
            }
            style
        };
        self.set_class_props(key, props);
        let id = self.nodes[key as usize].id;
        let _ = self.set_style(id, style);
    }

    /// Classes and the inline patch over `style`. `currentColor` is the
    /// default (black): no color-inheritance walk.
    fn resolve_classes(&mut self, key: Key, style: &mut Style, props: &mut ClassProps) {
        let Some(classes) = self.nodes[key as usize].classes.clone() else {
            return;
        };
        let states = self.nodes[key as usize].states;
        let rtl = self.resolved_rtl(key);
        let inline = self.nodes[key as usize].inline_style.clone();
        let space = self.space_of(key).cloned();
        let window = self.viewport;
        let group = |_: &str| 0u16;
        let cx = Context {
            env: ClassEnvironment::default(),
            window,
            rtl,
            states,
            position: 0,
            group: &group,
        };
        inline.apply_defaults(style, Color::BLACK);
        let theme = self.tailwind.theme();
        classes.apply(&cx, theme, style, props, |s| {
            inline.apply(s);
            if let Some(space) = &space {
                space.apply(s, true, rtl);
            }
        });
    }

    /// Stores the node props classes set, and restyles children when the
    /// `space-*` margins they inherit changed.
    fn set_class_props(&mut self, key: Key, props: ClassProps) {
        let node = &mut self.nodes[key as usize];
        let old = node.class_props.take().map(|b| *b).unwrap_or_default();
        if old == props {
            node.class_props = (props != ClassProps::default()).then(|| Box::new(props));
            return;
        }
        let space_changed = old.space != props.space;
        let selection_changed = old.selection_color != props.selection_color;
        let kind = node.kind;
        node.class_props = (props != ClassProps::default()).then(|| Box::new(props));
        if selection_changed && kind == Kind::Text {
            self.mark_text_block(key);
        }
        if !space_changed {
            return;
        }
        let children = self.nodes[key as usize].children.clone();
        for child in children {
            if self.is_element_child(child) && !self.nodes[child as usize].inline {
                self.restyle(child);
            }
        }
    }
}
