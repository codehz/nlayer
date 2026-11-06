//! Screens, modals, headers, back and `env()` insets.

use super::*;

/// What `env` comes to now: the surface safe area and the keyboard height.
/// Left and right stay physical.
fn env_value(safe: Edges<Points>, keyboard: Points, var: style::EnvVar) -> Points {
    use style::EnvVar;
    match var {
        EnvVar::KeyboardInsetHeight => keyboard.max(Points::ZERO),
        EnvVar::SafeAreaInsetTop => safe.top,
        EnvVar::SafeAreaInsetRight => safe.right,
        EnvVar::SafeAreaInsetBottom => safe.bottom,
        EnvVar::SafeAreaInsetLeft => safe.left,
    }
}

/// The points `term` is now.
fn env_term(safe: Edges<Points>, keyboard: Points, term: style::EnvTerm) -> Points {
    use style::EnvTerm;
    match term {
        EnvTerm::Var(var) => env_value(safe, keyboard, var),
        EnvTerm::Max(a, b) => env_value(safe, keyboard, a).max(env_value(safe, keyboard, b)),
    }
}

/// Adds point insets to padding and margin lengths. Percentages and `auto`
/// margins cannot take an addition, as in nsh.
pub(super) fn add_insets(layout: &mut TaffyStyle, padding: Edges<Points>, margin: Edges<Points>) {
    let add_pad = |p: &mut taffy::LengthPercentage, d: f32| {
        if d == 0.0 {
            return;
        }
        if let taffy::style::ExpandedLengthPercentage::Length(v) = p.expand() {
            *p = taffy::LengthPercentage::length(v + d);
        }
    };
    add_pad(&mut layout.padding.top, padding.top.0);
    add_pad(&mut layout.padding.right, padding.right.0);
    add_pad(&mut layout.padding.bottom, padding.bottom.0);
    add_pad(&mut layout.padding.left, padding.left.0);
    let add_margin = |m: &mut taffy::LengthPercentageAuto, d: f32| {
        if d == 0.0 {
            return;
        }
        if let taffy::style::ExpandedLengthPercentageAuto::Length(v) = m.expand() {
            *m = taffy::LengthPercentageAuto::length(v + d);
        }
    };
    add_margin(&mut layout.margin.top, margin.top.0);
    add_margin(&mut layout.margin.right, margin.right.0);
    add_margin(&mut layout.margin.bottom, margin.bottom.0);
    add_margin(&mut layout.margin.left, margin.left.0);
}

/// Replaces insets `was` with `now` on a style that already has `was` added.
fn shift_insets(layout: &mut TaffyStyle, was: [Edges<Points>; 2], now: [Edges<Points>; 2]) {
    let delta = |now: Edges<Points>, was: Edges<Points>| Edges {
        top: now.top - was.top,
        right: now.right - was.right,
        bottom: now.bottom - was.bottom,
        left: now.left - was.left,
    };
    add_insets(layout, delta(now[0], was[0]), delta(now[1], was[1]));
}

impl<M: TextEngine> Core<M> {
    /// Sets the height of the platform's screen header bar, normal and with
    /// a large title (status bar excluded). Platforms without a native header
    /// set [`NO_NATIVE_HEADER`].
    pub fn set_header_heights(&mut self, normal: Points, large: Points) {
        if self.header_heights != (normal, large) {
            self.header_heights = (normal, large);
            self.needs_layout = true;
            // A header appearing or leaving changes which top inset a
            // screen's content is owed.
            self.sync_env();
        }
    }

    /// The platform measured where the content of screen `id` starts below
    /// the header it shows (`None`: estimate it from the bar's height and
    /// the safe area). Used for screens that lay their content out below
    /// the header (not transparent headers).
    pub fn set_screen_header(&mut self, id: NodeId, height: Option<Points>) {
        let Ok(key) = self.key(id) else { return };
        let height = height.map(|h| h.0).filter(|h| h.is_finite() && *h >= 0.0);
        if let Some(root) = self.nodes[key as usize].root.as_mut()
            && root.header != height
        {
            root.header = height;
            self.needs_layout = true;
        }
    }

