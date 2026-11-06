//! A view that draws the `CTLine`s the engine already laid out.
//!
//! Selectable text (`userSelect: "text"`, and not inside a selectable view)
//! also adopts non-editable `UITextInput`, so a `UITextInteraction` supplies
//! the long press, loupe, handles and edit menu. The highlight of a
//! selectable view is drawn here too.

use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};

use crate::{
    text::{CoreTextEngine, CoreTextLayout},
    text_selection::{self, Selection},
};
use nlayer_core::{
    NodeId,
    geometry::{Bounds, Points, point},
    style::Color,
    text::{AttributedText, TextEngine, TextLayout},
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_core_foundation::{CGAffineTransform, CGFloat, CGPoint, CGRect};
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSArray, NSComparisonResult, NSDictionary, NSRange, NSString};
use objc2_ui_kit::{
    NSWritingDirection, UIColor, UIEvent, UIGraphicsGetCurrentContext, UIKeyInput, UIResponder,
    UITextInput, UITextInputDelegate, UITextInputTokenizer, UITextInputTraits, UITextInteraction,
    UITextInteractionDelegate, UITextLayoutDirection, UITextPosition, UITextRange,
    UITextSelectionRect, UITextStorageDirection, UIView,
};

/// Storage for one text view. The class macro exposes it on the class.
pub(crate) struct TextViewIvars {
    pub(crate) layout: RefCell<Option<(TextLayout, Bounds<Points>)>>,
    pub(crate) text: RefCell<Option<Arc<AttributedText>>>,
    /// An `activeStyle` / `hoverStyle` color (`0xRRGGBBAA`): the whole text
    /// is drawn in it. `None` keeps each span's own color.
    pub(crate) color: Cell<Option<u32>>,
    pub(crate) selection: Selection,
    /// A selectable view's highlight: UTF-16 range and packed color.
    area_highlight: Cell<Option<((usize, usize), u32)>>,
    /// Mounted `pointerEvents`: 0 auto, 1 none, 2 box-none, 3 box-only.
    pointer: Cell<u8>,
}

