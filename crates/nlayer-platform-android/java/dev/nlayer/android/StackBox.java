// Copyright 2025 the nlayer authors. All rights reserved. Apache-2.0 license.
//
// Derived from nsh's StackBox (com.longbridge.nsh.StackBox):
// Copyright 2024 Longbridge Whale Wealth Ltd.

package dev.nlayer.android;

import android.animation.Animator;
import android.animation.AnimatorListenerAdapter;
import android.annotation.SuppressLint;
import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.PorterDuff;
import android.graphics.PorterDuffColorFilter;
import android.graphics.RectF;
import android.graphics.Typeface;
import android.text.TextUtils;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.animation.DecelerateInterpolator;
import android.widget.FrameLayout;
import android.widget.TextView;
import java.util.ArrayList;
import java.util.List;

/**
 * `<stack>`: screens stacked natively, with a self-drawn header bar per screen (no system toolbar),
 * push / pop transitions and the back button (predictive back on Android 14). The bar lives inside
 * the page, so it moves with the transition. The core lays every screen out at the stack's size.
 */
@SuppressLint("ViewConstructor")
final class StackBox extends FrameLayout {
  final Mounter mounter;
  final int id;
  final Decor decor = new Decor();

  /** The app's screens, bottom to top. */
  final List<Integer> screens = new ArrayList<>();

  /** The screens currently shown, bottom to top. */
  private final List<ScreenBox> shown = new ArrayList<>();

  private Animator transition;

  StackBox(Context context, Mounter mounter, int id) {
    super(context);
    this.mounter = mounter;
    this.id = id;
    setBackground(decor);
    setClipChildren(true);
  }

  int depth() {
    return shown.size();
  }

  ScreenBox top() {
    return shown.isEmpty() ? null : shown.get(shown.size() - 1);
  }

  /** Shows the app's screens: pushes, pops or replaces as needed. */
  void reconcile() {
    List<ScreenBox> desired = new ArrayList<>();
    for (int s : screens) {
      View v = mounter.views.get(s);
      if (v instanceof ScreenBox) desired.add((ScreenBox) v);
    }
    if (desired.equals(shown)) return;
    ScreenBox oldTop = top();
    ScreenBox newTop = desired.isEmpty() ? null : desired.get(desired.size() - 1);
    for (ScreenBox s : desired) {
      if (s.getParent() != this) {
        if (s.getParent() instanceof ViewGroup) ((ViewGroup) s.getParent()).removeView(s);
        addView(s, new LayoutParams(LayoutParams.MATCH_PARENT, LayoutParams.MATCH_PARENT));
      }
    }
    // Order the children like the stack (top screen drawn last).
    for (ScreenBox s : desired) s.bringToFront();
    boolean pushing = !shown.isEmpty() && desired.size() >= shown.size() && !shown.contains(newTop);
    List<ScreenBox> leaving = new ArrayList<>(shown);
    leaving.removeAll(desired);
    shown.clear();
    shown.addAll(desired);
    if (transition != null) transition.cancel();
    if (oldTop == newTop || oldTop == null || newTop == null) {
      settle(leaving);
      return;
    }
    int animation = pushing ? newTop.animation : oldTop.animation;
    if (animation == 2) {
      settle(leaving);
      return;
    }
    float startProgress = 0;
    if (!pushing && getWidth() > 0) {
      startProgress = Math.max(0, Math.min(1, oldTop.getTranslationX() / Math.max(1, getWidth())));
    }
    animate(
        pushing ? oldTop : newTop,
        pushing ? newTop : oldTop,
        pushing,
        animation == 1,
        startProgress,
        leaving);
  }

  /** Only the top screen is visible; removed screens leave. */
  private void settle(List<ScreenBox> leaving) {
    for (ScreenBox s : leaving) removeView(s);
    for (int i = 0; i < shown.size(); i++) {
      ScreenBox s = shown.get(i);
      s.setTranslationX(0);
      s.setAlpha(1);
      s.setVisibility(i == shown.size() - 1 ? VISIBLE : INVISIBLE);
    }
    transition = null;
  }

  /**
   * Moves `upper` over `lower` (push) or away from it (pop), from `progress` (0..1 of the way to
   * the popped state).
   */
  private void animate(
      ScreenBox lower,
      ScreenBox upper,
      boolean push,
      boolean fade,
      float progress,
      List<ScreenBox> leaving) {
    float w = getWidth();
    lower.setVisibility(VISIBLE);
    upper.setVisibility(VISIBLE);
    android.animation.ValueAnimator a =
        android.animation.ValueAnimator.ofFloat(push ? 1 : progress, push ? 0 : 1);
    a.setDuration(push ? 300 : Math.round(300 * (1 - progress)));
    a.setInterpolator(new DecelerateInterpolator(1.5f));
    a.addUpdateListener(x -> place(lower, upper, (float) x.getAnimatedValue(), w, fade));
    a.addListener(
        new AnimatorListenerAdapter() {
          private boolean cancelled;

          @Override
          public void onAnimationCancel(Animator animation) {
            cancelled = true;
          }

          @Override
          public void onAnimationEnd(Animator animation) {
            if (!cancelled) settle(leaving);
          }
        });
    place(lower, upper, push ? 1 : progress, w, fade);
    transition = a;
    a.start();
  }

  /**
   * `t` 0: `upper` covers the stack; 1: `upper` is gone. Slides move by whole pixels: Skia keys the
   * blurred masks of shadows on the subpixel part of their position, so a fractional offset each
   * frame blurs every shadow the screens draw again, frame after frame.
   */
  private static void place(View lower, View upper, float t, float w, boolean fade) {
    if (fade) {
      upper.setTranslationX(0);
      lower.setTranslationX(0);
      upper.setAlpha(1 - t);
    } else {
      upper.setTranslationX(Math.round(t * w));
      lower.setTranslationX(Math.round(-(1 - t) * w * 0.25f));
    }
  }

  // --- Back -----------------------------------------------------------------

