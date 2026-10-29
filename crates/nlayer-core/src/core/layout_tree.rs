//! The core's layout tree: Taffy's algorithms (flexbox, grid, block, out of
//! flow) run over nodes the core keeps, through Taffy's low-level traits,
//! as `TaffyTree` runs them over its own (`TaffyView`, which this follows
//! call for call). What the core does differently is its own:
//!
//! - **dirty marking goes up to the root, always.** `TaffyTree::mark_dirty`
//!   stops at the first node whose cache is already empty, taking its
//!   ancestors for dirty too; a node of a `display: none` subtree or an
//!   absolutely positioned one always has an empty cache while the nodes
//!   above keep theirs, so a change there was never laid out.
//! - **no hoisted children.** Taffy records, on each containing block, the
//!   absolutely positioned nodes it laid out, for its rounding pass alone;
//!   the shadow tree rounds layouts itself, so nothing is kept,
//!   and nothing can go stale.
//! - **no rounding**: [`LayoutTree::unrounded_layout`] is what the
//!   algorithms computed.
//! - **relayout boundaries.** A change inside a node whose size its content
//!   cannot change (fixed points both ways, in a flex container, not
//!   shrinking nor aligned by its baseline: [`LayoutTree::is_boundary`])
//!   dirties nothing above it: [`LayoutTree::relayout_boundaries`] lays its
//!   subtree out again with what its parent gave it last time, and only
//!   when what it gives back differs (its overflow, say) is its parent laid
//!   out again. A large page with values changing often costs what changes,
//!   not what the page is.
//! - **leaves that measure as before.** A leaf whose content changed (its
//!   text) dirties nothing when it measures the same for every input the
//!   layout asked of it since its cache was last cleared
//!   ([`LayoutTree::measures_as_before`]): every layout above was computed
//!   from those answers alone, so all of them stand. A price that keeps its
//!   width, a wrapped paragraph that keeps its lines, cost their text.
//!
//! Extracted from `nsh_core::core::layout_tree`.
#![allow(dead_code)] // `Core` is the caller, and it is not extracted yet

use slab::Slab;
use taffy::{
    AvailableSpace, BlockContext, Cache, CacheTree, DetailedGridInfo, DetailedLayoutInfo, Display,
    Layout, LayoutBlockContainer, LayoutContainingBlock, LayoutFlexboxContainer,
    LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, RunMode, Size,
    Style, TraversePartialTree, TraverseTree, compute_block_layout, compute_cached_layout,
    compute_flexbox_layout, compute_grid_layout, compute_hidden_layout, compute_oof_layout,
    compute_root_layout,
};

/// A node: its style, place in the tree, cache and layout, and what
/// measures it when it is a leaf.
struct LayoutNode<C> {
    style: Style,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    cache: Cache,
    unrounded: Layout,
    detailed: DetailedLayoutInfo,
    context: Option<C>,
    /// The core's node it lays out ([`LayoutTree::set_tag`]).
    tag: Option<u32>,
    /// What its parent's last full layout gave it and what it gave back:
    /// what a boundary is laid out again with, and compared to.
    last: Option<(LayoutInput, LayoutOutput)>,
    /// It is a boundary waiting in [`LayoutTree::dirty_boundaries`].
    queued: bool,
    /// A leaf's answers to the inputs the layout asked of it since its
    /// cache was last cleared, each once: what the layouts above it were
    /// computed from. `None` once there were more than [`MEASURED`] to keep.
    measured: Option<Vec<(LayoutInput, LayoutOutput)>>,
}

/// How many answers of a leaf [`LayoutNode::measured`] keeps: a layout asks
/// a few (its sizes under a measuring pass and the final one); more come of
/// many layouts at other sizes (a window resized), and are not worth
/// measuring again.
const MEASURED: usize = 8;

