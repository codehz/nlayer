//! One selection across the texts of a `selectable` view, in reading order,
//! as [`nlayer_core::selection`] models it.
//!
//! `UITextInteraction` selects within one text, so an area has gestures of
//! its own: a long press selects the word under the finger (a drag after it
//! extends by words), two handles move the ends, a tap clears or toggles the
//! edit menu, and Copy / Select All go to the area as first responder.
//! `userSelect: "none"` texts stay out. An area inside another never starts
//! its own gestures. There is no `keep_rendered`: after each mount batch and
//! scroll the model reclamps to the texts still mounted.
//!
//! From `nsh-platform-ios/src/selectable_view.rs`, without pointer selection
//! or keeping virtual rows rendered.

use std::cell::{Cell, RefCell};

use nlayer_core::{
    geometry::{Bounds, Point, Points, bounds, point, px, size},
    selection::{Handle, SelectionModel, SelectionTexts, TextPosition, reading_order, reorder},
    style::Color,
    text::AttributedText,
};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
    rc::{Allocated, Retained},
    runtime::{AnyClass, AnyObject, NSObjectProtocol, Sel},
    sel,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSArray, NSNumber, NSObject};
use objc2_ui_kit::{
    UIGestureRecognizer, UIGestureRecognizerDelegate, UIGestureRecognizerState,
    UILongPressGestureRecognizer, UIPanGestureRecognizer, UIResponder, UIScrollView,
    UITapGestureRecognizer, UITouch, UITraitEnvironment, UIView,
};

use crate::text_view::TextView;

/// The opacity a selection highlight uses, of the tint.
const HIGHLIGHT_ALPHA: f64 = 0.2;
/// A handle's knob (diameter) and stem (width).
const KNOB: CGFloat = 11.0;
const STEM: CGFloat = 2.0;
/// How far a handle takes touches around its stem.
const HANDLE_SLOP: CGFloat = 22.0;
/// `UITouchType`s: a finger and the Pencil. Pointers are a desktop idiom.
const TOUCH_DIRECT: isize = 0;
const TOUCH_PENCIL: isize = 2;

/// One text of an area.
struct Entry {
    view: Retained<TextView>,
    text: std::sync::Arc<AttributedText>,
    /// The content box, in the area's coordinates.
    frame: Bounds<Points>,
}

/// The texts of an area as the model sees them.
#[derive(Default)]
struct AreaTexts {
    area: Option<Retained<UIView>>,
    entries: Vec<Entry>,
}

fn cg(p: Point<Points>) -> CGPoint {
    CGPoint::new(p.x.0 as CGFloat, p.y.0 as CGFloat)
}

fn pt(p: CGPoint) -> Point<Points> {
    point(px(p.x as f32), px(p.y as f32))
}

impl SelectionTexts for AreaTexts {
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn content(&self, i: usize) -> &str {
        &self.entries[i].text.text
    }

    fn frame(&self, i: usize) -> Bounds<Points> {
        self.entries[i].frame
    }

    fn offset_at(&self, i: usize, at: Point<Points>) -> u32 {
        let e = &self.entries[i];
        let local = match &self.area {
            Some(area) => area.convertPoint_toView(cg(at), Some(&e.view)),
            None => cg(at),
        };
        e.view.closest_offset(local) as u32
    }

    fn key(&self, i: usize) -> u64 {
        self.entries[i].view.tag() as u64
    }
}

