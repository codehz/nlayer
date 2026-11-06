// Copyright 2025 the nlayer authors. All rights reserved. Apache-2.0 license.
//
// Derived from nsh's StackBox (com.longbridge.nsh.StackBox):
// Copyright 2024 Longbridge Whale Wealth Ltd.

package dev.nlayer.android;

import android.animation.Animator;
import android.animation.AnimatorListenerAdapter;
import android.annotation.SuppressLint;
import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Path;
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
 * `<stack>`: screens stacked natively, with a header bar per screen, push / pop transitions and the
 * back button (predictive back on Android 14). The core lays every screen out at the stack's size,
 * below its header.
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
    private float headerHeight;
    private int tint;
    private int background;
    private boolean backgroundPresent;
    private int titleColor;
    private boolean backVisible = true;
    private boolean canGoBack;
    private int leftId = -1;
    private int rightId = -1;
    private int titleId = -1;
    private String titleText;
    private float lastHeaderHeight = -1;
    private float lastInsetTop = -1;
    private float lastInsetRight = -1;
    private float lastInsetBottom = -1;
    private float lastInsetLeft = -1;

    ScreenBox(Context context, Mounter mounter, int id) {
      super(context, mounter, id);
      bar = new Bar(context, mounter.density);
      bar.back.setOnClickListener(
          v -> {
            if (getParent() instanceof StackBox) ((StackBox) getParent()).back();
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
     * Applies `encode.rs` `screen_config`. `backgroundPresent` is distinct from a transparent
     * color.
     */
    void configure(
        Mounter m,
        String title,
        boolean shown,
        boolean transparent,
        int tint,
        int background,
        boolean backgroundPresent,
        int titleColor,
        boolean backVisible,
        boolean gesture,
        boolean preventBack,
        int animation,
        int left,
        int right,
        int titleView,
        float headerHeight,
        boolean canGoBack) {
      this.animation = animation;
      this.gestureEnabled = gesture;
      this.preventBack = preventBack;
      this.transparent = transparent;
      this.shown = shown;
      this.tint = tint;
      this.background = background;
      this.backgroundPresent = backgroundPresent;
      this.titleColor = titleColor;
      this.backVisible = backVisible;
      this.headerHeight = headerHeight;
      this.canGoBack = canGoBack;
      this.titleText = title;
      this.leftId = left;
      this.rightId = right;
      this.titleId = titleView;
      bar.setVisibility(shown ? VISIBLE : GONE);
      applyHeight();
      placeHeader(m);
      applyChrome();
      reportHeader();
      reportInsets();
    }

    /** The system scheme changed. Defaults follow `host.dark`; explicit colors stay. */
    void schemeChanged() {
      applyChrome();
    }

    /**
     * A positive encoded height is the whole bar, status inset included. Otherwise the bar is the
     * status inset plus the 44pt content row. The row sits at the bottom of the bar.
     */
    private void applyHeight() {
      float d = mounter.density;
      int row = contentRowPx(d);
      int encoded = Math.round(headerHeight * d);
      int height = encoded > 0 ? encoded : mounter.host.safeTop + row;
      bar.contentRow = row;
      ViewGroup.LayoutParams p = bar.getLayoutParams();
      if (p.height != height) {
        p.height = height;
        bar.setLayoutParams(p);
      }
    }

    private void placeHeader(Mounter m) {
      bar.leading.keep(bar.back);
      bar.trailing.removeAllViews();
      for (int i = bar.getChildCount() - 1; i >= 0; i--) {
        View child = bar.getChildAt(i);
        if (child != bar.leading && child != bar.trailing && child != bar.title)
          bar.removeViewAt(i);
      }
      boolean showBack = canGoBack && backVisible && leftId < 0;
      bar.back.setVisibility(showBack ? VISIBLE : GONE);
      if (leftId >= 0) addItem(m, leftId, bar.leading);
      if (rightId >= 0) addItem(m, rightId, bar.trailing);
      if (titleId >= 0) {
        bar.title.setVisibility(GONE);
        addItem(m, titleId, bar);
        View custom = m.views.get(titleId);
        if (custom != null) custom.bringToFront();
      } else {
        boolean has = titleText != null && !titleText.isEmpty();
        bar.title.setText(has ? titleText : "");
        bar.title.setVisibility(has ? VISIBLE : GONE);
        if (has) bar.title.bringToFront();
      }
    }

    private void addItem(Mounter m, int itemId, ViewGroup parent) {
      View item = m.views.get(itemId);
      if (item == null) return;
      if (item.getParent() instanceof ViewGroup) ((ViewGroup) item.getParent()).removeView(item);
      int[] size = m.headerSizes.get(itemId);
      int w = size != null ? size[0] : 1, h = size != null ? size[1] : 1;
      parent.addView(item, new ViewGroup.LayoutParams(Math.max(1, w), Math.max(1, h)));
    }

    private void applyChrome() {
      boolean dark = mounter.host.dark;
      // Light: background #ffffff, title #000000, buttons #007AFF.
      // Dark: background #000000, title #ffffff, buttons #0A84FF.
      int buttons = tint != 0 ? tint : (dark ? 0xFF0A84FF : 0xFF007AFF);
      int titles = titleColor != 0 ? titleColor : (dark ? 0xFFFFFFFF : 0xFF000000);
      int bg;
      if (transparent) bg = Color.TRANSPARENT;
      else if (backgroundPresent) bg = background;
      else bg = dark ? 0xFF000000 : 0xFFFFFFFF;
      bar.setBackgroundColor(bg);
      bar.title.setTextColor(titles);
      bar.back.setColor(buttons);
      // Hairline only when the bar is shown, opaque, and no background was set.
      bar.hairline = shown && !transparent && !backgroundPresent;
      bar.hairlineColor =
          dark
              ? Color.argb((int) Math.round(0.6 * 255), 0x54, 0x54, 0x58)
              : Color.argb((int) Math.round(0.29 * 255), 0x3C, 0x3C, 0x43);
      bar.invalidate();
    }

    /** Reports header height to native. */
    void reportHeader() {
      if (!isAttachedToWindow()) return;
      // A hidden or transparent bar does not push content. Reporting 0 would
      // replace the last real height, and the next opaque frame would start
      // at the top until this call reports again.
      if (!shown || transparent) return;
      int safeTopPx = mounter.host.safeTop;
      int barPx = contentRowPx(mounter.density);
      float height = (safeTopPx + barPx) / mounter.density;
      if (height == lastHeaderHeight) return;
      long handle = mounter.host.handle();
      if (handle == 0) return;
      lastHeaderHeight = height;
      Native.screenHeader(handle, id, height);
    }

    /**
     * Top inset of the screen's content, in points. An opaque bar already pushes the content down,
     * so only the status bar past that bar remains. A transparent bar does not, and still counts:
     * iOS's safe area includes a transparent navigation bar.
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
      if (!isAttachedToWindow()) return;
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
     * Header drawn in the page, so it moves with the transition. The status inset is the space
     * above a 44pt content row. Leading and trailing each take half the width and clip to it.
     */
    @SuppressLint("ViewConstructor")
    static final class Bar extends ViewGroup {
      final Column leading;
      final Column trailing;
      final TextView title;
      final Chevron back;
      int contentRow;
      private boolean hairline;
      private int hairlineColor;
      private final float density;
      private final Paint line = new Paint(Paint.ANTI_ALIAS_FLAG);

      Bar(Context context, float density) {
        super(context);
        this.density = density;
        contentRow = contentRowPx(density);
        setClipChildren(true);
        leading = new Column(context, density, true);
        trailing = new Column(context, density, false);
        title = new Title(context);
        title.setMaxLines(1);
        title.setHorizontallyScrolling(false);
        title.setEllipsize(TextUtils.TruncateAt.END);
        title.setTextSize(TypedValue.COMPLEX_UNIT_PX, 17f * density);
        title.setTypeface(Typeface.create(Typeface.DEFAULT, 600, false));
        title.setGravity(Gravity.CENTER);
        title.setPadding(0, 0, 0, 0);
        title.setMinimumWidth(0);
        title.setMinimumHeight(0);
        title.setClickable(false);
        title.setLongClickable(false);
        title.setFocusable(false);
        title.setVisibility(GONE);
        back = new Chevron(context, density);
        back.setVisibility(GONE);
        addView(leading);
        addView(trailing);
        addView(title);
        leading.addView(
            back, new LayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT));
      }

      /**
       * Back mark: a 24×24 chevron-left, stroked, no tail. The path is the vector, inlined rather
       * than a drawable resource. RTL mirrors it toward the start edge.
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
          setContentDescription("Back");
          setClickable(true);
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
          float unit = getWidth() / 24f;
          paint.setStrokeWidth(2f * unit);
          path.rewind();
          trace(path, PATH, unit);
          canvas.save();
          if (getLayoutDirection() == LAYOUT_DIRECTION_RTL) {
            canvas.scale(-1f, 1f, getWidth() / 2f, getHeight() / 2f);
          }
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

      /** Empty bar chrome swallows presses so they do not fall through to the page. */
      @Override
      public boolean onTouchEvent(MotionEvent event) {
        return true;
      }

      @Override
      protected void dispatchDraw(Canvas canvas) {
        super.dispatchDraw(canvas);
        if (!hairline || getWidth() <= 0 || getHeight() <= 0) return;
        float t = 0.5f * density;
        if (!(t > 0f)) return;
        line.setColor(hairlineColor);
        canvas.drawRect(0, getHeight() - t, getWidth(), getHeight(), line);
      }

      @Override
      protected void onMeasure(int ws, int hs) {
        int w = MeasureSpec.getSize(ws), h = MeasureSpec.getSize(hs);
        setMeasuredDimension(w, h);
        int row = Math.min(Math.max(0, contentRow), Math.max(0, h));
        int leadW = w / 2;
        int rowSpec = MeasureSpec.makeMeasureSpec(row, MeasureSpec.EXACTLY);
        leading.measure(MeasureSpec.makeMeasureSpec(leadW, MeasureSpec.EXACTLY), rowSpec);
        trailing.measure(MeasureSpec.makeMeasureSpec(w - leadW, MeasureSpec.EXACTLY), rowSpec);
        if (title.getVisibility() != GONE) {
          int max = Math.max(0, w - 2 * Math.round(16f * density));
          title.measure(
              MeasureSpec.makeMeasureSpec(max, MeasureSpec.AT_MOST),
              MeasureSpec.makeMeasureSpec(row, MeasureSpec.AT_MOST));
        }
        for (int i = 0; i < getChildCount(); i++) {
          View c = getChildAt(i);
          if (c == leading || c == trailing || c == title || c.getVisibility() == GONE) continue;
          ViewGroup.LayoutParams lp = c.getLayoutParams();
          int cw = lp != null && lp.width > 0 ? lp.width : 1;
          int ch = lp != null && lp.height > 0 ? lp.height : 1;
          c.measure(
              MeasureSpec.makeMeasureSpec(cw, MeasureSpec.EXACTLY),
              MeasureSpec.makeMeasureSpec(ch, MeasureSpec.EXACTLY));
        }
      }

      @Override
      protected void onLayout(boolean changed, int l, int t, int r, int b) {
        int w = r - l, h = b - t;
        int row = Math.min(Math.max(0, contentRow), Math.max(0, h));
        int top = h - row;
        boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
        int leadW = w / 2;
        int trailW = w - leadW;
        int leadX = rtl ? w - leadW : 0;
        int trailX = rtl ? 0 : leadW;
        leading.layout(leadX, top, leadX + leadW, top + row);
        trailing.layout(trailX, top, trailX + trailW, top + row);
        if (title.getVisibility() != GONE) {
          int tw = title.getMeasuredWidth(), th = title.getMeasuredHeight();
          int x = (w - tw) / 2;
          int y = top + Math.max(0, (row - th) / 2);
          title.layout(x, y, x + tw, y + th);
        }
        for (int i = 0; i < getChildCount(); i++) {
          View c = getChildAt(i);
          if (c == leading || c == trailing || c == title || c.getVisibility() == GONE) continue;
          int cw = c.getMeasuredWidth(), ch = c.getMeasuredHeight();
          int x = (w - cw) / 2;
          int y = top + (row - ch) / 2;
          c.layout(x, y, x + cw, y + ch);
        }
      }

      /** String title: not a hit target, so the chevron and header items still receive presses. */
      static final class Title extends TextView {
        Title(Context context) {
          super(context);
        }

        @Override
        public boolean dispatchTouchEvent(MotionEvent event) {
          return false;
        }
      }

      /** One half of the bar. Children clip to this half and inset 16pt from the outer edge. */
      static final class Column extends ViewGroup {
        final float density;
        final boolean leading;

        Column(Context context, float density, boolean leading) {
          super(context);
          this.density = density;
          this.leading = leading;
          setClipChildren(true);
        }

        void keep(View stay) {
          for (int i = getChildCount() - 1; i >= 0; i--) {
            if (getChildAt(i) != stay) removeViewAt(i);
          }
        }

        @Override
        protected void onMeasure(int ws, int hs) {
          int w = MeasureSpec.getSize(ws), h = MeasureSpec.getSize(hs);
          setMeasuredDimension(w, h);
          int avail = Math.max(0, w - Math.round(16f * density));
          for (int i = 0; i < getChildCount(); i++) {
            View c = getChildAt(i);
            if (c.getVisibility() == GONE) continue;
            ViewGroup.LayoutParams lp = c.getLayoutParams();
            if (lp != null && lp.width > 0 && lp.height > 0) {
              c.measure(
                  MeasureSpec.makeMeasureSpec(lp.width, MeasureSpec.EXACTLY),
                  MeasureSpec.makeMeasureSpec(lp.height, MeasureSpec.EXACTLY));
            } else {
              c.measure(
                  MeasureSpec.makeMeasureSpec(avail, MeasureSpec.AT_MOST),
                  MeasureSpec.makeMeasureSpec(Math.max(0, h), MeasureSpec.AT_MOST));
            }
          }
        }

        @Override
        protected void onLayout(boolean changed, int l, int t, int r, int b) {
          int w = r - l, h = b - t;
          int inset = Math.round(16f * density);
          boolean rtl = getLayoutDirection() == LAYOUT_DIRECTION_RTL;
          for (int i = 0; i < getChildCount(); i++) {
            View c = getChildAt(i);
            if (c.getVisibility() == GONE) continue;
            int cw = c.getMeasuredWidth(), ch = c.getMeasuredHeight();
            int x = (leading != rtl) ? inset : w - inset - cw;
            int y = (h - ch) / 2;
            c.layout(x, y, x + cw, y + ch);
          }
        }
      }
    }
  }
}