impl<C> LayoutNode<C> {
    fn new(style: Style, context: Option<C>) -> Self {
        LayoutNode {
            style,
            parent: None,
            children: Vec::new(),
            cache: Cache::new(),
            unrounded: Layout::new(),
            detailed: DetailedLayoutInfo::None,
            context,
            tag: None,
            last: None,
            queued: false,
            measured: Some(Vec::new()),
        }
    }

    /// Clears its cached layouts, and so what they were computed from.
    fn clear_cache(&mut self) {
        self.cache.clear();
        self.measured = Some(Vec::new());
    }
}

/// The layout nodes of a core, each with a context `C` its leaves are
/// measured with.
pub(crate) struct LayoutTree<C> {
    nodes: Slab<LayoutNode<C>>,
    /// Boundaries a change inside made dirty, to lay out again.
    dirty_boundaries: Vec<NodeId>,
}

fn key(node: NodeId) -> usize {
    usize::from(node)
}

impl<C> LayoutTree<C> {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        LayoutTree {
            nodes: Slab::with_capacity(capacity),
            dirty_boundaries: Vec::new(),
        }
    }

    /// A new detached node.
    pub(crate) fn new_leaf(&mut self, style: Style) -> NodeId {
        NodeId::from(self.nodes.insert(LayoutNode::new(style, None)))
    }

    /// A new detached node measured with `context`.
    pub(crate) fn new_leaf_with_context(&mut self, style: Style, context: C) -> NodeId {
        NodeId::from(self.nodes.insert(LayoutNode::new(style, Some(context))))
    }

    /// A new detached node holding `children` (detached nodes).
    pub(crate) fn new_with_children(&mut self, style: Style, children: &[NodeId]) -> NodeId {
        let id = NodeId::from(self.nodes.insert(LayoutNode::new(style, None)));
        for &child in children {
            self.nodes[key(child)].parent = Some(id);
        }
        self.nodes[key(id)].children = children.to_vec();
        id
    }

    /// Records that `node` lays out the core's node `tag`.
    pub(crate) fn set_tag(&mut self, node: NodeId, tag: u32) {
        self.nodes[key(node)].tag = Some(tag);
    }

    /// Frees `node`, detaching it from its parent; its children are left
    /// without one.
    pub(crate) fn remove(&mut self, node: NodeId) {
        if let Some(parent) = self.nodes[key(node)].parent {
            self.nodes[key(parent)].children.retain(|&c| c != node);
            self.mark_dirty(parent);
        }
        if self.nodes[key(node)].queued {
            self.dirty_boundaries.retain(|&b| b != node);
        }
        let removed = self.nodes.remove(key(node));
        for child in removed.children {
            self.nodes[key(child)].parent = None;
        }
    }

    pub(crate) fn style(&self, node: NodeId) -> &Style {
        &self.nodes[key(node)].style
    }

    /// Replaces `node`'s style: its own size may change, so its parent is
    /// laid out again (whether it is a boundary or not).
    pub(crate) fn set_style(&mut self, node: NodeId, style: Style) {
        let n = &mut self.nodes[key(node)];
        n.style = style;
        n.clear_cache();
        if let Some(parent) = n.parent {
            self.mark_dirty(parent);
        }
    }

    /// Replaces what measures `node`.
    pub(crate) fn set_node_context(&mut self, node: NodeId, context: Option<C>) {
        self.nodes[key(node)].context = context;
        self.mark_dirty(node);
    }

    pub(crate) fn get_node_context_mut(&mut self, node: NodeId) -> Option<&mut C> {
        self.nodes[key(node)].context.as_mut()
    }

    pub(crate) fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.nodes[key(node)].parent
    }

    #[cfg(test)]
    pub(crate) fn children(&self, node: NodeId) -> &[NodeId] {
        &self.nodes[key(node)].children
    }

    /// Appends `child` (detached) to `parent`'s children.
    pub(crate) fn add_child(&mut self, parent: NodeId, child: NodeId) {
        let index = self.nodes[key(parent)].children.len();
        self.insert_child_at_index(parent, index, child);
    }

    /// Inserts `child` (detached) into `parent` at `index`.
    pub(crate) fn insert_child_at_index(&mut self, parent: NodeId, index: usize, child: NodeId) {
        self.nodes[key(child)].parent = Some(parent);
        self.nodes[key(parent)].children.insert(index, child);
        self.mark_dirty(parent);
    }

    /// Detaches `child` from `parent`.
    pub(crate) fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        self.nodes[key(parent)].children.retain(|&c| c != child);
        self.nodes[key(child)].parent = None;
        self.mark_dirty(parent);
    }

    /// Something in `node` changed (its children, what measures it):
    /// clears the cached layouts of `node` and of every node above it up to
    /// the root (never stopping at a node whose cache is already empty: see
    /// the module's docs), or up to the first boundary on the way, `node`
    /// included, which is queued to be laid out again on its own.
    pub(crate) fn mark_dirty(&mut self, node: NodeId) {
        let mut at = Some(node);
        while let Some(node) = at {
            self.nodes[key(node)].clear_cache();
            if self.is_boundary(node) {
                let n = &mut self.nodes[key(node)];
                if !n.queued {
                    n.queued = true;
                    self.dirty_boundaries.push(node);
                }
                return;
            }
            at = self.nodes[key(node)].parent;
        }
    }

    /// Whether what changes inside `node` cannot change what its parent's
    /// layout takes from it, so that layout stands: it has a size of fixed
    /// points both ways in a flex container, and everything the flex
    /// algorithm takes from an item follows from that size alone. Its flex
    /// basis is `auto`, its main size (a percentage basis against a parent
    /// of no definite size, or another length under a content-based
    /// automatic minimum, would follow its content); it does not shrink (its
    /// automatic minimum, its content's, is never above its size, so it
    /// only matters to shrinking); growing takes the free space its parent
    /// and siblings leave; it is not aligned by its baseline; its intrinsic
    /// contributions are its size. And its parent laid it out before. What
    /// it gives back that the size does not fix (overflow, margins) is
    /// compared after ([`LayoutTree::relayout_boundaries`]).
    fn is_boundary(&self, node: NodeId) -> bool {
        let n = &self.nodes[key(node)];
        let Some(parent) = n.parent else {
            return false;
        };
        let fixed = |d: taffy::Dimension| d.into_raw().tag() == taffy::CompactLength::LENGTH_TAG;
        let (style, parent) = (&n.style, &self.nodes[key(parent)].style);
        let baseline = |a: taffy::AlignItems| a.keyword() == taffy::AlignItemsKeyword::Baseline;
        n.last.is_some()
            && parent.display == Display::Flex
            && style.display != Display::None
            && fixed(style.size.width)
            && fixed(style.size.height)
            && style.flex_shrink == 0.0
            && style.flex_basis == taffy::Dimension::auto()
            && !baseline(parent.align_items)
            && !style.align_self.is_some_and(baseline)
    }

    /// What measures the leaf `node` changed (its text, say): whether it
    /// measures with `measure` as it did for every input the layout asked of
    /// it since its cache was last cleared, so that every layout above it
    /// stands and nothing needs laying out again. When it does not, or it
    /// was asked too much to tell, the caller marks it dirty
    /// ([`LayoutTree::mark_dirty`]). A leaf never asked (new, or dirty
    /// already) measures as before: nothing was computed from it.
    pub(crate) fn measures_as_before<M>(&mut self, node: NodeId, mut measure: M) -> bool
    where
        M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
    {
        let n = &mut self.nodes[key(node)];
        let Some(measured) = &n.measured else {
            return false;
        };
        measured
            .iter()
            .all(|(input, output)| measure(*input, node, n.context.as_mut(), &n.style) == *output)
    }

    /// Whether `node` has no cached layout: it changed, or something in it
    /// did, since it was last laid out.
    pub(crate) fn dirty(&self, node: NodeId) -> bool {
        self.nodes[key(node)].cache.is_empty()
    }

    /// `node`'s layout as the algorithms computed it, relative to its
    /// parent, unrounded.
    pub(crate) fn unrounded_layout(&self, node: NodeId) -> &Layout {
        &self.nodes[key(node)].unrounded
    }

    /// Lays out the boundaries a change inside made dirty, each with what
    /// its parent gave it last time; one that gives back what it gave
    /// before (its size, overflow, baselines, margins) leaves its parent's
    /// layout standing, another dirties its parent, as a change of its own
    /// would. Returns the tags of those laid out on their own: their
    /// subtrees' frames are new, nothing else's is.
    pub(crate) fn relayout_boundaries<M>(&mut self, measure: M) -> Vec<u32>
    where
        M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
    {
        let mut relaid = Vec::new();
        if self.dirty_boundaries.is_empty() {
            return relaid;
        }
        let mut view = View {
            tree: self,
            measure,
        };
        while let Some(node) = view.tree.dirty_boundaries.pop() {
            let n = &mut view.tree.nodes[key(node)];
            n.queued = false;
            let parent = n.parent;
            // Moved or restyled since: no longer one on its own.
            if !view.tree.is_boundary(node) {
                if let Some(parent) = parent {
                    view.tree.mark_dirty(parent);
                }
                continue;
            }
            let (input, before) = view.tree.nodes[key(node)].last.clone().expect("laid out");
            let after = view.compute_child_layout(node, input, None);
            if after == before {
                relaid.extend(view.tree.nodes[key(node)].tag);
            } else if let Some(parent) = parent {
                view.tree.mark_dirty(parent);
            }
        }
        relaid
    }

    /// Lays out the tree under `root` within `available`, measuring leaves
    /// with `measure` (`taffy::TaffyTree::compute_layout_with_measure`'s).
    pub(crate) fn compute_layout_with_measure<M>(
        &mut self,
        root: NodeId,
        available: Size<AvailableSpace>,
        measure: M,
    ) where
        M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
    {
        let mut view = View {
            tree: self,
            measure,
        };
        compute_root_layout(&mut view, root, available);
    }
}

