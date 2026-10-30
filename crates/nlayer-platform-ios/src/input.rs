//! Text inputs: a `UITextField` (one line) or a text area (several).
//!
//! Extracted from `nsh-platform-ios/src/input.rs` and cut to `value`,
//! `onChange`, `placeholder`, `disabled`, `secureTextEntry`, `keyboardType`,
//! selection and composition. Submit, accessories and paste reporting are
//! not copied. Marked text stays in the editor until the composition ends,
//! so an in-progress input method is not sent to JS as a change and a value
//! from JS does not replace it.

#![cfg_attr(all(not(target_os = "ios"), not(test)), allow(dead_code))]
use nlayer_core::KeyboardType;
use nlayer_core::geometry::{Bounds, Points};
use nlayer_core::style::TextAlign;

/// Text the field shows, and which engine values are still current.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FieldText {
    /// Edits this field has reported. A value with a smaller count is stale.
    reported: u32,
    /// The last text shown or reported.
    known: String,
    /// A value from JS that arrived while marked text was on screen.
    pending: Option<(String, u32)>,
    /// The last composition reported (`None` when none is in progress).
    composition: Option<String>,
    /// The last selection reported, in UTF-16 units.
    selection: Option<(u32, u32)>,
}

/// What to do with the field after an edit or a value from JS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TextUpdate {
    /// The user committed `text`. The field already shows it.
    Report(String),
    /// Replace the field's text. A value from JS, or one deferred until a
    /// composition ended without changing the text.
    Show(String),
}

impl FieldText {
    /// The user edited the field. `composing` means marked text is still up:
    /// that edit is not a change, and a deferred value keeps waiting.
    pub(crate) fn edited(&mut self, text: &str, composing: bool) -> Option<TextUpdate> {
        if composing {
            return None;
        }
        if text != self.known {
            self.known = text.to_owned();
            self.reported = self.reported.wrapping_add(1);
            self.pending = None;
            return Some(TextUpdate::Report(self.known.clone()));
        }
        let (text, count) = self.pending.take()?;
        self.show(&text, count)
    }

    /// JS set the text. While a composition is up, the value waits.
    pub(crate) fn set_value(
        &mut self,
        text: &str,
        count: u32,
        composing: bool,
    ) -> Option<TextUpdate> {
        if composing {
            self.pending = Some((text.to_owned(), count));
            return None;
        }
        self.show(text, count)
    }

    /// Records the composition `marked`; returns the change to report
    /// (`composing`, text) if it changed.
    pub(crate) fn compose(&mut self, marked: Option<String>) -> Option<(bool, String)> {
        if self.composition == marked {
            return None;
        }
        self.composition.clone_from(&marked);
        match marked {
            Some(text) => Some((true, text)),
            None => Some((false, String::new())),
        }
    }

    /// The text the engine last saw (reported or set from JS).
    #[cfg(target_os = "ios")]
    pub(crate) fn known_text(&self) -> &str {
        &self.known
    }

    /// Records a selection; returns it if it differs from the last one.
    pub(crate) fn selected(&mut self, start: u32, end: u32) -> Option<(u32, u32)> {
        if self.selection == Some((start, end)) {
            return None;
        }
        self.selection = Some((start, end));
        self.selection
    }

    fn show(&mut self, text: &str, count: u32) -> Option<TextUpdate> {
        if count < self.reported || text == self.known {
            return None;
        }
        self.known = text.to_owned();
        Some(TextUpdate::Show(self.known.clone()))
    }
}

/// `UIKeyboardType` for `keyboard`. `text` is the default keyboard, `number`
/// a digit pad, `phone` the phone pad and `email` the address keyboard.
pub(crate) fn keyboard_type(keyboard: KeyboardType) -> isize {
    match keyboard {
        KeyboardType::Text => 0,
        KeyboardType::Number => 4,
        KeyboardType::Phone => 5,
        KeyboardType::Email => 7,
    }
}

