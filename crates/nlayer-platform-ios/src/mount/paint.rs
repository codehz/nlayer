//! View paint: fill, radius, border, shadow, gradient and transform.

use std::cell::Cell;
use std::sync::Arc;

use nlayer_core::Interaction;
use nlayer_core::effects::{BoxShadow, Effects, Gradient, GradientKind, GradientSize};
use nlayer_core::geometry::{Bounds, Percentage, Points, Size, deg};
use nlayer_core::style::{BorderStyle, Color, CornerRadii, TransformOrigin, ViewStyle};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGMutablePath, CGPath};
use objc2_quartz_core::{
    CAGradientLayer, CALayer, CAShapeLayer, CATransaction, CATransform3D, kCAFillRuleEvenOdd,
    kCAGradientLayerAxial, kCAGradientLayerConic, kCAGradientLayerRadial,
};
use objc2_ui_kit::{UIScreen, UITapGestureRecognizer, UIView};

use super::*;

/// A tap on a scroll view that resigns the keyboard. Its delegate lets
/// through only taps that should.
pub(super) fn keyboard_tap(
    mtm: MainThreadMarker,
    relay: &Relay,
    view: &UIView,
) -> Retained<UITapGestureRecognizer> {
    // SAFETY: the relay implements `dismissKeyboard:` and outlives the scroll.
    let tap = unsafe {
        UITapGestureRecognizer::initWithTarget_action(
            UITapGestureRecognizer::alloc(mtm),
            Some(relay),
            Some(sel!(dismissKeyboard:)),
        )
    };
    tap.setCancelsTouchesInView(false);
    tap.setDelegate(Some(ProtocolObject::from_ref(relay)));
    view.addGestureRecognizer(&tap);
    tap
}

pub(super) fn state_index(interaction: Interaction) -> usize {
    match interaction {
        Interaction::Active => 0,
        Interaction::Hover => 1,
        Interaction::Focus => 2,
    }
}

/// The view paint on screen: the base style, with the winning state's
/// non-default fields over it. Active beats hover, which beats focus.
pub(super) fn painted(slot: &Slot) -> ViewStyle {
    let mut style = slot.style.clone();
    let over = winning(slot);
    let Some(over) = over else { return style };
    let over = &over.view;
    if !over.background_color.is_transparent() {
        style.background_color = over.background_color;
    }
    if (over.opacity - 1.0).abs() > f32::EPSILON {
        style.opacity = over.opacity;
    }
    if over.border_radius != Points::ZERO || over.radii() != ViewStyle::default().radii() {
        style.border_radius = over.border_radius;
        style.corner_radius = over.corner_radius;
        style.continuous_corners = over.continuous_corners;
    }
    if over.borders.is_visible() {
        style.borders = over.borders;
    }
    if !over.transform.is_identity() {
        style.transform = over.transform;
        style.transform_origin = over.transform_origin;
    }
    if over.overflow_hidden {
        style.overflow_hidden = true;
    }
    if over.hidden {
        style.hidden = true;
    }
    // A state replaces one list only when it carries that list. The other
    // list, and filters, stay the base style's.
    let shadows = over
        .effects
        .as_ref()
        .filter(|effects| !effects.box_shadows.is_empty())
        .map(|effects| effects.box_shadows.clone());
    let gradients = over
        .effects
        .as_ref()
        .filter(|effects| !effects.background_images.is_empty())
        .map(|effects| effects.background_images.clone());
    if shadows.is_some() || gradients.is_some() {
        let mut effects = style
            .effects
            .clone()
            .map(|effects| (*effects).clone())
            .unwrap_or_else(Effects::default);
        if let Some(shadows) = shadows {
            effects.box_shadows = shadows;
        }
        if let Some(gradients) = gradients {
            effects.background_images = gradients;
        }
        style.effects = Some(Arc::new(effects));
    }
    style
}

pub(super) fn text_override(slot: &Slot) -> Option<Color> {
    winning(slot)?.text.color
}

pub(super) fn winning(slot: &Slot) -> Option<&Paint> {
    if slot.pressed {
        slot.states[state_index(Interaction::Active)].as_ref()
    } else if slot.hovered {
        slot.states[state_index(Interaction::Hover)].as_ref()
    } else if slot.focused {
        slot.states[state_index(Interaction::Focus)].as_ref()
    } else {
        None
    }
}

/// Outer shadows sit under the background. A hole mask keeps them from
/// covering it, so they can be sublayers and still paint outside the box.
const SHADOW_Z: CGFloat = -2048.0;
/// Gradients sit above the background color and below inset shadows.
const GRADIENT_Z: CGFloat = -1024.0 + 0.25;
/// Inset shadows sit above gradients and below children and the border.
const INSET_Z: CGFloat = -1024.0 + 0.5;

