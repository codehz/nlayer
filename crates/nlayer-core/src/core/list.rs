//! Virtualized list bookkeeping: item sizes, offsets, the render window and
//! cell recycling. Pure state: the core feeds measurements, scroll offsets
//! and commands and applies the decisions to the tree.
//!
//! Cells are recycled by slot. The core asks the runtime to bind rows with
//! `(slot, item)` pairs; the runtime renders the row into the slot's cell
//! and sets its `cellIndex`, and only confirmed cells are measured and
//! placed. Each cell is its own Taffy root, so scrolling relays out only
//! the cells that changed.
//!
//! Extracted from `nsh-core/src/core/list.rs`, cut to v2: vertical lists,
//! `numColumns`, header / footer / empty, measurement anchoring, load-more,
//! and scroll commands. Parent-viewport lists, inverted lists, sticky
//! headers, viewability and keep-rendered selections are not part of v2.

use rustc_hash::FxHashMap;

use super::fenwick::Fenwick;
use crate::ops::CellSlot;

/// How far beyond the viewport cells are kept rendered, in viewports.
const OVERSCAN: f32 = 1.0;
/// Default `loadMoreThreshold`, in viewports.
const END_THRESHOLD: f32 = 1.0;
/// Distances smaller than this are rounding, not a scroll.
pub(super) const PIN_SLACK: f32 = 0.5;
/// Where a cell with no item is moved: out of sight, never with a NaN
/// frame.
const HIDDEN: f32 = -1_000_000.0;

/// A recycling slot: one cell node, bound to at most one item.
#[derive(Debug, Clone, Copy)]
pub(super) struct Slot {
    /// Cell node key (`u32::MAX` until the runtime created the cell).
    pub(super) cell: u32,
    /// Item this slot is assigned to.
    pub(super) index: Option<u32>,
    /// Item the cell's content currently shows (confirmed by the runtime).
    pub(super) rendered: Option<u32>,
    /// Item the runtime was last asked to show in this slot.
    pub(super) requested: Option<u32>,
}

impl Slot {
    /// True when the cell shows its assigned item.
    pub(super) fn ready(&self) -> Option<u32> {
        match (self.index, self.rendered) {
            (Some(i), Some(r)) if i == r && self.cell != u32::MAX => Some(i),
            _ => None,
        }
    }
}

/// A non-recycled cell: the header, footer or empty placeholder.
#[derive(Debug, Clone, Copy)]
pub(super) struct Extra {
    /// Cell node key, or `u32::MAX`.
    pub(super) cell: u32,
    /// Measured main-axis size (0 until measured).
    pub(super) size: f32,
}

impl Extra {
    const NONE: Self = Self {
        cell: u32::MAX,
        size: 0.0,
    };

    fn present(&self) -> bool {
        self.cell != u32::MAX
    }
}

/// Which extra cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExtraKind {
    Header = 0,
    Footer = 1,
    Empty = 2,
}

/// Where the blocks of a list's content start, in natural coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Blocks {
    pub(super) header: f32,
    pub(super) body: f32,
    pub(super) footer: f32,
    pub(super) total: f32,
}

/// A scroll the core asked the platform for.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PendingScroll {
    By(f32),
    To(f32),
}

fn replay(offset: f32, pending: &[PendingScroll]) -> f32 {
    let mut at = offset;
    for scroll in pending {
        match scroll {
            PendingScroll::By(d) => at += d,
            PendingScroll::To(t) => at = *t,
        }
    }
    at
}

/// What a scroll command aims at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum PinTarget {
    Index(u32),
    End,
}

/// A scroll command's target, until the viewport's rows are measured or the
/// user scrolls away.
#[derive(Debug, Clone, Copy)]
struct Pin {
    target: PinTarget,
    at: f32,
}

#[derive(Debug)]
pub(super) struct ListState {
    pub(super) count: u32,
    estimated: f32,
    columns: u32,
    /// Measured main-axis sizes of items; NaN = not measured yet.
    sizes: Vec<f32>,
    rows: Fenwick,
    pub(super) slots: Vec<Slot>,
    by_index: FxHashMap<u32, u32>,
    free: Vec<u32>,
    pub(super) extras: [Extra; 3],
    pub(super) end_threshold: f32,
    /// Main-axis scroll offset reported by the platform (or jumped to).
    pub(super) offset: f32,
    /// Main-axis size of the viewport (from layout).
    pub(super) viewport: f32,
    /// Window or positions need recomputing.
    pub(super) dirty: bool,
    /// Cross-axis size the cells were last laid out at.
    pub(super) cells_cross: f32,
    load_more_at: Option<u32>,
    /// The items changed since cells without one were last hidden.
    pub(super) items_changed: bool,
    /// Cells whose item moved to another index (a splice): the runtime
    /// learns their new index with the next bindings.
    moved: Vec<(u32, u32)>,
    /// Binds of overscan cells wait for the engine's budget
    /// ([`Self::take_paced`]); the rest bind with the window.
    pub(super) paced: bool,
    /// The last scroll step the platform reported: its sign is the scroll
    /// direction, and its size bounds what binds ahead of the viewport.
    lead: f32,
    /// Scroll that keeps the visible content in place after a splice.
    splice_anchor: f32,
    pin: Option<Pin>,
    /// Scrolls issued to the platform, until a report shows them applied.
    pending: Vec<PendingScroll>,
    /// The offset before the first pending scroll.
    before_pending: f32,
}