/// The tree being laid out, with what measures its leaves: what Taffy's
/// algorithms run over.
struct View<'t, C, M> {
    tree: &'t mut LayoutTree<C>,
    measure: M,
}

impl<C, M> View<'_, C, M>
where
    M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
{
    fn node(&self, node: NodeId) -> &LayoutNode<C> {
        &self.tree.nodes[key(node)]
    }

    /// What both `compute_child_layout` and `compute_block_child_layout`
    /// run (`TaffyView::compute_child_layout`).
    fn compute_child_layout(
        &mut self,
        node: NodeId,
        inputs: LayoutInput,
        block: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        // An ancestor is `display: none`: hidden whatever its own display.
        if inputs.run_mode == RunMode::PerformHiddenLayout {
            self.tree.nodes[key(node)].last = None;
            return compute_hidden_layout(self, node);
        }
        let output = compute_cached_layout(self, node, inputs, |view, node, inputs| {
            let display = view.node(node).style.display;
            let has_children = !view.node(node).children.is_empty();
            let mut output = match (display, has_children) {
                (Display::None, _) => compute_hidden_layout(view, node),
                (Display::Block, true) => compute_block_layout(view, node, inputs, block),
                (Display::FlowRoot, true) => compute_block_layout(view, node, inputs, None),
                (Display::Flex, true) => compute_flexbox_layout(view, node, inputs),
                (Display::Grid, true) => compute_grid_layout(view, node, inputs),
                (_, false) => {
                    let n = &mut view.tree.nodes[key(node)];
                    let output = (view.measure)(inputs, node, n.context.as_mut(), &n.style);
                    if let Some(measured) = &mut n.measured
                        && !measured.iter().any(|(asked, _)| *asked == inputs)
                    {
                        if measured.len() < MEASURED {
                            measured.push((inputs, output.clone()));
                        } else {
                            n.measured = None;
                        }
                    }
                    output
                }
            };
            // The out-of-flow nodes this one is the containing block of, in
            // full layout passes only, as Taffy's tree does.
            if inputs.run_mode == RunMode::PerformLayout {
                compute_oof_layout(view, node, &mut output);
            }
            output
        });
        if inputs.run_mode == RunMode::PerformLayout {
            self.tree.nodes[key(node)].last = Some((inputs, output.clone()));
        }
        output
    }
}

