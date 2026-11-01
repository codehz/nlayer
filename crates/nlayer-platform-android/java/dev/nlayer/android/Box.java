package dev.nlayer.android;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Path;
import android.graphics.Rect;
import android.view.View;
import android.view.ViewGroup;

class Box extends ViewGroup {
  final int id;
  final Mounter mounter;
  final Decor decor = new Decor();
  boolean clip;
  int pressMask;
  boolean disabled;
  Gestures gestures;
  int pointerEvents; // 0 auto, 1 none, 2 box-only, 3 box-none
  int panAxis; // 0 free, 1 horizontal, 2 vertical (nsh convention)

  Box(Context c, Mounter m, int id) {
    super(c);
    this.mounter = m;
    this.id = id;
    setWillNotDraw(false);
    decor.bordersAbove = true;
    setBackground(decor);
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
    setMeasuredDimension(MeasureSpec.getSize(ws), MeasureSpec.getSize(hs));
    for (int i = 0; i < getChildCount(); i++) {
      View v = getChildAt(i);
      v.measure(
          MeasureSpec.makeMeasureSpec(v.getLayoutParams().width, MeasureSpec.EXACTLY),
          MeasureSpec.makeMeasureSpec(v.getLayoutParams().height, MeasureSpec.EXACTLY));
    }
  }

  @Override
  protected void onLayout(boolean changed, int l, int t, int r, int b) {
    for (int i = 0; i < getChildCount(); i++) {
      View v = getChildAt(i);
      ViewGroup.LayoutParams raw = v.getLayoutParams();
      if (raw instanceof ViewGroup.MarginLayoutParams) {
        ViewGroup.MarginLayoutParams p = (ViewGroup.MarginLayoutParams) raw;
        v.layout(p.leftMargin, p.topMargin, p.leftMargin + p.width, p.topMargin + p.height);
      }
    }
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
    if (clip) {
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

    // Gestures handled by Gestures class (nsh transplant)
    if (gestures != null && gestures.onTouchEvent(e)) {
      return true;
    }

    // Then press handling
    int action = e.getActionMasked();
    boolean press = (pressMask & 1) != 0;
    if (action == android.view.MotionEvent.ACTION_DOWN && press) {
      return true;
    } else if (action == android.view.MotionEvent.ACTION_UP && press) {
      if (e.getX() >= 0 && e.getY() >= 0 && e.getX() < getWidth() && e.getY() < getHeight()) {
        Native.sendPress(this, id);
      }
      return true;
    }
    return false;
  }
}