impl ListState {
    pub(super) fn new() -> Self {
        Self {
            count: 0,
            estimated: 50.0,
            columns: 1,
            sizes: Vec::new(),
            rows: Fenwick::default(),
            slots: Vec::new(),
            by_index: FxHashMap::default(),
            free: Vec::new(),
            extras: [Extra::NONE; 3],
            end_threshold: END_THRESHOLD,
            offset: 0.0,
            viewport: 0.0,
            dirty: true,
            cells_cross: f32::NAN,
            load_more_at: None,
            items_changed: false,
            moved: Vec::new(),
            paced: false,
            lead: 0.0,
            splice_anchor: 0.0,
            pin: None,
            pending: Vec::new(),
            before_pending: 0.0,
        }
    }

    /// The platform reported offset `offset` (main axis). A report from
    /// before a scroll the core issued is replayed onto it, so the window
    /// does not move backwards; a report at (or past) the issued target
    /// settles it.
    pub(super) fn reported(&mut self, offset: f32) {
        if self.pending.is_empty() {
            self.moved_to(offset);
            return;
        }
        let at_target = self
            .pending
            .iter()
            .any(|p| (offset - target(*p)).abs() <= PIN_SLACK);
        if at_target {
            self.pending.clear();
            self.moved_to(offset);
        } else if (offset - self.before_pending).abs() <= PIN_SLACK {
            self.moved_to(replay(offset, &self.pending));
        } else {
            self.pending.clear();
            self.moved_to(offset);
        }
    }

    fn issue(&mut self, scroll: PendingScroll) {
        if self.pending.is_empty() {
            self.before_pending = self.offset;
        }
        self.pending.push(scroll);
    }

    /// Records a `ScrollBy` sent to the platform.
    pub(super) fn issue_scroll_by(&mut self, delta: f32) {
        self.issue(PendingScroll::By(delta));
    }

    /// Records a `ScrollTo` sent to the platform.
    pub(super) fn issue_scroll_to(&mut self, offset: f32) {
        self.issue(PendingScroll::To(offset));
    }

    fn moved_to(&mut self, at: f32) {
        if at == self.offset {
            return;
        }
        let step = at - self.offset;
        if step.abs() > PIN_SLACK {
            self.lead = step;
        }
        self.offset = at;
        self.dirty = true;
        if let Some(pin) = self.pin
            && self.pending.is_empty()
            && (at - pin.at).abs() > PIN_SLACK
        {
            self.pin = None;
        }
    }

    /// The items changed: `removed` items at `at` were replaced by
    /// `inserted` new ones. Measurements and cells follow the items that
    /// stay; changes above the viewport keep the visible rows in place.
    pub(super) fn splice(&mut self, at: u32, removed: u32, inserted: u32) {
        let at = at.min(self.count);
        let removed = removed.min(self.count - at);
        if removed == 0 && inserted == 0 {
            return;
        }
        self.items_changed = true;
        self.pin = None;
        let anchor = (self.count > 0 && self.viewport > 0.0).then(|| {
            let row = self
                .rows
                .find((self.offset - self.blocks().body).max(0.0) as f64)
                as u32;
            let index = (row * self.columns).min(self.count - 1);
            (index, self.item_at(index) - self.offset)
        });
        let end = (at + removed) as usize;
        self.sizes.splice(
            at as usize..end,
            std::iter::repeat_n(f32::NAN, inserted as usize),
        );
        self.count = self.count - removed + inserted;
        let shift = |i: u32| -> Option<u32> {
            if i < at {
                Some(i)
            } else if i >= at + removed {
                Some(i - removed + inserted)
            } else {
                None
            }
        };
        self.by_index.clear();
        for (slot, s) in self.slots.iter_mut().enumerate() {
            let index = s.index.and_then(shift);
            let Some(new) = index else {
                if s.index.take().is_some() {
                    self.free.push(slot as u32);
                }
                s.rendered = None;
                s.requested = None;
                continue;
            };
            s.index = Some(new);
            s.rendered = s.rendered.and_then(shift);
            // The runtime's index for the cell follows its item at once, or
            // its content shows the item now at the old index.
            match s.requested.map(|r| (r, shift(r))) {
                Some((r, Some(n))) => {
                    s.requested = Some(n);
                    if n != r {
                        self.moved.push((slot as u32, n));
                    }
                }
                Some((_, None)) => {
                    s.requested = None;
                    s.rendered = None;
                }
                None => {}
            }
            self.by_index.insert(new, slot as u32);
        }
        self.free.sort_unstable();
        self.free.dedup();
        self.rebuild_offsets();
        if self.load_more_at.is_some() {
            self.load_more_at = None;
        }
        if let Some((index, from_top)) = anchor
            && index >= at + removed
        {
            let moved_to = index - removed + inserted;
            let delta = self.item_at(moved_to) - from_top - self.offset;
            self.offset += delta;
            self.splice_anchor += delta;
        }
    }

    /// The scroll a splice asked for, once.
    pub(super) fn take_splice_anchor(&mut self) -> f32 {
        std::mem::take(&mut self.splice_anchor)
    }

    fn effective(&self, i: usize) -> f32 {
        let s = self.sizes[i];
        if s.is_nan() { self.estimated } else { s }
    }

    fn row_count(&self) -> u32 {
        self.count.div_ceil(self.columns)
    }

    fn row_of(&self, i: u32) -> u32 {
        i / self.columns
    }

    /// Size of row `r`: its largest item.
    fn row_size(&self, r: u32) -> f32 {
        let start = (r * self.columns) as usize;
        let end = (start + self.columns as usize).min(self.sizes.len());
        (start..end).map(|i| self.effective(i)).fold(0.0, f32::max)
    }

    fn rebuild_offsets(&mut self) {
        let rows = (0..self.row_count()).map(|r| self.row_size(r) as f64);
        self.rows = Fenwick::from_sizes(rows.collect::<Vec<_>>().into_iter());
        self.dirty = true;
    }

    fn items_len(&self) -> f32 {
        self.rows.total() as f32
    }

