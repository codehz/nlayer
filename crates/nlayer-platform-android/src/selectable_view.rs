// Copyright 2026 the nlayer authors. All rights reserved. Apache-2.0 license.
//
// Derived from nsh-platform-android/src/selectable_view.rs.
// Copyright 2024 Longbridge Whale Wealth Ltd.

#![allow(unreachable_pub)]

//! Selectable views (`View` `selectable`) on the UI thread: the shared model
//! ([`nlayer_core::selection`]) behind `SelectableView.java`.
//!
//! Java owns the views, the gestures and the drawing; the model (which
//! text, which offset, words, handles, what a copy gives) is Rust's, as on
//! every backend, so it is not written twice. Each area with a selection
//! holds one [`Area`] (a `long` handle in Java, from `nativeCreate` to
//! `nativeDestroy`). Java hands over the texts' content when they change
//! (`nativeSetTexts`) and their frames with every call (they move with
//! scrolling); hit-testing calls back into Java (`SelectableView.offsetAt`
//! turns the point into the text's content box and asks
//! `TextLayouts.hitTest`, which the engine thread's
//! `TextEngine::hit_test` runs too), all on the UI thread within the native
//! call.
//! Coordinates are the area's, in points (dp), origin at its top left.
//!
//! A single text's word and its copy (`nativeWord`, `nativeCopy`) use the
//! same model, so Java does not keep a second word breaker.

use std::cell::RefCell;

use jni::{
    Env, EnvUnowned, JValue,
    errors::LogErrorAndDefault,
    jni_sig, jni_str,
    objects::{JClass, JFloatArray, JIntArray, JObject, JObjectArray, JString},
    sys::{jboolean, jfloat, jint, jlong},
};
use nlayer_core::geometry::{Bounds, Point, Points, bounds, point, px, size};
use nlayer_core::selection::{Handle, SelectionModel, SelectionTexts, reading_order, reorder};

/// The selection of one area and what it knows of the area's texts.
#[derive(Default)]
struct Area {
    model: SelectionModel,
    contents: Vec<String>,
    /// The texts' view ids: their identity across `texts_changed`.
    keys: Vec<u64>,
    frames: Vec<Bounds<Points>>,
}

/// The texts of an area for one native call: hit-tests ask the Java
/// `SelectableView` (`this`).
struct Texts<'a, 'local> {
    contents: &'a [String],
    keys: &'a [u64],
    frames: &'a [Bounds<Points>],
    env: RefCell<&'a mut Env<'local>>,
    this: &'a JObject<'local>,
}

impl SelectionTexts for Texts<'_, '_> {
    fn len(&self) -> usize {
        self.contents.len()
    }

    fn content(&self, i: usize) -> &str {
        &self.contents[i]
    }

    fn frame(&self, i: usize) -> Bounds<Points> {
        self.frames.get(i).copied().unwrap_or_default()
    }

    fn key(&self, i: usize) -> u64 {
        self.keys.get(i).copied().unwrap_or(i as u64)
    }

    fn offset_at(&self, i: usize, p: Point<Points>) -> u32 {
        let mut env = self.env.borrow_mut();
        let offset = env
            .call_method(
                self.this,
                jni_str!("offsetAt"),
                jni_sig!("(IFF)I"),
                &[
                    JValue::Int(i as jint),
                    JValue::Float(p.x.0),
                    JValue::Float(p.y.0),
                ],
            )
            .and_then(|v| v.i());
        match offset {
            Ok(offset) => offset.max(0) as u32,
            Err(err) => {
                if env.exception_check() {
                    env.exception_describe();
                    env.exception_clear();
                }
                log::error!("selection hit test failed: {err}");
                0
            }
        }
    }
}

/// One string, for a text block's own word or copy. `offset_at` reads the
/// point's x as the UTF-16 caret (`nativeCopy` places carets that way);
/// `at` is the caret `nativeWord` asks for, whatever the point is.
struct OneText<'a> {
    content: &'a str,
    at: Option<u32>,
}