pub(super) fn paint_view(view: &UIView, style: &ViewStyle, slot: &mut Slot, scroll: bool) {
    let layer = view.layer();
    view.setAlpha(style.opacity as CGFloat);
    view.setHidden(style.hidden);
    let color = if style.background_color.is_transparent() {
        None
    } else {
        Some(crate::text::cg_color(style.background_color))
    };
    layer.setBackgroundColor(color.as_deref());
    let bounds = view.bounds();
    // Effect layers live in the layer's own space. A scroll view's bounds
    // origin is its content offset; those views do not draw effects.
    let local = CGRect::new(CGPoint::ZERO, bounds.size);
    let size = Size::new(
        Points(local.size.width as f32),
        Points(local.size.height as f32),
    );
    let radii = style.radii().fit(size);
    apply_corners(&layer, radii, bounds, &mut slot.mask);
    let outside = draws_outside(style);
    let own_content = matches!(
        slot.native,
        Native::Text(_) | Native::Image(_) | Native::Input(_) | Native::Area(_)
    );
    if scroll {
        // The scroll viewport clips itself. Its outer shadow is clipped too.
        layer.setMasksToBounds(true);
        clear_content(view, slot);
        clear_effects(slot);
    } else if matches!(slot.native, Native::View(_)) && style.overflow_hidden && outside {
        // Children move into a clipping subview so the outer layer can
        // leave `masksToBounds` off and let the shadow out.
        layer.setMasksToBounds(false);
        ensure_content(view, slot, local, radii);
        paint_effects(view, style, slot, local, radii);
    } else {
        clear_content(view, slot);
        if outside && own_content && (!radii.is_zero() || style.overflow_hidden) {
            // A mask clips the contents to the corners. `masksToBounds`
            // would clip the shadow sublayers as well, so those move to a
            // host when the mask is present.
            layer.setMasksToBounds(false);
            install_corner_mask(&layer, radii, local, border_outset(style), &mut slot.mask);
        } else if outside && style.overflow_hidden {
            // A header, stack or screen has no content view. Keep clipping
            // its children; the shadow is hosted so this clip does not cut
            // it off.
            layer.setMasksToBounds(true);
        } else if outside {
            layer.setMasksToBounds(false);
        } else {
            layer.setMasksToBounds(style.overflow_hidden);
        }
        paint_effects(view, style, slot, local, radii);
    }
    apply_border(view, style, slot);
    layer.setTransform(transform_3d(style, bounds.size));
    place_shadow_host(view, slot);
}

/// A visible outer box shadow. Inset shadows stay inside the box.
pub(super) fn draws_outside(style: &ViewStyle) -> bool {
    style.effects.as_ref().is_some_and(|effects| {
        effects
            .box_shadows
            .iter()
            .any(|shadow| shadow.is_visible() && !shadow.inset)
    })
}

pub(super) fn border_outset(style: &ViewStyle) -> CGFloat {
    let width = style
        .borders
        .uniform()
        .map(|(width, _)| width.0)
        .unwrap_or_else(|| {
            style
                .borders
                .widths
                .to_array()
                .into_iter()
                .map(|width| width.0)
                .fold(0.0, f32::max)
        });
    width as CGFloat / 2.0
}

/// A corner mask grown by the border's outer half, so the border drawn on
/// the layer is not sliced while the contents still follow the corners.
pub(super) fn install_corner_mask(
    layer: &CALayer,
    radii: CornerRadii,
    local: CGRect,
    outset: CGFloat,
    mask: &mut Option<Retained<CAShapeLayer>>,
) {
    let shape = mask.get_or_insert_with(CAShapeLayer::layer);
    shape.setFrame(local);
    shape.setPath(Some(&rounded_path(
        grow(local, outset),
        grown(radii, Points(outset as f32)),
    )));
    // SAFETY: a mask layer owned by this slot; the layer retains it.
    unsafe { layer.setMask(Some(shape)) };
}

pub(super) fn paint_effects(
    view: &UIView,
    style: &ViewStyle,
    slot: &mut Slot,
    local: CGRect,
    radii: CornerRadii,
) {
    let effects = style.effects.as_deref();
    let shadows = effects.map_or(&[][..], |effects| &effects.box_shadows[..]);
    let outer: Vec<&BoxShadow> = shadows
        .iter()
        .filter(|shadow| !shadow.inset && shadow.is_visible())
        .collect();
    // Text is drawn into the layer's contents, under every sublayer, so a
    // gradient or inset shadow there would cover the glyphs. Outer shadows
    // stay: they paint outside the box.
    let text = matches!(slot.native, Native::Text(_));
    let inner: Vec<&BoxShadow> = if text {
        Vec::new()
    } else {
        shadows
            .iter()
            .filter(|shadow| shadow.inset && shadow.is_visible())
            .collect()
    };
    let gradients: &[Gradient] = if text {
        &[]
    } else {
        effects.map_or(&[][..], |effects| &effects.background_images[..])
    };
    // A mask or `masksToBounds` clips sublayers, shadow included. The host
    // is a sibling, so the shadow still paints outside the box.
    let clips = view.layer().mask().is_some() || view.layer().masksToBounds();
    let shadow_parent = if clips && !outer.is_empty() {
        ensure_shadow_host(view, slot).layer()
    } else {
        drop_shadow_host(slot);
        view.layer()
    };
    set_shadows(&mut slot.shadows, &shadow_parent, &outer, local, radii);
    let border = style
        .borders
        .uniform()
        .map(|(width, _)| width.0)
        .unwrap_or(0.0);
    set_insets(
        &mut slot.insets,
        &view.layer(),
        &inner,
        local,
        radii,
        border,
    );
    set_gradients(&mut slot.gradients, &view.layer(), gradients, local, radii);
}

