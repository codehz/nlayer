package dev.nlayer.android;

import android.content.Context;
import android.graphics.Color;
import android.os.Handler;
import android.os.Looper;
import android.util.AttributeSet;
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

  public HostView(Context c) {
    this(c, null);
  }

  public HostView(Context c, AttributeSet a) {
    super(c, a);
    density = getResources().getDisplayMetrics().density;
    setFocusable(true);
    mounter = new Mounter(this);
  }

  void start() {
    if (handle == 0) handle = Native.create(this, density);
  }

  public void submit(final byte[] bytes, final Object[] objects) {
    main.post(
        new Runnable() {
          public void run() {
            mounter.apply(bytes, objects == null ? new Object[0] : objects);
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
      Native.load(handle, code.getBytes(java.nio.charset.StandardCharsets.UTF_8), assetsDir);
      code = null;
    }
    if (!served && pin != null && pin.port != 0) {
      served = true;
      Native.serve(handle, pin.host, pin.port);
    }
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
