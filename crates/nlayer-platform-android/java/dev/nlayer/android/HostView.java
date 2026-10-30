package dev.nlayer.android;

import android.content.Context;
import android.graphics.Color;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.util.AttributeSet;
import android.view.Choreographer;
import android.view.Gravity;
import android.widget.FrameLayout;
import java.io.File;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;

public final class HostView extends FrameLayout {
  final float density;
  final Mounter mounter;
  final Handler main = new Handler(Looper.getMainLooper());
  Pin pin;
  String bootError;
  long downTime;
  private long handle;
  private String code, assetsDir;
  private int safeTop, safeRight, safeBottom, safeLeft, keyboard;
  private boolean served;
  private boolean framePosted;
  private boolean inFrame;
  private static final Object[] NO_OBJECTS = new Object[0];

  /** The engine is running: a back button can reach it. */
  private boolean started;

  /** The system's appearance, until a test pin says otherwise. */
  boolean dark;

  public HostView(Context c) {
    this(c, null);
  }

  public HostView(Context c, AttributeSet a) {
    super(c, a);
    density = getResources().getDisplayMetrics().density;
    setFocusable(true);
    // Holds focus when no input does. In touch mode a view that is only
    // focusable refuses requestFocus, and the search then lands on the first
    // field. clearFocus() does the same.
    setFocusableInTouchMode(true);
    if (Build.VERSION.SDK_INT >= 26) setDefaultFocusHighlightEnabled(false);
    setDescendantFocusability(FOCUS_BEFORE_DESCENDANTS);
    mounter = new Mounter(this);
  }

  private final Choreographer.FrameCallback animationFrame =
      new Choreographer.FrameCallback() {
        public void doFrame(long frameTimeNanos) {
          framePosted = false;
          inFrame = true;
          byte[] bytes = Native.tick(handle, frameTimeNanos);
          if (bytes != null && bytes.length > 0) mounter.apply(bytes, NO_OBJECTS);
          inFrame = false;
          animationChanged();
        }
      };

  /** Schedules the animation clock while the graph runs. */
  void animationChanged() {
    if (handle == 0 || inFrame || framePosted) return;
    if (!Native.animating(handle)) return;
    framePosted = true;
    Choreographer.getInstance().postFrameCallback(animationFrame);
  }

  /** `env(keyboard-inset-height)`, in points. A test pin replaces the live IME. */
  float keyboardInsetPoints() {
    if (pin != null) return pin.keyboard;
    return density == 0 ? 0 : keyboard / density;
  }

  /** The animation graph's `onLongPress` reached the app. */
  void longPress(int id) {
    if (handle != 0) Native.longPress(handle, id);
  }

  void dismissKeyboard() {
    android.view.View focused = findFocus();
    android.view.View token = focused != null && focused != this ? focused : this;
    android.view.inputmethod.InputMethodManager imm =
        (android.view.inputmethod.InputMethodManager)
            getContext().getSystemService(Context.INPUT_METHOD_SERVICE);
    if (imm != null) imm.hideSoftInputFromWindow(token.getWindowToken(), 0);
    requestFocus();
  }

  void start() {
    if (handle == 0) handle = Native.create(this, density);
  }

  public void submit(final byte[] bytes, final Object[] objects) {
    main.post(
        new Runnable() {
          public void run() {
            mounter.apply(bytes, objects == null ? new Object[0] : objects);
            animationChanged();
          }
        });
  }

  public void reportError(final String message) {
    bootError = message;
    main.post(
        new Runnable() {
          public void run() {
            removeAllViews();
            android.widget.TextView error = new android.widget.TextView(getContext());
            error.setText(message);
            error.setTextColor(Color.WHITE);
            error.setBackgroundColor(0xff8b0000);
            error.setPadding(16, 16, 16, 16);
            addView(
                error,
                new LayoutParams(
                    LayoutParams.MATCH_PARENT, LayoutParams.WRAP_CONTENT, Gravity.TOP));
          }
        });
  }

  public void load(String code, String assetsDir) {
    this.code = code;
    this.assetsDir = assetsDir;
    File files =
        assetsDir == null ? getContext().getFilesDir() : new File(assetsDir).getParentFile();
    pin = Pin.read(new File(files, "nlayer-test.json"));
    start();
    if (getWidth() > 0 && getHeight() > 0) finishLoad();
  }