    /// Start of each block of content, in natural coordinates.
    pub(super) fn blocks(&self) -> Blocks {
        let [header, footer, empty] = &self.extras;
        let mut at = 0.0;
        let mut any = false;
        let mut place = |present: bool, size: f32| {
            if !present {
                return 0.0;
            }
            if any {
                at += 0.0;
            }
            any = true;
            let here = at;
            at += size;
            here
        };
        let header_at = place(header.present(), header.size);
        let body_at = if self.count > 0 {
            place(true, self.items_len())
        } else {
            place(empty.present(), empty.size)
        };
        let footer_at = place(footer.present(), footer.size);
        Blocks {
            header: header_at,
            body: body_at,
            footer: footer_at,
            total: at,
        }
    }

    /// Total content length along the main axis.
    pub(super) fn content_len(&self) -> f32 {
        self.blocks().total
    }

    /// Natural position of item `i` (its row's start).
    pub(super) fn item_at(&self, i: u32) -> f32 {
        self.blocks().body + self.rows.prefix(self.row_of(i) as usize) as f32
    }

    /// Size and position of each cell across the main axis.
    pub(super) fn column(&self, cross: f32, col: u32) -> (f32, f32) {
        let n = self.columns as f32;
        let width = (cross / n).max(0.0);
        (col as f32 * width, width)
    }

    /// The cross-axis span of extras: the full content width.
    pub(super) fn full_width(&self, cross: f32) -> (f32, f32) {
        (0.0, cross.max(0.0))
    }

    pub(super) fn column_of(&self, i: u32) -> u32 {
        i % self.columns
    }

    pub(super) fn set_count(&mut self, count: u32) {
        if count == self.count {
            return;
        }
        self.count = count;
        self.items_changed = true;
        self.sizes.resize(count as usize, f32::NAN);
        self.rebuild_offsets();
        if self.load_more_at.is_some_and(|c| c != count) {
            self.load_more_at = None;
        }
    }

    pub(super) fn set_estimated(&mut self, estimated: f32) {
        if estimated > 0.0 && estimated != self.estimated {
            self.estimated = estimated;
            self.rebuild_offsets();
        }
    }

    pub(super) fn set_columns(&mut self, columns: u32) {
        let columns = columns.max(1);
        if columns != self.columns {
            self.columns = columns;
            self.sizes.fill(f32::NAN);
            self.cells_cross = f32::NAN;
            self.rebuild_offsets();
        }
    }

    pub(super) fn set_end_threshold(&mut self, threshold: f32) {
        if threshold >= 0.0 && threshold != self.end_threshold {
            self.end_threshold = threshold;
            self.dirty = true;
        }
    }

    /// Records a measured size. Returns the scroll that keeps the visible
    /// content in place when the item is entirely before the viewport.
    pub(super) fn measured(&mut self, i: u32, size: f32) -> f32 {
        let idx = i as usize;
        if idx >= self.sizes.len() || self.sizes[idx] == size {
            return 0.0;
        }
        let row = self.row_of(i);
        let before = self.row_size(row);
        let row_start = self.item_at(i);
        let above_viewport = row_start + before <= self.offset;
        self.sizes[idx] = size;
        let delta = self.row_size(row) - before;
        if delta != 0.0 {
            self.rows.add(row as usize, delta as f64);
        }
        self.dirty = true;
        if above_viewport { delta } else { 0.0 }
    }

    /// Records the size of an extra cell; returns the anchoring change as
    /// [`ListState::measured`] does.
    pub(super) fn measured_extra(&mut self, kind: ExtraKind, size: f32) -> f32 {
        let blocks = self.blocks();
        let extra = &mut self.extras[kind as usize];
        if extra.size == size {
            return 0.0;
        }
        let delta = size - extra.size;
        extra.size = size;
        self.dirty = true;
        let at = match kind {
            ExtraKind::Header => blocks.header,
            ExtraKind::Footer => blocks.footer,
            ExtraKind::Empty => blocks.body,
        };
        let old_end = at + size - delta;
        if at < self.offset && old_end <= self.offset {
            delta
        } else {
            0.0
        }
    }

    pub(super) fn add_slot_cell(&mut self, slot: CellSlot, cell: u32) {
        let s = match slot {
            CellSlot::Item(s) => s as usize,
            CellSlot::Header => return self.set_extra(ExtraKind::Header, cell),
            CellSlot::Footer => return self.set_extra(ExtraKind::Footer, cell),
            CellSlot::Empty => return self.set_extra(ExtraKind::Empty, cell),
        };
        if s >= self.slots.len() {
            self.slots.resize(
                s + 1,
                Slot {
                    cell: u32::MAX,
                    index: None,
                    rendered: None,
                    requested: None,
                },
            );
        }
        self.slots[s].cell = cell;
        self.dirty = true;
    }

    fn set_extra(&mut self, kind: ExtraKind, cell: u32) {
        self.extras[kind as usize] = Extra { cell, size: 0.0 };
        self.dirty = true;
    }

    pub(super) fn remove_cell(&mut self, cell: u32) {
        for slot in &mut self.slots {
            if slot.cell == cell {
                slot.cell = u32::MAX;
                slot.rendered = None;
                slot.requested = None;
                self.dirty = true;
            }
        }
        for extra in &mut self.extras {
            if extra.cell == cell {
                *extra = Extra::NONE;
                self.dirty = true;
            }
        }
    }

    /// The runtime confirmed cell `cell` renders item `index`.
    pub(super) fn rendered(&mut self, cell: u32, index: u32) {
        if let Some(slot) = self.slots.iter_mut().find(|s| s.cell == cell) {
            slot.rendered = Some(index);
            self.dirty = true;
        }
    }

