//! Touches and typing as UIKit receives them from the system.
//!
//! IOKit's event functions and BackBoardServices' digitizer info are
//! private. They are looked up at run time, as UI test tools for the
//! simulator do. Extracted from `nsh-platform-ios/src/synth.rs` and cut to
//! one finger and the software keyboard: no hardware modifiers, no drag
//! path, no screenshot.

use std::{ffi::c_void, sync::LazyLock};

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{MainThreadMarker, MainThreadOnly, ffi, msg_send, sel};
use objc2_ui_kit::UIWindow;

type Event = *mut HidEvent;

/// IOKit's `struct __IOHIDEvent` (opaque).
#[repr(C)]
struct HidEvent {
    _private: [u8; 0],
}

// SAFETY: a pointer to it is IOKit's `IOHIDEventRef`.
unsafe impl objc2::encode::RefEncode for HidEvent {
    const ENCODING_REF: objc2::encode::Encoding =
        objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct("?", &[]));
}

type Allocator = *const c_void;

struct Hid {
    digitizer: unsafe extern "C" fn(
        Allocator,
        u64,
        u32,
        u32,
        u32,
        u32,
        u32,
        f64,
        f64,
        f64,
        f64,
        f64,
        u8,
        u8,
        u8,
    ) -> Event,
    finger: unsafe extern "C" fn(
        Allocator,
        u64,
        u32,
        u32,
        u32,
        f64,
        f64,
        f64,
        f64,
        f64,
        u8,
        u8,
        u8,
    ) -> Event,
    append: unsafe extern "C" fn(Event, Event, u32),
    set_integer: unsafe extern "C" fn(Event, u32, i32),
    set_float: unsafe extern "C" fn(Event, u32, f64),
    set_sender: unsafe extern "C" fn(Event, u64),
    digitizer_info: unsafe extern "C" fn(Event, u32, u8, u8, *const c_void, f64, f64),
    release: unsafe extern "C" fn(*const c_void),
}

/// `IOHIDDigitizerTransducerType::Hand`.
const HAND: u32 = 3;
const RANGE: u32 = 1 << 0;
const TOUCH: u32 = 1 << 1;
const DISPLAY_INTEGRATED: u32 = (11 << 16) | 25;
const MAJOR_RADIUS: u32 = (11 << 16) | 20;
const MINOR_RADIUS: u32 = (11 << 16) | 21;
const SENDER: u64 = 0x0000_0000_4e53_4854;

fn hid() -> Option<&'static Hid> {
    static HID: LazyLock<Option<Hid>> = LazyLock::new(load);
    HID.as_ref()
}

fn load() -> Option<Hid> {
    let open = |path: &str| {
        let path = std::ffi::CString::new(path).ok()?;
        // SAFETY: a system framework, opened once and never closed.
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW) };
        (!handle.is_null()).then_some(handle)
    };
    let iokit = open("/System/Library/Frameworks/IOKit.framework/IOKit")?;
    let bbs =
        open("/System/Library/PrivateFrameworks/BackBoardServices.framework/BackBoardServices")?;
    let cf = open("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")?;
    // SAFETY: each symbol has the C signature of the field it goes into.
    unsafe {
        Some(Hid {
            digitizer: symbol(iokit, "IOHIDEventCreateDigitizerEvent")?,
            finger: symbol(iokit, "IOHIDEventCreateDigitizerFingerEvent")?,
            append: symbol(iokit, "IOHIDEventAppendEvent")?,
            set_integer: symbol(iokit, "IOHIDEventSetIntegerValue")?,
            set_float: symbol(iokit, "IOHIDEventSetFloatValue")?,
            set_sender: symbol(iokit, "IOHIDEventSetSenderID")?,
            digitizer_info: symbol(bbs, "BKSHIDEventSetDigitizerInfo")?,
            release: symbol(cf, "CFRelease")?,
        })
    }
}

/// # Safety
///
/// `F` is a function pointer type of the symbol's C signature.
unsafe fn symbol<F: Copy>(handle: *mut c_void, name: &str) -> Option<F> {
    let c = std::ffi::CString::new(name).ok()?;
    // SAFETY: looking a symbol up in a library opened before.
    let ptr = unsafe { libc::dlsym(handle, c.as_ptr()) };
    if ptr.is_null() {
        log::error!("test host: {name} is missing");
        return None;
    }
    debug_assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<*mut c_void>());
    // SAFETY: per the contract, a function pointer of the symbol's type.
    Some(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&ptr) })
}

unsafe extern "C" {
    fn mach_absolute_time() -> u64;
}

fn now() -> u64 {
    // SAFETY: a plain clock read.
    unsafe { mach_absolute_time() }
}