impl AreaTexts {
    /// The texts of `area`'s subtree, in document order.
    fn list(area: &UIView) -> Self {
        fn walk(view: &UIView, area: &UIView, out: &mut Vec<Entry>) {
            let start = out.len();
            let mut runs = Vec::new();
            for child in view.subviews().iter() {
                if child.isHidden() {
                    continue;
                }
                let from = out.len();
                let f = child.frame();
                let frame = bounds(
                    pt(f.origin),
                    size(px(f.size.width as f32), px(f.size.height as f32)),
                );
                if let Some(text) = child.downcast_ref::<TextView>() {
                    text.update_own_selection();
                    let Some(content) = text.attributed() else {
                        continue;
                    };
                    if content.selectable == Some(false) {
                        continue;
                    }
                    let Some(rect) = text.content_box() else {
                        continue;
                    };
                    let r = text.convertRect_toView(rect, Some(area));
                    out.push(Entry {
                        view: text.retain(),
                        text: content,
                        frame: bounds(
                            pt(r.origin),
                            size(px(r.size.width as f32), px(r.size.height as f32)),
                        ),
                    });
                } else if child.downcast_ref::<AreaResponder>().is_none() {
                    walk(&child, area, out);
                }
                runs.push((frame, from..out.len()));
            }
            if let Some(order) = reading_order(&runs) {
                reorder(out, start, &order);
            }
        }
        let mut entries = Vec::new();
        walk(area, area, &mut entries);
        Self {
            area: Some(area.retain()),
            entries,
        }
    }

    /// Whether `other` lists the same texts with the same contents.
    fn same_texts(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self.entries.iter().zip(&other.entries).all(|(a, b)| {
                Retained::as_ptr(&a.view) == Retained::as_ptr(&b.view)
                    && std::sync::Arc::ptr_eq(&a.text, &b.text)
            })
    }

    /// The UTF-16 length of text `i`.
    fn len16(&self, i: usize) -> u32 {
        self.entries[i].text.text.encode_utf16().count() as u32
    }

    /// The caret at `at` in the area's coordinates; `trailing` for an end.
    fn caret(&self, at: TextPosition, trailing: bool) -> Option<CGRect> {
        let e = self.entries.get(at.text)?;
        let area = self.area.as_ref()?;
        let rect = e.view.caret_rect_at(at.offset as usize, trailing);
        Some(e.view.convertRect_toView(rect, Some(area)))
    }

    /// Whether `rect` of text `i` shows: no clipping view between the text
    /// and the area cuts it off.
    fn shows(&self, i: usize, rect: CGRect) -> bool {
        let (Some(e), Some(area)) = (self.entries.get(i), self.area.as_ref()) else {
            return false;
        };
        let mid = CGPoint::new(
            rect.origin.x + rect.size.width / 2.0,
            rect.origin.y + rect.size.height / 2.0,
        );
        let mut view = e.view.superview();
        while let Some(v) = view {
            if std::ptr::eq::<UIView>(&*v, &**area) {
                return true;
            }
            if v.clipsToBounds() {
                let p = area.convertPoint_toView(mid, Some(&v));
                let b = v.bounds();
                if p.x < b.origin.x
                    || p.y < b.origin.y
                    || p.x > b.origin.x + b.size.width
                    || p.y > b.origin.y + b.size.height
                {
                    return false;
                }
            }
            view = v.superview();
        }
        true
    }
}

/// `color` at `alpha` of its opacity.
fn faded(color: Color, alpha: f64) -> Color {
    let a = (color.0 & 0xff) as f64 * alpha;
    Color((color.0 & !0xff) | a.round().clamp(0.0, 255.0) as u32)
}

/// Whether `view` is a selectable area (it hosts an [`AreaResponder`]).
pub(crate) fn is_selectable_area(view: &UIView) -> bool {
    view.subviews()
        .iter()
        .any(|child| child.downcast_ref::<AreaResponder>().is_some())
}

#[derive(Default)]
pub(crate) struct AreaIvars {
    model: RefCell<SelectionModel>,
    texts: RefCell<AreaTexts>,
    /// Texts drawing a part of the highlight.
    highlighted: RefCell<Vec<Retained<TextView>>>,
    /// The area's recognizers: long press, then tap.
    recognizers: RefCell<Vec<Retained<UIGestureRecognizer>>>,
    /// The two handles and which end each is now.
    handles: RefCell<Vec<(Retained<UIView>, Cell<Handle>)>>,
    /// The end a handle drag moves, and from the finger to the caret.
    dragging: Cell<Option<(Handle, CGPoint)>>,
    /// `UIEditMenuInteraction` (iOS 16), made on first use.
    edit_menu: RefCell<Option<Retained<AnyObject>>>,
    /// The interaction's menu shows.
    menu_shown: Cell<bool>,
}