    fn items_in(&self, start: f32, end: f32) -> Option<(u32, u32)> {
        if self.count == 0 {
            return None;
        }
        let body = self.blocks().body;
        if end < body || start > body + self.items_len() {
            return None;
        }
        let first_row = self.rows.find((start - body).max(0.0) as f64) as u32;
        let last_row =
            (self.rows.find((end - body).max(0.0) as f64) as u32).min(self.row_count() - 1);
        let first = first_row * self.columns;
        let last = ((last_row + 1) * self.columns - 1).min(self.count - 1);
        Some((first, last))
    }

    /// The span of content whose items are rendered: the viewport and the
    /// overscan around it.
    pub(super) fn render_window(&self) -> (f32, f32) {
        let overscan = self.viewport * OVERSCAN;
        (
            self.offset - overscan,
            self.offset + self.viewport + overscan,
        )
    }

    /// Recomputes the window. `(slot, item)` bindings due now are pushed:
    /// cells whose item moved, and new ones in the viewport or just ahead
    /// of it (all of them unless `paced`). Returns whether `onLoadMore`
    /// should fire.
    pub(super) fn update_window(&mut self, binds: &mut Vec<(u32, u32)>) -> bool {
        self.dirty = false;
        // Cells whose item moved: the runtime learns their new index (cheap:
        // the cell shows the same item) before anything else binds.
        binds.append(&mut self.moved);
        if self.count == 0 || self.viewport <= 0.0 {
            for slot in 0..self.slots.len() as u32 {
                self.unbind(slot);
            }
            return false;
        }
        let at = self.offset;
        let overscan = self.viewport * OVERSCAN;
        let range = self.items_in(at - overscan, at + self.viewport + overscan);
        for slot in 0..self.slots.len() as u32 {
            if let Some(i) = self.slots[slot as usize].index
                && !range.is_some_and(|(f, l)| (f..=l).contains(&i))
            {
                self.unbind(slot);
            }
        }
        let bind = |this: &mut Self, i: u32| {
            if this.by_index.contains_key(&i) {
                return;
            }
            let slot = match this.free.pop() {
                Some(slot) => slot,
                None => {
                    this.slots.push(Slot {
                        cell: u32::MAX,
                        index: None,
                        rendered: None,
                        requested: None,
                    });
                    this.slots.len() as u32 - 1
                }
            };
            this.slots[slot as usize].index = Some(i);
            this.by_index.insert(i, slot);
        };
        if let Some((first, last)) = range {
            for i in first..=last {
                bind(self, i);
            }
        }
        let (start, end) = self.urgent_span(at);
        let new = binds.len();
        for slot in 0..self.slots.len() {
            let s = self.slots[slot];
            let Some(i) = s.index else { continue };
            if s.requested == Some(i) {
                continue;
            }
            if !self.paced || self.item_overlaps(i, start, end) {
                self.slots[slot].requested = Some(i);
                binds.push((slot as u32, i));
            }
        }
        binds[new..].sort_unstable_by_key(|b| b.1);
        let total = self.blocks().total;
        let near_end = at + self.viewport >= total - self.viewport * self.end_threshold;
        if near_end && self.load_more_at != Some(self.count) {
            self.load_more_at = Some(self.count);
            return true;
        }
        false
    }

    /// The span (content coordinates) whose cells bind with the window: the
    /// viewport, and ahead of it in the scroll direction twice the last
    /// step (what the next frames of a fling show). A step longer than the
    /// viewport is a jump: only the viewport.
    fn urgent_span(&self, at: f32) -> (f32, f32) {
        let step = self.lead.abs();
        let ahead = if step <= self.viewport {
            (2.0 * step).min(self.viewport * OVERSCAN)
        } else {
            0.0
        };
        if self.lead > 0.0 {
            (at, at + self.viewport + ahead)
        } else {
            (at - ahead, at + self.viewport)
        }
    }

    /// Whether item `i`'s row overlaps `start..end` (content coordinates).
    fn item_overlaps(&self, i: u32, start: f32, end: f32) -> bool {
        let top = self.item_at(i);
        top < end && top + self.row_size(self.row_of(i)) > start
    }

    /// Whether cells wait for [`Self::take_paced`].
    pub(super) fn has_paced(&self) -> bool {
        self.slots
            .iter()
            .any(|s| s.index.is_some() && s.index != s.requested)
    }

    /// Up to `max` of the cells waiting to bind, nearest the viewport first
    /// on the side the list scrolls towards, then the other side; pushed to
    /// `binds`. Returns how many. The engine takes one at a time (it checks
    /// its budget after each), so this picks the nearest by a scan of the
    /// slots instead of sorting them.
    pub(super) fn take_paced(&mut self, max: usize, binds: &mut Vec<(u32, u32)>) -> usize {
        let mut taken = 0;
        while taken < max {
            let Some((slot, i)) = self.nearest_waiting() else {
                break;
            };
            self.slots[slot as usize].requested = Some(i);
            binds.push((slot, i));
            taken += 1;
        }
        taken
    }

    /// The waiting cell [`Self::take_paced`] binds next: `(slot, index)`.
    fn nearest_waiting(&self) -> Option<(u32, u32)> {
        let at = self.offset;
        let (top, bottom) = (at, at + self.viewport);
        let forward = self.lead >= 0.0;
        let mut best: Option<((bool, f32), u32, u32)> = None;
        for (slot, s) in self.slots.iter().enumerate() {
            let Some(i) = s.index else { continue };
            if s.requested == Some(i) {
                continue;
            }
            let start = self.item_at(i);
            let (distance, below) = if start >= bottom {
                (start - bottom, true)
            } else {
                let end = start + self.row_size(self.row_of(i));
                ((top - end).max(0.0), false)
            };
            // The side behind waits for the side ahead.
            let key = (below != forward, distance);
            if best.is_none_or(|(b, ..)| key.0.cmp(&b.0).then(key.1.total_cmp(&b.1)).is_lt()) {
                best = Some((key, slot as u32, i));
            }
        }
        best.map(|(_, slot, i)| (slot, i))
    }