/// `NSTextAlignment`. `Auto` is natural, so it follows the writing direction.
pub(crate) fn text_alignment(align: TextAlign) -> isize {
    match align {
        TextAlign::Left => 0,
        TextAlign::Center => 1,
        TextAlign::Right => 2,
        TextAlign::Justify => 3,
        TextAlign::Auto => 4,
    }
}

/// The text an input method is composing in `view` (its marked text), or
/// `None` while no composition is in progress.
#[cfg(target_os = "ios")]
pub(crate) fn marked_text(view: &objc2_ui_kit::UIView) -> Option<String> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    // SAFETY: UITextField and UITextView adopt UITextInput.
    unsafe {
        let range: Option<Retained<AnyObject>> = msg_send![view, markedTextRange];
        let range = range?;
        let text: Option<Retained<objc2_foundation::NSString>> =
            msg_send![view, textInRange: &*range];
        text.map(|t| t.to_string()).filter(|t| !t.is_empty())
    }
}

/// The selection of `view`, in UTF-16 units.
#[cfg(target_os = "ios")]
pub(crate) fn selection(view: &objc2_ui_kit::UIView) -> Option<(u32, u32)> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_ui_kit::UITextPosition;
    // SAFETY: UITextField and UITextView adopt UITextInput.
    unsafe {
        let range: Option<Retained<AnyObject>> = msg_send![view, selectedTextRange];
        let range = range?;
        let from: Retained<UITextPosition> = msg_send![&*range, start];
        let to: Retained<UITextPosition> = msg_send![&*range, end];
        let begin: Retained<UITextPosition> = msg_send![view, beginningOfDocument];
        let start: isize = msg_send![view, offsetFromPosition: &*begin, toPosition: &*from];
        let end: isize = msg_send![view, offsetFromPosition: &*begin, toPosition: &*to];
        Some((start.max(0) as u32, end.max(0) as u32))
    }
}

/// Selects `start..end` (UTF-16 units, clamped to the text) of `view`.
/// `UITextField` and `UITextView` both adopt `UITextInput`.
#[cfg(target_os = "ios")]
pub(crate) fn set_selection(view: &objc2_ui_kit::UIView, start: u32, end: u32) {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    // SAFETY: UITextInput's documented object methods; the positions and
    // range are UIKit's.
    unsafe {
        let begin: Retained<AnyObject> = msg_send![view, beginningOfDocument];
        let last: Retained<AnyObject> = msg_send![view, endOfDocument];
        let length: isize = msg_send![view, offsetFromPosition: &*begin, toPosition: &*last];
        let (a, b) = (
            (start as isize).min(length),
            (end.max(start) as isize).min(length),
        );
        let from: Option<Retained<AnyObject>> =
            msg_send![view, positionFromPosition: &*begin, offset: a];
        let to: Option<Retained<AnyObject>> =
            msg_send![view, positionFromPosition: &*begin, offset: b];
        let (Some(from), Some(to)) = (from, to) else {
            return;
        };
        let range: Option<Retained<AnyObject>> =
            msg_send![view, textRangeFromPosition: &*from, toPosition: &*to];
        let _: () = msg_send![view, setSelectedTextRange: range.as_deref()];
    }
}

/// Insets that put the text in `content` inside a field of `width` by `height`.
/// Top, left, bottom, right. A content box past the field clamps to zero.
pub(crate) fn content_insets(width: f32, height: f32, content: Bounds<Points>) -> [f32; 4] {
    let top = content.origin.y.0;
    let left = content.origin.x.0;
    let bottom = (height - content.origin.y.0 - content.size.height.0).max(0.0);
    let right = (width - content.origin.x.0 - content.size.width.0).max(0.0);
    [top, left, bottom, right]
}

#[cfg(target_os = "ios")]
mod field {
    use std::cell::{Cell, RefCell};

