package dev.nlayer.android;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Path;
import android.graphics.Rect;
import android.view.View;
import android.view.View.MeasureSpec;
import android.view.ViewGroup;

class Box extends ViewGroup {
  final int id;
  final Mounter mounter;
  final Decor decor = new Decor();
  boolean clip;
  int pressMask;
  boolean disabled;
  Gestures gestures;
  int pointerEvents; // 0 auto, 1 none
  int panAxis; // 0 free, 1 horizontal, 2 vertical (nsh convention)

  /** Scroll content never splits: the scroll view clips its own shadow. */
  boolean maySplit = true;

  /** Children go here while the box both clips and casts an outer shadow. */
  private Box content;

  Box(Context c, Mounter m, int id) {
    super(c);
    this.mounter = m;
    this.id = id;
    setWillNotDraw(false);
    // Children may draw shadows outside this box. Overflow hidden clips in
    // dispatchDraw, or in `content` when an outer shadow must survive.
    setClipChildren(false);
    setClipToPadding(false);
    decor.bordersAbove = true;
    setBackground(decor);
  }

  /** Where children are added. */
  ViewGroup container() {
    return content != null ? content : this;
  }

  @Override
  protected ViewGroup.LayoutParams generateDefaultLayoutParams() {
    return new MarginLayoutParams(LayoutParams.WRAP_CONTENT, LayoutParams.WRAP_CONTENT);
  }

  @Override
  protected ViewGroup.LayoutParams generateLayoutParams(ViewGroup.LayoutParams p) {
    return p instanceof MarginLayoutParams ? (MarginLayoutParams) p : new MarginLayoutParams(p);
  }

  @Override
  public ViewGroup.LayoutParams generateLayoutParams(android.util.AttributeSet a) {
    return new MarginLayoutParams(getContext(), a);
  }

  @Override
  protected void onMeasure(int ws, int hs) {
    int w = MeasureSpec.getSize(ws), h = MeasureSpec.getSize(hs);
    setMeasuredDimension(w, h);
    for (int i = 0; i < getChildCount(); i++) {
      View v = getChildAt(i);
      if (v == content) {
        v.measure(
            MeasureSpec.makeMeasureSpec(w, MeasureSpec.EXACTLY),
            MeasureSpec.makeMeasureSpec(h, MeasureSpec.EXACTLY));
        continue;
      }
      v.measure(
          MeasureSpec.makeMeasureSpec(v.getLayoutParams().width, MeasureSpec.EXACTLY),
          MeasureSpec.makeMeasureSpec(v.getLayoutParams().height, MeasureSpec.EXACTLY));
    }
  }

  @Override
  protected void onLayout(boolean changed, int l, int t, int r, int b) {
    int w = r - l, h = b - t;
    for (int i = 0; i < getChildCount(); i++) {
      View v = getChildAt(i);
      if (v == content) {
        v.layout(0, 0, w, h);
        continue;
      }
      ViewGroup.LayoutParams raw = v.getLayoutParams();
      if (raw instanceof ViewGroup.MarginLayoutParams) {
        ViewGroup.MarginLayoutParams p = (ViewGroup.MarginLayoutParams) raw;
        v.layout(p.leftMargin, p.topMargin, p.leftMargin + p.width, p.topMargin + p.height);
      }
    }
  }

  @Override
  protected void onSizeChanged(int w, int h, int oldw, int oldh) {
    super.onSizeChanged(w, h, oldw, oldh);
    if (content != null) content.layout(0, 0, w, h);
  }

  void frame(int x, int y, int w, int h) {
    ViewGroup.LayoutParams p = getLayoutParams();
    if (p == null) p = new ViewGroup.LayoutParams(w, h);
    p.width = w;
    p.height = h;
    if (getLayoutParams() == null) setLayoutParams(p);
    layout(x, y, x + w, y + h);
    requestLayout();
  }

  @Override
  protected void dispatchDraw(Canvas canvas) {
    int save = -1;
    // The content box clips. Clipping here would also clip this box's shadow.
    if (clip && content == null) {
      save = canvas.save();
      Path p = new Path();
      Rect bounds = new Rect(0, 0, getWidth(), getHeight());
      Decor.roundedRect(
          p,
          new android.graphics.RectF(bounds),
          new float[] {
            decor.radii[0],
            decor.radii[0],
            decor.radii[1],
            decor.radii[1],
            decor.radii[2],
            decor.radii[2],
            decor.radii[3],
            decor.radii[3]
          },
          decor.continuous);
      canvas.clipPath(p);
    }
    super.dispatchDraw(canvas);
    if (save >= 0) canvas.restoreToCount(save);
    decor.setBounds(0, 0, getWidth(), getHeight());
    decor.drawBorders(canvas);
  }

  /** Applies clipping after a style change. */
  void updateClip() {
    boolean split = maySplit && clip && decor.hasOuterShadows();
    if (split && content == null) {
      // Clipping this view would clip its shadow: children move into a
      // content box that clips instead.
      content = new Box(getContext(), mounter, id);
      content.maySplit = false;
      content.setBackground(null);
      content.setClickable(false);
      while (getChildCount() > 0) {
        View child = getChildAt(0);
        removeViewAt(0);
        content.addView(child);
      }
      addView(content);
      content.layout(0, 0, getWidth(), getHeight());
    } else if (!split && content != null) {
      Box old = content;
      content = null;
      removeView(old);
      while (old.getChildCount() > 0) {
        View child = old.getChildAt(0);
        old.removeViewAt(0);
        addView(child);
      }
    }
    if (content != null) {
      System.arraycopy(decor.radii, 0, content.decor.radii, 0, 4);
      content.decor.continuous = decor.continuous;
      content.clip = true;
    }
    invalidate();
  }

  @Override
  public boolean onInterceptTouchEvent(android.view.MotionEvent e) {
    if (gestures != null) {
      return gestures.onInterceptTouchEvent(e);
    }
    return super.onInterceptTouchEvent(e);
  }

  @Override
  public boolean onTouchEvent(android.view.MotionEvent e) {
    if (disabled || pointerEvents == 1) return false;

    int action = e.getActionMasked();
    // Gestures handled by Gestures class (nsh transplant)
    if (gestures != null && gestures.onTouchEvent(e)) {
      if ((action == android.view.MotionEvent.ACTION_UP
              || action == android.view.MotionEvent.ACTION_CANCEL)
          && isPressed()) Mounter.pressed(this, id, false);
      return true;
    }

    // Then press handling
    boolean press = (pressMask & 1) != 0;
    if (action == android.view.MotionEvent.ACTION_DOWN && press) {
      Mounter.pressed(this, id, true);
      return true;
    } else if (action == android.view.MotionEvent.ACTION_UP && press) {
      Mounter.pressed(this, id, false);
      if (e.getX() >= 0 && e.getY() >= 0 && e.getX() < getWidth() && e.getY() < getHeight()) {
        Native.sendPress(this, id);
      }
      return true;
    } else if (action == android.view.MotionEvent.ACTION_CANCEL && (press || isPressed())) {
      Mounter.pressed(this, id, false);
      return true;
    }
    return false;
  }
}