  /** Whether the back button pops a screen of this stack. */
  boolean canPop() {
    ScreenBox top = top();
    return shown.size() > 1 && top != null && !top.preventBack && isShown();
  }

  /** Whether the top screen keeps the user (`popOnBack` false): back only tells JS. */
  boolean preventsBack() {
    ScreenBox top = top();
    return top != null && top.preventBack && isShown();
  }

  /** Whether a back gesture may drag the top screen away (`popOnSwipe`). */
  boolean canSwipe() {
    ScreenBox top = top();
    return canPop() && top.gestureEnabled;
  }

  /**
   * The back button, the bar's chevron or a finished back gesture: the top screen hears of it when
   * it listens, and pops unless it keeps the user (`popOnBack` false).
   */
  void back() {
    mounter.host.back();
  }

  private float backProgress = -1;

  /** Predictive back: the gesture started. */
  void backStarted() {
    if (!canSwipe() || transition != null) return;
    backProgress = 0;
    ScreenBox lower = shown.get(shown.size() - 2);
    lower.setVisibility(VISIBLE);
    place(lower, top(), 0, getWidth(), false);
  }

  /** Predictive back: the finger moved (`progress` 0..1). */
  void backProgressed(float progress) {
    if (backProgress < 0) return;
    backProgress = progress;
    // Follow the finger part of the way, like the system animation.
    place(shown.get(shown.size() - 2), top(), progress * 0.4f, getWidth(), false);
  }

  /** Predictive back: the gesture was abandoned. */
  void backCancelled() {
    if (backProgress < 0) return;
    backProgress = -1;
    List<ScreenBox> none = new ArrayList<>();
    ScreenBox lower = shown.get(shown.size() - 2), upper = top();
    android.animation.ValueAnimator a =
        android.animation.ValueAnimator.ofFloat(
            upper.getTranslationX() / Math.max(1, getWidth()), 0);
    a.setDuration(200);
    a.addUpdateListener(x -> place(lower, upper, (float) x.getAnimatedValue(), getWidth(), false));
    a.addListener(
        new AnimatorListenerAdapter() {
          @Override
          public void onAnimationEnd(Animator animation) {
            settle(none);
          }
        });
    a.start();
  }

  // --- Screens --------------------------------------------------------------

  /** One screen: the core's root view plus the header bar drawn inside the page. */
  @SuppressLint("ViewConstructor")
  static final class ScreenBox extends Box {
    /** Content row in pixels: 44pt, not a 56dp toolbar. */
    static int contentRowPx(float density) {
      return Math.round(44f * density);
    }

    final Bar bar;
    int animation;
    boolean gestureEnabled = true;

    /** `popOnBack` false: the back buttons only send `onBackPress`. */
    boolean preventBack;

    boolean transparent;
    boolean shown;
    private Mounter.ScreenOptions opts;
    private float lastHeaderHeight = -1;
    private float lastInsetTop = -1;
    private float lastInsetRight = -1;
    private float lastInsetBottom = -1;
    private float lastInsetLeft = -1;

    ScreenBox(Context context, Mounter mounter, int id) {
      super(context, mounter, id);
      bar = new Bar(context, mounter.density);
      bar.backGroup.setOnClickListener(
          new View.OnClickListener() {
            @Override
            public void onClick(View v) {
              if (getParent() instanceof StackBox) ((StackBox) getParent()).back();
            }
          });
      addView(bar, new ViewGroup.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0));
    }

    @Override
    public void onViewAdded(View child) {
      super.onViewAdded(child);
      if (bar != null && child != bar) bar.bringToFront();
    }

    @Override
    protected void onMeasure(int widthSpec, int heightSpec) {
      int w = MeasureSpec.getSize(widthSpec), h = MeasureSpec.getSize(heightSpec);
      setMeasuredDimension(w, h);
      bar.measure(
          MeasureSpec.makeMeasureSpec(w, MeasureSpec.EXACTLY),
          MeasureSpec.makeMeasureSpec(bar.getLayoutParams().height, MeasureSpec.EXACTLY));
      for (int i = 0; i < getChildCount(); i++) {
        View v = getChildAt(i);
        if (v == bar) continue;
        v.measure(
            MeasureSpec.makeMeasureSpec(v.getLayoutParams().width, MeasureSpec.EXACTLY),
            MeasureSpec.makeMeasureSpec(v.getLayoutParams().height, MeasureSpec.EXACTLY));
      }
    }

    @Override
    protected void onLayout(boolean changed, int l, int t, int r, int b) {
      bar.layout(0, 0, r - l, bar.getMeasuredHeight());
      for (int i = 0; i < getChildCount(); i++) {
        View v = getChildAt(i);
        if (v == bar) continue;
        ViewGroup.LayoutParams raw = v.getLayoutParams();
        if (raw instanceof ViewGroup.MarginLayoutParams) {
          ViewGroup.MarginLayoutParams p = (ViewGroup.MarginLayoutParams) raw;
          v.layout(p.leftMargin, p.topMargin, p.leftMargin + p.width, p.topMargin + p.height);
        }
      }
    }

    /**
     * Applies `encode.rs` `screen_config`. `under` is the screen below this one: its back mode and
     * back title (else its title) decide the back control.
     */
    void configure(
        Mounter m, Mounter.ScreenOptions opts, boolean canGoBack, Mounter.ScreenOptions under) {
      if (opts == null) return;
      this.opts = opts;
      this.animation = opts.animation;
      this.gestureEnabled = opts.gesture;
      this.preventBack = opts.preventBack;
      this.transparent = opts.transparent;
      this.shown = opts.shown;
      bar.setVisibility(shown ? VISIBLE : GONE);
      applyHeight();
      placeHeader(m, opts, canGoBack, under);
      applyChrome();
      reportHeader();
      reportInsets();
    }

    /** The system scheme changed. Defaults follow `host.dark`; explicit colors stay. */
    void schemeChanged() {
      if (opts != null) applyChrome();
    }

