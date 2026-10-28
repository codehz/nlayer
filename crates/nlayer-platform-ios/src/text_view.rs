//! A view that draws the `CTLine`s the engine already laid out.

use std::{
    cell::{Cell, RefCell},
    sync::Arc,
};

use crate::text::{CoreTextEngine, CoreTextLayout};
use nlayer_core::{
    NodeId,
    geometry::{Bounds, Points, point},
    style::Color,
    text::{AttributedText, TextEngine, TextLayout},
};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send};
use objc2_core_foundation::{CGAffineTransform, CGFloat, CGPoint, CGRect};
use objc2_core_graphics::CGContext;
use objc2_ui_kit::{UIColor, UIGraphicsGetCurrentContext, UIResponder, UIView};

/// Storage for one text view. The class macro exposes it on the class.
pub(crate) struct TextViewIvars {
    layout: RefCell<Option<(TextLayout, Bounds<Points>)>>,
    text: RefCell<Option<Arc<AttributedText>>>,
    /// An `activeStyle` / `hoverStyle` color (`0xRRGGBBAA`): the whole text
    /// is drawn in it. `None` keeps each span's own color.
    color: Cell<Option<u32>>,
}

define_class!(
    /// Draws finished CoreText lines. No text layout happens here.
    #[unsafe(super(UIView, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TextViewIvars]
    pub(crate) struct TextView;

    impl TextView {
        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            false
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: CGRect) {
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
);

impl TextView {
    pub(crate) fn new(mtm: objc2::MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TextViewIvars {
            layout: RefCell::new(None),
            text: RefCell::new(None),
            color: Cell::new(None),
        });
        // SAFETY: UIView's designated initializer.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.setOpaque(false);
        this.setBackgroundColor(Some(&UIColor::clearColor()));
        this
    }

    pub(crate) fn set_text(
        &self,
        text: Arc<AttributedText>,
        layout: TextLayout,
        content: Bounds<Points>,
    ) {
        *self.ivars().text.borrow_mut() = Some(text);
        *self.ivars().layout.borrow_mut() = Some((layout, content));
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

    /// The pressable span under `point` (the view's coordinates).
    pub(crate) fn link_at(&self, at: CGPoint) -> Option<NodeId> {
        let layout = self.ivars().layout.borrow().clone()?;
        let text = self.attributed()?;
        let local = point(
            Points(at.x as f32 - layout.1.origin.x.0),
            Points(at.y as f32 - layout.1.origin.y.0),
        );
        let position = CoreTextEngine::default().hit_test(&layout.0, local);
        text.link_at(position.offset).map(|link| link.node)
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