    fn unbind(&mut self, slot: u32) {
        if let Some(i) = self.slots[slot as usize].index.take() {
            self.by_index.remove(&i);
            self.free.push(slot);
        }
    }

    /// The content offset that shows item `index` at the viewport's start,
    /// clamped to the content.
    pub(super) fn offset_for_index(&self, index: u32) -> f32 {
        let index = index.min(self.count.saturating_sub(1));
        let size = self.row_size(self.row_of(index));
        let at = self.item_at(index);
        let free = self.viewport - size;
        (at - 0.0 * free).clamp(0.0, (self.content_len() - self.viewport).max(0.0))
    }

    pub(super) fn end_offset(&self) -> f32 {
        (self.content_len() - self.viewport).max(0.0)
    }

    pub(super) fn target_offset(&self, target: PinTarget) -> f32 {
        match target {
            PinTarget::End => self.end_offset(),
            PinTarget::Index(index) => self.offset_for_index(index),
        }
    }

    /// The platform is about to scroll to `offset`: the window follows it
    /// before the platform reports.
    pub(super) fn jump(&mut self, offset: f32) {
        self.offset = offset;
        self.dirty = true;
    }

    pub(super) fn pin(&mut self, target: Option<PinTarget>, offset: f32) {
        self.pin = target.map(|target| Pin { target, at: offset });
    }

    /// After measurements: where a pinned list goes for its target, which
    /// moved as estimated sizes became real ones. Returns a new target when
    /// the list must move again.
    pub(super) fn follow_pin(&mut self) -> Option<f32> {
        let mut pin = self.pin?;
        if self.viewport <= 0.0 {
            return None;
        }
        let target = self.target_offset(pin.target);
        if (target - pin.at).abs() > PIN_SLACK {
            pin.at = target;
            self.pin = Some(pin);
            self.jump(target);
            return Some(target);
        }
        if self.viewport_measured() {
            self.pin = None;
        }
        None
    }

    /// Whether every item overlapping the viewport has its measured size.
    fn viewport_measured(&self) -> bool {
        let start = self.offset;
        self.items_in(start, start + self.viewport)
            .is_none_or(|(first, last)| {
                self.sizes[first as usize..=last as usize]
                    .iter()
                    .all(|s| !s.is_nan())
            })
    }

    /// Where an unbound cell has to be moved when it lies in the window.
    pub(super) fn hidden_y(&self) -> f32 {
        HIDDEN
    }
}

