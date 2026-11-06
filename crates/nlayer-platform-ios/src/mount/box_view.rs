//! Plain view whose hit testing follows `pointerEvents`.

use std::cell::Cell;

use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_core_foundation::CGPoint;
use objc2_ui_kit::{UIEvent, UIResponder, UIView};

pub(crate) struct BoxIvars {
    /// The view's own box is not a hit. Children still are.
    pass_box: Cell<bool>,
    /// Mounted `pointerEvents`: 0 auto, 1 none, 2 box-none, 3 box-only.
    pointer: Cell<u8>,
}

define_class!(
    /// A plain view. A dialog popup sets [`BoxView::set_pass_box`]: its box
    /// would otherwise take every press that should hit the backdrop behind
    /// it (`flex-1` centering a smaller card).
    #[unsafe(super(UIView, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = BoxIvars]
    pub(crate) struct BoxView;

    unsafe impl NSObjectProtocol for BoxView {}

    impl BoxView {
        #[unsafe(method_id(hitTest:withEvent:))]
        fn hit_test(&self, point: CGPoint, event: Option<&UIEvent>) -> Option<Retained<UIView>> {
            // SAFETY: UIView's hit test. `return` would skip `method_id`'s
            // retained conversion, so every path falls out of this block.
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
                if mode == 2 || self.ivars().pass_box.get() {
                    let mine = self as *const Self as *const UIView;
                    hit.filter(|found| {
                        !std::ptr::eq(Retained::as_ptr(found), mine) && found.tag() >= 0
                    })
                } else {
                    hit
                }
            };
            hit
        }
    }
);

impl BoxView {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(BoxIvars {
            pass_box: Cell::new(false),
            pointer: Cell::new(0),
        });
        // SAFETY: UIView's designated initializer.
        unsafe { msg_send![super(this), init] }
    }

    pub(super) fn set_pass_box(&self, pass: bool) {
        self.ivars().pass_box.set(pass);
    }

    pub(super) fn set_pointer(&self, events: nlayer_core::PointerEvents) {
        self.ivars().pointer.set(events as u8);
    }
}
