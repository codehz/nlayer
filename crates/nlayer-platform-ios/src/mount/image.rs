//! Image decode, layer contents and object-fit placement.

use std::sync::{Arc, Mutex, OnceLock};

use rustc_hash::FxHashMap as HashMap;

use dispatch2::DispatchQueue;
use nlayer_core::geometry::{Bounds, Point, Points, Size};
use nlayer_core::style::ViewStyle;
use nlayer_core::{ImageSource, NodeId, ResizeMode};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGImage;
use objc2_quartz_core::{CALayer, kCAGravityResize};
use objc2_ui_kit::UIView;

use super::*;

impl Mounter {
    pub(super) fn set_image(&mut self, id: NodeId, source: Option<ImageSource>, mode: ResizeMode) {
        let scale = screen_scale(self.mtm);
        let reload = {
            let Some(slot) = self.slot_mut(id) else {
                return;
            };
            let Some(picture) = &mut slot.picture else {
                return;
            };
            picture.mode = mode;
            let next = source.and_then(|source| (!source.uri.is_empty()).then_some(source.uri));
            if picture.uri == next {
                None
            } else {
                picture.uri = next.clone();
                picture.generation = picture.generation.wrapping_add(1);
                picture.natural = None;
                let generation = picture.generation;
                let layer = picture.layer.clone();
                quiet(|| unsafe { layer.setContents(None) });
                Some((generation, next))
            }
        };
        if let Some((generation, Some(uri))) = reload
            && let Some(decoded) = begin_load(id, generation, uri, &self.assets, scale)
        {
            self.finish_image(id, generation, scale, Ok(decoded));
            return;
        }
        self.repaint(id);
    }

    pub(super) fn finish_image(
        &mut self,
        id: NodeId,
        generation: u64,
        scale: f32,
        loaded: Result<crate::image::Decoded, String>,
    ) {
        let decoded = match loaded {
            Ok(decoded) => Some(decoded),
            Err(error) => {
                eprintln!("nlayer: image: {error}");
                None
            }
        };
        {
            let Some(slot) = self.slot_mut(id) else {
                return;
            };
            let Some(picture) = &mut slot.picture else {
                return;
            };
            if picture.generation != generation {
                return;
            }
            let Some(decoded) = decoded else {
                return;
            };
            picture.natural = Some(Size::new(
                Points(decoded.width as f32 / scale),
                Points(decoded.height as f32 / scale),
            ));
            let layer = picture.layer.clone();
            quiet(|| {
                layer.setContentsScale(scale as CGFloat);
                // SAFETY: a CGImage is a valid layer `contents` object, and
                // the layer retains it.
                unsafe {
                    layer.setContents(Some(
                        &*(&*decoded.image as *const CGImage as *const AnyObject),
                    ));
                }
            });
        }
        self.repaint(id);
    }
}

pub(super) struct Waiter {
    id: NodeId,
    generation: u64,
    scale: f32,
}

pub(super) fn inflight() -> &'static Mutex<HashMap<Arc<str>, Vec<Waiter>>> {
    static INFLIGHT: OnceLock<Mutex<HashMap<Arc<str>, Vec<Waiter>>>> = OnceLock::new();
    INFLIGHT.get_or_init(|| Mutex::new(Default::default()))
}

pub(super) fn begin_load(
    id: NodeId,
    generation: u64,
    uri: Arc<str>,
    assets: &std::path::Path,
    scale: f32,
) -> Option<crate::image::Decoded> {
    let epoch = crate::image::epoch();
    if let Some(decoded) = crate::image::cached(&uri) {
        return Some(decoded);
    }
    {
        let mut inflight = inflight().lock().expect("image loads");
        if let Some(waiters) = inflight.get_mut(&uri) {
            waiters.push(Waiter {
                id,
                generation,
                scale,
            });
            return None;
        }
        inflight.insert(
            Arc::clone(&uri),
            vec![Waiter {
                id,
                generation,
                scale,
            }],
        );
    }
    let assets = assets.to_path_buf();
    let key = uri;
    use dispatch2::{DispatchQueueGlobalPriority, GlobalQueueIdentifier};
    DispatchQueue::global_queue(GlobalQueueIdentifier::Priority(
        DispatchQueueGlobalPriority::Default,
    ))
    .exec_async(move || {
        let loaded = crate::image::load(&assets, &key).map_err(|error| format!("{key}: {error}"));
        DispatchQueue::main().exec_async(move || {
            // A recycle bumped the epoch: this bitmap belongs to the app
            // that was dropped, and the waiters may now be the next app's.
            if crate::image::epoch() != epoch {
                return;
            }
            if let Ok(decoded) = &loaded {
                crate::image::store(Arc::clone(&key), decoded, epoch);
            }
            let waiters = inflight()
                .lock()
                .expect("image loads")
                .remove(&key)
                .unwrap_or_default();
            let Some(mounter) = current() else {
                return;
            };
            let mut mounter = mounter.borrow_mut();
            for waiter in waiters {
                mounter.finish_image(waiter.id, waiter.generation, waiter.scale, loaded.clone());
            }
        });
    });
    None
}

/// Forgets decodes the dropped app started. Their callbacks see a new
/// [`crate::image::epoch`] and do not paint.
pub(super) fn abandon_loads() {
    inflight().lock().expect("image loads").clear();
    crate::image::clear();
}

pub(super) fn image_layers(mtm: MainThreadMarker) -> (Retained<UIView>, Picture) {
    let view = Retained::into_super(BoxView::new(mtm));
    let clip = CALayer::layer();
    clip.setMasksToBounds(true);
    let layer = CALayer::layer();
    // SAFETY: an immutable Core Animation constant.
    unsafe { layer.setContentsGravity(kCAGravityResize) };
    clip.addSublayer(&layer);
    view.layer().insertSublayer_atIndex(&clip, 0);
    (
        view,
        Picture {
            clip,
            layer,
            mode: ResizeMode::Cover,
            uri: None,
            generation: 0,
            natural: None,
            mask: None,
        },
    )
}

/// Frames the picture where `objectFit` puts it, and clips that to the view.
pub(super) fn place_image(view: &UIView, style: &ViewStyle, slot: &mut Slot) {
    let Some(picture) = &mut slot.picture else {
        return;
    };
    let bounds = view.bounds();
    let size = Size::new(
        Points(bounds.size.width as f32),
        Points(bounds.size.height as f32),
    );
    let radii = style.radii().fit(size);
    let fitted = match picture.natural {
        Some(natural) => picture.mode.fit(natural, size, style.object_position),
        None => Bounds {
            origin: Point::default(),
            size: Size::default(),
        },
    };
    let clip = picture.clip.clone();
    let layer = picture.layer.clone();
    clip.setFrame(bounds);
    apply_corners(&clip, radii, clip.bounds(), &mut picture.mask);
    layer.setFrame(CGRect::new(
        CGPoint::new(fitted.origin.x.0 as CGFloat, fitted.origin.y.0 as CGFloat),
        CGSize::new(
            fitted.size.width.0 as CGFloat,
            fitted.size.height.0 as CGFloat,
        ),
    ));
}