pub(super) fn clear_effects(slot: &mut Slot) {
    for layer in slot.shadows.drain(..) {
        layer.removeFromSuperlayer();
    }
    if let Some((holder, _)) = slot.insets.take() {
        holder.removeFromSuperlayer();
    }
    for layer in slot.gradients.drain(..) {
        layer.removeFromSuperlayer();
    }
    drop_shadow_host(slot);
}

/// The subview that clips a plain view's children while the outer layer
/// stays unmasked so its shadow can paint outside.
pub(super) fn ensure_content(view: &UIView, slot: &mut Slot, local: CGRect, radii: CornerRadii) {
    let content = if let Some(content) = &slot.content {
        content.clone()
    } else {
        let content = UIView::new(view.mtm());
        content.setTag(-1);
        content.setOpaque(false);
        content.setBackgroundColor(Some(&objc2_ui_kit::UIColor::clearColor()));
        content.setAutoresizingMask(
            objc2_ui_kit::UIViewAutoresizing::FlexibleWidth
                | objc2_ui_kit::UIViewAutoresizing::FlexibleHeight,
        );
        // Engine children and their shadow hosts. A selection responder
        // (tag 0) stays on the outer view, which is what finds it.
        let subs = view.subviews();
        let children: Vec<Retained<UIView>> = (0..subs.count())
            .map(|index| subs.objectAtIndex(index))
            .filter(|child| child.tag() > 0 || child.tag() == -1)
            .collect();
        view.addSubview(&content);
        for child in &children {
            content.addSubview(child);
        }
        slot.content = Some(content.clone());
        content
    };
    content.setFrame(local);
    let content_layer = content.layer();
    apply_corners(&content_layer, radii, local, &mut slot.content_mask);
    content_layer.setMasksToBounds(true);
}

pub(super) fn clear_content(view: &UIView, slot: &mut Slot) {
    let Some(content) = slot.content.take() else {
        return;
    };
    slot.content_mask = None;
    let subs = content.subviews();
    let children: Vec<Retained<UIView>> = (0..subs.count())
        .map(|index| subs.objectAtIndex(index))
        .collect();
    for child in &children {
        view.insertSubview_belowSubview(child, &content);
    }
    content.removeFromSuperview();
}

pub(super) fn ensure_shadow_host(view: &UIView, slot: &mut Slot) -> Retained<UIView> {
    let host = if let Some(host) = &slot.shadow_host {
        host.clone()
    } else {
        let host = UIView::new(view.mtm());
        host.setTag(-1);
        host.setOpaque(false);
        host.setUserInteractionEnabled(false);
        host.setBackgroundColor(Some(&objc2_ui_kit::UIColor::clearColor()));
        host.layer().setMasksToBounds(false);
        slot.shadow_host = Some(host.clone());
        host
    };
    place_shadow_host(view, slot);
    host
}

pub(super) fn drop_shadow_host(slot: &mut Slot) {
    if let Some(host) = slot.shadow_host.take() {
        host.removeFromSuperview();
    }
}

/// Keeps the shadow host directly under its view, in that view's parent,
/// with the same frame and transform. A move that does not resize never
/// repaints, so callers place the host on every frame change too.
pub(super) fn place_shadow_host(view: &UIView, slot: &Slot) {
    let Some(host) = &slot.shadow_host else {
        return;
    };
    host.setFrame(view.frame());
    host.setBounds(CGRect::new(CGPoint::ZERO, view.bounds().size));
    host.setHidden(view.isHidden());
    host.setAlpha(view.alpha());
    host.layer().setTransform(view.layer().transform());
    if let Some(parent) = view.superview() {
        parent.insertSubview_belowSubview(host, view);
    }
}

pub(super) fn set_shadows(
    layers: &mut Vec<Retained<CALayer>>,
    parent: &CALayer,
    shadows: &[&BoxShadow],
    local: CGRect,
    radii: CornerRadii,
) {
    resize(layers, shadows.len(), parent, || {
        let layer = CALayer::layer();
        layer.setZPosition(SHADOW_Z);
        layer
    });
    for layer in layers.iter() {
        parent.addSublayer(layer);
    }
    let shape = rounded_path(local, radii);
    // CSS order is top first; later sublayers draw on top.
    for (layer, shadow) in layers.iter().zip(shadows.iter().rev()) {
        let spread = shadow.spread.0 as CGFloat;
        layer.setFrame(local);
        layer.setShadowPath(Some(&rounded_path(
            grow(local, spread),
            grown(radii, shadow.spread),
        )));
        layer.setShadowColor(Some(&*crate::text::cg_color(Color(shadow.color.0 | 0xff))));
        layer.setShadowOpacity((shadow.color.0 & 0xff) as f32 / 255.0);
        layer.setShadowOffset(CGSize::new(
            shadow.offset_x.0 as CGFloat,
            shadow.offset_y.0 as CGFloat,
        ));
        // CSS blur is twice the Gaussian's deviation; Core Animation's
        // radius is close to the deviation.
        layer.setShadowRadius(shadow.blur.0 as CGFloat / 2.0);
        let reach = (shadow.offset_x.0.abs().max(shadow.offset_y.0.abs())
            + shadow.spread.0.abs()
            + shadow.blur.0 * 2.0
            + 4.0) as CGFloat;
        let mask = mask_layer(layer);
        let path = CGMutablePath::new();
        // SAFETY: valid mutable paths and a null transform.
        unsafe {
            CGMutablePath::add_rect(Some(&path), std::ptr::null(), grow(local, reach));
            CGMutablePath::add_path(Some(&path), std::ptr::null(), Some(&shape));
        }
        mask.setFrame(local);
        mask.setPath(Some(&path));
    }
}