    use nlayer_core::InputConfig;
    use nlayer_core::geometry::{Bounds, Points};
    use nlayer_core::style::Color;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, ProtocolObject};
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, ffi, msg_send, sel};
    use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
    use objc2_foundation::NSString;
    use objc2_ui_kit::{
        UIColor, UIControl, UIControlEvents, UIEdgeInsets, UIResponder, UISemanticContentAttribute,
        UITextBorderStyle, UITextField, UITextFieldDelegate, UIView,
    };

    use super::{FieldText, TextUpdate, content_insets, keyboard_type, text_alignment};
    use crate::text::ui_font;

    pub(crate) struct FieldIvars {
        insets: Cell<UIEdgeInsets>,
        text: RefCell<FieldText>,
        /// Color from the text style. An `activeStyle` / `hoverStyle` /
        /// `focusStyle` color replaces it while that state wins.
        color: Cell<Color>,
        /// `keyboardType`. Return resigns text and email only.
        keyboard: Cell<nlayer_core::KeyboardType>,
    }

    define_class!(
        /// One line of text, inset to the node's content box.
        #[unsafe(super(UITextField, UIControl, UIView, UIResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = FieldIvars]
        pub(crate) struct Field;

        impl Field {
            #[unsafe(method(textRectForBounds:))]
            fn text_rect(&self, bounds: CGRect) -> CGRect {
                inset(bounds, self.ivars().insets.get())
            }

            #[unsafe(method(editingRectForBounds:))]
            fn editing_rect(&self, bounds: CGRect) -> CGRect {
                inset(bounds, self.ivars().insets.get())
            }

            #[unsafe(method(placeholderRectForBounds:))]
            fn placeholder_rect(&self, bounds: CGRect) -> CGRect {
                inset(bounds, self.ivars().insets.get())
            }
        }
    );

    impl Field {
        pub(crate) fn new(
            mtm: MainThreadMarker,
            relay: &ProtocolObject<dyn UITextFieldDelegate>,
        ) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(FieldIvars {
                insets: Cell::new(UIEdgeInsets {
                    top: 0.0,
                    left: 0.0,
                    bottom: 0.0,
                    right: 0.0,
                }),
                text: RefCell::new(FieldText::default()),
                color: Cell::new(Color::BLACK),
                keyboard: Cell::new(nlayer_core::KeyboardType::Text),
            });
            // SAFETY: UITextField's superclass initializer. The field is main-thread only.
            let this: Retained<Self> = unsafe { msg_send![super(this), init] };
            this.setBorderStyle(UITextBorderStyle::None);
            // SAFETY: the relay implements `UITextFieldDelegate` and the
            // target actions (`edited:`, `editingBegan:`, `editingEnded:`).
            unsafe {
                this.setDelegate(Some(relay));
                this.addTarget_action_forControlEvents(
                    Some(relay.as_ref()),
                    sel!(edited:),
                    UIControlEvents::EditingChanged,
                );
                this.addTarget_action_forControlEvents(
                    Some(relay.as_ref()),
                    sel!(editingBegan:),
                    UIControlEvents::EditingDidBegin,
                );
                this.addTarget_action_forControlEvents(
                    Some(relay.as_ref()),
                    sel!(editingEnded:),
                    UIControlEvents::EditingDidEnd,
                );
                this.addTarget_action_forControlEvents(
                    Some(relay.as_ref()),
                    sel!(returned:),
                    UIControlEvents::EditingDidEndOnExit,
                );
            }
            this
        }

        pub(crate) fn configure(&self, config: &InputConfig) {
            self.setFont(Some(&ui_font(&config.style)));
            self.ivars().color.set(config.style.color);
            self.apply_color(None);
            self.setPlaceholder(Some(&NSString::from_str(&config.placeholder)));
            self.setEnabled(!config.disabled);
            self.setSemanticContentAttribute(if config.rtl {
                UISemanticContentAttribute::ForceRightToLeft
            } else {
                UISemanticContentAttribute::ForceLeftToRight
            });
            let keyboard = keyboard_type(config.keyboard);
            self.ivars().keyboard.set(config.keyboard);
            let align = text_alignment(config.align);
            // SAFETY: UITextField adopts UITextInputTraits. UIKit implements
            // those setters by forwarding, so the debug signature check
            // cannot find them on the class. `textAlignment` is a real
            // UITextField method, so the checked send is fine.
            unsafe {
                send_trait(self, sel!(setSecureTextEntry:), config.secure);
                send_trait(self, sel!(setKeyboardType:), keyboard);
                let _: () = msg_send![self, setTextAlignment: align];
            }
            // A field that is already editing keeps the previous keyboard until reloaded.
            if self.isFirstResponder() {
                self.reloadInputViews();
            }
            // Secure entry resets the font. Set it again after the traits.
            self.setFont(Some(&ui_font(&config.style)));
        }

        pub(crate) fn set_value(&self, text: &str, count: u32) {
            let update = self
                .ivars()
                .text
                .borrow_mut()
                .set_value(text, count, self.composing());
            if let Some(TextUpdate::Show(text)) = update {
                self.setText(Some(&NSString::from_str(&text)));
            }
        }

        pub(crate) fn set_content(&self, content: Bounds<Points>) {
            let size = self.bounds().size;
            let [top, left, bottom, right] =
                content_insets(size.width as f32, size.height as f32, content);
            self.ivars().insets.set(UIEdgeInsets {
                top: top as CGFloat,
                left: left as CGFloat,
                bottom: bottom as CGFloat,
                right: right as CGFloat,
            });
            self.setNeedsLayout();
        }

        /// `color` is the winning state's text color, or the field's own when `None`.
        pub(crate) fn set_override_color(&self, color: Option<Color>) {
            self.apply_color(color);
        }

        /// The user's edit, if it commits a change or a deferred value lands.
        pub(crate) fn edited(&self) -> Option<TextUpdate> {
            let text = self.string();
            let update = self
                .ivars()
                .text
                .borrow_mut()
                .edited(&text, self.composing());
            if let Some(TextUpdate::Show(text)) = &update {
                self.setText(Some(&NSString::from_str(text)));
            }
            update
        }

        pub(crate) fn shown(&self) -> String {
            self.string()
        }

        pub(crate) fn placeholder_text(&self) -> String {
            self.placeholder()
                .map(|text| text.to_string())
                .unwrap_or_default()
        }

        pub(crate) fn secure(&self) -> bool {
            // SAFETY: `isSecureTextEntry`, forwarded by UITextField. `msg_send!`
            // rejects it: the method is declared on the protocol, not the class.
            let bit: i8 = unsafe { recv_trait(self, sel!(isSecureTextEntry)) };
            bit != 0
        }

        pub(crate) fn keyboard_name(&self) -> &'static str {
            // SAFETY: `keyboardType`, same protocol as [`Self::secure`].
            let kind: isize = unsafe { recv_trait(self, sel!(keyboardType)) };
            match kind {
                4 => "number",
                5 => "phone",
                7 => "email",
                _ => "text",
            }
        }

        pub(crate) fn disabled(&self) -> bool {
            !self.isEnabled()
        }

        /// text and email Return resigns. number and phone have no Return.
        pub(crate) fn resigns_on_return(&self) -> bool {
            nlayer_core::return_resigns(self.ivars().keyboard.get())
        }

        fn string(&self) -> String {
            UITextField::text(self)
                .map(|text| text.to_string())
                .unwrap_or_default()
        }

        /// Reports a change of the composition, if `marked` differs from
        /// the last one.
        pub(crate) fn compose(&self, marked: Option<String>) -> Option<(bool, String)> {
            self.ivars().text.borrow_mut().compose(marked)
        }

        /// Reports a selection if it differs from the last one.
        pub(crate) fn selected(&self, start: u32, end: u32) -> Option<(u32, u32)> {
            self.ivars().text.borrow_mut().selected(start, end)
        }

        /// A composition change seen with the text the engine already has
        /// (a selection callback that unmarked without an edit).
        pub(crate) fn compose_if_known(&self, marked: Option<String>) -> Option<(bool, String)> {
            let mut text = self.ivars().text.borrow_mut();
            if self.shown() != text.known_text() {
                return None;
            }
            text.compose(marked)
        }

        /// The marked text (the composition in progress), if any.
        pub(crate) fn composed(&self) -> Option<String> {
            super::marked_text(self)
        }

        fn composing(&self) -> bool {
            // SAFETY: UITextField adopts UITextInput. The range is nil when nothing is marked.
            let range: Option<Retained<AnyObject>> = unsafe { msg_send![self, markedTextRange] };
            range.is_some()
        }

        fn apply_color(&self, color: Option<Color>) {
            let color = color.unwrap_or_else(|| self.ivars().color.get());
            let (r, g, b, a) = color.to_f64();
            self.setTextColor(Some(&UIColor::colorWithRed_green_blue_alpha(r, g, b, a)));
        }
    }

    fn inset(rect: CGRect, insets: UIEdgeInsets) -> CGRect {
        CGRect::new(
            CGPoint::new(rect.origin.x + insets.left, rect.origin.y + insets.top),
            CGSize::new(
                (rect.size.width - insets.left - insets.right).max(0.0),
                (rect.size.height - insets.top - insets.bottom).max(0.0),
            ),
        )
    }

    /// One-argument `UITextInputTraits` setter. See [`Field::configure`].
    ///
    /// # Safety
    ///
    /// `sel` is a setter the field forwards, and `value` matches its argument.
    unsafe fn send_trait<A: Copy>(view: &Field, sel: objc2::runtime::Sel, value: A) {
        unsafe {
            let send: unsafe extern "C-unwind" fn(&Field, objc2::runtime::Sel, A) =
                std::mem::transmute(ffi::objc_msgSend as unsafe extern "C-unwind" fn());
            send(view, sel, value);
        }
    }

    /// A zero-argument `UITextInputTraits` getter. See [`Field::secure`].
    ///
    /// # Safety
    ///
    /// `sel` is a getter the field forwards, and `R` matches its return.
    unsafe fn recv_trait<R: Copy>(view: &Field, sel: objc2::runtime::Sel) -> R {
        unsafe {
            let send: unsafe extern "C-unwind" fn(&Field, objc2::runtime::Sel) -> R =
                std::mem::transmute(ffi::objc_msgSend as unsafe extern "C-unwind" fn());
            send(view, sel)
        }
    }
}