    /// The platform presents root `id` (a modal) at `size`, with `insets`
    /// of it covered by system UI. Those insets are what `env()` reads
    /// inside the modal.
    pub fn set_root_size(&mut self, id: NodeId, size: Size<Points>, insets: Edges<Points>) {
        let Ok(key) = self.key(id) else { return };
        let insets_changed = {
            let Some(root) = self.nodes[key as usize].root.as_mut() else {
                return;
            };
            let transparent = root.screen.header_transparent;
            let insets_changed =
                !root.insets_set || root.insets != insets || root.insets_transparent != transparent;
            let size_changed = root.reported != Some(size);
            if !insets_changed && !size_changed {
                return;
            }
            root.reported = Some(size);
            root.insets = insets;
            root.insets_set = true;
            root.insets_transparent = transparent;
            insets_changed
        };
        self.needs_layout = true;
        if insets_changed {
            self.sync_env();
        }
    }

    /// The platform reported the insets of screen or modal `id` (`onInsets`):
    /// what `env(safe-area-inset-*)` reads inside it. For an opaque header
    /// the top is what the header does not already cover. For a transparent
    /// header the top includes the bar: the content starts underneath it,
    /// and iOS's safe area counts that bar.
    pub fn set_root_insets(&mut self, id: NodeId, insets: Edges<Points>) {
        let Ok(key) = self.key(id) else { return };
        let changed = {
            let Some(root) = self.nodes[key as usize].root.as_mut() else {
                return;
            };
            let transparent = root.screen.header_transparent;
            if root.insets_set && root.insets == insets && root.insets_transparent == transparent {
                return;
            }
            root.insets = insets;
            root.insets_set = true;
            root.insets_transparent = transparent;
            true
        };
        if changed {
            self.sync_env();
        }
    }

    /// Handles back navigation. Returns `true` when a modal receives a close
    /// request or a stack can pop. Emits `onBackPress` first, then
    /// `onRequestClose` for modals or `onDismiss` for a popped screen.
    /// The engine tries `BackHandler` and platform behavior only when this
    /// returns `false`.
    pub fn handle_back(&mut self) -> bool {
        // Priority: topmost modal (last created), then topmost screen (last in its stack).
        let mut candidate: Option<(Key, bool)> = None; // (key, is_modal)

        // Find the last modal still mounted. A removed one stays in `roots`
        // until release; detached, it is not on screen and must not take back.
        for &key in self.roots.iter().rev() {
            let mounted = {
                let node = &self.nodes[key as usize];
                node.created && node.kind == Kind::Modal
            };
            if mounted && self.is_ancestor(self.root, key) {
                candidate = Some((key, true));
                break;
            }
        }

        // If no modal, find the topmost screen of the last stack in creation order.
        if candidate.is_none() {
            for &key in self.roots.iter().rev() {
                let node = &self.nodes[key as usize];
                if !node.created || node.kind != Kind::Screen {
                    continue;
                }
                let stack_key = node.parent;
                if stack_key == NO_KEY {
                    continue;
                }
                let stack = &self.nodes[stack_key as usize];
                if stack.kind != Kind::Stack {
                    continue;
                }
                // Last child of the stack is the topmost screen.
                if let Some(&topmost_key) = stack.children.last() {
                    let topmost = &self.nodes[topmost_key as usize];
                    if topmost.created && topmost.kind == Kind::Screen {
                        candidate = Some((topmost_key, false));
                        break;
                    }
                }
            }
        }

        let Some((key, is_modal)) = candidate else {
            return false;
        };

        let node = &self.nodes[key as usize];
        let id = node.id;
        let back_listens = node.events.contains(EventKind::BackRequest);
        let can_act = if is_modal {
            node.root.as_ref().is_none_or(|r| r.modal.close_on_escape)
        } else {
            let stack_key = node.parent;
            if stack_key == NO_KEY {
                false
            } else {
                let stack = &self.nodes[stack_key as usize];
                stack.children.len() > 1
                    && !node.root.as_ref().is_some_and(|r| r.screen.prevent_back)
            }
        };
        if back_listens {
            self.events.push((id, EventKind::BackRequest));
        }
        if is_modal {
            if node.events.contains(EventKind::RequestClose) {
                self.events.push((id, EventKind::RequestClose));
            }
            // Always consume back for a modal (return true), even when closeOnEscape=false.
            // With closeOnEscape=false, the app decides via onRequestClose; must not fall
            // through to BackHandler/platform default (which would move app to background).
            true
        } else {
            // For screens: return true if non-root (even if popOnBack=false).
            // Emit Dismiss only if can_act is true.
            let stack_key = node.parent;
            let is_root = stack_key != NO_KEY && self.nodes[stack_key as usize].children.len() == 1;
            if is_root {
                false
            } else {
                if can_act && node.events.contains(EventKind::Dismiss) {
                    self.events.push((id, EventKind::Dismiss));
                }
                true
            }
        }
    }