impl<C, M> TraversePartialTree for View<'_, C, M>
where
    M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
{
    type ChildIter<'a>
        = std::iter::Copied<std::slice::Iter<'a, NodeId>>
    where
        Self: 'a;

    fn child_ids(&self, parent: NodeId) -> Self::ChildIter<'_> {
        self.node(parent).children.iter().copied()
    }

    fn child_count(&self, parent: NodeId) -> usize {
        self.node(parent).children.len()
    }

    fn get_child_id(&self, parent: NodeId, index: usize) -> NodeId {
        self.node(parent).children[index]
    }
}

impl<C, M> TraverseTree for View<'_, C, M> where
    M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput
{
}

impl<C, M> LayoutPartialTree for View<'_, C, M>
where
    M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
{
    type CoreContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;

    // `Style`'s default, Taffy's `DefaultCheapStr` with `std`.
    type CustomIdent = String;

    fn get_core_container_style(&self, node: NodeId) -> Self::CoreContainerStyle<'_> {
        &self.node(node).style
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        self.tree.nodes[key(node)].unrounded = *layout;
    }

    fn resolve_calc_value(&self, _: *const (), _: f32) -> f32 {
        0.0
    }

    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
        View::compute_child_layout(self, node, inputs, None)
    }
}

impl<C, M> LayoutContainingBlock for View<'_, C, M>
where
    M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
{
    type OofItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_oof_item_style(&self, node: NodeId) -> Self::OofItemStyle<'_> {
        &self.node(node).style
    }

    // Kept for Taffy's rounding pass alone, which the core does not run.
    fn clear_hoisted_children(&mut self, _: NodeId) {}

    fn add_hoisted_children(&mut self, _: NodeId, _: &[NodeId]) {}

    fn get_detailed_layout_info(&self, node: NodeId) -> &DetailedLayoutInfo {
        &self.node(node).detailed
    }
}