pub(super) fn set_insets(
    insets: &mut Option<(Retained<CALayer>, Vec<Retained<CAShapeLayer>>)>,
    layer: &CALayer,
    shadows: &[&BoxShadow],
    local: CGRect,
    radii: CornerRadii,
    border: f32,
) {
    if shadows.is_empty() {
        if let Some((holder, _)) = insets.take() {
            holder.removeFromSuperlayer();
        }
        return;
    }
    let (holder, shapes) = insets.get_or_insert_with(|| {
        let holder = CALayer::layer();
        holder.setZPosition(INSET_Z);
        layer.addSublayer(&holder);
        (holder, Vec::new())
    });
    layer.addSublayer(holder);
    let padding = inset_rect(local, border as CGFloat);
    let inner_radii = radii.inset(Points(border));
    holder.setFrame(local);
    let mask = mask_layer(holder);
    mask.setFrame(local);
    mask.setPath(Some(&rounded_path(padding, inner_radii)));
    resize(shapes, shadows.len(), holder, CAShapeLayer::layer);
    for (shape, shadow) in shapes.iter().zip(shadows.iter().rev()) {
        let reach = (shadow.offset_x.0.abs().max(shadow.offset_y.0.abs())
            + shadow.spread.0.abs()
            + shadow.blur.0 * 2.0
            + 4.0) as CGFloat;
        // The frame around the shifted, shrunk inner shape casts its
        // shadow inwards (non-zero winding: the hole runs backwards).
        let hole = offset_rect(
            grow(padding, -(shadow.spread.0 as CGFloat)),
            shadow.offset_x.0,
            shadow.offset_y.0,
        );
        let path = CGMutablePath::new();
        let reversed = reversed_rounded(hole, grown(inner_radii, -shadow.spread));
        // SAFETY: valid mutable paths and a null transform.
        unsafe {
            CGMutablePath::add_rect(Some(&path), std::ptr::null(), grow(local, reach));
            CGMutablePath::add_path(Some(&path), std::ptr::null(), Some(&reversed));
        }
        shape.setFrame(local);
        shape.setShadowPath(Some(&path));
        shape.setShadowColor(Some(&*crate::text::cg_color(Color(shadow.color.0 | 0xff))));
        shape.setShadowOpacity((shadow.color.0 & 0xff) as f32 / 255.0);
        shape.setShadowOffset(CGSize::ZERO);
        shape.setShadowRadius(shadow.blur.0 as CGFloat / 2.0);
        shape.setFillColor(None);
    }
}

pub(super) fn set_gradients(
    layers: &mut Vec<Retained<CAGradientLayer>>,
    parent: &CALayer,
    gradients: &[Gradient],
    local: CGRect,
    radii: CornerRadii,
) {
    resize(layers, gradients.len(), parent, || {
        let gradient = CAGradientLayer::new();
        gradient.setZPosition(GRADIENT_Z);
        gradient
    });
    for layer in layers.iter() {
        parent.addSublayer(layer);
    }
    let shared = radii.shared();
    // CSS order is top first.
    for (layer, gradient) in layers.iter().zip(gradients.iter().rev()) {
        layer.setFrame(local);
        match shared {
            Some(radius) => {
                layer.setCornerRadius(radius.0 as CGFloat);
                layer.setMaskedCorners(corner_bits(&radii));
                layer.setMasksToBounds(true);
                // SAFETY: clearing a mask this layer no longer needs.
                unsafe { layer.setMask(None) };
            }
            None => {
                layer.setCornerRadius(0.0);
                layer.setMasksToBounds(false);
                let mask = mask_layer(layer);
                mask.setFrame(local);
                mask.setPath(Some(&rounded_path(local, radii)));
            }
        }
        shape_gradient(layer, gradient, local.size);
    }
}

pub(super) fn corner_bits(radii: &CornerRadii) -> objc2_quartz_core::CACornerMask {
    let mut bits: usize = 0;
    if radii.top_left > Points::ZERO {
        bits |= 1 << 0;
    }
    if radii.top_right > Points::ZERO {
        bits |= 1 << 1;
    }
    if radii.bottom_left > Points::ZERO {
        bits |= 1 << 2;
    }
    if radii.bottom_right > Points::ZERO {
        bits |= 1 << 3;
    }
    objc2_quartz_core::CACornerMask(bits)
}

