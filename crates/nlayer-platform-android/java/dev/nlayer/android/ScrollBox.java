package dev.nlayer.android;

import android.content.Context;
import android.view.Choreographer;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.HorizontalScrollView;
import android.widget.ProgressBar;
import android.widget.ScrollView;

/**
 * One-axis scroll view. Drag and fling callbacks match UIKit's scroll delegate, with pull-to-refresh.
 * Event kinds are {@code EventKind} discriminants. Each callback carries the
 * content offset, the content size and this view's visible size, in points. {@code onScroll} is at
 * most once per frame. There is no decelerating callback: rest is no movement for a moment after
 * the finger lifts. An iOS bounce can report a negative offset; this view does not (but pull reports negative).
 */
final class ScrollBox extends FrameLayout {
  private static final int SCROLL = 1;
  private static final int SCROLL_END = 2;
  private static final int DRAG_START = 3;
  private static final int DRAG_END = 4;
  private static final int MOMENTUM_START = 5;
  private static final int MOMENTUM_END = 6;
  private static final int REFRESH = 7;
  private static final int REST_MS = 120;

  final int id;
  final Decor decor = new Decor();
  boolean horizontal;
  ViewGroup scroller;
  final Box content;
  int contentW, contentH;
  float contentPointsW, contentPointsH, offsetX, offsetY;
  int pressMask;
  boolean disabled;

  /** `keyboardShouldPersistTaps`: 0 never, 1 always, 2 handled. */
  int taps;

  /** `keyboardDismissMode`: 0 none, 1 on-drag, 2 interactive. */
  int dismiss;

  /** Pull-to-refresh state. */
  private boolean refreshing;
  private float pull;
  private boolean pulling;
  private float startY;
  private boolean refreshIndicator = true;
  private float refreshThreshold = 64f;
  private float refreshInset = 56f;
  private ProgressBar spinner;

  private final float density;
  private boolean armed;
  private float downX, downY;
  private boolean touching, dragging, flung, momentum, applying;

  /** This gesture is swallowed: children do not see it, the scroller still does. */
  private boolean swallowing;

  /** A lift that never became a drag should resign. */
  private boolean tapDismiss;

  private final Runnable rest = this::cameToRest;
  private boolean scrollQueued;

  /** Snap behavior configuration. */
  private boolean snapPaging;
  private float snapInterval;
  private float[] snapOffsets;
  private int snapAlignment;
  private boolean snapDisableIntervalMomentum;
  private float snapDeceleration;

  private final Choreographer.FrameCallback scrollFrame =
      new Choreographer.FrameCallback() {
        @Override
        public void doFrame(long nanos) {
          scrollQueued = false;
          send(SCROLL);
        }
      };

  ScrollBox(Context c, Mounter m, int id) {
    super(c);
    this.id = id;
    density = c.getResources().getDisplayMetrics().density;
    setBackground(decor);
    content = new Content(c, m, id);
    
    // Add spinner for pull-to-refresh
    spinner = new ProgressBar(c);
    spinner.setIndeterminate(true);
    spinner.setVisibility(GONE);
    int size = Math.round(28 * density);
    addView(spinner, new FrameLayout.LayoutParams(size, size, 
        android.view.Gravity.CENTER_HORIZONTAL | android.view.Gravity.TOP));
    
    build(c);
  }

  private void build(Context c) {
    if (scroller != null) {
      scroller.removeView(content);
      removeView(scroller);
    }
    dragging = false;
    flung = false;
    momentum = false;
    touching = false;
    dropScrollFrame();
    removeCallbacks(rest);
    scroller = horizontal ? new HScroll(c) : new VScroll(c);
    scroller.setClipToPadding(false);
    // A scroll view inside another scrolls first, then hands the rest to its
    // parent. Nested scrolling off, the outer ScrollView intercepts every drag
    // along its axis.
    scroller.setNestedScrollingEnabled(true);
    scroller.setVerticalScrollBarEnabled(!horizontal);
    scroller.setHorizontalScrollBarEnabled(horizontal);
    scroller.setOnTouchListener(
        new android.view.View.OnTouchListener() {
          public boolean onTouch(android.view.View v, MotionEvent e) {
            return tap(e);
          }
        });
    scroller.setOnScrollChangeListener((v, x, y, ox, oy) -> onScrolled(x, y));
    scroller.addView(content, new ViewGroup.LayoutParams(-2, -2));
    addView(scroller, 0, new LayoutParams(-1, -1));
    size(contentW, contentH);
  }