    /// Number of screens / modals / header items above `key`.
    pub(super) fn root_depth(&self, mut key: Key) -> u32 {
        let mut depth = 0;
        key = self.nodes[key as usize].parent;
        while key != NO_KEY {
            let n = &self.nodes[key as usize];
            if n.kind.is_layout_root() {
                depth += 1;
            }
            key = n.parent;
        }
        depth
    }

    /// How far the system UI at the top reaches into `key`:
    /// its root's top inset minus its offset in that root.
    pub(super) fn top_overlap(&self, key: Key) -> f32 {
        let mut y = 0.0;
        let mut k = key;
        loop {
            let n = &self.nodes[k as usize];
            if k == self.root {
                return (self.safe_area.top.0 - y).max(0.0);
            }
            if n.kind.is_layout_root() {
                let inset = match n.kind {
                    Kind::Modal => n.root.as_ref().map_or(0.0, |r| r.insets.top.0),
                    _ => 0.0,
                };
                return (inset - y).max(0.0);
            }
            if !n.frame.origin.y.0.is_nan() {
                y += n.frame.origin.y.0;
            }
            if n.parent == NO_KEY {
                return 0.0;
            }
            k = n.parent;
        }
    }

    fn covering_header(&self, parent: Key, root: &RootState) -> f32 {
        root.header
            .filter(|h| *h > 0.0)
            .unwrap_or_else(|| root.screen.content_bar(false).0 + self.top_overlap(parent))
    }

    /// The platform draws a header of its own above screens (iOS, Android);
    /// without one ([`NO_NATIVE_HEADER`]) a screen's `header` slot is placed
    /// at its top.
    pub(super) fn native_header(&self) -> bool {
        self.header_heights != crate::NO_NATIVE_HEADER
    }

    /// The size of screen `key`: its stack's (`None` outside a stack).
    pub(super) fn screen_size(&self, key: Key) -> Option<Size<Points>> {
        let parent = self.nodes[key as usize].parent;
        let p = self.nodes.get(parent as usize)?;
        let stack_layout = p.layout.filter(|_| p.kind == Kind::Stack)?;
        let s = self.rounded_layout(stack_layout).size;
        Some(crate::geometry::size_pt(s.width, s.height))
    }

    /// The `header` slot item of screen `key`, once created.
    fn header_slot(&self, key: Key) -> Option<Key> {
        self.nodes[key as usize]
            .children
            .iter()
            .copied()
            .find(|&c| {
                let n = &self.nodes[c as usize];
                n.kind == Kind::HeaderItem
                    && n.created
                    && n.root
                        .as_ref()
                        .is_some_and(|r| r.slot == crate::ops::HeaderSlot::Header)
            })
    }

    /// A created `titleView` item of screen `key`.
    pub(super) fn has_title_view(&self, key: Key) -> bool {
        self.nodes[key as usize].children.iter().any(|&c| {
            let n = &self.nodes[c as usize];
            n.kind == Kind::HeaderItem
                && n.created
                && n.root
                    .as_ref()
                    .is_some_and(|r| r.slot == crate::ops::HeaderSlot::Title)
        })
    }

    /// Without a native header: the height of screen `key`'s `header` slot,
    /// laid out first (as wide as the screen), 0 without one.
    pub(super) fn drawn_header(&mut self, key: Key) -> f32 {
        let Some(item) = self.header_slot(key) else {
            return 0.0;
        };
        self.layout_detached(item);
        self.nodes[item as usize]
            .layout
            .map_or(0.0, |l| self.rounded_layout(l).size.height)
    }