  private void finishLoad() {
    if (pin != null) {
      Native.resize(handle, pin.width, pin.height);
      Native.insets(handle, pin.top, pin.right, pin.bottom, pin.left, pin.keyboard);
    }
    if (code != null) {
      boolean scheme = dark;
      if (pin != null && pin.colorScheme != null) scheme = pin.colorScheme.equals("dark");
      started = true;
      Native.load(
          handle,
          code.getBytes(java.nio.charset.StandardCharsets.UTF_8),
          assetsDir,
          dataDir(),
          scheme);
      code = null;
    }
    if (!served && pin != null && pin.port != 0) {
      served = true;
      Native.serve(handle, pin.host, pin.port);
    }
  }

  /** Where the app's storage lives: `nlayer` in the private files directory. */
  String dataDir() {
    return new File(getContext().getFilesDir(), "nlayer").getAbsolutePath();
  }

  String assetsPath() {
    return assetsDir == null ? getContext().getFilesDir().getAbsolutePath() + "/assets" : assetsDir;
  }

  @Override
  protected void onSizeChanged(int w, int h, int ow, int oh) {
    super.onSizeChanged(w, h, ow, oh);
    if (w <= 0 || h <= 0) return;
    start();
    if (pin == null) {
      Native.resize(handle, w / density, h / density);
      Native.insets(
          handle,
          safeTop / density,
          safeRight / density,
          safeBottom / density,
          safeLeft / density,
          keyboard / density);
    }
    finishLoad();
  }

  void setInsets(int t, int r, int b, int l, int k) {
    if (pin != null) return;
    safeTop = t;
    safeRight = r;
    safeBottom = b;
    safeLeft = l;
    keyboard = k;
    if (handle != 0)
      Native.insets(handle, t / density, r / density, b / density, l / density, k / density);
  }

  /** The system's color scheme changed (`onConfigurationChanged`). */
  public void setColorScheme(boolean dark) {
    this.dark = dark;
    // A test pin replaces the system for the run.
    if (pin != null && pin.colorScheme != null) return;
    Native.colorScheme(handle, dark);
  }

  /** The back button reached the app. The engine reports it to `BackHandler`. */
  public boolean back() {
    if (!started) return false;
    Native.back(handle);
    return true;
  }

  /** No `BackHandler` handled the back: Android sends the task to the background. */
  public void backUnhandled() {
    post(
        new Runnable() {
          public void run() {
            android.app.Activity activity = (android.app.Activity) getContext();
            activity.moveTaskToBack(true);
          }
        });
  }

  public String snapshot() {
    return onMain(
        new Main<String>() {
          public String run() {
            return Tree.snapshot(HostView.this);
          }
        });
  }

  public void tap(final float x, final float y, final boolean up) {
    onMain(
        new Main<Void>() {
          public Void run() {
            Tree.tap(HostView.this, x, y, up);
            return null;
          }
        });
  }

  public void typeChar(final int id, final String text) {
    onMain(
        new Main<Void>() {
          public Void run() {
            Tree.typeChar(HostView.this, id, text);
            return null;
          }
        });
  }

  public void scrollTo(final int id, final float x, final float y) {
    onMain(
        new Main<Void>() {
          public Void run() {
            Tree.scrollTo(HostView.this, id, x, y);
            return null;
          }
        });
  }

  private interface Main<T> {
    T run();
  }

  private <T> T onMain(final Main<T> work) {
    if (Looper.myLooper() == Looper.getMainLooper()) return work.run();
    final Object[] box = new Object[1];
    final CountDownLatch latch = new CountDownLatch(1);
    main.post(
        new Runnable() {
          public void run() {
            box[0] = work.run();
            latch.countDown();
          }
        });
    try {
      latch.await(5, TimeUnit.SECONDS);
    } catch (InterruptedException e) {
      Thread.currentThread().interrupt();
    }
    @SuppressWarnings("unchecked")
    T value = (T) box[0];
    return value;
  }

  long handle() {
    return handle;
  }

  void destroy() {
    if (handle != 0) {
      Native.destroy(handle);
      handle = 0;
    }
  }
}