  /** A tap on the scroller, not a drag. A child that takes the touch never reaches here. */
  private boolean tap(MotionEvent e) {
    // `never` already resigns this gesture. It must not also fire `onPress`.
    if (swallowing || (pressMask & 1) == 0) return false;
    int action = e.getActionMasked();
    if (action == MotionEvent.ACTION_DOWN) {
      armed = true;
      downX = e.getX();
      downY = e.getY();
      Mounter.pressed(this, id, true);
    } else if (action == MotionEvent.ACTION_MOVE) {
      if (armed && moved(e)) cancel();
    } else if (action == MotionEvent.ACTION_UP) {
      if (armed) {
        cancel();
        Native.sendPress(this, id);
      }
    } else if (action == MotionEvent.ACTION_CANCEL) cancel();
    return false;
  }

  private void cancel() {
    armed = false;
    Mounter.pressed(this, id, false);
  }

  private boolean moved(MotionEvent e) {
    float slop = android.view.ViewConfiguration.get(getContext()).getScaledTouchSlop();
    float dx = e.getX() - downX, dy = e.getY() - downY;
    return dx * dx + dy * dy > slop * slop;
  }

  /** Before the scroll view sees {@code e}: whether the finger lifted. */
  private boolean beforeTouch(MotionEvent e) {
    int action = e.getActionMasked();
    boolean up = action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL;
    if (up) flung = false;
    touching = !up;
    return up;
  }

  private void released() {
    touching = false;
    removeCallbacks(rest);
    flushScroll();
    if (!dragging) return;
    dragging = false;
    emit(DRAG_END);
    if (flung) {
      momentum = true;
      emit(MOMENTUM_START);
      armRest();
    } else emit(SCROLL_END);
  }

  private void onScrolled(int x, int y) {
    if (!applying) {
      offsetX = x / density;
      offsetY = y / density;
    }
    if (touching && !dragging) {
      dragging = true;
      if (dismiss != 0 && keyboardUp()) Native.dismissKeyboard(this);
      // Caught while gliding: the glide ended.
      if (momentum) {
        momentum = false;
        emit(MOMENTUM_END);
      }
      emit(DRAG_START);
    }
    queueScroll();
    if (dragging || momentum) armRest();
  }

  private void armRest() {
    removeCallbacks(rest);
    postDelayed(rest, REST_MS);
  }

  /** The latest offset, once on the next frame. */
  private void queueScroll() {
    if (scrollQueued) return;
    scrollQueued = true;
    Choreographer.getInstance().postFrameCallback(scrollFrame);
  }

  /** Deliver a waiting offset before an end, a drag, or momentum. */
  private void flushScroll() {
    if (!scrollQueued) return;
    Choreographer.getInstance().removeFrameCallback(scrollFrame);
    scrollQueued = false;
    send(SCROLL);
  }

  /** Sends this offset to the core whatever listens: lists follow it. */
  private void send(int kind) {
    float x = 0;
    float y = 0;
    if (scroller != null) {
      x = scroller.getScrollX() / density;
      y = scroller.getScrollY() / density;
    }
    Native.sendScroll(
        this,
        id,
        kind,
        x,
        y,
        contentPointsW,
        contentPointsH,
        getWidth() / density,
        getHeight() / density);
  }

  private void dropScrollFrame() {
    if (!scrollQueued) return;
    Choreographer.getInstance().removeFrameCallback(scrollFrame);
    scrollQueued = false;
  }

  @Override
  protected void onDetachedFromWindow() {
    dropScrollFrame();
    super.onDetachedFromWindow();
  }