impl<C, M> CacheTree for View<'_, C, M>
where
    M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
{
    fn cache_get(&mut self, node: NodeId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.tree.nodes[key(node)].cache.get(input)
    }

    fn cache_store(&mut self, node: NodeId, input: &LayoutInput, output: LayoutOutput) {
        self.tree.nodes[key(node)].cache.store(input, output);
    }

    fn cache_clear(&mut self, node: NodeId) {
        self.tree.nodes[key(node)].clear_cache();
    }
}

impl<C, M> LayoutBlockContainer for View<'_, C, M>
where
    M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
{
    type BlockContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type BlockItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_block_container_style(&self, node: NodeId) -> Self::BlockContainerStyle<'_> {
        &self.node(node).style
    }

    fn get_block_child_style(&self, child: NodeId) -> Self::BlockItemStyle<'_> {
        &self.node(child).style
    }

    fn compute_block_child_layout(
        &mut self,
        node: NodeId,
        inputs: LayoutInput,
        block: Option<&mut BlockContext<'_>>,
    ) -> LayoutOutput {
        View::compute_child_layout(self, node, inputs, block)
    }
}

impl<C, M> LayoutFlexboxContainer for View<'_, C, M>
where
    M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
{
    type FlexboxContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type FlexboxItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node: NodeId) -> Self::FlexboxContainerStyle<'_> {
        &self.node(node).style
    }

    fn get_flexbox_child_style(&self, child: NodeId) -> Self::FlexboxItemStyle<'_> {
        &self.node(child).style
    }
}