    /** 44pt, or 56 with a subtitle and no title view. A large title keeps the content row at 44. */
    private int contentPoints() {
      if (opts.largeTitle == 1 && opts.titleView < 0) return 44;
      return opts.subtitle != null && opts.titleView < 0 ? 56 : 44;
    }

    /** 52pt, or 68 with a subtitle. Only `always`, and never beside a title view. */
    private int largePoints() {
      if (opts.largeTitle != 1 || opts.titleView >= 0) return 0;
      return opts.subtitle != null ? 68 : 52;
    }

    /**
     * The bar is the status inset plus {@code barHeight}: the content row, then a static large
     * title when there is one. A hidden bar takes no space.
     */
    private void applyHeight() {
      if (opts == null) return;
      float d = mounter.density;
      int contentPt = contentPoints();
      int largePt = largePoints();
      if (opts.barHeight > 0f && Math.abs(opts.barHeight - (contentPt + largePt)) > 0.5f) {
        int total = Math.round(opts.barHeight);
        if (largePt > total) largePt = total;
        contentPt = Math.max(0, total - largePt);
      }
      int contentPx = Math.round(contentPt * d);
      int largePx = Math.round(largePt * d);
      bar.contentRow = contentPx;
      bar.largeRow = largePx;
      int height = shown ? mounter.host.safeTop + contentPx + largePx : 0;
      ViewGroup.LayoutParams p = bar.getLayoutParams();
      if (p.height != height) {
        p.height = height;
        bar.setLayoutParams(p);
      } else {
        bar.requestLayout();
      }
    }

    private void placeHeader(
        Mounter m, Mounter.ScreenOptions opts, boolean canGoBack, Mounter.ScreenOptions under) {
      boolean leadingEmpty = noItems(opts.leading);
      boolean showBack = canGoBack && opts.backVisible && (leadingEmpty || opts.supplement);
      bar.showBack(showBack, backText(under));
      bar.leading.fill(showBack ? bar.backGroup : null, opts.leading, m);
      bar.trailing.fill(null, opts.trailing, m);
      boolean hasView = opts.titleView >= 0;
      boolean large = opts.largeTitle == 1 && !hasView;
      boolean sub = opts.subtitle != null && !opts.subtitle.isEmpty() && !hasView;
      boolean showTitle = !large && !hasView && opts.title != null && !opts.title.isEmpty();
      bar.title.setVisibility(showTitle ? VISIBLE : GONE);
      if (showTitle) bar.title.setText(opts.title);
      boolean contentSub = sub && !large;
      bar.subtitle.setVisibility(contentSub ? VISIBLE : GONE);
      if (contentSub) bar.subtitle.setText(opts.subtitle);
      boolean showLarge = large && opts.title != null && !opts.title.isEmpty();
      bar.largeTitle.setVisibility(showLarge ? VISIBLE : GONE);
      if (showLarge) bar.largeTitle.setText(opts.title);
      bar.largeSubtitle.setVisibility(large && sub ? VISIBLE : GONE);
      if (large && sub) bar.largeSubtitle.setText(opts.subtitle);
      View next = hasView ? m.views.get(opts.titleView) : null;
      if (bar.titleCustom != null && bar.titleCustom != next) {
        bar.removeView(bar.titleCustom);
        bar.titleCustom = null;
      }
      if (next != null) {
        int[] size = m.headerSizes.get(opts.titleView);
        int w = size != null ? Math.max(1, size[0]) : 1;
        int h = size != null ? Math.max(1, size[1]) : 1;
        if (next.getParent() != bar) {
          if (next.getParent() instanceof ViewGroup)
            ((ViewGroup) next.getParent()).removeView(next);
          bar.addView(next, new ViewGroup.LayoutParams(w, h));
        } else if (next.getLayoutParams().width != w || next.getLayoutParams().height != h) {
          next.getLayoutParams().width = w;
          next.getLayoutParams().height = h;
          next.requestLayout();
        }
        bar.titleCustom = next;
      }
      bar.leading.bringToFront();
      bar.trailing.bringToFront();
    }

    private static boolean noItems(ArrayList<Mounter.BarGroup> groups) {
      for (int i = 0; i < groups.size(); i++) {
        if (!groups.get(i).items.isEmpty()) return false;
      }
      return true;
    }

    /** Display mode and title come from the page underneath. */
    private static String backText(Mounter.ScreenOptions under) {
      if (under == null || under.backMode == 2) return "";
      if (under.backMode == 1) return "Back";
      if (under.backTitle != null && !under.backTitle.isEmpty()) return under.backTitle;
      return under.title == null ? "" : under.title;
    }

    private void applyChrome() {
      boolean dark = mounter.host.dark;
      int fg = dark ? 0xFFFFFFFF : 0xFF000000;
      int titles = opts.titleColorPresent ? opts.titleColor : fg;
      int large = opts.largeTitleColorPresent ? opts.largeTitleColor : titles;
      int tint = opts.tintPresent ? opts.tint : fg;
      boolean paint = opts.backgroundPresent && Color.alpha(opts.background) != 0;
      bar.opaque = paint && !transparent;
      bar.fill = bar.opaque ? opts.background : Color.TRANSPARENT;
      bar.hairline = shown && opts.shadowKind == 2;
      bar.hairlineColor = opts.shadowColor;
      bar.title.setTextColor(titles);
      bar.subtitle.setTextColor(fade(titles));
      bar.largeTitle.setTextColor(large);
      bar.largeSubtitle.setTextColor(fade(titles));
      bar.back.setColor(tint);
      bar.backLabel.setTextColor(tint);
      bar.paintGroups(Bar.fill(dark, false), Bar.fill(dark, true), tint);
      bar.invalidate();
    }

    /** Title color at 60% opacity. The color's own alpha is kept in the product. */
    private static int fade(int color) {
      int alpha = Math.round(Color.alpha(color) * 0.6f);
      return Color.argb(alpha, Color.red(color), Color.green(color), Color.blue(color));
    }