  private void cameToRest() {
    if (touching) {
      postDelayed(rest, REST_MS);
      return;
    }
    flushScroll();
    
    // Apply snap if flung
    if (flung && scroller != null) {
      flung = false;
      float snapTarget = computeSnapTarget();
      if (snapTarget >= 0) {
        if (horizontal) {
          ((HorizontalScrollView) scroller).smoothScrollTo(Math.round(snapTarget), scroller.getScrollY());
        } else {
          ((ScrollView) scroller).smoothScrollTo(scroller.getScrollX(), Math.round(snapTarget));
        }
        return; // Let smoothScroll complete, which will call cameToRest again
      }
    }
    
    if (momentum) {
      momentum = false;
      emit(MOMENTUM_END);
    }
    emit(SCROLL_END);
  }

  private void emit(int kind) {
    if ((pressMask & (1 << kind)) == 0) return;
    float x = 0;
    float y = 0;
    if (scroller != null) {
      x = scroller.getScrollX() / density;
      y = scroller.getScrollY() / density;
    }
    Native.sendScroll(
        this,
        id,
        kind,
        x,
        y,
        contentPointsW,
        contentPointsH,
        getWidth() / density,
        getHeight() / density);
  }

  private void noteFling(int velocity) {
    if (velocity != 0) flung = true;
  }

  void configure(boolean h, int taps, int dismiss, boolean indicator, Context c, Mounter m) {
    this.taps = taps;
    this.dismiss = dismiss;
    if (horizontal != h) {
      horizontal = h;
      build(c);
    }
    scroller.setVerticalScrollBarEnabled(indicator && !horizontal);
    scroller.setHorizontalScrollBarEnabled(indicator && horizontal);
  }

  /** Pull to refresh: whether the spinner shows, where (dp) a pull refreshes and the room kept while refreshing. */
  void setRefresh(boolean own, float height) {
    float inset = own ? height : 56f;
    boolean changed = refreshIndicator == own || inset != refreshInset;
    refreshIndicator = !own;
    refreshThreshold = own ? height : 64f;
    refreshInset = inset;
    if (changed && spinner != null) {
      spinner.setVisibility(refreshIndicator && pull > 0 ? VISIBLE : GONE);
    }
    // A refresh in progress keeps the room of the indicator now showing.
    if (refreshing && !pulling) animatePull(inset * density, null);
  }

  void setRefreshing(boolean on) {
    if (on == refreshing) return;
    refreshing = on;
    float d = density;
    if (on) {
      if (refreshIndicator && spinner != null) {
        spinner.setVisibility(VISIBLE);
        spinner.setAlpha(1);
      }
      animatePull(refreshInset * d, null);
    } else {
      animatePull(0, () -> {
        if (spinner != null) spinner.setVisibility(GONE);
      });
    }
  }

  void setSnapBehavior(boolean paging, float interval, float[] offsets, 
                       int alignment, boolean disableIntervalMomentum, float deceleration) {
    this.snapPaging = paging;
    this.snapInterval = interval;
    this.snapOffsets = offsets;
    this.snapAlignment = alignment;
    this.snapDisableIntervalMomentum = disableIntervalMomentum;
    this.snapDeceleration = deceleration;
  }

  private float computeSnapTarget() {
    if (scroller == null) return -1.0f;
    
    // Get current scroll position and dimensions in pixels
    float offsetPx = horizontal ? scroller.getScrollX() : scroller.getScrollY();
    float viewportPx = horizontal ? getWidth() : getHeight();
    float contentPx = horizontal ? contentW : contentH;
    
    // Compute deceleration rate using same logic as nlayer_core::snap::rate
    float decel = snapDeceleration;
    boolean hasSnapping = snapInterval > 0 || (snapOffsets != null && snapOffsets.length > 0);
    if (decel <= 0.0f || decel >= 1.0f) {
      if (hasSnapping || snapPaging) {
        decel = 0.99f;
      } else {
        decel = 0.998f;
      }
    }
    
    // Estimate velocity from last fling (we don't have actual velocity tracked)
    // Use projected = offset for simplicity (assumes velocity ~0 at rest)
    float targetPx = offsetPx;
    float velocityPxPerMs = 0.0f;
    
    return Native.snapTarget(
        getHostHandle(),
        id,
        offsetPx,
        targetPx,
        velocityPxPerMs,
        viewportPx,
        contentPx,
        snapPaging,
        snapInterval,
        snapOffsets,
        snapAlignment,
        snapDisableIntervalMomentum,
        decel
    );
  }

