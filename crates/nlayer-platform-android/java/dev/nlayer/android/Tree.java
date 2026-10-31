package dev.nlayer.android;

import android.text.Layout;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;
import org.json.JSONArray;
import org.json.JSONObject;

/** The views on screen, as JSON a test step can name. */
final class Tree {
  private Tree() {}

  static String snapshot(HostView host) {
    try {
      JSONObject root = new JSONObject();
      Mounter m = host.mounter;
      float d = host.density;
      Pin pin = host.pin;
      root.put("width", pin != null ? pin.width : host.getWidth() / d);
      root.put("height", pin != null ? pin.height : host.getHeight() / d);
      if (host.bootError != null) root.put("error", host.bootError);
      JSONArray nodes = new JSONArray();
      for (int i = 0; i < m.views.size(); i++) nodes.put(node(m, m.views.keyAt(i), d));
      root.put("nodes", nodes);
      return root.toString();
    } catch (Exception e) {
      return "{\"width\":0,\"height\":0,\"error\":"
          + org.json.JSONObject.quote(String.valueOf(e))
          + "}";
    }
  }

  private static JSONObject node(Mounter m, int id, float density) throws Exception {
    View v = m.views.get(id);
    JSONObject o = new JSONObject();
    o.put("id", id);
    int parent = parentId(m, v);
    if (parent >= 0) o.put("parent", parent);
    o.put("kind", kind(v));
    String testId = m.testIds.get(id);
    if (testId != null) o.put("testID", testId);
    A11y.Info a = m.accessibility.get(id);
    if (a != null) {
      if (a.role != null) o.put("role", a.role);
      if (a.label != null) o.put("accessibilityLabel", a.label);
      if (a.hint != null) o.put("accessibilityHint", a.hint);
      if (a.hidden) o.put("accessibilityHidden", true);
      JSONObject state = new JSONObject();
      if (a.disabled) state.put("disabled", true);
      if (a.selected) state.put("selected", true);
      if (a.checked == 2) state.put("checked", true);
      else if (a.checked == 1) state.put("checked", false);
      else if (a.checked == 3) state.put("checked", "mixed");
      if (a.expanded == 2) state.put("expanded", true);
      else if (a.expanded == 1) state.put("expanded", false);
      if (state.length() > 0) o.put("accessibilityState", state);
      if (!Float.isNaN(a.min)
          || !Float.isNaN(a.max)
          || !Float.isNaN(a.now)
          || a.valueText != null) {
        JSONObject value = new JSONObject();
        if (!Float.isNaN(a.min)) value.put("min", a.min);
        if (!Float.isNaN(a.max)) value.put("max", a.max);
        if (!Float.isNaN(a.now)) value.put("now", a.now);
        if (a.valueText != null) value.put("text", a.valueText);
        o.put("accessibilityValue", value);
      }
    }
    Frame frame = frame(v, density, id == 1);
    if (id == 1 && m.host.pin != null) frame = new Frame(0, 0, m.host.pin.width, m.host.pin.height);
    o.put("frame", frame.json());
    Mounter.Style style = m.bases.get(id);
    o.put("hidden", style != null && style.hidden);
    o.put("opacity", style == null ? 1 : style.opacity);
    Integer mask = m.events.get(id);
    o.put("pressable", mask != null && (mask & 1) != 0);
    if (m.chainFocused(v)) o.put("focused", true);
    o.put("children", children(m, v));
    if (style != null && style.opKind.length > 0) {
      JSONArray ops = new JSONArray();
      for (int i = 0; i < style.opKind.length; i++) {
        JSONObject op = new JSONObject();
        op.put("kind", opName(style.opKind[i]));
        op.put("value", style.opValue[i]);
        ops.put(op);
      }
      o.put("transform", ops);
    }
    if (v instanceof TextBox) text((TextBox) v, o, density);
    if (v instanceof InputBox) input((InputBox) v, o);
    if (v instanceof ScrollBox) scroll((ScrollBox) v, o, density);
    if (v instanceof ImageBox) image((ImageBox) v, o, density);
    return o;
  }

  private static void text(TextBox box, JSONObject o, float density) throws Exception {
    String value = box.layout == null ? "" : box.layout.getText().toString();
    o.put("text", value);
    JSONArray runs = new JSONArray();
    for (int i = 0; i < box.runStart.length; i++) {
      JSONObject run = new JSONObject();
      run.put("text", slice(value, box.runStart[i], box.runEnd[i]));
      run.put("fontSize", box.runSize[i]);
      run.put("fontWeight", box.runWeight[i]);
      run.put("italic", box.runItalic[i]);
      int argb = box.runColor[i];
      long rgba = ((argb & 0x00ffffffL) << 8) | ((argb >>> 24) & 0xff);
      run.put("color", rgba);
      runs.put(run);
    }
    o.put("runs", runs);
    JSONArray spans = new JSONArray();
    for (int i = 0; i < box.idStart.length; i++) {
      spans.put(
          span(
              box,
              box.idStart[i],
              box.idEnd[i],
              box.idName[i],
              pressNode(box, box.idStart[i], box.idEnd[i]),
              density));
    }
    for (int i = 0; i < box.linkPress.length; i++) {
      if (!box.linkPress[i]) continue;
      int start = box.links[i * 3], end = box.links[i * 3 + 1];
      if (named(box, start, end)) continue;
      spans.put(span(box, start, end, null, box.links[i * 3 + 2], density));
    }
    o.put("spans", spans);
  }