    /** Reports header height to native. */
    void reportHeader() {
      if (!isAttachedToWindow() || opts == null) return;
      // A hidden or transparent bar does not push content. Reporting 0 would
      // replace the last real height, and the next opaque frame would start
      // at the top until this call reports again.
      if (!shown || transparent) return;
      float height = bar.getLayoutParams().height / mounter.density;
      if (height == lastHeaderHeight) return;
      long handle = mounter.host.handle();
      if (handle == 0) return;
      lastHeaderHeight = height;
      Native.screenHeader(handle, id, height);
    }

    /**
     * Top inset of the screen's content, in points. An opaque bar already pushes the content down,
     * so only the status bar past that bar remains. A transparent bar does not, and still counts:
     * iOS's safe area includes a transparent navigation bar. A hidden bar leaves the safe area.
     */
    static float contentTopInset(
        boolean shown, boolean transparent, int safeTopPx, int toolbarHeightPx, float density) {
      if (!(density > 0f)) return 0f;
      if (shown && transparent) return toolbarHeightPx / density;
      int covered = shown ? toolbarHeightPx : 0;
      return Math.max(0, safeTopPx - covered) / density;
    }

    /** Reports insets to native. */
    void reportInsets() {
      if (!isAttachedToWindow() || opts == null) return;
      float top =
          contentTopInset(
              shown,
              transparent,
              mounter.host.safeTop,
              bar.getLayoutParams().height,
              mounter.density);
      float right = mounter.host.safeRight / mounter.density;
      float bottom = mounter.host.safeBottom / mounter.density;
      float left = mounter.host.safeLeft / mounter.density;
      if (top == lastInsetTop
          && right == lastInsetRight
          && bottom == lastInsetBottom
          && left == lastInsetLeft) return;
      long handle = mounter.host.handle();
      if (handle == 0) return;
      lastInsetTop = top;
      lastInsetRight = right;
      lastInsetBottom = bottom;
      lastInsetLeft = left;
      Native.rootInsets(handle, id, top, right, bottom, left);
    }

    /** Called when host insets change after the screen is mounted. */
    void insetsChanged() {
      applyHeight();
      reportHeader();
      reportInsets();
      requestLayout();
    }

    @Override
    protected void onAttachedToWindow() {
      super.onAttachedToWindow();
      reportHeader();
      reportInsets();
    }

    /**
     * Header drawn in the page, so it moves with the transition. The status inset sits above a 44pt
     * content row (56 with a subtitle). Groups float on that row; a static large title is a second
     * row under it, with no background of its own.
     */
    @SuppressLint("ViewConstructor")
    static final class Bar extends ViewGroup {
      final Side leading;
      final Side trailing;
      final Title title;
      final Title subtitle;
      final Title largeTitle;
      final Title largeSubtitle;
      final Title backLabel;
      final Group backGroup;
      final Chevron back;
      View titleCustom;
      int contentRow;
      int largeRow;
      boolean opaque;
      int fill;
      private boolean hairline;
      private int hairlineColor;
      final float density;
      private final Paint bg = new Paint();
      private final Paint line = new Paint();

      Bar(Context context, float density) {
        super(context);
        this.density = density;
        contentRow = contentRowPx(density);
        setClipChildren(true);
        setWillNotDraw(false);
        leading = new Side(context);
        trailing = new Side(context);
        title = text(context, 17f, 600, Gravity.CENTER);
        subtitle = text(context, 12f, 400, Gravity.CENTER);
        largeTitle = text(context, 34f, 700, Gravity.START | Gravity.CENTER_VERTICAL);
        largeSubtitle = text(context, 12f, 400, Gravity.START | Gravity.CENTER_VERTICAL);
        backLabel = text(context, 17f, 400, Gravity.CENTER_VERTICAL);
        backLabel.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        backGroup = new Group(context, true);
        back = new Chevron(context, density);
        back.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
        backGroup.addView(back);
        backGroup.addView(backLabel);
        backGroup.setVisibility(GONE);
        back.setVisibility(GONE);
        addView(title);
        addView(subtitle);
        addView(largeTitle);
        addView(largeSubtitle);
        addView(leading);
        addView(trailing);
      }

      /** Light `white / 0.82` (pressed 1); dark `#1e1e1e / 0.62` (pressed 0.8). */
      static int fill(boolean dark, boolean pressed) {
        if (dark) {
          int alpha = Math.round((pressed ? 0.8f : 0.62f) * 255f);
          return Color.argb(alpha, 0x1e, 0x1e, 0x1e);
        }
        int alpha = Math.round((pressed ? 1f : 0.82f) * 255f);
        return Color.argb(alpha, 255, 255, 255);
      }

      private Title text(Context context, float pt, int weight, int gravity) {
        Title view = new Title(context);
        view.setMaxLines(1);
        view.setEllipsize(TextUtils.TruncateAt.END);
        view.setIncludeFontPadding(false);
        view.setTextSize(TypedValue.COMPLEX_UNIT_PX, pt * density);
        view.setTypeface(Typeface.create(Typeface.DEFAULT, weight, false));
        view.setGravity(gravity);
        view.setVisibility(GONE);
        return view;
      }

      /** Circle chevron when `text` is empty; otherwise a capsule with that text. */
      void showBack(boolean show, String text) {
        boolean capsule = text != null && !text.isEmpty();
        String name = capsule ? text : "Back";
        back.setVisibility(show ? VISIBLE : GONE);
        back.setContentDescription(name);
        backGroup.setVisibility(show ? VISIBLE : GONE);
        backGroup.setContentDescription(name);
        backGroup.circle = !capsule;
        backGroup.expanded = show && !capsule;
        backLabel.setVisibility(show && capsule ? VISIBLE : GONE);
        backLabel.setText(capsule ? text : "");
      }

      void paintGroups(int fill, int pressed, int tint) {
        paintGroup(backGroup, fill, pressed);
        paintSide(leading, fill, pressed, tint);
        paintSide(trailing, fill, pressed, tint);
      }