#[cfg(target_os = "ios")]
pub(crate) use field::Field;

#[cfg(target_os = "ios")]
pub(crate) use area::AreaInput;

#[cfg(test)]
mod tests {
    use nlayer_core::KeyboardType;
    use nlayer_core::geometry::{Bounds, Point, Points, Size};
    use nlayer_core::style::TextAlign;

    use super::{FieldText, TextUpdate, content_insets, keyboard_type, text_alignment};

    #[test]
    fn a_stale_value_does_not_replace_a_newer_edit() {
        let mut field = FieldText::default();
        assert_eq!(
            field.set_value("a", 0, false),
            Some(TextUpdate::Show("a".to_owned()))
        );
        assert_eq!(field.set_value("a", 0, false), None);
        assert_eq!(
            field.edited("ab", false),
            Some(TextUpdate::Report("ab".to_owned()))
        );
        assert_eq!(field.edited("ab", false), None);
        assert_eq!(field.set_value("a", 0, false), None);
        assert_eq!(
            field.set_value("AB", 1, false),
            Some(TextUpdate::Show("AB".to_owned()))
        );
    }

    #[test]
    fn a_composition_is_not_a_change_and_a_value_waits_for_it() {
        let mut field = FieldText::default();
        field.set_value("a", 0, false);
        assert_eq!(field.edited("a`", true), None);
        assert_eq!(field.set_value("b", 0, true), None);
        assert_eq!(
            field.edited("a中", false),
            Some(TextUpdate::Report("a中".to_owned()))
        );

        let mut field = FieldText::default();
        field.set_value("a", 0, false);
        assert_eq!(field.set_value("b", 0, true), None);
        assert_eq!(
            field.edited("a", false),
            Some(TextUpdate::Show("b".to_owned()))
        );
    }