impl<C, M> LayoutGridContainer for View<'_, C, M>
where
    M: FnMut(LayoutInput, NodeId, Option<&mut C>, &Style) -> LayoutOutput,
{
    type GridContainerStyle<'a>
        = &'a Style
    where
        Self: 'a;
    type GridItemStyle<'a>
        = &'a Style
    where
        Self: 'a;

    fn get_grid_container_style(&self, node: NodeId) -> Self::GridContainerStyle<'_> {
        &self.node(node).style
    }

    fn get_grid_child_style(&self, child: NodeId) -> Self::GridItemStyle<'_> {
        &self.node(child).style
    }

    fn set_detailed_grid_info(&mut self, node: NodeId, info: DetailedGridInfo) {
        self.tree.nodes[key(node)].detailed = DetailedLayoutInfo::Grid(Box::new(info));
    }
}

#[cfg(test)]
mod tests {
    use taffy::{
        AvailableSpace, Dimension, Display, FlexDirection, LayoutOutput, Position, Size, Style,
        style_helpers::{auto, length},
    };

    use super::LayoutTree;
    use crate::style::{self, Style as NStyle, StyleValue};

    fn definite(width: f32, height: f32) -> Size<AvailableSpace> {
        Size {
            width: AvailableSpace::Definite(width),
            height: AvailableSpace::Definite(height),
        }
    }

    /// What the shadow tree's leaves measure with: style sizes win, content
    /// is zero unless a test passes its own function.
    fn measure_leaf(
        input: taffy::LayoutInput,
        _: taffy::NodeId,
        _: Option<&mut ()>,
        style: &Style,
    ) -> LayoutOutput {
        taffy::compute_leaf_layout(input, style, |_, _| 0.0, |_, _| Size::ZERO)
    }

    /// nsh/RN defaults, then the fields a test sets, then the alignment
    /// resolution the shadow tree applies before layout.
    fn rn(extra: impl FnOnce(&mut Style)) -> Style {
        let mut style = NStyle::default();
        extra(&mut style.layout);
        style::resolve_normal_alignment(&mut style.layout);
        style.layout
    }

    fn fixed(width: f32, height: f32) -> Style {
        rn(|style| {
            style.size = Size {
                width: Dimension::length(width),
                height: Dimension::length(height),
            };
        })
    }

    /// A grid container from the declarations an app writes.
    fn grid(columns: &str, rows: &str, flow: Option<&str>) -> Style {
        let mut style = NStyle::default();
        style
            .set("display", StyleValue::Str("grid"))
            .expect("display");
        style
            .set("gridTemplateColumns", StyleValue::Str(columns))
            .expect("columns");
        style
            .set("gridTemplateRows", StyleValue::Str(rows))
            .expect("rows");
        if let Some(flow) = flow {
            style
                .set("gridAutoFlow", StyleValue::Str(flow))
                .expect("auto flow");
        }
        style::resolve_normal_alignment(&mut style.layout);
        style.layout
    }

    /// A cell placed by the declarations an app writes.
    fn cell(column: &str, row: &str) -> Style {
        let mut style = NStyle::default();
        style
            .set("gridColumn", StyleValue::Str(column))
            .expect("column");
        style.set("gridRow", StyleValue::Str(row)).expect("row");
        style::resolve_normal_alignment(&mut style.layout);
        style.layout
    }

    fn measure_from_context(
        _: taffy::LayoutInput,
        _: taffy::NodeId,
        context: Option<&mut f32>,
        _: &Style,
    ) -> LayoutOutput {
        let side = *context.expect("leaf context");
        LayoutOutput::from_outer_size(Size {
            width: side,
            height: side,
        })
    }