fn target(scroll: PendingScroll) -> f32 {
    match scroll {
        PendingScroll::By(_) => f32::NAN,
        PendingScroll::To(t) => t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(count: u32, estimated: f32, viewport: f32) -> ListState {
        let mut l = ListState::new();
        l.set_estimated(estimated);
        l.set_count(count);
        l.viewport = viewport;
        l
    }

    #[test]
    fn window_covers_viewport_and_overscan() {
        let mut l = list(100, 50.0, 200.0);
        let mut binds = Vec::new();
        assert!(!l.update_window(&mut binds));
        // 200 viewport + 200 overscan each way: items 0..=8.
        assert_eq!(binds[0].1, 0);
        assert_eq!(binds.last().expect("binds").1, 8);
    }

    #[test]
    fn paced_lists_bind_the_viewport_now_and_the_overscan_ahead_first() {
        let mut l = list(1000, 50.0, 400.0);
        l.paced = true;
        let mut binds = Vec::new();
        l.update_window(&mut binds);
        let now: Vec<u32> = binds.iter().map(|b| b.1).collect();
        assert_eq!(now, (0..8).collect::<Vec<_>>(), "the viewport");
        assert!(l.has_paced());
        binds.clear();
        assert_eq!(l.take_paced(3, &mut binds), 3);
        assert_eq!(binds.iter().map(|b| b.1).collect::<Vec<_>>(), [8, 9, 10]);
        while l.take_paced(100, &mut binds) > 0 {}
        assert!(!l.has_paced());

        // The platform reports the scroll: scrolling up by 100, the
        // viewport and two steps above it bind now; the rest above comes
        // before what is below.
        l.reported(2000.0);
        l.update_window(&mut binds);
        while l.take_paced(100, &mut binds) > 0 {}
        l.reported(1900.0);
        binds.clear();
        l.update_window(&mut binds);
        // 1700..2300 was bound already; 1500..1600 is new and waits.
        assert_eq!(binds, []);
        // A jump binds its viewport only.
        l.reported(5000.0);
        l.update_window(&mut binds);
        let now: Vec<u32> = binds.iter().map(|b| b.1).collect();
        assert_eq!(now, (100..108).collect::<Vec<_>>());
        // A step up before the overscan bound: the viewport (4900..5300)
        // and two steps ahead (4700..4900) bind now ...
        l.reported(4900.0);
        binds.clear();
        l.update_window(&mut binds);
        let now: Vec<u32> = binds.iter().map(|b| b.1).collect();
        assert_eq!(now, (94..100).collect::<Vec<_>>());
        // ... the rest above before what is below.
        binds.clear();
        l.take_paced(3, &mut binds);
        assert_eq!(binds.iter().map(|b| b.1).collect::<Vec<_>>(), [93, 92, 91]);
    }

    #[test]
    fn splices_tell_bound_cells_their_index_and_forget_gone_items() {
        let mut l = list(100, 50.0, 400.0);
        l.paced = true;
        let mut binds = Vec::new();
        l.update_window(&mut binds);
        for &(slot, index) in &binds {
            l.add_slot_cell(CellSlot::Item(slot), 100 + slot);
            l.rendered(100 + slot, index);
        }
        // Overscan cells wait; one row arrives at the top.
        l.splice(0, 0, 1);
        binds.clear();
        l.update_window(&mut binds);
        // Every bound cell learns its new index, at once (the anchoring
        // scroll keeps them in place). The anchoring scroll is the core's,
        // not a step of the user's: the cells waiting below it, and the new
        // row above, still wait.
        let now: Vec<u32> = binds.iter().map(|b| b.1).collect();
        assert_eq!(now, (1..=8).collect::<Vec<_>>(), "{binds:?}");
        assert!(l.has_paced());
        // The data shrinks under the cells still waiting: none is asked
        // for an item that is gone.
        l.splice(3, 98, 0);
        binds.clear();
        l.update_window(&mut binds);
        while l.take_paced(100, &mut binds) > 0 {}
        assert!(binds.iter().all(|&(_, i)| i < 3), "{binds:?}");
    }

    #[test]
    fn measurements_shift_offsets_and_anchor_above_viewport() {
        let mut l = list(100, 50.0, 200.0);
        l.offset = 1000.0;
        let mut binds = Vec::new();
        l.update_window(&mut binds);
        // The first visible row (20) grew: keep it in place.
        let delta = l.measured(0, 100.0);
        assert_eq!(delta, 50.0);
        assert_eq!(l.item_at(1), 100.0);
    }

    #[test]
    fn load_more_fires_once_per_count() {
        let mut l = list(100, 50.0, 200.0);
        let mut binds = Vec::new();
        assert!(!l.update_window(&mut binds));
        l.offset = 5000.0;
        l.dirty = true;
        assert!(l.update_window(&mut binds));
        l.dirty = true;
        assert!(!l.update_window(&mut binds));
        l.set_count(150);
        l.offset = 7500.0;
        l.dirty = true;
        assert!(l.update_window(&mut binds));
    }

    #[test]
    fn shrinking_count_unbinds() {
        let mut l = list(100, 50.0, 200.0);
        let mut binds = Vec::new();
        l.update_window(&mut binds);
        l.set_count(2);
        l.update_window(&mut binds);
        assert!(
            l.slots
                .iter()
                .all(|s| s.index.is_none() || s.index < Some(2))
        );
    }

    #[test]
    fn columns_share_rows() {
        let mut l = list(10, 50.0, 200.0);
        l.set_columns(2);
        assert_eq!(l.row_count(), 5);
        assert_eq!(l.item_at(2), l.item_at(0) + 50.0);
        assert_eq!(l.column(300.0, 1), (150.0, 150.0));
    }

    #[test]
    fn scroll_to_index_pins_until_measured() {
        let mut l = list(1000, 50.0, 200.0);
        l.pin(Some(PinTarget::Index(500)), 25000.0);
        l.jump(25000.0);
        let mut binds = Vec::new();
        l.update_window(&mut binds);
        assert_eq!(binds[0].1, 496);
        // The first visible row grows: the target moves by 50.
        let pinned = l.pin;
        l.measured(498, 100.0);
        assert_eq!(l.follow_pin(), Some(25050.0));
        assert_eq!(l.offset, 25050.0);
        assert!(l.pin.is_some());
        assert_ne!(pinned.map(|p| p.at), None);
    }

    #[test]
    fn stale_scroll_reports_are_replayed() {
        let mut l = list(100, 50.0, 200.0);
        l.jump(500.0);
        l.issue_scroll_to(1000.0);
        l.reported(500.0);
        assert_eq!(l.offset, 1000.0);
        l.reported(1000.0);
        assert_eq!(l.offset, 1000.0);
    }

    #[test]
    fn measurements_before_the_viewport_report_their_delta() {
        let mut l = list(100, 50.0, 200.0);
        l.offset = 500.0;
        // A row above the viewport grew: the scroll follows it.
        assert_eq!(l.measured(2, 80.0), 30.0);
        assert_eq!(l.item_at(3), 180.0);
        // Shrinking reports the fall.
        assert_eq!(l.measured(2, 20.0), -60.0);
        assert_eq!(l.item_at(3), 120.0);
        // The same size again is not a change.
        assert_eq!(l.measured(2, 20.0), 0.0);
        // A row at or below the viewport grows the content, not the scroll.
        let before = l.item_at(10);
        assert_eq!(l.measured(10, 90.0), 0.0);
        assert_eq!(l.item_at(11) - before, 90.0);
        assert_eq!(l.item_at(10), before);
    }

    #[test]
    fn a_row_takes_its_tallest_cell() {
        let mut l = list(6, 50.0, 100.0);
        l.set_columns(2);
        // Items 2 and 3 share row 1.
        assert_eq!(l.measured(3, 80.0), 0.0);
        assert_eq!(l.row_size(1), 80.0);
        assert_eq!(l.item_at(4), 130.0);
        // A shorter cell leaves the row as it was.
        assert_eq!(l.measured(2, 30.0), 0.0);
        assert_eq!(l.row_size(1), 80.0);
        assert_eq!(l.item_at(4), 130.0);
        // A taller one grows the row and moves everything after it.
        assert_eq!(l.measured(2, 100.0), 0.0);
        assert_eq!(l.row_size(1), 100.0);
        assert_eq!(l.item_at(4), 150.0);
        assert_eq!(l.item_at(5), 150.0);
    }

    #[test]
    fn extra_sizes_anchor_only_above_the_viewport() {
        let mut l = list(10, 50.0, 200.0);
        l.add_slot_cell(CellSlot::Header, 1);
        l.add_slot_cell(CellSlot::Footer, 2);
        // In the viewport the header pushes the items down without scrolling.
        assert_eq!(l.measured_extra(ExtraKind::Header, 30.0), 0.0);
        assert_eq!(l.blocks().body, 30.0);
        assert_eq!(l.item_at(0), 30.0);
        // Above it, its growth is the scroll that keeps the rows in place.
        l.offset = 300.0;
        assert_eq!(l.measured_extra(ExtraKind::Header, 50.0), 20.0);
        assert_eq!(l.blocks().body, 50.0);
        assert_eq!(l.measured_extra(ExtraKind::Header, 50.0), 0.0);
        // Nothing follows the footer, so it never scrolls the list.
        l.offset = l.end_offset();
        assert_eq!(l.measured_extra(ExtraKind::Footer, 40.0), 0.0);
        assert_eq!(l.blocks().footer, 550.0);
        assert_eq!(l.content_len(), 590.0);
    }

    #[test]
    fn an_empty_placeholder_below_the_viewport_does_not_scroll() {
        let mut l = list(0, 50.0, 100.0);
        l.add_slot_cell(CellSlot::Header, 1);
        l.add_slot_cell(CellSlot::Empty, 2);
        assert_eq!(l.measured_extra(ExtraKind::Header, 60.0), 0.0);
        assert_eq!(l.measured_extra(ExtraKind::Empty, 200.0), 0.0);
        assert_eq!(l.blocks().body, 60.0);
        // It ends after the viewport from anywhere in the content.
        l.offset = l.end_offset();
        assert_eq!(l.measured_extra(ExtraKind::Empty, 100.0), 0.0);
        assert_eq!(l.content_len(), 160.0);
    }

    #[test]
    fn blocks_swap_the_empty_placeholder_for_items() {
        let mut l = list(0, 50.0, 200.0);
        l.add_slot_cell(CellSlot::Header, 1);
        l.add_slot_cell(CellSlot::Footer, 2);
        l.add_slot_cell(CellSlot::Empty, 3);
        l.measured_extra(ExtraKind::Header, 20.0);
        l.measured_extra(ExtraKind::Empty, 60.0);
        l.measured_extra(ExtraKind::Footer, 10.0);
        let empty = l.blocks();
        assert_eq!((empty.header, empty.body, empty.footer), (0.0, 20.0, 80.0));
        assert_eq!(empty.total, 90.0);
        l.set_count(3);
        let items = l.blocks();
        assert_eq!((items.header, items.body), (0.0, 20.0));
        assert_eq!(items.footer, 170.0);
        assert_eq!(items.total, 180.0);
        l.set_count(0);
        assert_eq!(l.blocks(), empty);
    }

    #[test]
    fn a_new_window_reuses_the_cells_of_the_old_one() {
        let mut l = list(1000, 50.0, 200.0);
        l.offset = 10_000.0;
        let mut binds = Vec::new();
        l.update_window(&mut binds);
        assert_eq!(l.slots.len(), 13);
        assert_eq!(binds.first().map(|b| b.1), Some(196));
        assert_eq!(binds.last().map(|b| b.1), Some(208));
        // Far away: the window's slots are the old ones, rebound.
        l.offset = 20_000.0;
        binds.clear();
        l.update_window(&mut binds);
        assert_eq!(l.slots.len(), 13);
        assert_eq!(binds.first().map(|b| b.1), Some(396));
        assert_eq!(binds.last().map(|b| b.1), Some(408));
        assert!(!l.by_index.contains_key(&196));
        for index in l.slots.iter().filter_map(|s| s.index) {
            assert!((396..=408).contains(&index), "slot holds {index}");
        }
    }

    #[test]
    fn shrinking_count_leaves_no_out_of_range_slot() {
        let mut l = list(100, 50.0, 200.0);
        let mut binds = Vec::new();
        l.update_window(&mut binds);
        l.set_count(2);
        binds.clear();
        l.update_window(&mut binds);
        assert!(l.slots.iter().all(|s| s.index.is_none_or(|i| i < 2)));
        assert_eq!(l.by_index.len(), 2);
        assert_eq!(l.content_len(), 100.0);
    }

    #[test]
    fn columns_reset_measurements_to_the_estimate() {
        let mut l = list(10, 50.0, 200.0);
        l.measured(1, 80.0);
        assert_eq!(l.item_at(2), 130.0);
        l.set_columns(2);
        assert_eq!(l.row_count(), 5);
        assert_eq!(l.item_at(2), 50.0);
        assert_eq!(l.item_at(3), 50.0);
        assert_eq!(l.item_at(4), 100.0);
        assert_eq!(l.column_of(3), 1);
        assert_eq!(l.column(300.0, 1), (150.0, 150.0));
        // Zero columns is one column.
        l.set_columns(0);
        assert_eq!(l.row_count(), 10);
        assert_eq!(l.column_of(3), 0);
        assert_eq!(l.column(300.0, 0), (0.0, 300.0));
    }

    #[test]
    fn splicing_above_the_viewport_keeps_the_visible_rows() {
        let mut l = list(10, 50.0, 200.0);
        l.measured(0, 80.0);
        l.offset = 200.0;
        let anchor = l.item_at(3) - l.offset;
        l.splice(0, 0, 2);
        assert_eq!(l.count, 12);
        // The first visible row is where it was, two items further down.
        assert_eq!(l.item_at(5) - l.offset, anchor);
        assert_eq!(l.take_splice_anchor(), 100.0);
        // Its measurement followed it.
        assert_eq!(l.measured(2, 80.0), 0.0);
        // Removing the two again puts the list back.
        l.splice(0, 2, 0);
        assert_eq!(l.count, 10);
        assert_eq!(l.item_at(3) - l.offset, anchor);
        assert_eq!(l.take_splice_anchor(), -100.0);
        assert_eq!(l.measured(0, 80.0), 0.0);
    }

    #[test]
    fn splicing_a_visible_item_frees_its_cell() {
        let mut l = list(10, 50.0, 200.0);
        let mut binds = Vec::new();
        l.update_window(&mut binds);
        let (slot, item) = binds[0];
        assert_eq!(item, 0);
        l.add_slot_cell(CellSlot::Item(slot), 500);
        l.rendered(500, 0);
        assert_eq!(l.slots[slot as usize].ready(), Some(0));
        l.splice(0, 1, 0);
        // The cell loses its item and waits for a new one.
        assert_eq!(l.count, 9);
        assert_eq!(l.slots[slot as usize].index, None);
        assert_eq!(l.slots[slot as usize].rendered, None);
        assert!(l.free.contains(&slot));
        // The old item 1 is item 0 now; the deleted one binds nothing.
        assert_eq!(l.by_index.get(&0), Some(&1));
        assert_eq!(l.by_index.get(&9), None);
        binds.clear();
        l.update_window(&mut binds);
        // Rebinding takes the next item, never the deleted index.
        assert_eq!(l.slots[slot as usize].index, Some(8));
        assert_eq!(binds.iter().find(|b| b.0 == slot).map(|b| b.1), Some(8));
        assert!(!binds.iter().any(|b| b.1 == 9));
    }

    #[test]
    fn offsets_stay_inside_the_content() {
        let l = list(10, 50.0, 200.0);
        assert_eq!(l.offset_for_index(0), 0.0);
        assert_eq!(l.offset_for_index(9), 300.0);
        // Past the end is the last item, not past it.
        assert_eq!(l.offset_for_index(999), l.offset_for_index(9));
        assert_eq!(l.end_offset(), 300.0);
        // Content shorter than the viewport never scrolls.
        let short = list(3, 50.0, 200.0);
        assert_eq!(short.offset_for_index(2), 0.0);
        assert_eq!(short.end_offset(), 0.0);
        // No content at all: no negative range.
        let empty = list(0, 50.0, 200.0);
        assert_eq!(empty.content_len(), 0.0);
        assert_eq!(empty.offset_for_index(0), 0.0);
        assert_eq!(empty.end_offset(), 0.0);
    }

    #[test]
    fn end_threshold_gates_load_more() {
        let mut l = list(100, 50.0, 200.0);
        l.set_end_threshold(0.0);
        let mut binds = Vec::new();
        // Not at the end: no load.
        l.offset = 2000.0;
        assert!(!l.update_window(&mut binds));
        // The very end: once.
        l.offset = 4800.0;
        assert!(l.update_window(&mut binds));
        assert!(!l.update_window(&mut binds));
        // Away and back: still the same items.
        l.offset = 0.0;
        assert!(!l.update_window(&mut binds));
        l.offset = 4800.0;
        assert!(!l.update_window(&mut binds));
        // More items move the end: reaching it loads once more.
        l.set_count(150);
        l.offset = 7300.0;
        assert!(l.update_window(&mut binds));
        assert!(!l.update_window(&mut binds));
    }

    #[test]
    fn the_end_threshold_counts_headers_and_footers() {
        let mut l = list(100, 50.0, 200.0);
        l.add_slot_cell(CellSlot::Header, 1);
        l.add_slot_cell(CellSlot::Footer, 2);
        l.measured_extra(ExtraKind::Header, 100.0);
        l.measured_extra(ExtraKind::Footer, 100.0);
        assert_eq!(l.content_len(), 5200.0);
        let mut binds = Vec::new();
        // One viewport before the content ends, extras included.
        l.offset = 4750.0;
        assert!(!l.update_window(&mut binds));
        l.offset = 4800.0;
        assert!(l.update_window(&mut binds));
    }

    #[test]
    fn an_empty_or_unmeasured_list_never_loads() {
        let mut l = list(0, 50.0, 200.0);
        let mut binds = Vec::new();
        assert!(!l.update_window(&mut binds));
        assert!(l.slots.iter().all(|s| s.index.is_none()));
        let mut zero = list(100, 50.0, 0.0);
        assert!(!zero.update_window(&mut binds));
        assert!(zero.slots.iter().all(|s| s.index.is_none()));
    }

    #[test]
    fn a_splice_rebinds_the_cells_it_moved() {
        let mut l = list(10, 50.0, 200.0);
        let mut binds = Vec::new();
        l.update_window(&mut binds);
        assert_eq!(l.slots[0].index, Some(0));
        // Two items above: each cell shows a new index.
        l.splice(0, 0, 2);
        binds.clear();
        l.update_window(&mut binds);
        assert!(binds.contains(&(0, 2)), "binds {binds:?}");
        assert_eq!(l.slots[0].index, Some(2));
        assert_eq!(l.slots[0].requested, Some(2));
        // A cell whose index did not move is not asked again.
        let mut still = list(10, 50.0, 200.0);
        let mut binds = Vec::new();
        still.update_window(&mut binds);
        still.splice(4, 0, 1);
        binds.clear();
        still.update_window(&mut binds);
        assert!(!binds.iter().any(|b| b.1 == 0), "binds {binds:?}");
    }

    #[test]
    fn splices_keep_measurements_and_visible_content() {
        let mut l = list(10, 50.0, 200.0);
        l.measured(0, 80.0);
        l.offset = 200.0;
        l.splice(0, 1, 2);
        assert_eq!(l.count, 11);
        // Two items where one was measured: the new ones are estimates.
        assert_eq!(l.measured(0, 80.0), 30.0);
        assert!(l.take_splice_anchor() >= 0.0);
    }
}