pub(super) fn resize<L: AsRef<CALayer>>(
    layers: &mut Vec<Retained<L>>,
    count: usize,
    parent: &CALayer,
    make: impl Fn() -> Retained<L>,
) {
    while layers.len() > count {
        if let Some(layer) = layers.pop() {
            layer.as_ref().removeFromSuperlayer();
        }
    }
    while layers.len() < count {
        let layer = make();
        parent.addSublayer(layer.as_ref());
        layers.push(layer);
    }
}

/// The even-odd shape mask of `layer`, created on first use.
pub(super) fn mask_layer(layer: &CALayer) -> Retained<CAShapeLayer> {
    if let Some(mask) = layer.mask()
        && let Ok(mask) = mask.downcast::<CAShapeLayer>()
    {
        return mask;
    }
    let mask = CAShapeLayer::layer();
    // SAFETY: an immutable Core Animation constant.
    mask.setFillRule(unsafe { kCAFillRuleEvenOdd });
    // SAFETY: a fresh mask layer, owned by `layer`.
    unsafe { layer.setMask(Some(&mask)) };
    mask
}

pub(super) fn grow(rect: CGRect, d: CGFloat) -> CGRect {
    CGRect::new(
        CGPoint::new(rect.origin.x - d, rect.origin.y - d),
        CGSize::new(
            (rect.size.width + 2.0 * d).max(0.0),
            (rect.size.height + 2.0 * d).max(0.0),
        ),
    )
}

pub(super) fn inset_rect(rect: CGRect, d: CGFloat) -> CGRect {
    CGRect::new(
        CGPoint::new(rect.origin.x + d, rect.origin.y + d),
        CGSize::new(
            (rect.size.width - 2.0 * d).max(0.0),
            (rect.size.height - 2.0 * d).max(0.0),
        ),
    )
}

pub(super) fn offset_rect(rect: CGRect, x: f32, y: f32) -> CGRect {
    CGRect::new(
        CGPoint::new(rect.origin.x + x as CGFloat, rect.origin.y + y as CGFloat),
        rect.size,
    )
}

/// Radii grown by `d` (shrunk when negative). A square corner stays square.
pub(super) fn grown(radii: CornerRadii, d: Points) -> CornerRadii {
    let grow_one = |radius: Points| {
        if radius > Points::ZERO {
            (radius + d).max(Points::ZERO)
        } else {
            Points::ZERO
        }
    };
    CornerRadii {
        top_left: grow_one(radii.top_left),
        top_right: grow_one(radii.top_right),
        bottom_right: grow_one(radii.bottom_right),
        bottom_left: grow_one(radii.bottom_left),
        continuous: radii.continuous,
    }
}

/// `path` reversed, so it is a hole under the non-zero rule.
pub(super) fn reversed_rounded(rect: CGRect, radii: CornerRadii) -> Retained<CGPath> {
    use objc2_ui_kit::UIBezierPath;
    UIBezierPath::bezierPathWithCGPath(&rounded_path(rect, radii))
        .bezierPathByReversingPath()
        .CGPath()
}

/// Configures `gradient_layer` to draw `gradient` over a `size` box.
pub(super) fn shape_gradient(gradient_layer: &CAGradientLayer, gradient: &Gradient, size: CGSize) {
    use objc2_foundation::{NSArray, NSNumber};
    let (w, h) = (size.width.max(1.0), size.height.max(1.0));
    // How far along the gradient line the layer's `0..1` reaches. A
    // repeating radial gradient's rings run past the ending shape, out to
    // the farthest corner: Core Animation repeats nothing itself.
    let reach = shape_gradient_geometry(gradient_layer, gradient, w, h);
    let stops = gradient.unrolled_stops(
        Size::new(Points(w as f32), Points(h as f32)),
        Percentage(reach as f32),
    );
    // A transparent stop takes its neighbor's hue, as CSS's premultiplied
    // mixing does, so a fade to transparent does not darken.
    let colors: Vec<Retained<AnyObject>> = (0..stops.len())
        .map(|index| {
            let color = stops[index].0;
            let color = if color.is_transparent() {
                let neighbor = stops[index + 1..]
                    .iter()
                    .chain(stops[..index].iter().rev())
                    .map(|(neighbor, _)| *neighbor)
                    .find(|neighbor| !neighbor.is_transparent());
                neighbor.map_or(color, |neighbor| Color(neighbor.0 & 0xffff_ff00))
            } else {
                color
            };
            let cg = crate::text::cg_color(color);
            // SAFETY: a CGColor, which `colors` takes. CF objects are
            // Objective-C objects.
            unsafe {
                Retained::from_raw(
                    objc2_core_foundation::CFRetained::into_raw(cg)
                        .as_ptr()
                        .cast(),
                )
                .expect("cg color")
            }
        })
        .collect();
    let locations: Vec<Retained<NSNumber>> = stops
        .iter()
        .map(|(_, position)| NSNumber::numberWithDouble(position.0 as f64))
        .collect();
    // SAFETY: an array of CGColors, as `colors` documents.
    unsafe { gradient_layer.setColors(Some(&NSArray::from_retained_slice(&colors))) };
    gradient_layer.setLocations(Some(&NSArray::from_retained_slice(&locations)));
}