    /// The options of screen `key`, with its header items.
    pub(super) fn screen_config(&self, key: Key) -> Option<crate::ScreenConfig> {
        let node = &self.nodes[key as usize];
        let root = node.root.as_ref().filter(|_| node.kind == Kind::Screen)?;
        let mut config = root.screen.clone();
        config.header_height = Points(root.laid_out.map_or(0.0, |(_, h)| h));
        let mut leading = Vec::new();
        let mut trailing = Vec::new();
        for &c in &node.children {
            let child = &self.nodes[c as usize];
            if child.kind != Kind::HeaderItem || !child.created {
                continue;
            }
            let item_root = child.root.as_ref();
            let slot = item_root.map_or(crate::ops::HeaderSlot::Leading, |r| r.slot);
            match slot {
                crate::ops::HeaderSlot::Title => config.title_view = Some(child.id),
                crate::ops::HeaderSlot::Header => config.header_view = Some(child.id),
                crate::ops::HeaderSlot::Leading | crate::ops::HeaderSlot::Trailing => {
                    let group = item_root.map_or(0, |r| r.group) as usize;
                    let index = item_root.map_or(0, |r| r.index) as usize;
                    let groups = if slot == crate::ops::HeaderSlot::Leading {
                        &mut leading
                    } else {
                        &mut trailing
                    };
                    if groups.len() <= group {
                        groups.resize_with(group + 1, Vec::new);
                    }
                    let row = &mut groups[group];
                    if row.len() <= index {
                        row.resize_with(index + 1, || crate::BarItem::Custom(0));
                    }
                    row[index] = bar_item(child.id, item_root.map(|root| root.as_ref()));
                }
            }
        }
        config.leading = compact_groups(leading);
        config.trailing = compact_groups(trailing);
        config.bar_height = config.content_bar(config.title_view.is_some());
        config.sync_cover();
        Some(config)
    }

    /// The insets `env()` uses for `key`: its screen's or modal's, once
    /// reported, else an estimate. A native header covers the top until
    /// the screen reports (nsh `insets_of`), unless it is transparent:
    /// the bar still counts, the way iOS's safe area includes a
    /// transparent navigation bar, estimated as the bar plus the status
    /// inset. A drawn `header` slot (no native header) still reads the
    /// surface's top inset; the screen reports that edge as zero when
    /// the bar is opaque (the slot already covers it) and as the bar's
    /// height when the bar is transparent. Left, right and bottom stay
    /// the screen's.
    fn insets_of(&self, key: Key) -> Edges<Points> {
        let safe = self.safe_area;
        let drawn_header = self.in_drawn_header(key);
        let mut k = key;
        loop {
            let node = &self.nodes[k as usize];
            if matches!(node.kind, Kind::Screen | Kind::Modal)
                && let Some(root) = node.root.as_ref()
            {
                if root.insets_set {
                    let mut insets = root.insets;
                    if drawn_header {
                        insets.top = safe.top;
                        return insets;
                    }
                    // The report still describes the other cover. Use the
                    // bar this commit is leaving or entering, so content
                    // that pads by `env(safe-area-inset-top)` does not jump
                    // while the platform's next report is in flight.
                    if node.kind == Kind::Screen
                        && self.native_header()
                        && root.screen.header_visible
                        && root.screen.header_transparent != root.insets_transparent
                    {
                        insets.top = if root.screen.header_transparent {
                            Points(self.covering_header(node.parent, root))
                        } else {
                            Points::ZERO
                        };
                    }
                    return insets;
                }
                if node.kind == Kind::Screen && self.native_header() && root.screen.header_visible {
                    let top = if root.screen.header_transparent {
                        // The bar plus the surface's top inset: what iOS
                        // reports for a screen under the status bar. Not
                        // the stack's frame; that is not known yet, and
                        // reading it would move this inset on a later layout.
                        Points(root.screen.content_bar(false).0 + safe.top.0)
                    } else {
                        Points::ZERO
                    };
                    return Edges { top, ..safe };
                }
                return safe;
            }
            if node.parent == NO_KEY || k == self.root {
                return safe;
            }
            k = node.parent;
        }
    }