    #[test]
    fn compositions_and_selections_are_reported_once() {
        let mut field = FieldText::default();
        assert_eq!(
            field.compose(Some("zhong".to_owned())),
            Some((true, "zhong".to_owned()))
        );
        assert_eq!(field.compose(Some("zhong".to_owned())), None);
        assert_eq!(
            field.compose(Some("中".to_owned())),
            Some((true, "中".to_owned()))
        );
        assert_eq!(field.compose(None), Some((false, String::new())));
        assert_eq!(field.compose(None), None);
        assert_eq!(field.selected(1, 3), Some((1, 3)));
        assert_eq!(field.selected(1, 3), None);
        assert_eq!(field.selected(2, 2), Some((2, 2)));
    }

    #[test]
    fn text_sits_in_the_content_box() {
        let content = Bounds {
            origin: Point::new(Points(8.0), Points(6.0)),
            size: Size::new(Points(80.0), Points(20.0)),
        };
        assert_eq!(content_insets(100.0, 40.0, content), [6.0, 8.0, 14.0, 12.0]);
        assert_eq!(
            keyboard_type(KeyboardType::Email),
            7,
            "UIKeyboardTypeEmailAddress"
        );
        assert_eq!(text_alignment(TextAlign::Auto), 4, "NSTextAlignmentNatural");
    }
}