/// Sets the layer's type and its start and end points for `gradient` over
/// a `w` × `h` box. Returns how far along the gradient line the end point is.
pub(super) fn shape_gradient_geometry(
    gradient_layer: &CAGradientLayer,
    gradient: &Gradient,
    w: CGFloat,
    h: CGFloat,
) -> CGFloat {
    match gradient.kind {
        GradientKind::Radial {
            circle,
            size: extent,
            x,
            y,
        } => {
            // SAFETY: an immutable Core Animation constant.
            gradient_layer.setType(unsafe { kCAGradientLayerRadial });
            let (cx, cy) = (x.0 as CGFloat * w, y.0 as CGFloat * h);
            let (dx, dy) = match extent {
                GradientSize::ClosestSide => (cx.min(w - cx), cy.min(h - cy)),
                GradientSize::FarthestSide => (cx.max(w - cx), cy.max(h - cy)),
                GradientSize::ClosestCorner => {
                    let (a, b) = (cx.min(w - cx), cy.min(h - cy));
                    (a * std::f64::consts::SQRT_2, b * std::f64::consts::SQRT_2)
                }
                GradientSize::FarthestCorner => {
                    let (a, b) = (cx.max(w - cx), cy.max(h - cy));
                    (a * std::f64::consts::SQRT_2, b * std::f64::consts::SQRT_2)
                }
            };
            let (rx, ry) = if circle {
                let radius = match extent {
                    GradientSize::ClosestSide => dx.min(dy),
                    GradientSize::FarthestSide => dx.max(dy),
                    _ => {
                        let (a, b) = (dx / std::f64::consts::SQRT_2, dy / std::f64::consts::SQRT_2);
                        (a * a + b * b).sqrt()
                    }
                };
                (radius, radius)
            } else {
                (dx, dy)
            };
            // The farthest corner, in radii of the ending shape.
            let reach = if gradient.repeating && rx > 0.0 && ry > 0.0 {
                (cx.max(w - cx) / rx).hypot(cy.max(h - cy) / ry).max(1.0)
            } else {
                1.0
            };
            gradient_layer.setStartPoint(CGPoint::new(cx / w, cy / h));
            gradient_layer.setEndPoint(CGPoint::new((cx + rx * reach) / w, (cy + ry * reach) / h));
            reach
        }
        GradientKind::Conic { from, x, y } => {
            // SAFETY: an immutable Core Animation constant.
            gradient_layer.setType(unsafe { kCAGradientLayerConic });
            let (x, y) = (x.0 as CGFloat, y.0 as CGFloat);
            gradient_layer.setStartPoint(CGPoint::new(x, y));
            // Core Animation starts at the end point's direction, clockwise
            // from the right. CSS starts up.
            let angle = from.0 as f64 - std::f64::consts::FRAC_PI_2;
            gradient_layer.setEndPoint(CGPoint::new(x + angle.cos() / 2.0, y + angle.sin() / 2.0));
            1.0
        }
        kind => {
            // SAFETY: an immutable Core Animation constant.
            gradient_layer.setType(unsafe { kCAGradientLayerAxial });
            let box_size = Size::new(Points(w as f32), Points(h as f32));
            let angle = kind.linear_angle(box_size).unwrap_or(deg(180.0));
            // CSS: the gradient line through the center at `angle` (0 up,
            // clockwise), long enough that the corners get the end colors.
            let (sin, cos) = (angle.0 as f64).sin_cos();
            let len = (w * sin).abs() + (h * cos).abs();
            let (dx, dy) = (sin * len / 2.0, -cos * len / 2.0);
            let (cx, cy) = (w / 2.0, h / 2.0);
            gradient_layer.setStartPoint(CGPoint::new((cx - dx) / w, (cy - dy) / h));
            gradient_layer.setEndPoint(CGPoint::new((cx + dx) / w, (cy + dy) / h));
            1.0
        }
    }
}

pub(super) fn apply_corners(
    layer: &CALayer,
    radii: nlayer_core::style::CornerRadii,
    bounds: CGRect,
    mask: &mut Option<Retained<CAShapeLayer>>,
) {
    if let Some(shared) = radii.shared() {
        if mask.take().is_some() {
            // SAFETY: clearing a mask this slot installed.
            unsafe { layer.setMask(None) };
        }
        layer.setCornerRadius(shared.0 as CGFloat);
        // MinXMinY is the top left in UIKit's y-down space.
        let mut bits: usize = 0;
        if radii.top_left > Points::ZERO {
            bits |= 1 << 0;
        }
        if radii.top_right > Points::ZERO {
            bits |= 1 << 1;
        }
        if radii.bottom_left > Points::ZERO {
            bits |= 1 << 2;
        }
        if radii.bottom_right > Points::ZERO {
            bits |= 1 << 3;
        }
        layer.setMaskedCorners(objc2_quartz_core::CACornerMask(bits));
        return;
    }
    layer.setCornerRadius(0.0);
    let shape = mask.get_or_insert_with(CAShapeLayer::layer);
    shape.setFrame(bounds);
    shape.setPath(Some(&rounded_path(bounds, radii)));
    // SAFETY: a mask layer owned by this slot; the layer retains it.
    unsafe { layer.setMask(Some(shape)) };
}

