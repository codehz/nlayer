package dev.nlayer.android;

import android.content.Context;
import android.graphics.Canvas;
import android.text.StaticLayout;
import android.view.MotionEvent;
import android.view.ViewGroup;

final class TextBox extends ViewGroup {
  final int id;
  final Decor decor = new Decor();
  StaticLayout layout;
  float x, y, w, h;
  int[] links = new int[0];
  boolean[] linkPress = new boolean[0];
  int[] idStart = new int[0], idEnd = new int[0];
  String[] idName = new String[0];
  int[] runStart = new int[0], runEnd = new int[0], runWeight = new int[0], runColor = new int[0];
  boolean[] runItalic = new boolean[0];
  float[] runSize = new float[0];
  boolean clip;
  int pressMask;
  private int pressedLink = -1;

  TextBox(Context c, int id) {
    super(c);
    this.id = id;
    setWillNotDraw(false);
    setBackground(decor);
  }

  void setText(StaticLayout l, float x, float y, float w, float h) {
    layout = l;
    this.x = x;
    this.y = y;
    this.w = w;
    this.h = h;
    invalidate();
  }

  void setLinks(int[] data, boolean[] presses) {
    links = data;
    linkPress = presses;
    setClickable(hasPressLink() || (pressMask & 1) != 0);
  }

  /** A nested span takes `onPress`. That wins over the text view's own. */
  boolean hasPressLink() {
    for (boolean press : linkPress) if (press) return true;
    return false;
  }

  void setIds(int[] start, int[] end, String[] names) {
    idStart = start;
    idEnd = end;
    idName = names;
  }

  void setRuns(int[] start, int[] end, int[] weight, boolean[] italic, int[] color, float[] size) {
    runStart = start;
    runEnd = end;
    runWeight = weight;
    runItalic = italic;
    runColor = color;
    runSize = size;
  }

  @Override
  protected void onMeasure(int ws, int hs) {
    setMeasuredDimension(MeasureSpec.getSize(ws), MeasureSpec.getSize(hs));
  }

  @Override
  protected void onLayout(boolean c, int l, int t, int r, int b) {}

  @Override
  protected void onDraw(Canvas c) {
    super.onDraw(c);
    if (layout == null) return;
    int s = c.save();
    c.translate(x, y);
    c.clipRect(0, 0, w, h);
    layout.draw(c);
    c.restoreToCount(s);
  }

  void setTextColor(int color) {
    if (layout == null) return;
    CharSequence text = layout.getText();
    if (text instanceof android.text.Spannable) {
      android.text.Spannable s = (android.text.Spannable) text;
      android.text.style.ForegroundColorSpan[] spans =
          s.getSpans(0, s.length(), android.text.style.ForegroundColorSpan.class);
      for (android.text.style.ForegroundColorSpan span : spans) {
        s.setSpan(
            new android.text.style.ForegroundColorSpan(color),
            s.getSpanStart(span),
            s.getSpanEnd(span),
            s.getSpanFlags(span));
      }
    }
    invalidate();
  }

  private int hitLink(MotionEvent e) {
    if (layout == null) return -1;
    int line = layout.getLineForVertical((int) (e.getY() - y));
    int off = layout.getOffsetForHorizontal(line, e.getX() - x);
    for (int i = 0; i < linkPress.length; i++) {
      if (linkPress[i] && off >= links[i * 3] && off < links[i * 3 + 1]) return i;
    }
    return -1;
  }

  @Override
  public boolean onTouchEvent(MotionEvent e) {
    int action = e.getActionMasked();
    if (action == MotionEvent.ACTION_DOWN) {
      pressedLink = hitLink(e);
      // A span wins. Otherwise the text view's own `onPress`. Neither:
      // leave the touch for the nearest pressable ancestor.
      if (pressedLink < 0 && (pressMask & 1) == 0) return false;
      Mounter.pressed(this, id, true);
      return true;
    }
    if (pressedLink < 0 && (pressMask & 1) == 0) return false;
    if (action == MotionEvent.ACTION_UP) {
      int target = pressedLink >= 0 ? links[pressedLink * 3 + 2] : id;
      pressedLink = -1;
      Mounter.pressed(this, id, false);
      Native.sendPress(this, target);
    } else if (action == MotionEvent.ACTION_CANCEL) {
      pressedLink = -1;
      Mounter.pressed(this, id, false);
    }
    return true;
  }
}