#[cfg(target_os = "ios")]
mod area {
    use std::cell::{Cell, RefCell};

    use nlayer_core::InputConfig;
    use nlayer_core::geometry::{Bounds, Points};
    use nlayer_core::style::Color;
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2::{MainThreadMarker, MainThreadOnly, ffi, msg_send, sel};
    use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
    use objc2_foundation::NSString;
    use objc2_ui_kit::{
        NSTextAlignment, UIEdgeInsets, UILabel, UISemanticContentAttribute, UITextView,
        UITextViewDelegate,
    };

    use super::{FieldText, TextUpdate, keyboard_type, text_alignment};
    use crate::text::ui_font;

    /// A text area: `UITextView` with the text inset to the content box and
    /// a label for the placeholder (the text view has none).
    pub(crate) struct AreaInput {
        view: Retained<UITextView>,
        placeholder: Retained<UILabel>,
        insets: Cell<UIEdgeInsets>,
        text: RefCell<FieldText>,
        color: Cell<Color>,
        keyboard: Cell<nlayer_core::KeyboardType>,
        empty_text: Cell<bool>,
    }

    impl AreaInput {
        pub(crate) fn new(
            mtm: MainThreadMarker,
            relay: &ProtocolObject<dyn UITextViewDelegate>,
        ) -> Self {
            let view = UITextView::initWithFrame(UITextView::alloc(mtm), CGRect::ZERO);
            // SAFETY: the caller's relay implements `UITextViewDelegate`.
            unsafe { view.setDelegate(Some(relay)) };
            view.textContainer().setLineFragmentPadding(0.0);
            view.setScrollEnabled(true);
            let placeholder = UILabel::new(mtm);
            placeholder.setUserInteractionEnabled(false);
            placeholder.setNumberOfLines(0);
            view.addSubview(&placeholder);
            Self {
                view,
                placeholder,
                insets: Cell::new(UIEdgeInsets {
                    top: 0.0,
                    left: 0.0,
                    bottom: 0.0,
                    right: 0.0,
                }),
                text: RefCell::new(FieldText::default()),
                color: Cell::new(Color::BLACK),
                keyboard: Cell::new(nlayer_core::KeyboardType::Text),
                empty_text: Cell::new(true),
            }
        }