pub(super) fn rounded_path(
    rect: CGRect,
    radii: nlayer_core::style::CornerRadii,
) -> Retained<objc2_core_graphics::CGPath> {
    use objc2_ui_kit::UIBezierPath;
    let path = UIBezierPath::bezierPath();
    let (x, y, w, h) = (
        rect.origin.x,
        rect.origin.y,
        rect.size.width,
        rect.size.height,
    );
    let (tl, tr, br, bl) = (
        radii.top_left.0 as CGFloat,
        radii.top_right.0 as CGFloat,
        radii.bottom_right.0 as CGFloat,
        radii.bottom_left.0 as CGFloat,
    );
    path.moveToPoint(CGPoint::new(x + tl, y));
    path.addLineToPoint(CGPoint::new(x + w - tr, y));
    if tr > 0.0 {
        path.addArcWithCenter_radius_startAngle_endAngle_clockwise(
            CGPoint::new(x + w - tr, y + tr),
            tr,
            -std::f64::consts::FRAC_PI_2,
            0.0,
            true,
        );
    }
    path.addLineToPoint(CGPoint::new(x + w, y + h - br));
    if br > 0.0 {
        path.addArcWithCenter_radius_startAngle_endAngle_clockwise(
            CGPoint::new(x + w - br, y + h - br),
            br,
            0.0,
            std::f64::consts::FRAC_PI_2,
            true,
        );
    }
    path.addLineToPoint(CGPoint::new(x + bl, y + h));
    if bl > 0.0 {
        path.addArcWithCenter_radius_startAngle_endAngle_clockwise(
            CGPoint::new(x + bl, y + h - bl),
            bl,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            true,
        );
    }
    path.addLineToPoint(CGPoint::new(x, y + tl));
    if tl > 0.0 {
        path.addArcWithCenter_radius_startAngle_endAngle_clockwise(
            CGPoint::new(x + tl, y + tl),
            tl,
            std::f64::consts::PI,
            std::f64::consts::PI + std::f64::consts::FRAC_PI_2,
            true,
        );
    }
    path.closePath();
    path.CGPath().clone()
}

pub(super) fn apply_border(view: &UIView, style: &ViewStyle, slot: &mut Slot) {
    let layer = view.layer();
    let borders = &style.borders;
    if let Some((width, color)) = borders.uniform() {
        clear_sides(slot);
        if borders.style == BorderStyle::Solid || width <= Points::ZERO {
            if let Some(dash) = slot.dash.take() {
                dash.removeFromSuperlayer();
            }
            layer.setBorderWidth(width.0 as CGFloat);
            layer.setBorderColor(Some(&*crate::text::cg_color(color)));
            return;
        }
        layer.setBorderWidth(0.0);
        let dash = slot.dash.get_or_insert_with(CAShapeLayer::layer);
        if dash.superlayer().is_none() {
            layer.addSublayer(dash);
        }
        let inset = width.0 as CGFloat / 2.0;
        let bounds = view.bounds();
        let rect = CGRect::new(
            CGPoint::new(bounds.origin.x + inset, bounds.origin.y + inset),
            CGSize::new(
                (bounds.size.width - width.0 as CGFloat).max(0.0),
                (bounds.size.height - width.0 as CGFloat).max(0.0),
            ),
        );
        let radii = style.radii().fit(Size::new(
            Points(bounds.size.width as f32),
            Points(bounds.size.height as f32),
        ));
        let path = rounded_path(rect, radii.inset(Points(inset as f32)));
        dash.setFrame(bounds);
        dash.setPath(Some(&path));
        dash.setFillColor(None);
        dash.setStrokeColor(Some(&*crate::text::cg_color(color)));
        dash.setLineWidth(width.0 as CGFloat);
        dash.setLineDashPattern(Some(&dash_pattern(borders.style == BorderStyle::Dotted)));
        dash.setZPosition(1_000.0);
        return;
    }
    layer.setBorderWidth(0.0);
    if let Some(dash) = slot.dash.take() {
        dash.removeFromSuperlayer();
    }
    let bounds = view.bounds();
    let (w, h) = (bounds.size.width, bounds.size.height);
    let frames = [
        CGRect::new(
            CGPoint::ZERO,
            CGSize::new(w, borders.widths.top.0 as CGFloat),
        ),
        CGRect::new(
            CGPoint::new(w - borders.widths.right.0 as CGFloat, 0.0),
            CGSize::new(borders.widths.right.0 as CGFloat, h),
        ),
        CGRect::new(
            CGPoint::new(0.0, h - borders.widths.bottom.0 as CGFloat),
            CGSize::new(w, borders.widths.bottom.0 as CGFloat),
        ),
        CGRect::new(
            CGPoint::ZERO,
            CGSize::new(borders.widths.left.0 as CGFloat, h),
        ),
    ];
    for (i, frame) in frames.into_iter().enumerate() {
        let visible = borders.width(i) > Points::ZERO && !borders.colors[i].is_transparent();
        if !visible {
            if let Some(side) = slot.sides[i].take() {
                side.removeFromSuperlayer();
            }
            continue;
        }
        let side = slot.sides[i].get_or_insert_with(CALayer::layer);
        if side.superlayer().is_none() {
            layer.addSublayer(side);
        }
        side.setFrame(frame);
        side.setBackgroundColor(Some(&*crate::text::cg_color(borders.colors[i])));
        side.setZPosition(1_000.0);
    }
}

