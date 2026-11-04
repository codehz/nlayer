package dev.nlayer.android;

import android.animation.Animator;
import android.animation.AnimatorListenerAdapter;
import android.annotation.SuppressLint;
import android.app.Dialog;
import android.content.Context;
import android.graphics.drawable.ColorDrawable;
import android.os.Build;
import android.view.KeyEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowManager;
import android.view.animation.DecelerateInterpolator;
import android.widget.FrameLayout;

/**
 * A modal dialog: content presented in its own window over the app. The core lays the content out
 * at the size this window reports. The content starts below the status bar; the top inset the core
 * sees is what remains after that (a cutout taller than the status bar).
 */
final class ModalHost {
  final Mounter mounter;
  final int id;

  /** The modal's root view, placed by the core. */
  final Box root;

  private final Container container;
  Dialog dialog;
  int animation; // 0 slide, 1 fade, 2 none
  boolean closeOnEscape = true;
  boolean closeOnBackdropPress = true;
  boolean closeOnSwipe = true;
  private boolean closing;

  /** The dialog window's keyboard inset (IME bottom), in pixels. */
  int keyboardInset;

  /** Show order for topmost determination (from mounter.modalShowOrder). */
  int showOrder;

  ModalHost(Context context, Mounter mounter, int id) {
    this.mounter = mounter;
    this.id = id;
    root = new Box(context, mounter, id);
    container = new Container(context);
  }

  boolean shown() {
    return dialog != null && !closing;
  }