        pub(crate) fn view(&self) -> &UITextView {
            &self.view
        }

        pub(crate) fn configure(&self, config: &InputConfig) {
            let font = ui_font(&config.style);
            self.view.setFont(Some(&font));
            self.color.set(config.style.color);
            self.apply_color(None);
            self.view.setEditable(!config.disabled);
            self.view.setSemanticContentAttribute(if config.rtl {
                UISemanticContentAttribute::ForceRightToLeft
            } else {
                UISemanticContentAttribute::ForceLeftToRight
            });
            self.keyboard.set(config.keyboard);
            let align = text_alignment(config.align);
            // SAFETY: UITextView redeclares `textAlignment`; the value is an
            // `NSTextAlignment`.
            unsafe {
                let _: () = msg_send![&*self.view, setTextAlignment: align];
                send_trait::<isize>(
                    &self.view,
                    sel!(setKeyboardType:),
                    keyboard_type(config.keyboard),
                );
                send_trait::<bool>(&self.view, sel!(setSecureTextEntry:), config.secure);
            }
            // SAFETY: plain setters.
            unsafe {
                self.placeholder.setFont(Some(&font));
                let (r, g, b, a) = config.style.color.to_f64();
                self.placeholder.setTextColor(Some(
                    &objc2_ui_kit::UIColor::colorWithRed_green_blue_alpha(r, g, b, a * 0.4),
                ));
            }
            self.placeholder
                .setText(Some(&NSString::from_str(&config.placeholder)));
            self.placeholder.setTextAlignment(alignment(align));
            self.show_placeholder();
            if self.view.isFirstResponder() {
                self.view.reloadInputViews();
            }
        }

        pub(crate) fn set_value(&self, text: &str, count: u32) {
            let update = self
                .text
                .borrow_mut()
                .set_value(text, count, self.composing());
            if let Some(TextUpdate::Show(text)) = update {
                self.view.setText(Some(&NSString::from_str(&text)));
                self.show_placeholder();
            }
        }

        pub(crate) fn set_content(&self, content: Bounds<Points>) {
            let bounds = self.view.bounds().size;
            let insets = UIEdgeInsets {
                top: content.origin.y.0 as CGFloat,
                left: content.origin.x.0 as CGFloat,
                bottom: (bounds.height as f32 - content.origin.y.0 - content.size.height.0).max(0.0)
                    as CGFloat,
                right: (bounds.width as f32 - content.origin.x.0 - content.size.width.0).max(0.0)
                    as CGFloat,
            };
            self.insets.set(insets);
            self.view.setTextContainerInset(insets);
            self.place_placeholder(content);
        }

        fn place_placeholder(&self, content: Bounds<Points>) {
            self.placeholder.setFrame(CGRect::new(
                CGPoint::new(content.origin.x.0 as CGFloat, content.origin.y.0 as CGFloat),
                CGSize::new(
                    content.size.width.0 as CGFloat,
                    content.size.height.0 as CGFloat,
                ),
            ));
            self.placeholder.sizeToFit();
            let size = self.placeholder.frame().size;
            self.placeholder.setFrame(CGRect::new(
                CGPoint::new(content.origin.x.0 as CGFloat, content.origin.y.0 as CGFloat),
                CGSize::new(content.size.width.0 as CGFloat, size.height),
            ));
        }

        pub(crate) fn set_override_color(&self, color: Option<Color>) {
            self.apply_color(color);
        }