pub(super) fn clear_sides(slot: &mut Slot) {
    for side in &mut slot.sides {
        if let Some(side) = side.take() {
            side.removeFromSuperlayer();
        }
    }
}

/// Keeps the bounds origin, which is a scroll view's content offset.
/// Returns whether the size changed. A move with the same size does not
/// need a repaint.
pub(super) fn set_frame(view: &UIView, frame: Bounds<Points>) -> bool {
    let bounds = view.bounds();
    let width = frame.size.width.0 as CGFloat;
    let height = frame.size.height.0 as CGFloat;
    let resized = bounds.size.width != width || bounds.size.height != height;
    let center = CGPoint::new(
        frame.origin.x.0 as CGFloat + width / 2.0,
        frame.origin.y.0 as CGFloat + height / 2.0,
    );
    let placed = view.center();
    if resized || placed.x != center.x || placed.y != center.y {
        view.setBounds(CGRect::new(bounds.origin, CGSize::new(width, height)));
        view.setCenter(center);
    }
    resized
}

pub(super) fn transform_3d(style: &ViewStyle, size: CGSize) -> CATransform3D {
    let mut m = style.transform.matrix(|_| None).0;
    let origin = style.transform_origin;
    if origin != TransformOrigin::CENTER {
        let (d, dz) = origin.offset(Size::new(
            Points(size.width as f32),
            Points(size.height as f32),
        ));
        let (dx, dy, dz) = (d.x.0 as CGFloat, d.y.0 as CGFloat, dz.0 as CGFloat);
        m = mat_mul(
            &mat_mul(&translation(-dx, -dy, -dz), &m.map(|n| n as CGFloat)),
            &translation(dx, dy, dz),
        );
    }
    CATransform3D {
        m11: m[0] as CGFloat,
        m12: m[1] as CGFloat,
        m13: m[2] as CGFloat,
        m14: m[3] as CGFloat,
        m21: m[4] as CGFloat,
        m22: m[5] as CGFloat,
        m23: m[6] as CGFloat,
        m24: m[7] as CGFloat,
        m31: m[8] as CGFloat,
        m32: m[9] as CGFloat,
        m33: m[10] as CGFloat,
        m34: m[11] as CGFloat,
        m41: m[12] as CGFloat,
        m42: m[13] as CGFloat,
        m43: m[14] as CGFloat,
        m44: m[15] as CGFloat,
    }
}

/// A translation matrix (row vectors, as Core Animation).
pub(super) fn translation(x: CGFloat, y: CGFloat, z: CGFloat) -> [CGFloat; 16] {
    [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        x, y, z, 1.0,
    ]
}

pub(super) fn mat_mul(a: &[CGFloat; 16], b: &[CGFloat; 16]) -> [CGFloat; 16] {
    let mut out = [0.0; 16];
    for r in 0..4 {
        for c in 0..4 {
            out[r * 4 + c] = (0..4).map(|k| a[r * 4 + k] * b[k * 4 + c]).sum();
        }
    }
    out
}

pub(super) fn dash_pattern(
    dotted: bool,
) -> Retained<objc2_foundation::NSArray<objc2_foundation::NSNumber>> {
    use objc2_foundation::{NSArray, NSNumber};
    use std::cell::RefCell;
    thread_local! {
        static DASHED: RefCell<Option<Retained<NSArray<NSNumber>>>> = const { RefCell::new(None) };
        static DOTTED: RefCell<Option<Retained<NSArray<NSNumber>>>> = const { RefCell::new(None) };
    }
    let fill = |slot: &RefCell<Option<Retained<NSArray<NSNumber>>>>, pattern: &[f64]| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            let dashes: Vec<_> = pattern
                .iter()
                .copied()
                .map(NSNumber::numberWithDouble)
                .collect();
            *slot = Some(NSArray::from_retained_slice(&dashes));
        }
        slot.as_ref().expect("dash pattern").clone()
    };
    if dotted {
        DOTTED.with(|slot| fill(slot, &[1.0, 2.0]))
    } else {
        DASHED.with(|slot| fill(slot, &[4.0, 3.0]))
    }
}

thread_local! {
    static QUIET: Cell<u32> = const { Cell::new(0) };
}

struct QuietGuard {
    outer: bool,
}

impl Drop for QuietGuard {
    fn drop(&mut self) {
        if self.outer {
            CATransaction::commit();
        }
        QUIET.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Runs `f` without Core Animation's implicit animations. Nested calls
/// share the outer transaction, so a batch is one commit.
pub(super) fn quiet<R>(f: impl FnOnce() -> R) -> R {
    let outer = QUIET.with(|depth| {
        let current = depth.get();
        depth.set(current + 1);
        current == 0
    });
    if outer {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
    }
    let _guard = QuietGuard { outer };
    f()
}

#[allow(deprecated)]
pub(super) fn screen_scale(mtm: MainThreadMarker) -> f32 {
    let scale = UIScreen::mainScreen(mtm).scale() as f32;
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}
