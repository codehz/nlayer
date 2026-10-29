//! A single-line text field.
//!
//! Extracted from `nsh-platform-ios/src/input.rs` (the text rect inset, secure
//! entry and keyboard type) and cut to v1: `value`, `onChange`, `placeholder`,
//! `disabled`, `secureTextEntry` and `keyboardType`. Multiline, submit,
//! accessories, selection and composition events are not copied. Marked text
//! stays on the field until the composition ends, so an in-progress input
//! method is not sent to JS and a value from JS does not replace it.

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
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, ffi, msg_send, sel};
    use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
    use objc2_foundation::NSString;
    use objc2_ui_kit::{
        UIColor, UIControl, UIControlEvents, UIEdgeInsets, UIResponder, UISemanticContentAttribute,
        UITextBorderStyle, UITextField, UIView,
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
        pub(crate) fn new(mtm: MainThreadMarker, relay: &AnyObject) -> Retained<Self> {
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
            // SAFETY: the relay implements `edited:`, `editingBegan:` and `editingEnded:`.
            unsafe {
                this.addTarget_action_forControlEvents(
                    Some(relay),
                    sel!(edited:),
                    UIControlEvents::EditingChanged,
                );
                this.addTarget_action_forControlEvents(
                    Some(relay),
                    sel!(editingBegan:),
                    UIControlEvents::EditingDidBegin,
                );
                this.addTarget_action_forControlEvents(
                    Some(relay),
                    sel!(editingEnded:),
                    UIControlEvents::EditingDidEnd,
                );
                this.addTarget_action_forControlEvents(
                    Some(relay),
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