      private void paintSide(Side side, int fill, int pressed, int tint) {
        for (int i = 0; i < side.getChildCount(); i++) {
          View child = side.getChildAt(i);
          if (!(child instanceof Group)) continue;
          paintGroup((Group) child, fill, pressed);
          tintGroup((Group) child, tint);
        }
      }

      private static void paintGroup(Group group, int fill, int pressed) {
        group.fillColor = fill;
        group.pressedColor = pressed;
        group.invalidate();
      }

      private static void tintGroup(Group group, int tint) {
        for (int i = 0; i < group.getChildCount(); i++) {
          View child = group.getChildAt(i);
          if (child instanceof ButtonItem) ((ButtonItem) child).applyTint(tint);
        }
      }

      private int px(float pt) {
        return Math.round(pt * density);
      }

      /** Symmetric width that keeps a geometrically centered title clear of both sides. */
      private int titleSlot(int w, int leadW, int trailW) {
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        int edge = px(16);
        int left = !rtl ? (leadW > 0 ? leadW : edge) : (trailW > 0 ? trailW : edge);
        int right =
            !rtl ? (trailW > 0 ? w - trailW : w - edge) : (leadW > 0 ? w - leadW : w - edge);
        int half = Math.min(w / 2 - left, right - w / 2);
        return Math.max(0, half * 2);
      }

      private int rowTop() {
        return Math.max(0, getHeight() - largeRow - contentRow);
      }

      /** Opaque chrome swallows presses above the large title. A clear bar lets them through. */
      @Override
      public boolean onTouchEvent(MotionEvent event) {
        if (!opaque) return false;
        return event.getY() < getHeight() - largeRow;
      }

      @Override
      protected void onDraw(Canvas canvas) {
        if (!opaque || Color.alpha(fill) == 0 || getWidth() <= 0) return;
        bg.setColor(fill);
        canvas.drawRect(0, 0, getWidth(), rowTop() + contentRow, bg);
      }

      @Override
      protected void dispatchDraw(Canvas canvas) {
        super.dispatchDraw(canvas);
        if (!hairline || getWidth() <= 0 || getHeight() <= 0) return;
        float t = 0.5f * density;
        if (!(t > 0f)) return;
        float bottom = rowTop() + contentRow;
        line.setColor(hairlineColor);
        canvas.drawRect(0, bottom - t, getWidth(), bottom, line);
      }

      @Override
      protected void onMeasure(int ws, int hs) {
        int w = MeasureSpec.getSize(ws), h = MeasureSpec.getSize(hs);
        setMeasuredDimension(w, h);
        int row = Math.min(Math.max(0, contentRow), Math.max(0, h));
        int rowSpec = MeasureSpec.makeMeasureSpec(row, MeasureSpec.EXACTLY);
        int atMost = MeasureSpec.makeMeasureSpec(w, MeasureSpec.AT_MOST);
        leading.measure(atMost, rowSpec);
        trailing.measure(atMost, rowSpec);
        int slot = titleSlot(w, leading.getMeasuredWidth(), trailing.getMeasuredWidth());
        int slotSpec = MeasureSpec.makeMeasureSpec(slot, MeasureSpec.EXACTLY);
        int wrapH = MeasureSpec.makeMeasureSpec(row, MeasureSpec.AT_MOST);
        if (title.getVisibility() != GONE) title.measure(slotSpec, wrapH);
        if (subtitle.getVisibility() != GONE) subtitle.measure(slotSpec, wrapH);
        if (titleCustom != null && titleCustom.getVisibility() != GONE) {
          ViewGroup.LayoutParams lp = titleCustom.getLayoutParams();
          int cw = lp != null && lp.width > 0 ? lp.width : 1;
          int ch = lp != null && lp.height > 0 ? lp.height : 1;
          titleCustom.measure(exactly(cw), exactly(ch));
        }
        int textW = Math.max(0, w - px(32));
        if (largeTitle.getVisibility() != GONE) largeTitle.measure(exactly(textW), exactly(px(41)));
        if (largeSubtitle.getVisibility() != GONE)
          largeSubtitle.measure(exactly(textW), exactly(px(16)));
      }

      @Override
      protected void onLayout(boolean changed, int l, int t, int r, int b) {
        int w = r - l, h = b - t;
        int top = Math.max(0, h - largeRow - contentRow);
        int row = Math.min(Math.max(0, contentRow), Math.max(0, h - top));
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        int lw = leading.getMeasuredWidth(), tw = trailing.getMeasuredWidth();
        if (rtl) {
          leading.layout(w - lw, top, w, top + row);
          trailing.layout(0, top, tw, top + row);
        } else {
          leading.layout(0, top, lw, top + row);
          trailing.layout(w - tw, top, w, top + row);
        }
        int slot = titleSlot(w, lw, tw);
        int x = (w - slot) / 2;
        int th = title.getVisibility() != GONE ? title.getMeasuredHeight() : 0;
        int sh = subtitle.getVisibility() != GONE ? subtitle.getMeasuredHeight() : 0;
        int gap = th > 0 && sh > 0 ? px(2) : 0;
        int y = top + Math.max(0, (row - th - gap - sh) / 2);
        if (title.getVisibility() != GONE) {
          title.layout(x, y, x + slot, y + th);
          y += th + gap;
        }
        if (subtitle.getVisibility() != GONE) subtitle.layout(x, y, x + slot, y + sh);
        if (titleCustom != null && titleCustom.getVisibility() != GONE) {
          int cw = titleCustom.getMeasuredWidth(), ch = titleCustom.getMeasuredHeight();
          int cx = (w - cw) / 2;
          int cy = top + (row - ch) / 2;
          titleCustom.layout(cx, cy, cx + cw, cy + ch);
        }
        int textW = Math.max(0, w - px(32));
        int lx = px(16);
        int titleH = largeTitle.getVisibility() != GONE ? largeTitle.getMeasuredHeight() : 0;
        int subH = largeSubtitle.getVisibility() != GONE ? largeSubtitle.getMeasuredHeight() : 0;
        int ly = h - titleH - subH;
        if (largeTitle.getVisibility() != GONE) {
          largeTitle.layout(lx, ly, lx + textW, ly + titleH);
          ly += titleH;
        }
        if (largeSubtitle.getVisibility() != GONE)
          largeSubtitle.layout(lx, ly, lx + textW, ly + subH);
      }