define_class!(
    /// Draws finished CoreText lines. Selection, when the text is
    /// selectable, is UIKit's non-editable text interaction.
    #[unsafe(super(UIView, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TextViewIvars]
    pub(crate) struct TextView;

    impl TextView {
        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            false
        }

        #[unsafe(method_id(hitTest:withEvent:))]
        fn hit_test(&self, point: CGPoint, event: Option<&UIEvent>) -> Option<Retained<UIView>> {
            let mode = self.ivars().pointer.get();
            let hit: Option<Retained<UIView>> = if mode == 1 {
                None
            } else if mode == 3 {
                let bounds = self.bounds();
                let inside = point.x >= 0.0
                    && point.y >= 0.0
                    && point.x < bounds.size.width
                    && point.y < bounds.size.height;
                inside.then(|| Retained::from(self as &UIView))
            } else {
                let hit: Option<Retained<UIView>> =
                    unsafe { msg_send![super(self), hitTest: point, withEvent: event] };
                if mode == 2 {
                    let mine = self as *const Self as *const UIView;
                    hit.filter(|found| !std::ptr::eq(Retained::as_ptr(found), mine))
                } else {
                    hit
                }
            };
            hit
        }

        #[unsafe(method(canBecomeFirstResponder))]
        fn can_become_first_responder(&self) -> bool {
            self.selectable()
        }

        #[unsafe(method(didMoveToWindow))]
        fn did_move_to_window(&self) {
            // SAFETY: calling the superclass implementation.
            let _: () = unsafe { msg_send![super(self), didMoveToWindow] };
            self.update_own_selection();
        }

        #[unsafe(method(canPerformAction:withSender:))]
        fn can_perform_action(&self, action: Sel, sender: Option<&AnyObject>) -> bool {
            let editing = [
                sel!(cut:),
                sel!(paste:),
                sel!(delete:),
                sel!(toggleBoldface:),
                sel!(toggleItalics:),
                sel!(toggleUnderline:),
            ];
            if !self.selectable() || editing.contains(&action) {
                false
            } else if action == sel!(copy:) {
                self.has_selection()
            } else if action == sel!(selectAll:) {
                !self.all_selected() && self.text_length() > 0
            } else {
                // SAFETY: calling the superclass implementation.
                let can: bool =
                    unsafe { msg_send![super(self), canPerformAction: action, withSender: sender] };
                can
            }
        }

        #[unsafe(method(copy:))]
        fn copy(&self, _sender: Option<&AnyObject>) {
            if let Some(text) = self.selected_string() {
                text_selection::set_clipboard(&text);
            }
        }

        #[unsafe(method(selectAll:))]
        fn select_all(&self, _sender: Option<&AnyObject>) {
            let len = self.text_length();
            if len > 0 {
                self.change_selection(Some((0, len)));
            }
        }

        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            // SAFETY: calling the superclass implementation.
            let resigned: bool = unsafe { msg_send![super(self), resignFirstResponder] };
            if resigned {
                self.end_selection();
            }
            resigned
        }

        #[unsafe(method_id(inputView))]
        fn input_view(&self) -> Option<Retained<UIView>> {
            Some(self.sel_input_view())
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: CGRect) {
            let highlight = self.ivars().area_highlight.get().map(|((a, b), color)| {
                (color, self.range_rects(a, b))
            });
            let layout = self.ivars().layout.borrow();
            let Some((layout, content)) = layout.as_ref() else {
                return;
            };
            let Some(text) = layout.downcast::<CoreTextLayout>() else {
                return;
            };
            let Some(cx) = UIGraphicsGetCurrentContext() else {
                return;
            };
            let c = Some(&*cx);
            CGContext::save_g_state(c);
            if let Some((color, rects)) = &highlight {
                let fill = crate::text::cg_color(Color(*color));
                CGContext::set_fill_color_with_color(c, Some(&fill));
                for (rect, ..) in rects {
                    CGContext::fill_rect(c, *rect);
                }
            }
            // CoreText draws in a y-up space with the origin at the bottom
            // of the box.
            CGContext::set_text_matrix(
                c,
                CGAffineTransform {
                    a: 1.0,
                    b: 0.0,
                    c: 0.0,
                    d: 1.0,
                    tx: 0.0,
                    ty: 0.0,
                },
            );
            CGContext::translate_ctm(
                c,
                content.origin.x.0 as CGFloat,
                (content.origin.y.0 + content.size.height.0) as CGFloat,
            );
            CGContext::scale_ctm(c, 1.0, -1.0);
            let color = self.ivars().color.get();
            if color.is_some() {
                // SAFETY: no auxiliary info.
                unsafe { CGContext::begin_transparency_layer(c, None) };
            }
            for line in &text.lines {
                CGContext::set_text_position(c, line.origin.x, line.origin.y);
                // SAFETY: a valid line and the current context.
                unsafe { line.line.draw(&cx) };
            }
            if let Some(color) = color {
                let fill = crate::text::cg_color(Color(color));
                CGContext::set_blend_mode(c, objc2_core_graphics::CGBlendMode::SourceIn);
                CGContext::set_fill_color_with_color(c, Some(&fill));
                CGContext::fill_rect(c, CGContext::clip_bounding_box(c));
                CGContext::end_transparency_layer(c);
            }
            CGContext::restore_g_state(c);
        }
    }

    unsafe impl NSObjectProtocol for TextView {}

    unsafe impl UITextInputTraits for TextView {}

    unsafe impl UIKeyInput for TextView {
        #[unsafe(method(hasText))]
        fn key_has_text(&self) -> bool {
            self.text_length() > 0
        }

        #[unsafe(method(insertText:))]
        fn insert_text(&self, _text: &NSString) {}

        #[unsafe(method(deleteBackward))]
        fn delete_backward(&self) {}
    }

    unsafe impl UITextInput for TextView {
        #[unsafe(method_id(textInRange:))]
        fn text_in_range(&self, range: &UITextRange) -> Option<Retained<NSString>> {
            self.sel_text_in(range)
        }

        #[unsafe(method(replaceRange:withText:))]
        fn replace_range(&self, _range: &UITextRange, _text: &NSString) {}

        #[unsafe(method_id(selectedTextRange))]
        fn selected_text_range(&self) -> Option<Retained<UITextRange>> {
            self.sel_selected_range()
        }

        #[unsafe(method(setSelectedTextRange:))]
        fn set_selected_text_range(&self, range: Option<&UITextRange>) {
            self.sel_set_selected_range(range);
        }

        #[unsafe(method_id(markedTextRange))]
        fn marked_text_range(&self) -> Option<Retained<UITextRange>> {
            None
        }

        #[unsafe(method_id(markedTextStyle))]
        fn marked_text_style(
            &self,
        ) -> Option<Retained<NSDictionary<objc2_foundation::NSAttributedStringKey, AnyObject>>> {
            None
        }

        #[unsafe(method(setMarkedTextStyle:))]
        fn set_marked_text_style(
            &self,
            _style: Option<&NSDictionary<objc2_foundation::NSAttributedStringKey, AnyObject>>,
        ) {
        }

        #[unsafe(method(setMarkedText:selectedRange:))]
        fn set_marked_text(&self, _text: Option<&NSString>, _range: NSRange) {}

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {}

        #[unsafe(method_id(beginningOfDocument))]
        fn beginning_of_document(&self) -> Retained<UITextPosition> {
            self.sel_position(0)
        }

        #[unsafe(method_id(endOfDocument))]
        fn end_of_document(&self) -> Retained<UITextPosition> {
            self.sel_position(self.text_length())
        }

        #[unsafe(method_id(textRangeFromPosition:toPosition:))]
        fn text_range(
            &self,
            from: &UITextPosition,
            to: &UITextPosition,
        ) -> Option<Retained<UITextRange>> {
            Some(self.sel_range(from, to))
        }

        #[unsafe(method_id(positionFromPosition:offset:))]
        fn position_offset(
            &self,
            position: &UITextPosition,
            offset: isize,
        ) -> Option<Retained<UITextPosition>> {
            self.sel_moved(position, offset)
        }

        #[unsafe(method_id(positionFromPosition:inDirection:offset:))]
        fn position_direction(
            &self,
            position: &UITextPosition,
            direction: UITextLayoutDirection,
            offset: isize,
        ) -> Option<Retained<UITextPosition>> {
            self.sel_moved_in(position, direction, offset)
        }

        #[unsafe(method(comparePosition:toPosition:))]
        fn compare_position(
            &self,
            position: &UITextPosition,
            other: &UITextPosition,
        ) -> NSComparisonResult {
            match text_selection::offset(position).cmp(&text_selection::offset(other)) {
                std::cmp::Ordering::Less => NSComparisonResult::Ascending,
                std::cmp::Ordering::Equal => NSComparisonResult::Same,
                std::cmp::Ordering::Greater => NSComparisonResult::Descending,
            }
        }

        #[unsafe(method(offsetFromPosition:toPosition:))]
        fn offset_from(&self, from: &UITextPosition, to: &UITextPosition) -> isize {
            text_selection::offset(to) as isize - text_selection::offset(from) as isize
        }

        #[unsafe(method_id(inputDelegate))]
        fn input_delegate(&self) -> Option<Retained<ProtocolObject<dyn UITextInputDelegate>>> {
            self.sel_delegate()
        }

        #[unsafe(method(setInputDelegate:))]
        fn set_input_delegate(&self, delegate: Option<&ProtocolObject<dyn UITextInputDelegate>>) {
            self.sel_set_delegate(delegate);
        }

        #[unsafe(method_id(tokenizer))]
        fn tokenizer(&self) -> Retained<ProtocolObject<dyn UITextInputTokenizer>> {
            self.sel_tokenizer()
        }

        #[unsafe(method_id(positionWithinRange:farthestInDirection:))]
        fn farthest(
            &self,
            range: &UITextRange,
            direction: UITextLayoutDirection,
        ) -> Option<Retained<UITextPosition>> {
            Some(self.sel_farthest(range, direction))
        }

        #[unsafe(method_id(characterRangeByExtendingPosition:inDirection:))]
        fn extending(
            &self,
            position: &UITextPosition,
            direction: UITextLayoutDirection,
        ) -> Option<Retained<UITextRange>> {
            Some(self.sel_extend(position, direction))
        }

        #[unsafe(method(baseWritingDirectionForPosition:inDirection:))]
        fn writing_direction(
            &self,
            _position: &UITextPosition,
            _direction: UITextStorageDirection,
        ) -> NSWritingDirection {
            self.sel_direction()
        }

        #[unsafe(method(setBaseWritingDirection:forRange:))]
        fn set_writing_direction(&self, _direction: NSWritingDirection, _range: &UITextRange) {}

        #[unsafe(method(firstRectForRange:))]
        fn first_rect(&self, range: &UITextRange) -> CGRect {
            self.sel_first_rect(range)
        }

        #[unsafe(method(caretRectForPosition:))]
        fn caret_rect(&self, position: &UITextPosition) -> CGRect {
            self.sel_caret(position)
        }

        #[unsafe(method_id(selectionRectsForRange:))]
        fn selection_rects(&self, range: &UITextRange) -> Retained<NSArray<UITextSelectionRect>> {
            self.sel_rects(range)
        }

        #[unsafe(method_id(closestPositionToPoint:))]
        fn closest(&self, point: CGPoint) -> Option<Retained<UITextPosition>> {
            self.sel_closest(point, None)
        }

        #[unsafe(method_id(closestPositionToPoint:withinRange:))]
        fn closest_within(
            &self,
            point: CGPoint,
            range: &UITextRange,
        ) -> Option<Retained<UITextPosition>> {
            self.sel_closest(point, Some(range))
        }

        #[unsafe(method_id(characterRangeAtPoint:))]
        fn character_at(&self, point: CGPoint) -> Option<Retained<UITextRange>> {
            self.sel_character_at(point)
        }

        #[unsafe(method(isEditable))]
        fn is_editable(&self) -> bool {
            false
        }

        #[unsafe(method_id(textInputView))]
        fn text_input_view(&self) -> Retained<UIView> {
            self.retain().into_super()
        }
    }

    unsafe impl UITextInteractionDelegate for TextView {
        #[unsafe(method(interactionWillBegin:))]
        fn interaction_will_begin(&self, _interaction: &UITextInteraction) {
            if let Some(mounter) = crate::mount::current()
                && let Ok(mut mounter) = mounter.try_borrow_mut()
            {
                mounter.cancel_press_chain(self);
            }
        }
    }
);