define_class!(
    /// The selection of one area: its model, gestures, handles and menu.
    #[unsafe(super(UIView, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = AreaIvars]
    pub(crate) struct AreaResponder;

    impl AreaResponder {
        #[unsafe(method(canBecomeFirstResponder))]
        fn can_become_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            // SAFETY: calling the superclass implementation.
            let resigned: bool = unsafe { msg_send![super(self), resignFirstResponder] };
            if resigned {
                self.clear_selection();
            }
            resigned
        }

        #[unsafe(method(canPerformAction:withSender:))]
        fn can_perform_action(&self, action: Sel, _sender: Option<&AnyObject>) -> bool {
            if action == sel!(copy:) {
                !self.ivars().model.borrow().is_empty()
            } else if action == sel!(selectAll:) {
                !self.all_selected()
            } else {
                false
            }
        }

        #[unsafe(method(copy:))]
        fn copy(&self, _sender: Option<&AnyObject>) {
            let text = {
                let texts = self.ivars().texts.borrow();
                self.ivars().model.borrow().text(&*texts)
            };
            if !text.is_empty() {
                crate::text_selection::set_clipboard(&text);
            }
        }

        #[unsafe(method(selectAll:))]
        fn select_all(&self, _sender: Option<&AnyObject>) {
            self.refresh_texts();
            let changed = {
                let texts = self.ivars().texts.borrow();
                self.ivars().model.borrow_mut().select_all(&*texts)
            };
            if changed {
                self.redraw();
            }
            self.show_menu();
        }

        #[unsafe(method(handleLongPress:))]
        fn handle_long_press(&self, recognizer: &UIGestureRecognizer) {
            let Some(area) = self.superview() else { return };
            let at = recognizer.locationInView(Some(&area));
            let state = recognizer.state();
            if state == UIGestureRecognizerState::Began {
                // The long press selects: this gesture's press and `href` do
                // not also fire.
                if let Some(hit) = area.hitTest_withEvent(at, None)
                    && let Some(mounter) = crate::mount::current()
                    && let Ok(mut mounter) = mounter.try_borrow_mut()
                {
                    mounter.cancel_press_chain(&hit);
                }
                self.hide_menu();
                self.refresh_texts();
                let changed = {
                    let texts = self.ivars().texts.borrow();
                    self.ivars()
                        .model
                        .borrow_mut()
                        .select_word_at(&*texts, pt(at))
                };
                self.becomeFirstResponder();
                if changed {
                    self.redraw();
                }
            } else if state == UIGestureRecognizerState::Changed {
                let changed = {
                    let texts = self.ivars().texts.borrow();
                    self.ivars().model.borrow_mut().drag(&*texts, pt(at))
                };
                if changed {
                    self.redraw();
                }
            } else if state == UIGestureRecognizerState::Ended {
                self.show_menu();
            }
        }

        #[unsafe(method(handleTap:))]
        fn handle_tap(&self, recognizer: &UIGestureRecognizer) {
            if recognizer.state() != UIGestureRecognizerState::Ended {
                return;
            }
            let Some(area) = self.superview() else { return };
            let at = recognizer.locationInView(Some(&area));
            if self.selection_contains(at) {
                if self.menu_visible() {
                    self.hide_menu();
                } else {
                    self.show_menu();
                }
            } else {
                self.clear_selection();
                self.resignFirstResponder();
            }
        }

        #[unsafe(method(handleKnob:))]
        fn handle_knob(&self, recognizer: &UIGestureRecognizer) {
            let (Some(area), Some(knob)) = (self.superview(), recognizer.view()) else {
                return;
            };
            let finger = recognizer.locationInView(Some(&area));
            let state = recognizer.state();
            if state == UIGestureRecognizerState::Began {
                self.hide_menu();
                self.refresh_texts();
                let Some(handle) = self.role_of(&knob) else { return };
                let caret = {
                    let texts = self.ivars().texts.borrow();
                    let bounds = self.ivars().model.borrow().bounds();
                    bounds.and_then(|(start, end)| match handle {
                        Handle::Start => texts.caret(start, false),
                        Handle::End => texts.caret(end, true),
                    })
                };
                let Some(caret) = caret else { return };
                let mid = CGPoint::new(
                    caret.origin.x + caret.size.width / 2.0,
                    caret.origin.y + caret.size.height / 2.0,
                );
                self.ivars().dragging.set(Some((
                    handle,
                    CGPoint::new(mid.x - finger.x, mid.y - finger.y),
                )));
            } else if state == UIGestureRecognizerState::Changed {
                let Some((handle, delta)) = self.ivars().dragging.get() else {
                    return;
                };
                let at = pt(CGPoint::new(finger.x + delta.x, finger.y + delta.y));
                let (changed, before, after) = {
                    let texts = self.ivars().texts.borrow();
                    let mut model = self.ivars().model.borrow_mut();
                    let before = model.bounds();
                    let changed = model.move_handle(&*texts, handle, at);
                    (changed, before, model.bounds())
                };
                if let (Some((s0, e0)), Some((s1, e1))) = (before, after) {
                    let swapped = match handle {
                        Handle::Start => s1 == e0 && e1 != e0,
                        Handle::End => e1 == s0 && s1 != s0,
                    };
                    if swapped {
                        let other = match handle {
                            Handle::Start => Handle::End,
                            Handle::End => Handle::Start,
                        };
                        for (_, role) in self.ivars().handles.borrow().iter() {
                            role.set(if role.get() == handle { other } else { handle });
                        }
                        self.ivars().dragging.set(Some((other, delta)));
                    }
                }
                if changed {
                    self.redraw();
                }
            } else if state == UIGestureRecognizerState::Ended
                || state == UIGestureRecognizerState::Cancelled
            {
                self.ivars().dragging.set(None);
                self.show_menu();
            }
        }

        #[unsafe(method(editMenuInteraction:targetRectForConfiguration:))]
        fn edit_menu_target_rect(
            &self,
            _interaction: &AnyObject,
            _configuration: &AnyObject,
        ) -> CGRect {
            self.selection_rect()
                .unwrap_or(CGRect::new(CGPoint::ZERO, CGSize::ZERO))
        }

        #[unsafe(method(editMenuInteraction:willPresentMenuForConfiguration:animator:))]
        fn edit_menu_will_present(
            &self,
            _interaction: &AnyObject,
            _configuration: &AnyObject,
            _animator: &AnyObject,
        ) {
            self.ivars().menu_shown.set(true);
        }

        #[unsafe(method(editMenuInteraction:willDismissMenuForConfiguration:animator:))]
        fn edit_menu_will_dismiss(
            &self,
            _interaction: &AnyObject,
            _configuration: &AnyObject,
            _animator: &AnyObject,
        ) {
            self.ivars().menu_shown.set(false);
        }
    }

    unsafe impl NSObjectProtocol for AreaResponder {}

    unsafe impl UIGestureRecognizerDelegate for AreaResponder {
        #[unsafe(method(gestureRecognizerShouldBegin:))]
        fn should_begin(&self, recognizer: &UIGestureRecognizer) -> bool {
            self.gesture_should_begin(recognizer)
        }

        #[unsafe(method(gestureRecognizer:shouldRecognizeSimultaneouslyWithGestureRecognizer:))]
        fn should_recognize_simultaneously(
            &self,
            recognizer: &UIGestureRecognizer,
            _other: &UIGestureRecognizer,
        ) -> bool {
            recognizer.isKindOfClass(UITapGestureRecognizer::class())
        }

        #[unsafe(method(gestureRecognizer:shouldBeRequiredToFailByGestureRecognizer:))]
        fn should_be_required_to_fail(
            &self,
            recognizer: &UIGestureRecognizer,
            other: &UIGestureRecognizer,
        ) -> bool {
            recognizer.isKindOfClass(UIPanGestureRecognizer::class())
                && other
                    .view()
                    .is_some_and(|v| v.isKindOfClass(UIScrollView::class()))
        }

        #[unsafe(method(gestureRecognizer:shouldReceiveTouch:))]
        fn should_receive_touch(&self, recognizer: &UIGestureRecognizer, touch: &UITouch) -> bool {
            let on_handle = touch.view().is_some_and(|v| {
                self.ivars()
                    .handles
                    .borrow()
                    .iter()
                    .any(|(h, _)| v.isDescendantOfView(h))
            });
            let ours = self
                .ivars()
                .recognizers
                .borrow()
                .iter()
                .any(|r| std::ptr::eq::<UIGestureRecognizer>(&**r, recognizer));
            !(ours && on_handle)
        }
    }
);