      private static int exactly(int px) {
        return MeasureSpec.makeMeasureSpec(Math.max(0, px), MeasureSpec.EXACTLY);
      }

      private static int unspecified() {
        return MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED);
      }

      /** String title: not a hit target, so the controls still receive presses. */
      static final class Title extends TextView {
        Title(Context context) {
          super(context);
          setMinWidth(0);
          setMinHeight(0);
          setPadding(0, 0, 0, 0);
        }

        @Override
        public boolean dispatchTouchEvent(MotionEvent event) {
          return false;
        }
      }

      /**
       * Back mark: a 24×24 chevron-left, stroked, no tail. The path is the vector. RTL mirrors it.
       * The hit target is the group around it. A circle back keeps the v10 rect: the content row,
       * from the outer edge through 44pt past the 16pt inset.
       */
      static final class Chevron extends View {
        /** 24 viewport. {@code M} then implicit line-tos. */
        static final String PATH = "M15 18 9 12 15 6";

        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Path path = new Path();
        private final float density;

        Chevron(Context context, float density) {
          super(context);
          this.density = density;
          paint.setStyle(Paint.Style.STROKE);
          paint.setStrokeCap(Paint.Cap.ROUND);
          paint.setStrokeJoin(Paint.Join.ROUND);
          setClickable(false);
          setContentDescription("Back");
        }

        void setColor(int color) {
          paint.setColor(color);
          invalidate();
        }

        @Override
        protected void onMeasure(int ws, int hs) {
          int size = Math.round(24f * density);
          setMeasuredDimension(size, size);
        }

        @Override
        protected void onDraw(Canvas canvas) {
          float glyph = 24f * density;
          if (getWidth() <= 0 || getHeight() <= 0 || !(glyph > 0f)) return;
          float unit = glyph / 24f;
          paint.setStrokeWidth(2f * unit);
          path.rewind();
          trace(path, PATH, unit);
          float dx = (getWidth() - glyph) / 2f;
          float dy = (getHeight() - glyph) / 2f;
          boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
          canvas.save();
          canvas.translate(dx, dy);
          if (rtl) canvas.scale(-1f, 1f, glyph / 2f, glyph / 2f);
          canvas.drawPath(path, paint);
          canvas.restore();
        }

        /**
         * {@code M x y} then more pairs as line-tos. A letter starts a command; {@code M15} too.
         */
        private static void trace(Path path, String data, float unit) {
          String[] tokens = data.trim().split("\\s+");
          char command = 0;
          for (int i = 0; i < tokens.length; i++) {
            String token = tokens[i];
            if (token.isEmpty()) continue;
            if (token.length() == 1 && Character.isLetter(token.charAt(0))) {
              command = token.charAt(0);
              continue;
            }
            if (Character.isLetter(token.charAt(0))) {
              command = token.charAt(0);
              token = token.substring(1);
            }
            float x = Float.parseFloat(token) * unit;
            float y = Float.parseFloat(tokens[++i]) * unit;
            if (command == 'M' || command == 'm') {
              path.moveTo(x, y);
              command = 'L';
            } else {
              path.lineTo(x, y);
            }
          }
        }
      }

      /** One side of the content row. Children run from the outer edge inward. */
      final class Side extends ViewGroup {
        Side(Context context) {
          super(context);
          setClipChildren(false);
        }

        /** Trailing packs toward the end; leading toward the start. RTL flips both. */
        boolean fromEnd() {
          boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
          return (this == trailing) ^ rtl;
        }

        void fill(Group back, ArrayList<Mounter.BarGroup> groups, Mounter m) {
          removeAllViews();
          if (back != null && back.getVisibility() == VISIBLE) addView(back);
          if (groups == null) return;
          for (int g = 0; g < groups.size(); g++) {
            Mounter.BarGroup spec = groups.get(g);
            if (spec.items.isEmpty()) continue;
            Group group = new Group(getContext(), false);
            group.circle = spec.items.size() == 1 && loneIcon(spec.items.get(0));
            int added = 0;
            for (int i = 0; i < spec.items.size(); i++) {
              Mounter.BarEntry item = spec.items.get(i);
              if (item.custom) {
                if (addCustom(m, item.id, group)) added++;
              } else {
                group.addView(new ButtonItem(getContext(), item, m.host));
                added++;
              }
            }
            if (added > 0) addView(group);
          }
        }

        private boolean loneIcon(Mounter.BarEntry item) {
          return !item.custom && item.image != null && (item.title == null || item.title.isEmpty());
        }

        private boolean addCustom(Mounter m, int itemId, ViewGroup parent) {
          View item = m.views.get(itemId);
          if (item == null) return false;
          if (item.getParent() instanceof ViewGroup)
            ((ViewGroup) item.getParent()).removeView(item);
          int[] size = m.headerSizes.get(itemId);
          int w = size != null ? Math.max(1, size[0]) : 1;
          int h = size != null ? Math.max(1, size[1]) : 1;
          parent.addView(item, new LayoutParams(w, h));
          return true;
        }