        pub(crate) fn edited(&self) -> Option<TextUpdate> {
            let text = self.shown();
            let update = self.text.borrow_mut().edited(&text, self.composing());
            if let Some(TextUpdate::Show(text)) = &update {
                self.view.setText(Some(&NSString::from_str(text)));
            }
            self.show_placeholder();
            update
        }

        pub(crate) fn shown(&self) -> String {
            self.view.text().to_string()
        }

        pub(crate) fn placeholder_text(&self) -> String {
            self.placeholder
                .text()
                .map(|t| t.to_string())
                .unwrap_or_default()
        }

        pub(crate) fn secure(&self) -> bool {
            // SAFETY: `isSecureTextEntry`, forwarded by the text view.
            let bit: bool = unsafe { recv_trait(&self.view, sel!(isSecureTextEntry)) };
            bit
        }

        pub(crate) fn keyboard_name(&self) -> &'static str {
            match self.keyboard.get() {
                nlayer_core::KeyboardType::Number => "number",
                nlayer_core::KeyboardType::Phone => "phone",
                nlayer_core::KeyboardType::Email => "email",
                nlayer_core::KeyboardType::Text => "text",
            }
        }

        pub(crate) fn disabled(&self) -> bool {
            !self.view.isEditable()
        }

        pub(crate) fn compose(&self, marked: Option<String>) -> Option<(bool, String)> {
            self.text.borrow_mut().compose(marked)
        }

        pub(crate) fn selected(&self, start: u32, end: u32) -> Option<(u32, u32)> {
            self.text.borrow_mut().selected(start, end)
        }

        /// A composition change seen with the text the engine already has.
        pub(crate) fn compose_if_known(&self, marked: Option<String>) -> Option<(bool, String)> {
            let mut text = self.text.borrow_mut();
            if self.shown() != text.known_text() {
                return None;
            }
            text.compose(marked)
        }

        fn composing(&self) -> bool {
            super::marked_text(&self.view).is_some()
        }

        fn apply_color(&self, color: Option<Color>) {
            let color = color.unwrap_or_else(|| self.color.get());
            let (r, g, b, a) = color.to_f64();
            self.view
                .setTextColor(Some(&objc2_ui_kit::UIColor::colorWithRed_green_blue_alpha(
                    r, g, b, a,
                )));
        }

        fn show_placeholder(&self) {
            let empty = self.shown().is_empty();
            self.empty_text.set(empty);
            self.placeholder.setHidden(!empty);
        }
    }

    /// An `NSTextAlignment` for the core's alignment.
    fn alignment(align: isize) -> NSTextAlignment {
        // The core's values are UIKit's: left 0, center 1, right 2, justify 3, natural 4.
        NSTextAlignment(align)
    }

    /// One-argument `UITextInputTraits` setter (protocol methods are not on
    /// the class, so the checked `msg_send!` cannot find them).
    ///
    /// # Safety
    ///
    /// `sel` is a setter the view forwards, and `value` matches its argument.
    unsafe fn send_trait<A: Copy>(view: &UITextView, sel: objc2::runtime::Sel, value: A) {
        unsafe {
            let send: unsafe extern "C-unwind" fn(&UITextView, objc2::runtime::Sel, A) =
                std::mem::transmute(ffi::objc_msgSend as unsafe extern "C-unwind" fn());
            send(view, sel, value);
        }
    }

    /// A zero-argument `UITextInputTraits` getter.
    ///
    /// # Safety
    ///
    /// `sel` is a getter the view forwards, and `R` matches its return.
    unsafe fn recv_trait<R: Copy>(view: &UITextView, sel: objc2::runtime::Sel) -> R {
        unsafe {
            let send: unsafe extern "C-unwind" fn(&UITextView, objc2::runtime::Sel) -> R =
                std::mem::transmute(ffi::objc_msgSend as unsafe extern "C-unwind" fn());
            send(view, sel)
        }
    }
}