impl SelectionTexts for OneText<'_> {
    fn len(&self) -> usize {
        1
    }

    fn content(&self, _: usize) -> &str {
        self.content
    }

    fn frame(&self, _: usize) -> Bounds<Points> {
        bounds(point(px(0.0), px(0.0)), size(px(1.0e6), px(1.0e6)))
    }

    fn offset_at(&self, _: usize, p: Point<Points>) -> u32 {
        self.at.unwrap_or(p.x.0.max(0.0) as u32)
    }
}

/// The area behind a handle from `nativeCreate`.
///
/// # Safety
///
/// `handle` is live: created by `nativeCreate`, not yet destroyed, and
/// used only on the UI thread.
unsafe fn area<'a>(handle: jlong) -> &'a mut Area {
    // SAFETY: the caller's contract.
    unsafe { &mut *(handle as *mut Area) }
}

/// Frames as Java sends them: x, y, width, height per text (points).
fn read_frames(env: &Env<'_>, frames: &JFloatArray<'_>, out: &mut Vec<Bounds<Points>>) {
    out.clear();
    let len = frames.len(env).unwrap_or(0);
    let mut values = vec![0.0f32; len];
    if frames.get_region(env, 0, &mut values).is_err() {
        return;
    }
    out.extend(
        values
            .chunks_exact(4)
            .map(|f| bounds(point(px(f[0]), px(f[1])), size(px(f[2]), px(f[3])))),
    );
}

/// Runs `f` with the area's model and its texts.
fn with_texts<'local, R>(
    env: &mut Env<'local>,
    this: &JObject<'local>,
    area: &mut Area,
    f: impl FnOnce(&mut SelectionModel, &Texts<'_, 'local>) -> R,
) -> R {
    let Area {
        model,
        contents,
        keys,
        frames,
    } = area;
    let texts = Texts {
        contents,
        keys,
        frames,
        env: RefCell::new(env),
        this,
    };
    f(model, &texts)
}

/// A new area without a selection.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_nlayer_android_SelectableView_nativeCreate(
    _: EnvUnowned<'_>,
    _: JClass<'_>,
) -> jlong {
    Box::into_raw(Box::<Area>::default()) as jlong
}

/// Frees an area's handle.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_nlayer_android_SelectableView_nativeDestroy(
    _: EnvUnowned<'_>,
    _: JClass<'_>,
    handle: jlong,
) {
    if handle != 0 {
        // SAFETY: Java passes a handle it never uses again.
        drop(unsafe { Box::from_raw(handle as *mut Area) });
    }
}

/// The area's texts, in document order, are now these (content, view
/// ids and frames): the selection follows (`texts_changed`). Returns whether it
/// changed.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_nlayer_android_SelectableView_nativeSetTexts<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    handle: jlong,
    this: JObject<'local>,
    contents: JObjectArray<'local, JString<'local>>,
    keys: JIntArray<'local>,
    frames: JFloatArray<'local>,
) -> jboolean {
    env.with_env(|env| -> jni::errors::Result<jboolean> {
        // SAFETY: Java passes its live handle, on the UI thread.
        let area = unsafe { area(handle) };
        let read = (|| -> jni::errors::Result<Vec<String>> {
            let count = contents.len(env)?;
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let element = contents.get_element(env, i)?;
                out.push(element.try_to_string(env)?);
                env.delete_local_ref(element);
            }
            Ok(out)
        })();
        match read {
            Ok(contents) => area.contents = contents,
            Err(err) => {
                log::error!("cannot read the selectable view's texts: {err}");
                area.contents.clear();
            }
        }
        let len = keys.len(env).unwrap_or(0);
        let mut ids = vec![0; len];
        if keys.get_region(env, 0, &mut ids).is_err() {
            ids.clear();
        }
        area.keys = ids.into_iter().map(|id| id as u32 as u64).collect();
        read_frames(env, &frames, &mut area.frames);
        // Frames for every text, even if Java's array falls short.
        area.frames.resize(area.contents.len(), Bounds::default());
        Ok(with_texts(env, &this, area, |model, texts| {
            model.texts_changed(texts)
        }))
    })
    .resolve::<LogErrorAndDefault>()
}

