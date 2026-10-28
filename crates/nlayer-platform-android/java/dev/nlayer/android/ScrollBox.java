package dev.nlayer.android;

import android.content.Context;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.HorizontalScrollView;
import android.widget.ScrollView;

final class ScrollBox extends FrameLayout {
  final int id;
  final Decor decor = new Decor();
  boolean horizontal;
  ViewGroup scroller;
  final Box content;
  int contentW, contentH;
  float contentPointsW, contentPointsH, offsetX, offsetY;
  int pressMask;
  private boolean armed;
  private float downX, downY;

  ScrollBox(Context c, Mounter m, int id) {
    super(c);
    this.id = id;
    setBackground(decor);
    content = new Content(c, m, id);
    build(c);
  }

  private void build(Context c) {
    scroller = horizontal ? new HorizontalScrollView(c) : new ScrollView(c);
    scroller.setClipToPadding(false);
    scroller.setVerticalScrollBarEnabled(!horizontal);
    scroller.setHorizontalScrollBarEnabled(horizontal);
    scroller.setOnTouchListener(
        new android.view.View.OnTouchListener() {
          public boolean onTouch(android.view.View v, android.view.MotionEvent e) {
            return tap(e);
          }
        });
    scroller.addView(content, new ViewGroup.LayoutParams(-2, -2));
    addView(scroller, 0, new LayoutParams(-1, -1));
    size(contentW, contentH);
  }

  /** A tap on the scroller, not a drag. A child that takes the touch never reaches here. */
  private boolean tap(android.view.MotionEvent e) {
    if ((pressMask & 1) == 0) return false;
    int action = e.getActionMasked();
    if (action == android.view.MotionEvent.ACTION_DOWN) {
      armed = true;
      downX = e.getX();
      downY = e.getY();
      Mounter.pressed(this, id, true);
    } else if (action == android.view.MotionEvent.ACTION_MOVE) {
      if (armed && moved(e)) cancel();
    } else if (action == android.view.MotionEvent.ACTION_UP) {
      if (armed) {
        cancel();
        Native.sendPress(this, id);
      }
    } else if (action == android.view.MotionEvent.ACTION_CANCEL) cancel();
    return false;
  }

  private void cancel() {
    armed = false;
    Mounter.pressed(this, id, false);
  }

  private boolean moved(android.view.MotionEvent e) {
    float slop = android.view.ViewConfiguration.get(getContext()).getScaledTouchSlop();
    float dx = e.getX() - downX, dy = e.getY() - downY;
    return dx * dx + dy * dy > slop * slop;
  }

  void configure(boolean h, boolean indicator, Context c, Mounter m) {
    if (horizontal != h) {
      horizontal = h;
      if (scroller != null) {
        scroller.removeView(content);
        removeView(scroller);
      }
      build(c);
    }
    scroller.setVerticalScrollBarEnabled(indicator && !horizontal);
    scroller.setHorizontalScrollBarEnabled(indicator && horizontal);
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
    if (horizontal) scroller.scrollBy(Math.round(dx * density), 0);
    else scroller.scrollBy(0, Math.round(dy * density));
  }

  void scrollToPoints(float x, float y, float density) {
    offsetX = x;
    offsetY = y;
    int px = Math.round(x * density), py = Math.round(y * density);
    if (horizontal) scroller.scrollTo(px, 0);
    else scroller.scrollTo(0, py);
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
}