        @Override
        protected void onMeasure(int ws, int hs) {
          int row = MeasureSpec.getSize(hs);
          int inset = px(16), gap = px(8);
          int width = 0;
          boolean any = false, first = true;
          for (int i = 0; i < getChildCount(); i++) {
            View c = getChildAt(i);
            if (c.getVisibility() == GONE) continue;
            any = true;
            boolean expanded = c instanceof Group && ((Group) c).expanded;
            if (expanded) c.measure(exactly(px(60)), exactly(row));
            else c.measure(unspecified(), MeasureSpec.makeMeasureSpec(row, MeasureSpec.AT_MOST));
            int lead = expanded ? 0 : (first ? inset : gap);
            width += lead + c.getMeasuredWidth();
            first = false;
          }
          if (!any) width = 0;
          int mode = MeasureSpec.getMode(ws), spec = MeasureSpec.getSize(ws);
          if (mode == MeasureSpec.EXACTLY) width = spec;
          else if (mode == MeasureSpec.AT_MOST) width = Math.min(width, spec);
          setMeasuredDimension(width, row);
        }

        @Override
        protected void onLayout(boolean changed, int l, int t, int r, int b) {
          int h = b - t;
          boolean end = fromEnd();
          int cursor = end ? r - l : 0;
          int inset = px(16), gap = px(8);
          boolean first = true;
          for (int i = 0; i < getChildCount(); i++) {
            View c = getChildAt(i);
            if (c.getVisibility() == GONE) continue;
            boolean expanded = c instanceof Group && ((Group) c).expanded;
            int cw = c.getMeasuredWidth();
            int ch = expanded ? h : c.getMeasuredHeight();
            int y = expanded ? 0 : Math.max(0, (h - ch) / 2);
            int lead = expanded ? 0 : (first ? inset : gap);
            if (end) {
              cursor -= lead + cw;
              c.layout(cursor, y, cursor + cw, y + ch);
            } else {
              cursor += lead;
              c.layout(cursor, y, cursor + cw, y + ch);
              cursor += cw;
            }
            first = false;
          }
        }
      }

      /**
       * One floating control. A lone icon is a 44×44 circle; anything else is a 44pt capsule. A
       * circle back keeps the wider v10 hit rect and draws the circle inside it.
       */
      final class Group extends ViewGroup {
        final boolean back;
        boolean circle;
        boolean expanded;
        int fillColor;
        int pressedColor;
        private boolean down;
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final RectF rect = new RectF();

        Group(Context context, boolean back) {
          super(context);
          this.back = back;
          setWillNotDraw(false);
          setClipChildren(false);
          setClickable(true);
          if (!back) setSoundEffectsEnabled(false);
        }

        @Override
        public boolean dispatchTouchEvent(MotionEvent event) {
          int action = event.getActionMasked();
          if (!back && action == MotionEvent.ACTION_DOWN) {
            View hit = childAt(event.getX(), event.getY());
            down = !(hit instanceof ButtonItem) || hit.isEnabled();
            if (down) invalidate();
          }
          boolean handled = super.dispatchTouchEvent(event);
          if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) {
            if (down) {
              down = false;
              invalidate();
            }
          }
          return handled;
        }

        private View childAt(float x, float y) {
          for (int i = getChildCount() - 1; i >= 0; i--) {
            View c = getChildAt(i);
            if (c.getVisibility() == GONE) continue;
            if (x >= c.getLeft() && x < c.getRight() && y >= c.getTop() && y < c.getBottom())
              return c;
          }
          return null;
        }

        @Override
        protected void onDraw(Canvas canvas) {
          int color = (down || isPressed()) ? pressedColor : fillColor;
          if (Color.alpha(color) == 0 || getWidth() <= 0 || getHeight() <= 0) return;
          float radius;
          if (circle) {
            circle(rect);
            radius = rect.height() / 2f;
          } else {
            rect.set(0, 0, getWidth(), getHeight());
            radius = getHeight() / 2f;
          }
          paint.setColor(color);
          canvas.drawRoundRect(rect, radius, radius, paint);
        }

        /** 44pt circle, 16pt from the outer edge when this group is the wide back target. */
        private void circle(RectF out) {
          float size = 44f * density;
          float left;
          float top = (getHeight() - size) / 2f;
          if (expanded) {
            boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
            float inset = 16f * density;
            left = rtl ? getWidth() - inset - size : inset;
          } else {
            left = (getWidth() - size) / 2f;
          }
          out.set(left, top, left + size, top + size);
        }

        @Override
        protected void onMeasure(int ws, int hs) {
          int row = px(44);
          measureKids();
          if (expanded) {
            int hitH =
                MeasureSpec.getMode(hs) == MeasureSpec.EXACTLY ? MeasureSpec.getSize(hs) : row;
            setMeasuredDimension(px(60), hitH);
            return;
          }
          if (circle) {
            setMeasuredDimension(row, row);
            return;
          }
          int pad = px(14), gap = px(8), sum = 0, n = 0;
          for (int i = 0; i < getChildCount(); i++) {
            View c = getChildAt(i);
            if (c.getVisibility() == GONE) continue;
            sum += c.getMeasuredWidth();
            n++;
          }
          int width = n == 0 ? row : pad * 2 + sum + gap * Math.max(0, n - 1);
          setMeasuredDimension(width, row);
        }

        private void measureKids() {
          for (int i = 0; i < getChildCount(); i++) {
            View c = getChildAt(i);
            if (c.getVisibility() == GONE) continue;
            ViewGroup.LayoutParams lp = c.getLayoutParams();
            boolean custom =
                lp != null && lp.width > 0 && lp.height > 0 && !(c instanceof ButtonItem);
            if (custom) c.measure(exactly(lp.width), exactly(lp.height));
            else c.measure(unspecified(), unspecified());
          }
        }

        @Override
        protected void onLayout(boolean changed, int l, int t, int r, int b) {
          if (expanded) {
            circle(rect);
            centerIn(rect);
            return;
          }
          if (circle || soleButton()) {
            View only = firstVisible();
            if (only != null) only.layout(0, 0, getWidth(), getHeight());
            return;
          }
          int pad = px(14), gap = px(8);
          boolean end = getParent() instanceof Side && ((Side) getParent()).fromEnd();
          int x = end ? getWidth() - pad : pad;
          for (int i = 0; i < getChildCount(); i++) {
            View c = getChildAt(i);
            if (c.getVisibility() == GONE) continue;
            int cw = c.getMeasuredWidth(), ch = c.getMeasuredHeight();
            int y = (getHeight() - ch) / 2;
            if (end) {
              x -= cw;
              c.layout(x, y, x + cw, y + ch);
              x -= gap;
            } else {
              c.layout(x, y, x + cw, y + ch);
              x += cw + gap;
            }
          }
        }