    /// `key` is the app-drawn `header` slot, or inside it. Only when this
    /// platform has no native header: left / right / title slots are not
    /// that bar, and a native header places its items itself.
    fn in_drawn_header(&self, mut key: Key) -> bool {
        if self.native_header() {
            return false;
        }
        loop {
            let node = &self.nodes[key as usize];
            if node.kind == Kind::HeaderItem {
                return node
                    .root
                    .as_ref()
                    .is_some_and(|root| root.slot == crate::ops::HeaderSlot::Header);
            }
            if matches!(node.kind, Kind::Screen | Kind::Modal)
                || node.parent == NO_KEY
                || key == self.root
            {
                return false;
            }
            key = node.parent;
        }
    }

    pub(super) fn env_insets(&self, key: Key, env: &EnvEdges) -> [Edges<Points>; 2] {
        let safe = self.insets_of(key);
        let edge = |term: Option<style::EnvTerm>| {
            term.map_or(Points::ZERO, |term| {
                env_term(safe, self.keyboard_inset, term)
            })
        };
        let edges = |terms: &[Option<style::EnvTerm>; 4]| Edges {
            top: edge(terms[0]),
            right: edge(terms[1]),
            bottom: edge(terms[2]),
            left: edge(terms[3]),
        };
        [edges(&env.padding), edges(&env.margin)]
    }

    /// Restyles nodes whose `env()` terms changed value. Nothing to do
    /// without such nodes. No layout animation: v1 applies the keyboard
    /// inset on the next commit.
    pub(super) fn sync_env(&mut self) {
        if self.env_styled.is_empty() {
            return;
        }
        let ids: Vec<NodeId> = self.env_styled.iter().copied().collect();
        let mut stale = Vec::new();
        for id in ids {
            let Ok(key) = self.key(id) else {
                stale.push(id);
                continue;
            };
            if self.nodes[key as usize].env.is_none() {
                stale.push(id);
                continue;
            }
            self.sync_env_key(key);
        }
        for id in stale {
            self.env_styled.remove(&id);
        }
    }

    /// Restyles `key` when the insets its `env()` terms come to changed.
    fn sync_env_key(&mut self, key: Key) {
        let Some(env) = self.nodes[key as usize].env.as_deref().map(|(env, _)| *env) else {
            return;
        };
        let now = self.env_insets(key, &env);
        let was = self.nodes[key as usize].env.as_ref().expect("env").1;
        if now == was {
            return;
        }
        self.nodes[key as usize].env.as_mut().expect("env").1 = now;
        if self.nodes[key as usize].kind == Kind::Scroll {
            self.sync_scroll_style(key);
            return;
        }
        // A layout root is laid out from its base style every time.
        if self.nodes[key as usize].kind.is_layout_root()
            && let Some(base) = self.nodes[key as usize].base_layout.as_deref_mut()
        {
            shift_insets(base, was, now);
        }
        let Some(layout) = self.nodes[key as usize].layout else {
            return;
        };
        let mut style = self.layout.style(layout).clone();
        shift_insets(&mut style, was, now);
        self.layout.set_style(layout, style);
        self.needs_layout = true;
    }

    /// `key` moved under a screen or modal. Its `env()` nodes, styled
    /// before they were inserted, follow that root's insets.
    pub(super) fn refresh_env(&mut self, key: Key) {
        let mut stack = vec![key];
        let mut keys = Vec::new();
        while let Some(k) = stack.pop() {
            if self.nodes[k as usize].env.is_some() {
                keys.push(k);
            }
            stack.extend(self.nodes[k as usize].children.iter().copied());
        }
        for k in keys {
            self.sync_env_key(k);
        }
    }
}

fn bar_item(id: crate::NodeId, root: Option<&RootState>) -> crate::BarItem {
    let Some(root) = root else {
        return crate::BarItem::Custom(id);
    };
    if !root.button {
        return crate::BarItem::Custom(id);
    }
    if root.bar_title.is_none() && root.bar_image.is_none() {
        return crate::BarItem::Custom(0);
    }
    crate::BarItem::Button {
        id,
        title: root.bar_title.clone(),
        image: root.bar_image.clone(),
        enabled: root.bar_enabled,
        tint_color: root.bar_tint,
    }
}

fn compact_groups(groups: Vec<Vec<crate::BarItem>>) -> Vec<Vec<crate::BarItem>> {
    groups
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .filter(|item| !matches!(item, crate::BarItem::Custom(0)))
                .collect::<Vec<_>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}