impl AreaResponder {
    /// Makes `area` a selectable view.
    pub(crate) fn attach(area: &UIView, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(AreaIvars::default());
        // SAFETY: UIView's designated initializer.
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: CGRect::new(CGPoint::ZERO, CGSize::ZERO)]
        };
        area.addSubview(&this);
        let types = |types: &[isize]| {
            let numbers: Vec<_> = types.iter().map(|&t| NSNumber::new_isize(t)).collect();
            NSArray::from_retained_slice(&numbers)
        };
        let target: &AnyObject = &this;
        // SAFETY: the actions are implemented by the responder, which the
        // area holds as long as the recognizers are attached.
        let recognizers: Vec<Retained<UIGestureRecognizer>> = unsafe {
            let long_press = UILongPressGestureRecognizer::initWithTarget_action(
                UILongPressGestureRecognizer::alloc(mtm),
                Some(target),
                Some(sel!(handleLongPress:)),
            );
            long_press.setAllowedTouchTypes(&types(&[TOUCH_DIRECT, TOUCH_PENCIL]));
            let tap = UITapGestureRecognizer::initWithTarget_action(
                UITapGestureRecognizer::alloc(mtm),
                Some(target),
                Some(sel!(handleTap:)),
            );
            tap.setAllowedTouchTypes(&types(&[TOUCH_DIRECT, TOUCH_PENCIL]));
            vec![Retained::into_super(long_press), Retained::into_super(tap)]
        };
        for r in &recognizers {
            r.setCancelsTouchesInView(false);
            r.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*this)));
            area.addGestureRecognizer(r);
        }
        recognizers[0].setCancelsTouchesInView(true);
        *this.ivars().recognizers.borrow_mut() = recognizers;
        area.setUserInteractionEnabled(true);
        update_texts(area);
        this
    }

    /// The area is no longer one: its gestures, handles and highlight go.
    pub(crate) fn detach(&self) {
        self.clear_selection();
        if self.isFirstResponder() {
            self.resignFirstResponder();
        }
        let area = self.superview();
        for r in self.ivars().recognizers.take() {
            if let Some(area) = &area {
                area.removeGestureRecognizer(&r);
            }
        }
        for (h, _) in self.ivars().handles.take() {
            h.removeFromSuperview();
        }
        if let Some(menu) = self.ivars().edit_menu.take() {
            // SAFETY: UIView's `removeInteraction:` with the interaction
            // added in `edit_menu`.
            let _: () = unsafe { msg_send![self, removeInteraction: &*menu] };
        }
        *self.ivars().texts.borrow_mut() = AreaTexts::default();
        self.removeFromSuperview();
        if let Some(area) = area {
            update_texts(&area);
        }
    }

    /// A mount batch was applied: while something is selected, the texts,
    /// highlight and handles follow what changed.
    pub(crate) fn batch_applied(&self) {
        if !self.ivars().model.borrow().is_empty() {
            self.refresh_texts();
            self.redraw();
        }
    }

    /// A scroll view scrolled: handles follow texts that moved.
    pub(crate) fn scrolled(&self) {
        if !self.ivars().model.borrow().is_empty() {
            self.refresh_texts();
            self.place_handles();
        }
    }

    /// Lists the texts again; the model hears of new ones.
    fn refresh_texts(&self) {
        let Some(area) = self.superview() else { return };
        let texts = AreaTexts::list(&area);
        let same = self.ivars().texts.borrow().same_texts(&texts);
        if !same {
            self.ivars().model.borrow_mut().texts_changed(&texts);
        }
        *self.ivars().texts.borrow_mut() = texts;
    }

    /// Whether `area` is inside another area (which selects for it).
    fn nested(&self) -> bool {
        let mut view = self.superview().and_then(|a| a.superview());
        while let Some(v) = view {
            if is_selectable_area(&v) {
                return true;
            }
            view = v.superview();
        }
        false
    }

    fn gesture_should_begin(&self, recognizer: &UIGestureRecognizer) -> bool {
        let Some(area) = self.superview() else {
            return false;
        };
        let on_area = recognizer
            .view()
            .is_some_and(|v| std::ptr::eq::<UIView>(&*v, &*area));
        if !on_area {
            return !self.ivars().model.borrow().is_empty();
        }
        if self.nested() {
            return false;
        }
        let is_tap = recognizer.isKindOfClass(UITapGestureRecognizer::class());
        if is_tap {
            return !self.ivars().model.borrow().is_empty();
        }
        let at = recognizer.locationInView(Some(&area));
        self.refresh_texts();
        let texts = self.ivars().texts.borrow();
        texts.entries.iter().any(|e| {
            let f = e.frame;
            let slop = 4.0;
            at.x >= f.origin.x.0 as CGFloat - slop
                && at.y >= f.origin.y.0 as CGFloat - slop
                && at.x <= (f.origin.x.0 + f.size.width.0) as CGFloat + slop
                && at.y <= (f.origin.y.0 + f.size.height.0) as CGFloat + slop
        })
    }

    /// Clears the selection, its highlight, handles and menu.
    fn clear_selection(&self) {
        self.hide_menu();
        self.ivars().dragging.set(None);
        let changed = self.ivars().model.borrow_mut().clear();
        if changed || !self.ivars().highlighted.borrow().is_empty() {
            self.redraw();
        }
    }

    /// Whether every text is selected whole.
    fn all_selected(&self) -> bool {
        self.refresh_texts();
        let texts = self.ivars().texts.borrow();
        let Some(last) = texts.entries.len().checked_sub(1) else {
            return true;
        };
        self.ivars().model.borrow().bounds()
            == Some((
                TextPosition::new(0, 0),
                TextPosition::new(last, texts.len16(last)),
            ))
    }

    /// The tint of the area, packed.
    fn tint(&self) -> Color {
        let color = self.superview().and_then(|a| {
            a.tintColor()
                .map(|c| c.resolvedColorWithTraitCollection(&a.traitCollection()))
        });
        let (mut r, mut g, mut b, mut a) = (0.0, 0.48, 1.0, 1.0);
        if let Some(color) = color {
            // SAFETY: four valid out pointers.
            unsafe { color.getRed_green_blue_alpha(&mut r, &mut g, &mut b, &mut a) };
        }
        let byte = |v: CGFloat| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Color::rgba(byte(r), byte(g), byte(b), byte(a))
    }

    /// Draws what is selected: each text's part of the highlight, the handles.
    fn redraw(&self) {
        let tint = self.tint();
        let mut now = Vec::new();
        {
            let texts = self.ivars().texts.borrow();
            let ranges = self.ivars().model.borrow().ranges(&*texts);
            for (i, range) in ranges {
                let Some(e) = texts.entries.get(i) else {
                    continue;
                };
                let color = e.text.selection_color.unwrap_or(tint);
                e.view.set_area_highlight(Some((
                    (range.start as usize, range.end as usize),
                    faded(color, HIGHLIGHT_ALPHA),
                )));
                now.push(e.view.clone());
            }
        }
        let before = std::mem::replace(&mut *self.ivars().highlighted.borrow_mut(), Vec::new());
        for view in before {
            if !now
                .iter()
                .any(|v| Retained::as_ptr(v) == Retained::as_ptr(&view))
            {
                view.set_area_highlight(None);
            }
        }
        *self.ivars().highlighted.borrow_mut() = now;
        self.place_handles();
    }

    /// Puts the handles on the ends of the selection.
    fn place_handles(&self) {
        let Some(area) = self.superview() else { return };
        let ends = {
            let model = self.ivars().model.borrow();
            (!model.is_empty()).then(|| model.bounds()).flatten()
        };
        let Some((start, end)) = ends else {
            for (h, _) in self.ivars().handles.borrow().iter() {
                h.setHidden(true);
            }
            return;
        };
        self.make_handles(&area);
        let tint = self.superview().and_then(|a| a.tintColor());
        let texts = self.ivars().texts.borrow();
        for (view, role) in self.ivars().handles.borrow().iter() {
            let (at, trailing) = match role.get() {
                Handle::Start => (start, false),
                Handle::End => (end, true),
            };
            let Some(caret) = texts
                .caret(at, trailing)
                .filter(|c| texts.shows(at.text, *c))
            else {
                view.setHidden(true);
                continue;
            };
            let h = caret.size.height;
            let width = HANDLE_SLOP * 2.0;
            view.setFrame(CGRect::new(
                CGPoint::new(
                    caret.origin.x + caret.size.width / 2.0 - HANDLE_SLOP,
                    caret.origin.y - HANDLE_SLOP,
                ),
                CGSize::new(width, h + HANDLE_SLOP * 2.0),
            ));
            let parts = view.subviews();
            if let (Some(stem), Some(knob)) = (parts.firstObject(), parts.lastObject()) {
                stem.setFrame(CGRect::new(
                    CGPoint::new(HANDLE_SLOP - STEM / 2.0, HANDLE_SLOP),
                    CGSize::new(STEM, h),
                ));
                let y = match role.get() {
                    Handle::Start => HANDLE_SLOP - KNOB + 1.0,
                    Handle::End => HANDLE_SLOP + h - 1.0,
                };
                knob.setFrame(CGRect::new(
                    CGPoint::new(HANDLE_SLOP - KNOB / 2.0, y),
                    CGSize::new(KNOB, KNOB),
                ));
                stem.setBackgroundColor(tint.as_deref());
                knob.setBackgroundColor(tint.as_deref());
            }
            view.setHidden(false);
            area.bringSubviewToFront(view);
        }
    }

    /// The handles, made on first use: a stem and a knob each, with a pan.
    fn make_handles(&self, area: &UIView) {
        if !self.ivars().handles.borrow().is_empty() {
            return;
        }
        let mtm = self.mtm();
        let target: &AnyObject = self;
        let mut handles = Vec::new();
        for role in [Handle::Start, Handle::End] {
            let new = || {
                UIView::initWithFrame(UIView::alloc(mtm), CGRect::new(CGPoint::ZERO, CGSize::ZERO))
            };
            let (view, stem, knob) = (new(), new(), new());
            stem.setUserInteractionEnabled(false);
            knob.setUserInteractionEnabled(false);
            knob.layer().setCornerRadius(KNOB / 2.0);
            view.addSubview(&stem);
            view.addSubview(&knob);
            // SAFETY: `handleKnob:` is implemented by the responder, which
            // outlives the handles.
            let pan = unsafe {
                UIPanGestureRecognizer::initWithTarget_action(
                    UIPanGestureRecognizer::alloc(mtm),
                    Some(target),
                    Some(sel!(handleKnob:)),
                )
            };
            pan.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(self)));
            view.addGestureRecognizer(&pan);
            view.setHidden(true);
            area.addSubview(&view);
            handles.push((view, Cell::new(role)));
        }
        *self.ivars().handles.borrow_mut() = handles;
    }

    /// Which end `view` (a handle) is.
    fn role_of(&self, view: &UIView) -> Option<Handle> {
        self.ivars()
            .handles
            .borrow()
            .iter()
            .find(|(h, _)| std::ptr::eq::<UIView>(&**h, view))
            .map(|(_, role)| role.get())
    }

    /// The selection's extent in the area.
    fn selection_rect(&self) -> Option<CGRect> {
        let texts = self.ivars().texts.borrow();
        let model = self.ivars().model.borrow();
        if model.is_empty() {
            return None;
        }
        let (start, end) = model.bounds()?;
        let a = texts.caret(start, false)?;
        let b = texts.caret(end, true)?;
        let (x1, y1) = (a.origin.x.min(b.origin.x), a.origin.y.min(b.origin.y));
        let x2 = (a.origin.x + a.size.width).max(b.origin.x + b.size.width);
        let y2 = (a.origin.y + a.size.height).max(b.origin.y + b.size.height);
        Some(CGRect::new(
            CGPoint::new(x1, y1),
            CGSize::new(x2 - x1, y2 - y1),
        ))
    }

    /// Whether `at` (area coordinates) is on the highlight.
    fn selection_contains(&self, at: CGPoint) -> bool {
        let Some(area) = self.superview() else {
            return false;
        };
        let texts = self.ivars().texts.borrow();
        let ranges = self.ivars().model.borrow().ranges(&*texts);
        ranges.into_iter().any(|(i, range)| {
            let Some(e) = texts.entries.get(i) else {
                return false;
            };
            let local = area.convertPoint_toView(at, Some(&e.view));
            e.view
                .range_rects(range.start as usize, range.end as usize)
                .iter()
                .any(|(r, ..)| {
                    local.x >= r.origin.x
                        && local.y >= r.origin.y
                        && local.x <= r.origin.x + r.size.width
                        && local.y <= r.origin.y + r.size.height
                })
        })
    }

    /// `UIEditMenuInteraction` on the responder, if the system has it.
    fn edit_menu(&self) -> Option<Retained<AnyObject>> {
        let class = AnyClass::get(c"UIEditMenuInteraction")?;
        let mut menu = self.ivars().edit_menu.borrow_mut();
        if menu.is_none() {
            // SAFETY: `initWithDelegate:` is the interaction's initializer;
            // the responder implements the delegate methods it answers.
            let interaction: Retained<AnyObject> = unsafe {
                let alloc: Allocated<AnyObject> = msg_send![class, alloc];
                msg_send![alloc, initWithDelegate: self]
            };
            // SAFETY: UIView's `addInteraction:` with a UIInteraction.
            let _: () = unsafe { msg_send![self, addInteraction: &*interaction] };
            *menu = Some(interaction);
        }
        menu.clone()
    }

    /// Shows the edit menu over the selection.
    #[allow(deprecated)]
    fn show_menu(&self) {
        let Some(rect) = self.selection_rect() else {
            return;
        };
        if !self.isFirstResponder() && !self.becomeFirstResponder() {
            return;
        }
        match (self.edit_menu(), AnyClass::get(c"UIEditMenuConfiguration")) {
            (Some(menu), Some(configuration)) => {
                let source = CGPoint::new(rect.origin.x + rect.size.width / 2.0, rect.origin.y);
                // SAFETY: UIEditMenuConfiguration's factory and the
                // interaction's `presentEditMenuWithConfiguration:`.
                unsafe {
                    let config: Retained<AnyObject> = msg_send![
                        configuration,
                        configurationWithIdentifier: Option::<&AnyObject>::None,
                        sourcePoint: source
                    ];
                    let _: () = msg_send![&*menu, presentEditMenuWithConfiguration: &*config];
                }
            }
            _ => {
                objc2_ui_kit::UIMenuController::sharedMenuController(self.mtm())
                    .showMenuFromView_rect(self, rect);
            }
        }
    }

    #[allow(deprecated)]
    fn hide_menu(&self) {
        if let Some(menu) = self.ivars().edit_menu.borrow().as_ref() {
            // SAFETY: UIEditMenuInteraction's `dismissMenu`.
            let _: () = unsafe { msg_send![&**menu, dismissMenu] };
        } else if AnyClass::get(c"UIEditMenuInteraction").is_none() {
            objc2_ui_kit::UIMenuController::sharedMenuController(self.mtm()).hideMenuFromView(self);
        }
    }

    #[allow(deprecated)]
    fn menu_visible(&self) -> bool {
        if self.ivars().edit_menu.borrow().is_some() {
            return self.ivars().menu_shown.get();
        }
        objc2_ui_kit::UIMenuController::sharedMenuController(self.mtm()).isMenuVisible()
    }
}

/// Turns the own selection of the texts in `area` on or off.
fn update_texts(area: &UIView) {
    for child in area.subviews().iter() {
        match child.downcast_ref::<TextView>() {
            Some(text) => text.update_own_selection(),
            None => update_texts(&child),
        }
    }
}