  private long getHostHandle() {
    android.view.View v = this;
    while (v != null) {
      if (v instanceof HostView) {
        return ((HostView) v).handle();
      }
      android.view.ViewParent p = v.getParent();
      v = p instanceof android.view.View ? (android.view.View) p : null;
    }
    return 0;
  }

  private boolean canPull() {
    return !horizontal && (pressMask & (1 << REFRESH)) != 0 && 
           scroller != null && scroller.getScrollY() == 0;
  }

  @Override
  public boolean onInterceptTouchEvent(MotionEvent e) {
    if (refreshing || !canPull()) return false;
    if (e.getActionMasked() == MotionEvent.ACTION_MOVE) {
      float dy = e.getY() - startY;
      float slop = android.view.ViewConfiguration.get(getContext()).getScaledTouchSlop();
      if (dy > slop) {
        pulling = true;
        startY = e.getY();
        return true;
      }
    }
    return false;
  }

  @Override
  public boolean onTouchEvent(MotionEvent e) {
    if (!pulling) return false;
    float d = density;
    switch (e.getActionMasked()) {
      case MotionEvent.ACTION_MOVE:
        // Apply rubber band resistance (nsh formula: (1-1/(over*0.55/dim+1))*dim)
        float rawPull = Math.max(0, e.getY() - startY);
        float viewport = getHeight();
        float resistedPull = viewport > 0 
            ? (1.0f - 1.0f / (rawPull * 0.55f / viewport + 1.0f)) * viewport
            : rawPull * 0.5f;
        setPull(resistedPull);
        if (refreshIndicator && spinner != null) {
          spinner.setVisibility(VISIBLE);
          spinner.setAlpha(Math.min(1, pull / (refreshThreshold * d)));
        }
        return true;
      case MotionEvent.ACTION_UP:
      case MotionEvent.ACTION_CANCEL:
        pulling = false;
        if (pull >= refreshThreshold * d) {
          setRefreshing(true);
          // Core fires REFRESH via update_scroll_position, don't emit here
        } else {
          animatePull(0, () -> {
            if (spinner != null) spinner.setVisibility(GONE);
          });
        }
        return true;
    }
    return true;
  }

  private void setPull(float px) {
    if (px == pull) return;
    pull = px;
    if (scroller != null) {
      scroller.setTranslationY(px);
    }
    if (spinner != null) {
      spinner.setTranslationY(Math.max(0, px / 2 - spinner.getHeight() / 2f));
    }
    // Report pull distance to core (negative offset means pull)
    Native.sendScroll(this, id, SCROLL, 0, -px / density, 
        contentPointsW, contentPointsH, 
        getWidth() / density, getHeight() / density);
  }

  private void animatePull(float to, Runnable end) {
    android.animation.ValueAnimator a = android.animation.ValueAnimator.ofFloat(pull, to);
    a.setDuration(250);
    a.addUpdateListener(v -> setPull((float) v.getAnimatedValue()));
    if (end != null) {
      a.addListener(new android.animation.AnimatorListenerAdapter() {
        @Override
        public void onAnimationEnd(android.animation.Animator animation) {
          end.run();
        }
      });
    }
    a.start();
  }