fn enqueue(hid: &Hid, event: Event, window: &UIWindow, mtm: MainThreadMarker) {
    // SAFETY: `_contextId` routes the event to this window, and UIKit takes
    // `_enqueueHIDEvent:` as it takes the system's events.
    unsafe {
        let context: u32 = msg_send![window, _contextId];
        (hid.set_sender)(event, SENDER);
        (hid.digitizer_info)(event, context, 0, 0, std::ptr::null(), 0.0, 0.0);
        let app = objc2_ui_kit::UIApplication::sharedApplication(mtm);
        // Not `msg_send!`: the event is an IOKit object, and the selector is private.
        let send: unsafe extern "C-unwind" fn(&objc2_ui_kit::UIApplication, Sel, Event) =
            std::mem::transmute(ffi::objc_msgSend as unsafe extern "C-unwind" fn());
        send(&app, sel!(_enqueueHIDEvent:), event);
        (hid.release)(event.cast_const().cast());
    }
}

/// A finger's phase.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Down,
    Up,
}

/// One finger at `(x, y)` in `window`'s points.
pub(crate) fn touch(window: &UIWindow, x: f64, y: f64, phase: Phase) -> bool {
    let Some(hid) = hid() else { return false };
    let mtm = window.mtm();
    let touching = u8::from(phase != Phase::Up);
    let mask = RANGE | TOUCH;
    let time = now();
    // SAFETY: IOKit's constructors with their documented arguments.
    unsafe {
        let hand = (hid.digitizer)(
            std::ptr::null(),
            time,
            HAND,
            0,
            0,
            mask,
            0,
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
            touching,
            touching,
            0,
        );
        if hand.is_null() {
            return false;
        }
        (hid.set_integer)(hand, DISPLAY_INTEGRATED, 1);
        let finger = (hid.finger)(
            std::ptr::null(),
            time,
            1,
            2,
            mask,
            x,
            y,
            0.0,
            f64::from(touching),
            0.0,
            touching,
            touching,
            0,
        );
        if !finger.is_null() {
            (hid.set_float)(finger, MAJOR_RADIUS, 5.0);
            (hid.set_float)(finger, MINOR_RADIUS, 5.0);
            (hid.set_integer)(finger, DISPLAY_INTEGRATED, 1);
            (hid.append)(hand, finger, 0);
            (hid.release)(finger.cast_const().cast());
        }
        enqueue(hid, hand, window, mtm);
    }
    true
}

fn keyboard() -> Option<Retained<AnyObject>> {
    let class = AnyClass::get(c"UIKeyboardImpl")?;
    // SAFETY: `activeInstance` is the class's singleton getter. +0, so retain it.
    unsafe { retain0((class as *const AnyClass).cast(), sel!(activeInstance)) }
}

/// Types `text` into the first responder as the software keyboard does.
pub(crate) fn insert_text(window: &UIWindow, text: &str) {
    let s = objc2_foundation::NSString::from_str(text);
    // The software keyboard calls this on the focused field. Prefer it: the
    // keyboard singleton exists before it is attached, and its insert is a no-op then.
    if let Some(responder) =
        unsafe { retain0((window as *const UIWindow).cast(), sel!(firstResponder)) }
    {
        // SAFETY: UIKeyInput's method, which a text field implements.
        unsafe { msg_void(&responder, sel!(insertText:), &s) };
        return;
    }
    if let Some(keyboard) = keyboard() {
        // SAFETY: UIKeyboardImpl's method for a typed string. Private, so not `msg_send!`.
        unsafe { msg_void(&keyboard, sel!(addInputString:), &s) };
    } else {
        log::warn!("test host: no focused input to type into");
    }
}

/// # Safety
///
/// `sel` is a zero-argument getter that returns an object at +0.
unsafe fn retain0(obj: *const AnyObject, sel: Sel) -> Option<Retained<AnyObject>> {
    unsafe {
        let send: unsafe extern "C-unwind" fn(*const AnyObject, Sel) -> *mut AnyObject =
            std::mem::transmute(ffi::objc_msgSend as unsafe extern "C-unwind" fn());
        Retained::retain(send(obj, sel))
    }
}

/// # Safety
///
/// `sel` takes one object and returns void.
unsafe fn msg_void(obj: &AnyObject, sel: Sel, arg: &objc2_foundation::NSString) {
    unsafe {
        let send: unsafe extern "C-unwind" fn(&AnyObject, Sel, &objc2_foundation::NSString) =
            std::mem::transmute(ffi::objc_msgSend as unsafe extern "C-unwind" fn());
        send(obj, sel, arg);
    }
}