impl TextView {
    pub(crate) fn new(mtm: objc2::MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TextViewIvars {
            layout: RefCell::new(None),
            text: RefCell::new(None),
            color: Cell::new(None),
            selection: Selection::default(),
            area_highlight: Cell::new(None),
            pointer: Cell::new(0),
        });
        // SAFETY: UIView's designated initializer.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setOpaque(false);
        this.setBackgroundColor(Some(&UIColor::clearColor()));
        this
    }
    pub(crate) fn set_pointer(&self, events: nlayer_core::PointerEvents) {
        self.ivars().pointer.set(events as u8);
    }

    pub(crate) fn set_text(
        &self,
        text: Arc<AttributedText>,
        layout: TextLayout,
        content: Bounds<Points>,
    ) {
        let (selectable, color) = (
            text.is_selectable() && !self.in_selectable_view(),
            text.selection_color,
        );
        let changed = self
            .ivars()
            .text
            .borrow()
            .as_ref()
            .is_none_or(|t| t.text != text.text);
        *self.ivars().text.borrow_mut() = Some(text);
        *self.ivars().layout.borrow_mut() = Some((layout, content));
        if selectable || self.ivars().selection.is_on() {
            self.set_selectable(selectable, color, changed);
        }
        self.setNeedsDisplay();
    }

    /// Paints every span in `color`, or restores each span's color.
    pub(crate) fn set_color(&self, color: Option<Color>) {
        let bits = color.map(|c| c.0);
        if self.ivars().color.replace(bits) != bits {
            self.setNeedsDisplay();
        }
    }

    pub(crate) fn attributed(&self) -> Option<Arc<AttributedText>> {
        self.ivars().text.borrow().clone()
    }

    fn selectable(&self) -> bool {
        self.ivars()
            .text
            .borrow()
            .as_ref()
            .is_some_and(|t| t.is_selectable())
            && !self.in_selectable_view()
    }

    /// Whether a view above selects this text with its subtree.
    pub(crate) fn in_selectable_view(&self) -> bool {
        let mut view = self.superview();
        while let Some(v) = view {
            if crate::selectable_view::is_selectable_area(&v) {
                return true;
            }
            view = v.superview();
        }
        false
    }

    /// Turns the text's own selection on or off after it moved into or out
    /// of a selectable view.
    pub(crate) fn update_own_selection(&self) {
        let Some((selectable, color)) = self
            .ivars()
            .text
            .borrow()
            .as_ref()
            .map(|t| (t.is_selectable(), t.selection_color))
        else {
            return;
        };
        if !selectable && !self.ivars().selection.is_on() {
            return;
        }
        let on = selectable && !self.in_selectable_view();
        if on != self.ivars().selection.is_on() {
            self.set_selectable(on, color, false);
        }
    }

    /// Highlights a UTF-16 range in `color`, or nothing.
    pub(crate) fn set_area_highlight(&self, highlight: Option<((usize, usize), Color)>) {
        let packed = highlight.map(|(range, color)| (range, color.0));
        if self.ivars().area_highlight.replace(packed) != packed {
            self.setNeedsDisplay();
        }
    }

    /// The pressable span under `point` (the view's coordinates), or the
    /// `href` there when the span has no press listener of its own.
    pub(crate) fn link_at(&self, at: CGPoint) -> Option<NodeId> {
        let layout = self.ivars().layout.borrow().clone()?;
        let text = self.attributed()?;
        let local = point(
            Points(at.x as f32 - layout.1.origin.x.0),
            Points(at.y as f32 - layout.1.origin.y.0),
        );
        let position = CoreTextEngine::default().hit_test(&layout.0, local);
        text.link_at(position.offset)
            .map(|link| link.node)
            .or_else(|| text.href_at(position.offset).map(|href| href.node))
    }

    /// The center of the span `start..end`, in the view's coordinates.
    pub(crate) fn span_point(&self, start: u32, end: u32) -> Option<CGPoint> {
        let (layout, content) = self.ivars().layout.borrow().clone()?;
        let mut rects = Vec::new();
        CoreTextEngine::default().range_rects(&layout, start..end, &mut rects);
        let rect = rects.first().copied()?;
        Some(CGPoint::new(
            (content.origin.x.0 + rect.origin.x.0 + rect.size.width.0 / 2.0) as CGFloat,
            (content.origin.y.0 + rect.origin.y.0 + rect.size.height.0 / 2.0) as CGFloat,
        ))
    }

    /// The span's box in the view's coordinates.
    pub(crate) fn span_frame(&self, start: u32, end: u32) -> Option<(f32, f32, f32, f32)> {
        let (layout, content) = self.ivars().layout.borrow().clone()?;
        let mut rects = Vec::new();
        CoreTextEngine::default().range_rects(&layout, start..end, &mut rects);
        let rect = rects.first().copied()?;
        Some((
            content.origin.x.0 + rect.origin.x.0,
            content.origin.y.0 + rect.origin.y.0,
            rect.size.width.0,
            rect.size.height.0,
        ))
    }
}

/// A long press at `point` in `view` starts text selection (the text is
/// selectable, or it sits in a selectable view and is not
/// `userSelect: "none"`), so the press must not also become `onLongPress`.
pub(crate) fn selection_takes_long_press(view: &UIView, point: CGPoint) -> bool {
    let Some(hit) = view.hitTest_withEvent(point, None) else {
        return false;
    };
    let mut current = Some(hit);
    let mut skipped = false;
    let mut selectable = false;
    while let Some(v) = current {
        if let Some(text) = v.downcast_ref::<TextView>() {
            match text.attributed().and_then(|t| t.selectable) {
                Some(false) => skipped = true,
                Some(true) => selectable = true,
                None => {}
            }
        }
        if !skipped && crate::selectable_view::is_selectable_area(&v) {
            return true;
        }
        current = v.superview();
    }
    selectable && !skipped
}