  private static boolean named(TextBox box, int start, int end) {
    for (int i = 0; i < box.idStart.length; i++)
      if (box.idStart[i] == start && box.idEnd[i] == end) return true;
    return false;
  }

  private static Integer pressNode(TextBox box, int start, int end) {
    for (int i = 0; i < box.linkPress.length; i++) {
      if (box.linkPress[i] && box.links[i * 3] == start && box.links[i * 3 + 1] == end)
        return box.links[i * 3 + 2];
    }
    return null;
  }

  private static JSONObject span(
      TextBox box, int start, int end, String testId, Integer node, float density)
      throws Exception {
    JSONObject span = new JSONObject();
    span.put("start", start);
    span.put("end", end);
    if (testId != null) span.put("testID", testId);
    if (node != null) span.put("node", node);
    String value = box.layout == null ? "" : box.layout.getText().toString();
    span.put("text", slice(value, start, end));
    span.put("frame", spanFrame(box, start, end, density).json());
    return span;
  }

  private static Frame spanFrame(TextBox box, int start, int end, float density) {
    if (box.layout == null || start >= end) return new Frame(0, 0, 0, 0);
    Layout layout = box.layout;
    float left = Float.MAX_VALUE,
        top = Float.MAX_VALUE,
        right = -Float.MAX_VALUE,
        bottom = -Float.MAX_VALUE;
    int offset = start;
    while (offset < end) {
      int line = layout.getLineForOffset(offset);
      int lineEnd = Math.min(end, layout.getLineEnd(line));
      if (lineEnd <= offset) break;
      float a = layout.getPrimaryHorizontal(offset);
      // The offset after the last character of a line is not a caret
      // `getPrimaryHorizontal` can place: it collapses onto the start.
      int lineLimit = layout.getLineEnd(line);
      float b =
          lineEnd < lineLimit
              ? layout.getPrimaryHorizontal(lineEnd)
              : layout.getParagraphDirection(line) < 0
                  ? layout.getLineLeft(line)
                  : layout.getLineRight(line);
      left = Math.min(left, Math.min(a, b));
      right = Math.max(right, Math.max(a, b));
      top = Math.min(top, layout.getLineTop(line));
      bottom = Math.max(bottom, layout.getLineBottom(line));
      offset = lineEnd;
    }
    if (left == Float.MAX_VALUE) return new Frame(0, 0, 0, 0);
    return new Frame(
        (box.x + left) / density,
        (box.y + top) / density,
        (right - left) / density,
        (bottom - top) / density);
  }

  private static String slice(String text, int start, int end) {
    int n = text.length();
    start = Math.max(0, Math.min(start, n));
    end = Math.max(start, Math.min(end, n));
    return text.substring(start, end);
  }

  private static void input(InputBox box, JSONObject o) throws Exception {
    JSONObject input = new JSONObject();
    input.put("value", box.edit.getText().toString());
    input.put("placeholder", box.placeholder);
    input.put("secure", box.secure);
    input.put("keyboard", box.keyboardName);
    input.put("disabled", box.disabled);
    input.put("focused", box.edit.isFocused());
    input.put("multiline", box.multiline);
    o.put("input", input);
  }

  private static void scroll(ScrollBox box, JSONObject o, float density) throws Exception {
    JSONObject scroll = new JSONObject();
    scroll.put("horizontal", box.horizontal);
    scroll.put("x", box.offsetX);
    scroll.put("y", box.offsetY);
    scroll.put("contentW", box.contentPointsW);
    scroll.put("contentH", box.contentPointsH);
    o.put("scroll", scroll);
    o.put("frame", frame(box, density, false).json());
  }

  private static void image(ImageBox box, JSONObject o, float density) throws Exception {
    JSONObject image = new JSONObject();
    image.put("src", box.uri == null ? "" : box.uri);
    image.put("fit", fit(box.mode));
    float[] fitted =
        box.decor.fitted(new android.graphics.Rect(0, 0, box.getWidth(), box.getHeight()));
    boolean rounded = false;
    for (float r : fitted) if (r > 0) rounded = true;
    if (rounded) {
      JSONArray radius = new JSONArray();
      float d = density == 0 ? 1 : density;
      for (float r : fitted) radius.put(r / d);
      image.put("radius", radius);
    }
    o.put("image", image);
  }