  @Override
  public boolean dispatchTouchEvent(MotionEvent e) {
    int action = e.getActionMasked();
    if (action == MotionEvent.ACTION_DOWN) {
      startY = e.getY();
      swallowing = false;
      tapDismiss = false;
      if (scrollUnder(this, e) == null) {
        View hit = leaf(this, e.getX(), e.getY());
        boolean onInput = onInput(hit);
        boolean handled = handles(hit, e);
        tapDismiss = shouldDismiss(onInput, handled);
        // `never`: the tap reaches nothing but the scroll view.
        swallowing = tapDismiss && taps == 0 && !onInput;
      }
    }
    // `released` clears `dragging` while handling the lift. Read it first.
    boolean dragged = dragging;
    boolean handledEvent;
    if (swallowing) {
      MotionEvent local = MotionEvent.obtain(e);
      local.offsetLocation(
          -scroller.getLeft() - scroller.getTranslationX(),
          -scroller.getTop() - scroller.getTranslationY());
      scroller.onTouchEvent(local);
      local.recycle();
      handledEvent = true;
    } else {
      handledEvent = super.dispatchTouchEvent(e);
    }
    // A tap resigns. A drag does not, unless `keyboardDismissMode` says so.
    if (action == MotionEvent.ACTION_UP && tapDismiss && !dragged) {
      Native.dismissKeyboard(this);
    }
    if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) {
      swallowing = false;
      tapDismiss = false;
    }
    return handledEvent;
  }

  private boolean keyboardUp() {
    for (android.view.ViewParent p = getParent(); p != null; p = p.getParent()) {
      if (p instanceof HostView) return ((HostView) p).keyboardInsetPoints() > 0;
    }
    return false;
  }

  private boolean shouldDismiss(boolean onInput, boolean handled) {
    if (!keyboardUp() || taps == 1) return false;
    if (taps == 2) return !handled && !onInput;
    return !onInput;
  }

  /** A descendant scroll under the touch. That scroll decides, not this one. */
  private ScrollBox scrollUnder(View root, MotionEvent e) {
    int[] origin = new int[2];
    getLocationOnScreen(origin);
    return scrollAt(root, origin[0] + e.getX(), origin[1] + e.getY());
  }

  private ScrollBox scrollAt(View v, float sx, float sy) {
    if (!(v instanceof ViewGroup)) return null;
    ViewGroup group = (ViewGroup) v;
    for (int i = group.getChildCount() - 1; i >= 0; i--) {
      android.view.View child = group.getChildAt(i);
      ScrollBox found = scrollAt(child, sx, sy);
      if (found != null) return found;
      if (child instanceof ScrollBox && child != this && onScreen(child, sx, sy))
        return (ScrollBox) child;
    }
    return null;
  }

  private static boolean onScreen(android.view.View v, float sx, float sy) {
    int[] loc = new int[2];
    v.getLocationOnScreen(loc);
    return sx >= loc[0]
        && sy >= loc[1]
        && sx < loc[0] + v.getWidth()
        && sy < loc[1] + v.getHeight();
  }

  private android.view.View leaf(android.view.View v, float x, float y) {
    if (!(v instanceof ViewGroup)) return v;
    ViewGroup group = (ViewGroup) v;
    float sx = x + group.getScrollX(), sy = y + group.getScrollY();
    for (int i = group.getChildCount() - 1; i >= 0; i--) {
      android.view.View child = group.getChildAt(i);
      if (child.getVisibility() != VISIBLE) continue;
      float lx = sx - child.getLeft(), ly = sy - child.getTop();
      if (lx >= 0 && ly >= 0 && lx < child.getWidth() && ly < child.getHeight())
        return leaf(child, lx, ly);
    }
    return v;
  }

  private boolean onInput(android.view.View hit) {
    for (android.view.View v = hit; v != null && v != this; v = parentOf(v)) {
      if (v instanceof InputBox || v instanceof android.widget.EditText) return true;
    }
    return false;
  }

  private boolean handles(android.view.View hit, MotionEvent e) {
    int[] origin = new int[2];
    getLocationOnScreen(origin);
    float sx = origin[0] + e.getX(), sy = origin[1] + e.getY();
    for (android.view.View v = hit; v != null; v = parentOf(v)) {
      if (v instanceof InputBox || v instanceof android.widget.EditText) return true;
      if (v instanceof TextBox) {
        TextBox text = (TextBox) v;
        if ((text.pressMask & 1) != 0) return true;
        int[] at = new int[2];
        text.getLocationOnScreen(at);
        if (text.pressLinkAt(sx - at[0], sy - at[1])) return true;
      } else if (v instanceof Box && (((Box) v).pressMask & 1) != 0) return true;
      else if (v instanceof ImageBox && (((ImageBox) v).pressMask & 1) != 0) return true;
      else if (v instanceof ScrollBox && (((ScrollBox) v).pressMask & 1) != 0) return true;
      if (v == this) return false;
    }
    return false;
  }

  private static android.view.View parentOf(android.view.View v) {
    android.view.ViewParent p = v.getParent();
    return p instanceof android.view.View ? (android.view.View) p : null;
  }

  void size(int w, int h) {
    contentW = w;
    contentH = h;
    if (content == null) return;
    int cw = Math.max(w, getWidth()), ch = Math.max(h, getHeight());
    contentW = cw;
    contentH = ch;
    content.measure(
        android.view.View.MeasureSpec.makeMeasureSpec(cw, android.view.View.MeasureSpec.EXACTLY),
        android.view.View.MeasureSpec.makeMeasureSpec(ch, android.view.View.MeasureSpec.EXACTLY));
    content.layout(0, 0, cw, ch);
    content.requestLayout();
  }

  void scrollByPoints(float dx, float dy, float density) {
    offsetX += dx;
    offsetY += dy;
    applying = true;
    if (horizontal) scroller.scrollBy(Math.round(dx * density), 0);
    else scroller.scrollBy(0, Math.round(dy * density));
    applying = false;
  }

  void scrollToPoints(float x, float y, float density) {
    offsetX = x;
    offsetY = y;
    applying = true;
    int px = Math.round(x * density), py = Math.round(y * density);
    if (horizontal) scroller.scrollTo(px, 0);
    else scroller.scrollTo(0, py);
    applying = false;
  }

  /** A test scroll landed at the current offset: the six callbacks, in order. */
  void noteTestGesture() {
    dropScrollFrame();
    emitTest(DRAG_START);
    emitTest(SCROLL);
    emitTest(DRAG_END);
    emitTest(MOMENTUM_START);
    emitTest(MOMENTUM_END);
    emitTest(SCROLL_END);
  }

  /** The offset the tree reports, not the pixel scroll rounded back to points. */
  private void emitTest(int kind) {
    if ((pressMask & (1 << kind)) == 0) return;
    float w = getWidth() / density;
    float h = getHeight() / density;
    if (getTag() instanceof float[]) {
      float[] points = (float[]) getTag();
      w = points[2];
      h = points[3];
    }
    Native.sendScroll(this, id, kind, offsetX, offsetY, contentPointsW, contentPointsH, w, h);
  }

  /**
   * ScrollView measures the scroll axis as UNSPECIFIED but packs the viewport size in the spec.
   * {@link Box#onMeasure} uses that size, so a later layout pass clamps the content to the viewport
   * and the scroll range to 0.
   */
  final class Content extends Box {
    Content(Context c, Mounter m, int id) {
      super(c, m, id);
    }

    @Override
    protected void onMeasure(int ws, int hs) {
      super.onMeasure(ws, hs);
      if (contentW > 0 || contentH > 0)
        setMeasuredDimension(
            contentW > 0 ? contentW : getMeasuredWidth(),
            contentH > 0 ? contentH : getMeasuredHeight());
    }
  }

  private final class VScroll extends ScrollView {
    VScroll(Context c) {
      super(c);
    }

    @Override
    public boolean onTouchEvent(MotionEvent e) {
      boolean up = beforeTouch(e);
      boolean handled = super.onTouchEvent(e);
      if (up) released();
      return handled;
    }

    @Override
    public void fling(int velocity) {
      noteFling(velocity);
      super.fling(velocity);
    }
  }

  private final class HScroll extends HorizontalScrollView {
    HScroll(Context c) {
      super(c);
    }

    @Override
    public boolean onTouchEvent(MotionEvent e) {
      boolean up = beforeTouch(e);
      boolean handled = super.onTouchEvent(e);
      if (up) released();
      return handled;
    }

    @Override
    public void fling(int velocity) {
      noteFling(velocity);
      super.fling(velocity);
    }
  }
}