/// What `nativeGesture`'s `kind` asks.
const SELECT_WORD: jint = 0;
const DRAG: jint = 1;
const MOVE_START: jint = 2;
const MOVE_END: jint = 3;
const SELECT_ALL: jint = 4;
const CLEAR: jint = 5;

/// A gesture at (`x`, `y`) with the texts' current `frames`: select the
/// word there (a long press), drag the focus there (the long press goes
/// on), move the start or end handle there, select all, clear. Returns
/// whether the selection changed.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_dev_nlayer_android_SelectableView_nativeGesture<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    handle: jlong,
    this: JObject<'local>,
    frames: JFloatArray<'local>,
    kind: jint,
    x: jfloat,
    y: jfloat,
) -> jboolean {
    env.with_env(|env| -> jni::errors::Result<jboolean> {
        // SAFETY: Java passes its live handle, on the UI thread.
        let area = unsafe { area(handle) };
        read_frames(env, &frames, &mut area.frames);
        area.frames.resize(area.contents.len(), Bounds::default());
        let at = point(px(x), px(y));
        Ok(with_texts(env, &this, area, |model, texts| match kind {
            SELECT_WORD => model.select_word_at(texts, at),
            DRAG => model.drag(texts, at),
            MOVE_START => model.move_handle(texts, Handle::Start, at),
            MOVE_END => model.move_handle(texts, Handle::End, at),
            SELECT_ALL => model.select_all(texts),
            CLEAR => model.clear(),
            _ => false,
        }))
    })
    .resolve::<LogErrorAndDefault>()
}

/// What each text has selected: (text, start, end) triples, UTF-16.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_nlayer_android_SelectableView_nativeRanges<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    handle: jlong,
    this: JObject<'local>,
) -> JIntArray<'local> {
    env.with_env(|env| -> jni::errors::Result<JIntArray<'local>> {
        // SAFETY: Java passes its live handle, on the UI thread.
        let area = unsafe { area(handle) };
        let ranges = with_texts(env, &this, area, |model, texts| model.ranges(texts));
        let ints: Vec<jint> = ranges
            .iter()
            .flat_map(|(i, r)| [*i as jint, r.start as jint, r.end as jint])
            .collect();
        Ok(ints_array(env, &ints))
    })
    .resolve::<LogErrorAndDefault>()
}

/// The selection's start and end (text, offset, text, offset), or an
/// empty array without one.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_nlayer_android_SelectableView_nativeBounds<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    handle: jlong,
) -> JIntArray<'local> {
    env.with_env(|env| -> jni::errors::Result<JIntArray<'local>> {
        // SAFETY: Java passes its live handle, on the UI thread.
        let area = unsafe { area(handle) };
        let ints: Vec<jint> = match area.model.bounds() {
            Some((start, end)) if !area.model.is_empty() => vec![
                start.text as jint,
                start.offset as jint,
                end.text as jint,
                end.offset as jint,
            ],
            _ => Vec::new(),
        };
        Ok(ints_array(env, &ints))
    })
    .resolve::<LogErrorAndDefault>()
}

/// The selected text, as a copy gives it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_nlayer_android_SelectableView_nativeText<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    handle: jlong,
    this: JObject<'local>,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        // SAFETY: Java passes its live handle, on the UI thread.
        let area = unsafe { area(handle) };
        let text = with_texts(env, &this, area, |model, texts| model.text(texts));
        env.new_string(text)
    })
    .resolve::<LogErrorAndDefault>()
}