    #[test]
    fn column_flex_stacks_fixed_children() {
        let mut tree = LayoutTree::<()>::with_capacity(4);
        let first = tree.new_leaf(fixed(100.0, 20.0));
        let second = tree.new_leaf(fixed(100.0, 30.0));
        let root = tree.new_with_children(fixed(100.0, 200.0), &[first, second]);
        tree.compute_layout_with_measure(root, definite(100.0, 200.0), measure_leaf);

        assert_eq!(tree.style(root).flex_direction, FlexDirection::Column);
        assert_eq!(tree.style(root).flex_shrink, 0.0);
        let first = tree.unrounded_layout(first);
        let second = tree.unrounded_layout(second);
        assert_eq!(first.location.y, 0.0);
        assert_eq!(first.size.height, 20.0);
        assert_eq!(second.location.y, 20.0);
        assert_eq!(second.size.height, 30.0);
        assert_eq!(tree.unrounded_layout(root).size.height, 200.0);
    }

    #[test]
    fn grid_places_cells_in_document_order() {
        let mut tree = LayoutTree::<()>::with_capacity(4);
        let first = tree.new_leaf(fixed(10.0, 10.0));
        let second = tree.new_leaf(fixed(10.0, 10.0));
        let root = tree.new_with_children(
            rn(|style| {
                style.display = Display::Grid;
                style.size = Size {
                    width: Dimension::length(100.0),
                    height: Dimension::length(80.0),
                };
                style.grid_template_columns = vec![length(40.0), length(60.0)];
                style.grid_template_rows = vec![length(30.0), length(50.0)];
            }),
            &[first, second],
        );
        tree.compute_layout_with_measure(root, definite(100.0, 80.0), measure_leaf);

        let first = tree.unrounded_layout(first);
        let second = tree.unrounded_layout(second);
        assert_eq!((first.location.x, first.location.y), (0.0, 0.0));
        assert_eq!((second.location.x, second.location.y), (40.0, 0.0));
        assert_eq!((first.size.width, first.size.height), (10.0, 10.0));
    }

    #[test]
    fn named_grid_lines_place_cells() {
        let mut tree = LayoutTree::<()>::with_capacity(4);
        let first = tree.new_leaf(cell("mid / full-end", "top / bottom"));
        let second = tree.new_leaf(cell("full-start / mid", "top / bottom"));
        let root = tree.new_with_children(
            grid(
                "[full-start] 40px [mid] 60px [full-end]",
                "[top] 30px [bottom]",
                None,
            ),
            &[first, second],
        );
        tree.compute_layout_with_measure(root, definite(100.0, 30.0), measure_leaf);

        let first = tree.unrounded_layout(first);
        let second = tree.unrounded_layout(second);
        assert_eq!((first.location.x, first.location.y), (40.0, 0.0));
        assert_eq!((first.size.width, first.size.height), (60.0, 30.0));
        assert_eq!((second.location.x, second.location.y), (0.0, 0.0));
        assert_eq!((second.size.width, second.size.height), (40.0, 30.0));
    }

    fn auto_cells(flow: Option<&str>) -> Vec<(f32, f32)> {
        let mut tree = LayoutTree::<()>::with_capacity(3);
        let children = [
            tree.new_leaf(fixed(10.0, 10.0)),
            tree.new_leaf(fixed(10.0, 10.0)),
            tree.new_leaf(fixed(10.0, 10.0)),
        ];
        let root = tree.new_with_children(grid("40px 60px", "30px 20px", flow), &children);
        tree.compute_layout_with_measure(root, definite(100.0, 50.0), measure_leaf);
        children
            .iter()
            .map(|&child| {
                let layout = tree.unrounded_layout(child);
                (layout.location.x, layout.location.y)
            })
            .collect()
    }

    #[test]
    fn grid_auto_flow_orders_cells() {
        assert_eq!(
            auto_cells(None),
            [(0.0, 0.0), (40.0, 0.0), (0.0, 30.0)],
            "row flow fills each row before the next"
        );
        assert_eq!(
            auto_cells(Some("column")),
            [(0.0, 0.0), (0.0, 30.0), (40.0, 0.0)],
            "column flow fills each column before the next"
        );
    }