  private static String fit(int mode) {
    switch (mode) {
      case 0:
        return "cover";
      case 1:
        return "contain";
      case 2:
        return "fill";
      case 3:
        return "none";
      default:
        return "scale-down";
    }
  }

  private static String kind(View v) {
    if (v instanceof TextBox) return "text";
    if (v instanceof ImageBox) return "image";
    if (v instanceof ScrollBox) return "scroll";
    if (v instanceof InputBox) return "input";
    return "view";
  }

  private static int parentId(Mounter m, View v) {
    android.view.ViewParent parent = v.getParent();
    while (parent instanceof View) {
      View view = (View) parent;
      int id = idOf(view);
      if (id >= 0 && m.views.get(id) == view) return id;
      parent = view.getParent();
    }
    return -1;
  }

  private static int idOf(View v) {
    if (v instanceof HostView) return 1;
    if (v instanceof StackBox) return ((StackBox) v).id;
    if (v instanceof Box) return ((Box) v).id;
    if (v instanceof TextBox) return ((TextBox) v).id;
    if (v instanceof ImageBox) return ((ImageBox) v).id;
    if (v instanceof ScrollBox) return ((ScrollBox) v).id;
    if (v instanceof InputBox) return ((InputBox) v).id;
    return -1;
  }

  private static JSONArray children(Mounter m, View v) {
    JSONArray out = new JSONArray();
    ViewGroup group =
        v instanceof ScrollBox
            ? ((ScrollBox) v).content
            : v instanceof ViewGroup ? (ViewGroup) v : null;
    if (group == null) return out;
    for (int i = 0; i < group.getChildCount(); i++) {
      View child = group.getChildAt(i);
      int id = idOf(child);
      if (id >= 0 && m.views.get(id) == child) out.put(id);
    }
    return out;
  }

  private static Frame frame(View v, float density, boolean root) {
    if (!root && v.getTag() instanceof float[]) {
      float[] points = (float[]) v.getTag();
      return new Frame(points[0], points[1], points[2], points[3]);
    }
    if (root) return new Frame(0, 0, v.getWidth() / density, v.getHeight() / density);
    ViewGroup.LayoutParams lp = v.getLayoutParams();
    float x = 0, y = 0, w = v.getWidth() / density, h = v.getHeight() / density;
    if (lp instanceof ViewGroup.MarginLayoutParams) {
      ViewGroup.MarginLayoutParams m = (ViewGroup.MarginLayoutParams) lp;
      x = m.leftMargin / density;
      y = m.topMargin / density;
      if (m.width > 0) w = m.width / density;
      if (m.height > 0) h = m.height / density;
    }
    return new Frame(x, y, w, h);
  }

  static void tap(HostView host, float xDp, float yDp, boolean up) {
    float x = xDp * host.density, y = yDp * host.density;
    long now = android.os.SystemClock.uptimeMillis();
    if (!up) host.downTime = now;
    MotionEvent event =
        MotionEvent.obtain(
            host.downTime, now, up ? MotionEvent.ACTION_UP : MotionEvent.ACTION_DOWN, x, y, 0);
    host.dispatchTouchEvent(event);
    event.recycle();
  }

  static void typeChar(HostView host, int id, String text) {
    View v = host.mounter.views.get(id);
    if (!(v instanceof InputBox) || text == null || text.isEmpty()) return;
    android.widget.EditText edit = ((InputBox) v).edit;
    edit.setShowSoftInputOnFocus(false);
    edit.requestFocus();
    InputConnection connection = edit.onCreateInputConnection(new EditorInfo());
    if (connection != null) connection.commitText(text, 1);
  }

  static void scrollTo(HostView host, int id, float x, float y) {
    View v = host.mounter.views.get(id);
    if (v instanceof ScrollBox) {
      ScrollBox box = (ScrollBox) v;
      box.scrollToPoints(x, y, host.density);
      box.noteTestGesture();
    }
  }

  static String opName(int kind) {
    switch (kind) {
      case 0:
      case 11:
        return "translateX";
      case 1:
      case 12:
        return "translateY";
      case 2:
        return "scale";
      case 3:
        return "scaleX";
      case 4:
        return "scaleY";
      case 5:
        return "rotate";
      case 6:
        return "rotateX";
      case 7:
        return "rotateY";
      case 10:
        return "perspective";
      default:
        return "translateX";
    }
  }

  private static final class Frame {
    final float x, y, w, h;

    Frame(float x, float y, float w, float h) {
      this.x = x;
      this.y = y;
      this.w = w;
      this.h = h;
    }

    JSONObject json() throws Exception {
      JSONObject o = new JSONObject();
      o.put("x", x);
      o.put("y", y);
      o.put("w", w);
      o.put("h", h);
      return o;
    }
  }
}