/// The word at UTF-16 `offset` of `content`: start and end, or empty when
/// there is nothing to select. A text block's long press asks this; the
/// breaker is the shared model's.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_nlayer_android_SelectableView_nativeWord<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    content: JString<'local>,
    offset: jint,
) -> JIntArray<'local> {
    env.with_env(|env| -> jni::errors::Result<JIntArray<'local>> {
        let content = content.try_to_string(env)?;
        let texts = OneText {
            content: &content,
            at: Some(offset.max(0) as u32),
        };
        let mut model = SelectionModel::new();
        model.select_word_at(&texts, point(px(0.0), px(0.0)));
        let ints: Vec<jint> = model
            .ranges(&texts)
            .into_iter()
            .flat_map(|(_, range)| [range.start as jint, range.end as jint])
            .collect();
        Ok(ints_array(env, &ints))
    })
    .resolve::<LogErrorAndDefault>()
}

/// What a copy of `content`'s UTF-16 range [`start`, `end`) gives: the
/// shared model's text, attachments left out.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_nlayer_android_SelectableView_nativeCopy<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    content: JString<'local>,
    start: jint,
    end: jint,
) -> JString<'local> {
    env.with_env(|env| -> jni::errors::Result<JString<'local>> {
        let content = content.try_to_string(env)?;
        let texts = OneText {
            content: &content,
            at: None,
        };
        let mut model = SelectionModel::new();
        let start = start.max(0) as f32;
        let end = end.max(0) as f32;
        model.press(&texts, point(px(start), px(0.0)), 1, false);
        model.drag(&texts, point(px(end), px(0.0)));
        env.new_string(model.text(&texts))
    })
    .resolve::<LogErrorAndDefault>()
}

/// The order of the texts Java listed depth first (`SelectableView.collect`,
/// in mount order), from its `runs`: for each view group, after its
/// children's, a count of children and each child's frame and range of
/// texts (`left, top, width, height, from, to`). The shared rule
/// ([`reading_order`]) puts a virtual list's recycled cells in reading
/// order. Returns the texts' original indices in their new order, or null
/// when the order stays.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_nlayer_android_SelectableView_nativeReadingOrder<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    count: jint,
    runs: JFloatArray<'local>,
) -> JIntArray<'local> {
    env.with_env(|env| -> jni::errors::Result<JIntArray<'local>> {
        let len = runs.len(env).unwrap_or(0);
        let mut data = vec![0.0; len];
        if runs.get_region(env, 0, &mut data).is_err() {
            return Ok(JIntArray::null());
        }
        let mut texts: Vec<jint> = (0..count.max(0)).collect();
        let mut moved = false;
        let mut at = 0;
        while at < data.len() {
            let n = data[at] as usize;
            at += 1;
            let Some(group) = data.get(at..at + n * 6) else {
                break;
            };
            at += n * 6;
            let group: Vec<_> = group
                .chunks_exact(6)
                .map(|r| {
                    (
                        bounds(point(px(r[0]), px(r[1])), size(px(r[2]), px(r[3]))),
                        r[4] as usize..r[5] as usize,
                    )
                })
                .collect();
            let start = group.first().map_or(0, |(_, r)| r.start);
            if group.iter().all(|(_, r)| r.end <= texts.len())
                && let Some(order) = reading_order(&group)
            {
                reorder(&mut texts, start, &order);
                moved = true;
            }
        }
        if moved {
            Ok(ints_array(env, &texts))
        } else {
            Ok(JIntArray::null())
        }
    })
    .resolve::<LogErrorAndDefault>()
}

fn ints_array<'local>(env: &mut Env<'local>, values: &[jint]) -> JIntArray<'local> {
    let Ok(array) = JIntArray::new(env, values.len()) else {
        return JIntArray::null();
    };
    if array.set_region(env, 0, values).is_err() {
        return JIntArray::null();
    }
    array
}