    #[test]
    fn dirty_reaches_through_an_empty_cache() {
        let mut tree = LayoutTree::<()>::with_capacity(4);
        let leaf = tree.new_leaf(fixed(10.0, 10.0));
        let hidden = tree.new_with_children(rn(|style| style.display = Display::None), &[leaf]);
        let root = tree.new_with_children(fixed(100.0, 100.0), &[hidden]);
        tree.compute_layout_with_measure(root, definite(100.0, 100.0), measure_leaf);

        assert!(!tree.dirty(root), "a laid-out root keeps its cache");
        assert!(
            !tree.dirty(hidden),
            "the hidden node itself is cached by its visible parent"
        );
        assert!(
            tree.dirty(leaf),
            "a node laid out under display:none keeps an empty cache"
        );
        tree.mark_dirty(leaf);
        assert!(
            tree.dirty(root),
            "dirty marking must not stop at the empty cache"
        );
    }

    #[test]
    fn an_absolute_child_is_placed_in_its_containing_block() {
        let mut tree = LayoutTree::<()>::with_capacity(4);
        let abs = tree.new_leaf(rn(|style| {
            style.position = Position::Absolute;
            style.inset = taffy::Rect {
                top: length(4.0),
                left: length(6.0),
                right: auto(),
                bottom: auto(),
            };
            style.size = Size {
                width: Dimension::length(10.0),
                height: Dimension::length(12.0),
            };
        }));
        let root = tree.new_with_children(fixed(100.0, 100.0), &[abs]);
        tree.compute_layout_with_measure(root, definite(100.0, 100.0), measure_leaf);

        let laid = tree.unrounded_layout(abs);
        assert_eq!((laid.location.x, laid.location.y), (6.0, 4.0));
        assert_eq!((laid.size.width, laid.size.height), (10.0, 12.0));
    }

    #[test]
    fn a_fixed_flex_item_relayouts_without_dirtying_its_parent() {
        let mut tree = LayoutTree::<()>::with_capacity(4);
        let inner = tree.new_leaf(rn(|style| {
            style.size.width = Dimension::auto();
            style.size.height = Dimension::length(10.0);
        }));
        let boundary = tree.new_with_children(fixed(80.0, 80.0), &[inner]);
        tree.set_tag(boundary, 7);
        let root = tree.new_with_children(fixed(200.0, 200.0), &[boundary]);
        tree.compute_layout_with_measure(root, definite(200.0, 200.0), measure_leaf);

        tree.mark_dirty(inner);
        assert!(
            !tree.dirty(root),
            "a fixed flex item absorbs a change that cannot change its size"
        );
        assert_eq!(tree.relayout_boundaries(measure_leaf), vec![7]);
        assert!(!tree.dirty(root));
    }

    #[test]
    fn a_boundary_whose_overflow_changes_dirties_its_parent() {
        let mut tree = LayoutTree::<()>::with_capacity(4);
        let inner = tree.new_leaf(fixed(10.0, 10.0));
        let boundary = tree.new_with_children(fixed(80.0, 80.0), &[inner]);
        tree.set_tag(boundary, 7);
        let root = tree.new_with_children(fixed(200.0, 200.0), &[boundary]);
        tree.compute_layout_with_measure(root, definite(200.0, 200.0), measure_leaf);

        tree.set_style(inner, fixed(200.0, 200.0));
        assert!(!tree.dirty(root));
        assert!(tree.relayout_boundaries(measure_leaf).is_empty());
        assert!(
            tree.dirty(root),
            "overflow the fixed size does not cover invalidates the parent"
        );
    }

    #[test]
    fn a_leaf_that_measures_the_same_needs_no_relayout() {
        let mut tree = LayoutTree::with_capacity(1);
        let leaf = tree.new_leaf_with_context(Style::DEFAULT, 10.0);
        tree.compute_layout_with_measure(leaf, definite(100.0, 100.0), measure_from_context);

        assert!(tree.measures_as_before(leaf, measure_from_context));
        *tree.get_node_context_mut(leaf).unwrap() = 20.0;
        assert!(!tree.measures_as_before(leaf, measure_from_context));
    }
}