  void show() {
    if (dialog != null && !closing) return;
    closing = false;
    Context context = mounter.host.getContext();
    dialog = new Dialog(context, android.R.style.Theme_DeviceDefault_Light_NoActionBar);
    if (container.getParent() instanceof ViewGroup)
      ((ViewGroup) container.getParent()).removeView(container);
    dialog.setContentView(container);
    Window window = dialog.getWindow();
    if (window != null) {
      window.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT);
      window.setBackgroundDrawable(new ColorDrawable(0));
      window.setWindowAnimations(0);
      window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
      window.addFlags(WindowManager.LayoutParams.FLAG_DRAWS_SYSTEM_BAR_BACKGROUNDS);
      window.setStatusBarColor(0);
      window.setNavigationBarColor(0);
      if (Build.VERSION.SDK_INT >= 30) window.setDecorFitsSystemWindows(false);
      else
        window
            .getDecorView()
            .setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
    }
    container.setBackgroundColor(0);
    dialog.setCancelable(false);
    if (Build.VERSION.SDK_INT < 33) {
      dialog.setOnKeyListener(
          (d, keyCode, event) -> {
            if (keyCode == KeyEvent.KEYCODE_BACK && event.getAction() == KeyEvent.ACTION_UP) {
              mounter.host.back();
              return true;
            }
            return keyCode == KeyEvent.KEYCODE_BACK;
          });
    }
    if (Build.VERSION.SDK_INT >= 33) {
      dialog
          .getOnBackInvokedDispatcher()
          .registerOnBackInvokedCallback(
              android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, () -> mounter.host.back());
    }
    dialog.show();
    showOrder = ++mounter.modalShowOrder;
    mounter.host.updateKeyboard();
    matchSystemBars(window);
    container.setAlpha(animation == 1 ? 0 : 1);
    container.post(
        () -> {
          if (animation == 2) {
            long handle = mounter.host.handle();
            if (handle != 0) Native.modalShow(handle, id);
            return;
          }
          if (animation == 0) container.setTranslationY(container.getHeight());
          container
              .animate()
              .alpha(1)
              .translationY(0)
              .setDuration(300)
              .setInterpolator(new DecelerateInterpolator(1.5f))
              .setListener(
                  new AnimatorListenerAdapter() {
                    @Override
                    public void onAnimationEnd(Animator a) {
                      container.animate().setListener(null);
                      container.setAlpha(1);
                      container.setTranslationY(0);
                      long handle = mounter.host.handle();
                      if (handle != 0) Native.modalShow(handle, id);
                    }
                  })
              .start();
        });
  }

  /** Match the activity's status and navigation bar icon appearance. */
  @SuppressWarnings("deprecation")
  private void matchSystemBars(Window window) {
    if (window == null) return;
    View decor = window.getDecorView();
    int flags = decor.getSystemUiVisibility();
    boolean dark = mounter.host.dark;
    // Light icons on dark scheme, dark icons on light scheme
    if (Build.VERSION.SDK_INT >= 23) {
      if (dark) flags &= ~View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR;
      else flags |= View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR;
    }
    if (Build.VERSION.SDK_INT >= 26) {
      if (dark) flags &= ~View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
      else flags |= View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
    }
    decor.setSystemUiVisibility(flags);
  }

  void dismiss() {
    if (dialog == null || closing) return;
    closing = true;
    keyboardInset = 0;
    mounter.host.updateKeyboard();
    Dialog d = dialog;
    Runnable done =
        () -> {
          mounter.releaseClosing(ModalHost.this);
          d.dismiss();
          if (dialog == d) dialog = null;
          closing = false;
        };
    if (animation == 2) {
      done.run();
      return;
    }
    // The app already dropped the element. Keep this window's views until
    // the motion ends; the batch's deletes would otherwise empty it first.
    mounter.retainClosing(this);
    container
        .animate()
        .alpha(animation == 1 ? 0 : 1)
        .translationY(animation == 0 ? container.getHeight() : 0)
        .setDuration(250)
        .setListener(
            new AnimatorListenerAdapter() {
              @Override
              public void onAnimationEnd(Animator a) {
                container.animate().setListener(null);
                done.run();
              }
            })
        .start();
  }

  /** `v` is this dialog's content, so an exit motion still has to draw it. */
  boolean holds(View v) {
    for (android.view.ViewParent p = v.getParent(); p instanceof View; p = p.getParent()) {
      if (p == container) return true;
    }
    return false;
  }

  void configure(
      int animation,
      int presentation,
      boolean closeOnEscape,
      boolean closeOnBackdropPress,
      boolean closeOnSwipe) {
    this.animation = animation;
    // presentation is ignored on Android but kept
    this.closeOnEscape = closeOnEscape;
    this.closeOnBackdropPress = closeOnBackdropPress;
    this.closeOnSwipe = closeOnSwipe;
  }

  /** Edge-to-edge container. Content is laid out below the status bar. */
  @SuppressLint("ViewConstructor")
  private final class Container extends FrameLayout {
    private int[] reported = {0, 0, 0, 0, 0, 0};

    /** Holds {@code root}. Its top is the status bar, so core y=0 is already clear of it. */
    final FrameLayout content;

    Container(Context context) {
      super(context);
      content = new FrameLayout(context);
      content.addView(root);
      addView(content);
    }

    @Override
    public boolean dispatchTouchEvent(android.view.MotionEvent event) {
      mounter.observePress(this, event);
      return super.dispatchTouchEvent(event);
    }

    @Override
    protected void onMeasure(int widthSpec, int heightSpec) {
      int w = MeasureSpec.getSize(widthSpec), h = MeasureSpec.getSize(heightSpec);
      setMeasuredDimension(w, h);
      int ch = Math.max(0, h - statusBar());
      content.measure(
          MeasureSpec.makeMeasureSpec(w, MeasureSpec.EXACTLY),
          MeasureSpec.makeMeasureSpec(ch, MeasureSpec.EXACTLY));
    }

    /** The status bar's height: content starts below it. */
    private int statusBar() {
      WindowInsets insets = getRootWindowInsets();
      if (insets == null) return 0;
      if (Build.VERSION.SDK_INT >= 30) return insets.getInsets(WindowInsets.Type.statusBars()).top;
      return insets.getSystemWindowInsetTop();
    }

    @Override
    protected void onLayout(boolean changed, int l, int t, int r, int b) {
      int below = statusBar();
      content.layout(0, below, r - l, b - t);
      report();
    }

    @Override
    public WindowInsets onApplyWindowInsets(WindowInsets insets) {
      report();
      trackKeyboard(insets);
      return super.onApplyWindowInsets(insets);
    }

    /**
     * Tracks the keyboard (IME) inset from this dialog's window. On API 30+, uses Type.ime(). On
     * API 28-29, measures the visible frame overlap (matching HostActivity.keyboardOverlap).
     */
    private void trackKeyboard(WindowInsets insets) {
      int kb = 0;
      if (insets != null && Build.VERSION.SDK_INT >= 30) {
        kb = insets.getInsets(WindowInsets.Type.ime()).bottom;
      } else if (insets != null && Build.VERSION.SDK_INT >= 28) {
        // API 28-29: use visible-frame math like HostActivity
        kb = HostActivity.keyboardOverlap(this, insets);
      }
      if (kb != keyboardInset) {
        keyboardInset = kb;
        mounter.host.updateKeyboard();
      }
    }

    private void report() {
      WindowInsets insets = getRootWindowInsets();
      int top = 0, right = 0, bottom = 0, left = 0;
      if (insets != null) {
        if (Build.VERSION.SDK_INT >= 30) {
          android.graphics.Insets bars =
              insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
          top = bars.top;
          right = bars.right;
          bottom = bars.bottom;
          left = bars.left;
        } else {
          top = insets.getSystemWindowInsetTop();
          right = insets.getSystemWindowInsetRight();
          bottom = insets.getSystemWindowInsetBottom();
          left = insets.getSystemWindowInsetLeft();
        }
      }
      int below = statusBar();
      top = Math.max(0, top - below);
      int[] now = {getWidth(), Math.max(0, getHeight() - below), top, right, bottom, left};
      if (now[0] == 0 || now[1] == 0 || java.util.Arrays.equals(now, reported)) return;
      reported = now;
      float d = mounter.density;
      long handle = mounter.host.handle();
      if (handle != 0) {
        Native.rootSize(
            handle, id, now[0] / d, now[1] / d, top / d, right / d, bottom / d, left / d);
      }
    }
  }
}