        private void centerIn(RectF box) {
          for (int i = 0; i < getChildCount(); i++) {
            View c = getChildAt(i);
            if (c.getVisibility() == GONE) continue;
            int cw = c.getMeasuredWidth(), ch = c.getMeasuredHeight();
            int x = Math.round(box.left + (box.width() - cw) / 2f);
            int y = Math.round(box.top + (box.height() - ch) / 2f);
            c.layout(x, y, x + cw, y + ch);
          }
        }

        private boolean soleButton() {
          int buttons = 0, customs = 0;
          for (int i = 0; i < getChildCount(); i++) {
            View c = getChildAt(i);
            if (c.getVisibility() == GONE) continue;
            if (c instanceof ButtonItem) buttons++;
            else if (c instanceof Box) customs++;
          }
          return buttons == 1 && customs == 0;
        }

        private View firstVisible() {
          for (int i = 0; i < getChildCount(); i++) {
            View c = getChildAt(i);
            if (c.getVisibility() != GONE) return c;
          }
          return null;
        }
      }

      /** A bar button: 17pt title and/or a 24pt image. Disabled items do not press. */
      final class ButtonItem extends ViewGroup {
        final int itemId;
        final boolean ownTint;
        final int own;
        final Icon icon;
        final Title label;

        ButtonItem(Context context, Mounter.BarEntry item, HostView host) {
          super(context);
          itemId = item.id;
          ownTint = item.tintPresent;
          own = item.tint;
          setClickable(true);
          setSoundEffectsEnabled(false);
          setEnabled(item.enabled);
          setAlpha(item.enabled ? 1f : 0.4f);
          boolean hasTitle = item.title != null && !item.title.isEmpty();
          boolean hasImage = item.image != null;
          icon = new Icon(context);
          label = text(context, 17f, 400, Gravity.CENTER_VERTICAL);
          icon.setVisibility(hasImage ? VISIBLE : GONE);
          label.setVisibility(hasTitle ? VISIBLE : GONE);
          icon.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
          label.setImportantForAccessibility(IMPORTANT_FOR_ACCESSIBILITY_NO);
          if (hasTitle) {
            label.setText(item.title);
            setContentDescription(item.title);
          }
          if (hasImage) icon.load(item.image, host);
          addView(icon);
          addView(label);
          setOnClickListener(
              new OnClickListener() {
                @Override
                public void onClick(View v) {
                  if (isEnabled()) Native.sendPress(ButtonItem.this, itemId);
                }
              });
        }

        void applyTint(int barTint) {
          int color = ownTint ? own : barTint;
          label.setTextColor(color);
          icon.setTint(color);
        }

        @Override
        protected void onMeasure(int ws, int hs) {
          int w = 0, h = 0, n = 0;
          int iconPx = px(24);
          if (icon.getVisibility() != GONE) {
            icon.measure(exactly(iconPx), exactly(iconPx));
            w += iconPx;
            h = iconPx;
            n++;
          }
          if (label.getVisibility() != GONE) {
            label.measure(unspecified(), unspecified());
            if (n > 0) w += px(6);
            w += label.getMeasuredWidth();
            h = Math.max(h, label.getMeasuredHeight());
          }
          setMeasuredDimension(Math.max(w, 1), Math.max(h, 1));
        }

        @Override
        protected void onLayout(boolean changed, int l, int t, int r, int b) {
          int gap = px(6);
          int pack = 0, n = 0;
          if (icon.getVisibility() != GONE) {
            pack += icon.getMeasuredWidth();
            n++;
          }
          if (label.getVisibility() != GONE) {
            if (n > 0) pack += gap;
            pack += label.getMeasuredWidth();
          }
          boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
          int x = rtl ? (getWidth() + pack) / 2 : (getWidth() - pack) / 2;
          if (icon.getVisibility() != GONE) {
            int iw = icon.getMeasuredWidth(), ih = icon.getMeasuredHeight();
            int y = (getHeight() - ih) / 2;
            if (rtl) x -= iw;
            icon.layout(x, y, x + iw, y + ih);
            if (rtl) x -= gap;
            else x += iw + gap;
          }
          if (label.getVisibility() != GONE) {
            int lw = label.getMeasuredWidth(), lh = label.getMeasuredHeight();
            int y = (getHeight() - lh) / 2;
            if (rtl) x -= lw;
            label.layout(x, y, x + lw, y + lh);
          }
        }
      }

      /** 24pt bar-button image, tinted as a template. */
      static final class Icon extends View {
        Bitmap bitmap;
        String source;
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG | Paint.FILTER_BITMAP_FLAG);
        private final RectF dst = new RectF();

        Icon(Context context) {
          super(context);
          setClickable(false);
        }

        void setTint(int color) {
          paint.setColorFilter(new PorterDuffColorFilter(color, PorterDuff.Mode.SRC_IN));
          invalidate();
        }

        void load(final String source, final HostView host) {
          this.source = source;
          if (source == null) {
            bitmap = null;
            invalidate();
            return;
          }
          new Thread(
                  new Runnable() {
                    public void run() {
                      final Bitmap decoded = ImageBox.decode(host, source);
                      post(
                          new Runnable() {
                            public void run() {
                              if (source.equals(Icon.this.source)) {
                                bitmap = decoded;
                                invalidate();
                              }
                            }
                          });
                    }
                  })
              .start();
        }

        @Override
        protected void onDraw(Canvas canvas) {
          if (bitmap == null) return;
          dst.set(0, 0, getWidth(), getHeight());
          canvas.drawBitmap(bitmap, null, dst, paint);
        }
      }
    }
  }
}
