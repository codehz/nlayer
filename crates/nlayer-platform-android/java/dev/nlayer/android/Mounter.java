package dev.nlayer.android;

import android.text.StaticLayout;
import android.util.SparseArray;
import android.view.View;
import android.view.ViewGroup;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;

final class Mounter {
  private static final int CREATE = 0,
      INSERT = 1,
      REMOVE = 2,
      DELETE = 3,
      FRAME = 4,
      VIEW_STYLE = 5,
      TEXT = 6,
      EVENTS = 7,
      IMAGE = 8,
      SCROLL_CONFIG = 9,
      SCROLL_CONTENT = 10,
      SCROLL_BY = 11,
      STATE_STYLE = 12,
      INPUT = 13,
      INPUT_VALUE = 14,
      INPUT_CONTENT = 15,
      TEST_ID = 16,
      COMMAND = 17,
      ACCESSIBILITY = 18,
      LONG_PRESS_DELAY = 19;
  final HostView host;
  final float density;
  final SparseArray<View> views = new SparseArray<>();
  final SparseArray<Integer> events = new SparseArray<>();
  final SparseArray<String> testIds = new SparseArray<>();
  final SparseArray<A11y.Info> accessibility = new SparseArray<>();
  final SparseArray<Style> bases = new SparseArray<>();
  final SparseArray<Style> active = new SparseArray<>();
  final SparseArray<Style> hover = new SparseArray<>();
  final SparseArray<Style> focus = new SparseArray<>();
  final SparseArray<Integer> activeColor = new SparseArray<>();
  final SparseArray<Integer> hoverColor = new SparseArray<>();
  final SparseArray<Integer> focusColor = new SparseArray<>();
  final SparseArray<Float> longDelays = new SparseArray<>();
  final SparseArray<Boolean> longDown = new SparseArray<>();
  final SparseArray<Boolean> longFired = new SparseArray<>();
  final SparseArray<Runnable> longRuns = new SparseArray<>();

  Mounter(HostView h) {
    host = h;
    density = h.density;
    views.put(1, h);
  }

  void apply(byte[] bytes, Object[] objects) {
    ByteBuffer b = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN);
    while (b.hasRemaining()) {
      switch (b.getInt()) {
        case CREATE:
          create(b.getInt(), b.getInt());
          break;
        case INSERT:
          insert(b.getInt(), b.getInt(), b.getInt());
          break;
        case REMOVE:
          remove(b.getInt());
          break;
        case DELETE:
          delete(b.getInt());
          break;
        case FRAME:
          frame(b.getInt(), b.getFloat(), b.getFloat(), b.getFloat(), b.getFloat());
          break;
        case VIEW_STYLE:
          style(b);
          break;
        case TEXT:
          text(b, objects);
          break;
        case EVENTS:
          events(b);
          break;
        case IMAGE:
          image(b, objects);
          break;
        case SCROLL_CONFIG:
          scrollConfig(b);
          break;
        case SCROLL_CONTENT:
          scrollContent(b);
          break;
        case SCROLL_BY:
          scrollBy(b);
          break;
        case STATE_STYLE:
          stateStyle(b);
          break;
        case INPUT:
          input(b, objects);
          break;
        case INPUT_VALUE:
          inputValue(b, objects);
          break;
        case INPUT_CONTENT:
          inputContent(b);
          break;
        case TEST_ID:
          {
            int id = b.getInt(), s = b.getInt();
            testIds.put(id, s < 0 ? null : (String) objects[s]);
            break;
          }
        case COMMAND:
          command(b);
          break;
        case ACCESSIBILITY:
          accessibility(b, objects);
          break;
        case LONG_PRESS_DELAY:
          longDelay(b.getInt(), b.getFloat());
          break;
        default:
          throw new IllegalArgumentException("Unknown nlayer opcode");
      }
    }
  }

  private void events(ByteBuffer b) {
    int id = b.getInt(), mask = b.getInt();
    b.getInt();
    b.getInt();
    b.getInt();
    events.put(id, mask);
    View v = views.get(id);
    if (v == null) return;
    boolean press = (mask & 1) != 0 || (mask & (1 << 11)) != 0;
    if (v instanceof Box) ((Box) v).pressMask = mask;
    if (v instanceof ImageBox) ((ImageBox) v).pressMask = mask;
    if (v instanceof TextBox) {
      ((TextBox) v).pressMask = mask;
      press = press || ((TextBox) v).hasPressLink();
    }
    if (v instanceof ScrollBox) ((ScrollBox) v).pressMask = mask;
    if (v instanceof InputBox) ((InputBox) v).pressMask = mask;
    v.setClickable(press);
    if ((mask & (1 << 11)) != 0) {
      v.setOnTouchListener(
          (view, event) -> {
            longTouch(id, event);
            return false;
          });
    } else {
      v.setOnTouchListener(null);
      cancelLong(id);
    }
  }

  private void longDelay(int id, float delay) {
    if (delay < 0) longDelays.remove(id);
    else longDelays.put(id, delay);
  }

  private void longTouch(int id, android.view.MotionEvent event) {
    int action = event.getActionMasked();
    if (action == android.view.MotionEvent.ACTION_DOWN) {
      longDown.put(id, true);
      longFired.put(id, false);
      cancelRun(id);
      Runnable run =
          () -> {
            longRuns.remove(id);
            if (!Boolean.TRUE.equals(longDown.get(id))) return;
            View view = views.get(id);
            if (view == null || Mounter.blocked(view.getParent())) return;
            longFired.put(id, true);
            host.longPress(id);
          };
      longRuns.put(id, run);
      float delay = longDelays.get(id, 500f);
      host.postDelayed(run, Math.max(0, (long) delay));
    } else if (action == android.view.MotionEvent.ACTION_UP
        || action == android.view.MotionEvent.ACTION_CANCEL) {
      longDown.put(id, false);
      cancelRun(id);
      if (action == android.view.MotionEvent.ACTION_CANCEL) longFired.put(id, false);
    }
  }

  private void cancelRun(int id) {
    Runnable run = longRuns.get(id);
    if (run != null) {
      host.removeCallbacks(run);
      longRuns.remove(id);
    }
  }

  private void cancelLong(int id) {
    longDown.put(id, false);
    cancelRun(id);
  }

  /** A release after a long press does not also fire `onPress`. */
  boolean longPressSuppresses(int id) {
    boolean fired = Boolean.TRUE.equals(longFired.get(id));
    if (fired) longFired.put(id, false);
    return fired;
  }

  private void accessibility(ByteBuffer b, Object[] objects) {
    int id = b.getInt();
    String role = string(objects, b.getInt());
    String label = string(objects, b.getInt());
    String hint = string(objects, b.getInt());
    int flags = b.getInt();
    A11y.Info info = new A11y.Info();
    info.role = role == null || role.equals("none") ? null : role;
    info.label = label;
    info.hint = hint;
    info.hidden = (flags & 1) != 0;
    info.disabled = (flags & 2) != 0;
    info.selected = (flags & 4) != 0;
    info.checked = b.getInt();
    info.expanded = b.getInt();
    info.min = b.getFloat();
    info.max = b.getFloat();
    info.now = b.getFloat();
    info.valueText = string(objects, b.getInt());
    accessibility.put(id, info);
    View v = views.get(id);
    if (v == null) return;
    A11y.apply(v, info);
    setDisabled(v, info.disabled);
  }

  private static String string(Object[] objects, int index) {
    return index < 0 ? null : (String) objects[index];
  }

  /** A view above `v` (not `v`) is disabled: its presses do not happen there. */
  static boolean blocked(android.view.ViewParent parent) {
    while (parent != null) {
      if (parent instanceof Box && ((Box) parent).disabled) return true;
      if (parent instanceof ScrollBox && ((ScrollBox) parent).disabled) return true;
      if (parent instanceof InputBox && ((InputBox) parent).disabled) return true;
      parent = parent.getParent();
    }
    return false;
  }

  private static void setDisabled(View v, boolean disabled) {
    if (v instanceof Box) ((Box) v).disabled = disabled;
    if (v instanceof TextBox) ((TextBox) v).disabled = disabled;
    if (v instanceof ImageBox) ((ImageBox) v).disabled = disabled;
    if (v instanceof ScrollBox) ((ScrollBox) v).disabled = disabled;
    if (v instanceof InputBox) ((InputBox) v).disabled = disabled;
  }

  private void create(int id, int kind) {
    if (views.get(id) != null) return;
    View v;
    switch (kind) {
      case 0:
        v = new Box(host.getContext(), this, id);
        break;
      case 1:
        v = new TextBox(host.getContext(), id);
        break;
      case 2:
        v = new ImageBox(host.getContext(), id);
        break;
      case 3:
        v = new ScrollBox(host.getContext(), this, id);
        break;
      case 4:
        v = new InputBox(host.getContext(), id);
        break;
      default:
        throw new IllegalArgumentException("Unknown kind");
    }
    final int vid = id;
    if (v instanceof InputBox)
      ((InputBox) v)
          .edit.setOnFocusChangeListener(
              new View.OnFocusChangeListener() {
                public void onFocusChange(View view, boolean has) {
                  focusChanged(view);
                }
              });
    v.setOnHoverListener(
        new View.OnHoverListener() {
          public boolean onHover(View view, android.view.MotionEvent e) {
            view.setHovered(e.getAction() != android.view.MotionEvent.ACTION_HOVER_EXIT);
            paint(vid);
            return false;
          }
        });
    v.setOnFocusChangeListener(
        new View.OnFocusChangeListener() {
          public void onFocusChange(View view, boolean has) {
            focusChanged(view);
          }
        });
    views.put(id, v);
  }

  private void insert(int parent, int child, int before) {
    View p = views.get(parent), c = views.get(child);
    if (p instanceof ScrollBox) p = ((ScrollBox) p).content;
    if (!(p instanceof ViewGroup) || c == null || c == host) return;
    ViewGroup g = (ViewGroup) p;
    if (c.getParent() instanceof ViewGroup) ((ViewGroup) c.getParent()).removeView(c);
    int at = g.getChildCount();
    if (before >= 0) {
      View next = views.get(before);
      for (int i = 0; i < g.getChildCount(); i++)
        if (g.getChildAt(i) == next) {
          at = i;
          break;
        }
    }
    g.addView(c, at);
  }

  private void remove(int id) {
    View v = views.get(id);
    if (v != null && v.getParent() instanceof ViewGroup) ((ViewGroup) v.getParent()).removeView(v);
  }

  private void delete(int id) {
    if (id == 1) return;
    remove(id);
    views.remove(id);
    events.remove(id);
    testIds.remove(id);
    accessibility.remove(id);
    bases.remove(id);
    active.remove(id);
    hover.remove(id);
    focus.remove(id);
    activeColor.remove(id);
    hoverColor.remove(id);
    focusColor.remove(id);
    longDelays.remove(id);
    longDown.remove(id);
    longFired.remove(id);
    cancelRun(id);
  }

  private void frame(int id, float x, float y, float w, float h) {
    View v = views.get(id);
    if (v == null || id == 1) return;
    v.setTag(new float[] {x, y, w, h});
    int px = Math.round(x * density),
        py = Math.round(y * density),
        pw = Math.round(w * density),
        ph = Math.round(h * density);
    ViewGroup.LayoutParams lp = v.getLayoutParams();
    if (!(lp instanceof ViewGroup.MarginLayoutParams))
      lp = new ViewGroup.MarginLayoutParams(pw, ph);
    lp.width = pw;
    lp.height = ph;
    ((ViewGroup.MarginLayoutParams) lp).leftMargin = px;
    ((ViewGroup.MarginLayoutParams) lp).topMargin = py;
    v.setLayoutParams(lp);
    v.layout(px, py, px + pw, py + ph);
    Decor d = decor(v);
    if (d != null) d.applyPivot(v);
  }

  static final class Style {
    int bg, bs;
    float[] r = new float[4], w = new float[4], t = new float[8];
    int[] c = new int[4];
    int[] opKind = new int[0];
    float[] opValue = new float[0];
    boolean continuous, overflow, hidden, oxFraction, oyFraction, xEnd, yEnd;
    int oxKind, oyKind, xKind, yKind;
    float ox, oy, x, y, opacity;
  }

  private Style readStyle(ByteBuffer b) {
    Style s = new Style();
    s.bg = b.getInt();
    for (int i = 0; i < 4; i++) s.r[i] = b.getFloat() * density;
    s.continuous = b.getInt() != 0;
    for (int i = 0; i < 4; i++) s.w[i] = b.getFloat() * density;
    for (int i = 0; i < 4; i++) s.c[i] = b.getInt();
    s.bs = b.getInt();
    s.opacity = b.getFloat();
    s.overflow = b.getInt() != 0;
    for (int i = 0; i < 8; i++) s.t[i] = b.getFloat();
    s.hidden = b.getInt() != 0;
    s.oxKind = b.getInt();
    s.ox = b.getFloat();
    s.oyKind = b.getInt();
    s.oy = b.getFloat();
    s.xEnd = b.getInt() != 0;
    s.xKind = b.getInt();
    s.x = b.getFloat();
    s.yEnd = b.getInt() != 0;
    s.yKind = b.getInt();
    s.y = b.getFloat();
    s.oxFraction = s.oxKind == 1;
    s.oyFraction = s.oyKind == 1;
    int n = b.getInt();
    s.opKind = new int[n];
    s.opValue = new float[n];
    for (int i = 0; i < n; i++) {
      s.opKind[i] = b.getInt();
      s.opValue[i] = b.getFloat();
    }
    return s;
  }

  private Decor decor(View v) {
    if (v instanceof Box) return ((Box) v).decor;
    if (v instanceof TextBox) return ((TextBox) v).decor;
    if (v instanceof ImageBox) return ((ImageBox) v).decor;
    if (v instanceof ScrollBox) return ((ScrollBox) v).decor;
    if (v instanceof InputBox) return ((InputBox) v).decor;
    return null;
  }

  private void applyStyle(View v, Style s) {
    Decor d = decor(v);
    if (d != null) {
      d.fill = s.bg;
      System.arraycopy(s.r, 0, d.radii, 0, 4);
      d.continuous = s.continuous;
      System.arraycopy(s.w, 0, d.widths, 0, 4);
      System.arraycopy(s.c, 0, d.colors, 0, 4);
      d.borderStyle = s.bs;
      d.hidden = s.hidden;
      d.originXFraction = s.oxFraction;
      d.originYFraction = s.oyFraction;
      d.originX = s.oxFraction ? s.ox : s.ox * density;
      d.originY = s.oyFraction ? s.oy : s.oy * density;
      d.invalidateSelf();
    }
    v.setAlpha(s.opacity);
    v.setVisibility(s.hidden ? View.INVISIBLE : View.VISIBLE);
    v.setTranslationX(s.t[0] * density);
    v.setTranslationY(s.t[1] * density);
    v.setScaleX(s.t[2]);
    v.setScaleY(s.t[3]);
    v.setRotation(s.t[4]);
    v.setRotationX(s.t[5]);
    v.setRotationY(s.t[6]);
    if (s.t[7] != 0) v.setCameraDistance(-density / s.t[7]);
    if (d != null) d.applyPivot(v);
    if (v instanceof Box) ((Box) v).clip = s.overflow;
    if (v instanceof InputBox) ((InputBox) v).clip = s.overflow;
    if (v instanceof ImageBox)
      ((ImageBox) v)
          .setPosition(
              s.xEnd,
              s.xKind,
              s.xKind == 0 ? s.x * density : s.x,
              s.yEnd,
              s.yKind,
              s.yKind == 0 ? s.y * density : s.y);
  }

  private float position(boolean end, int kind, float val) {
    float f = kind == 0 ? val * density : val;
    return end ? 1 - f : f;
  }

  private void style(ByteBuffer b) {
    int id = b.getInt();
    Style s = readStyle(b);
    bases.put(id, s);
    paint(id);
  }

  private void text(ByteBuffer b, Object[] o) {
    int id = b.getInt(), li = b.getInt();
    float x = b.getFloat() * density,
        y = b.getFloat() * density,
        w = b.getFloat() * density,
        h = b.getFloat() * density;
    int n = b.getInt();
    int[] links = new int[n * 3];
    boolean[] presses = new boolean[n];
    for (int i = 0; i < n; i++) {
      links[3 * i] = b.getInt();
      links[3 * i + 1] = b.getInt();
      links[3 * i + 2] = b.getInt();
      presses[i] = (b.getInt() & 1) != 0;
      b.getInt();
      b.getInt();
      b.getInt();
    }
    int count = b.getInt();
    int[] idStart = new int[count], idEnd = new int[count];
    String[] ids = new String[count];
    for (int i = 0; i < count; i++) {
      idStart[i] = b.getInt();
      idEnd[i] = b.getInt();
      int index = b.getInt();
      ids[i] = index < 0 ? "" : (String) o[index];
    }
    int runs = b.getInt();
    int[] runStart = new int[runs],
        runEnd = new int[runs],
        runWeight = new int[runs],
        runColor = new int[runs];
    boolean[] runItalic = new boolean[runs];
    float[] runSize = new float[runs];
    for (int i = 0; i < runs; i++) {
      runStart[i] = b.getInt();
      runEnd[i] = b.getInt();
      runWeight[i] = b.getInt();
      runItalic[i] = b.getInt() != 0;
      runColor[i] = b.getInt();
      runSize[i] = b.getFloat();
    }
    View v = views.get(id);
    if (v instanceof TextBox && li >= 0 && li < o.length && o[li] instanceof StaticLayout) {
      TextBox text = (TextBox) v;
      text.setText((StaticLayout) o[li], x, y, w, h);
      text.setLinks(links, presses);
      text.setIds(idStart, idEnd, ids);
      text.setRuns(runStart, runEnd, runWeight, runItalic, runColor, runSize);
    }
  }

  private void image(ByteBuffer b, Object[] o) {
    int id = b.getInt(), ix = b.getInt(), mode = b.getInt();
    View v = views.get(id);
    if (v instanceof ImageBox) ((ImageBox) v).load(ix < 0 ? null : (String) o[ix], mode, host);
  }

  private void scrollConfig(ByteBuffer b) {
    int id = b.getInt();
    boolean horizontal = b.getInt() != 0;
    int taps = b.getInt();
    int dismiss = b.getInt();
    View v = views.get(id);
    if (v instanceof ScrollBox)
      ((ScrollBox) v).configure(horizontal, taps, dismiss, true, host.getContext(), this);
  }

  private void scrollContent(ByteBuffer b) {
    View v = views.get(b.getInt());
    float w = b.getFloat(), h = b.getFloat();
    if (v instanceof ScrollBox) {
      ScrollBox s = (ScrollBox) v;
      s.contentPointsW = w;
      s.contentPointsH = h;
      s.size(Math.round(w * density), Math.round(h * density));
    }
  }

  private void scrollBy(ByteBuffer b) {
    View v = views.get(b.getInt());
    float x = b.getFloat(), y = b.getFloat();
    if (v instanceof ScrollBox) ((ScrollBox) v).scrollByPoints(x, y, density);
  }

  private void stateStyle(ByteBuffer b) {
    int id = b.getInt(), which = b.getInt();
    boolean present = b.getInt() != 0;
    Style s = null;
    Integer color = null;
    if (present) {
      s = readStyle(b);
      if (b.getInt() != 0) color = b.getInt();
    }
    SparseArray<Style> slot = which == 0 ? active : which == 1 ? hover : focus;
    SparseArray<Integer> colors = which == 0 ? activeColor : which == 1 ? hoverColor : focusColor;
    if (!present) {
      slot.remove(id);
      colors.remove(id);
    } else {
      slot.put(id, s);
      if (color == null) colors.remove(id);
      else colors.put(id, color);
    }
    View v = views.get(id);
    if (which == 2 && v != null) v.setFocusable(present);
    paint(id);
  }

  static void pressed(View v, int id, boolean down) {
    v.setPressed(down);
    for (View p = v; p != null; ) {
      if (p instanceof HostView) {
        ((HostView) p).mounter.paint(id);
        return;
      }
      android.view.ViewParent parent = p.getParent();
      p = parent instanceof View ? (View) parent : null;
    }
  }

  void paint(int id) {
    View v = views.get(id);
    Style base = bases.get(id);
    if (v == null || base == null) return;
    Style over = null;
    Integer color = null;
    boolean focused = focus.get(id) != null && chainFocused(v);
    if (v.isPressed()) {
      over = active.get(id);
      color = activeColor.get(id);
    } else if (v.isHovered()) {
      over = hover.get(id);
      color = hoverColor.get(id);
    } else if (focused) {
      over = focus.get(id);
      color = focusColor.get(id);
    }
    applyStyle(v, over == null ? base : merge(base, over));
    if (v instanceof TextBox) {
      if (color != null) ((TextBox) v).setTextColor(color);
    } else if (v instanceof InputBox) {
      InputBox in = (InputBox) v;
      in.applyTextColor(color != null ? color : in.textColor);
    }
  }

  /** The view, or a field inside it, has focus. `focusStyle` uses this. */
  boolean chainFocused(View v) {
    if (v.isFocused()) return true;
    if (v instanceof InputBox && ((InputBox) v).edit.isFocused()) return true;
    if (!(v instanceof ViewGroup)) return false;
    ViewGroup group = (ViewGroup) v;
    for (int i = 0; i < group.getChildCount(); i++) {
      if (chainFocused(group.getChildAt(i))) return true;
    }
    return false;
  }

  /** Repaint this view and the ancestors whose `focusStyle` tracks the field. */
  private void focusChanged(View origin) {
    for (View v = origin; v != null; ) {
      int id = idOf(v);
      if (id >= 0) paint(id);
      android.view.ViewParent parent = v.getParent();
      v = parent instanceof View ? (View) parent : null;
    }
  }

  private int idOf(View v) {
    if (v == host) return 1;
    if (v instanceof Box) return ((Box) v).id;
    if (v instanceof TextBox) return ((TextBox) v).id;
    if (v instanceof ImageBox) return ((ImageBox) v).id;
    if (v instanceof ScrollBox) return ((ScrollBox) v).id;
    if (v instanceof InputBox) return ((InputBox) v).id;
    return -1;
  }

  private Style merge(Style base, Style over) {
    Style s = copy(base);
    if (over.bg != 0) s.bg = over.bg;
    if (over.r[0] != 0 || over.r[1] != 0 || over.r[2] != 0 || over.r[3] != 0)
      System.arraycopy(over.r, 0, s.r, 0, 4);
    if (over.continuous) s.continuous = true;
    if (over.w[0] != 0 || over.w[1] != 0 || over.w[2] != 0 || over.w[3] != 0) {
      System.arraycopy(over.w, 0, s.w, 0, 4);
      System.arraycopy(over.c, 0, s.c, 0, 4);
      s.bs = over.bs;
    }
    if (over.opacity != 1) s.opacity = over.opacity;
    if (over.overflow) s.overflow = true;
    if (over.hidden) s.hidden = true;
    boolean identity =
        over.t[0] == 0
            && over.t[1] == 0
            && over.t[2] == 1
            && over.t[3] == 1
            && over.t[4] == 0
            && over.t[5] == 0
            && over.t[6] == 0
            && over.t[7] == 0;
    if (!identity) System.arraycopy(over.t, 0, s.t, 0, 8);
    return s;
  }

  private Style copy(Style a) {
    Style s = new Style();
    s.bg = a.bg;
    System.arraycopy(a.r, 0, s.r, 0, 4);
    s.continuous = a.continuous;
    System.arraycopy(a.w, 0, s.w, 0, 4);
    System.arraycopy(a.c, 0, s.c, 0, 4);
    s.bs = a.bs;
    s.opacity = a.opacity;
    s.overflow = a.overflow;
    System.arraycopy(a.t, 0, s.t, 0, 8);
    s.hidden = a.hidden;
    s.oxFraction = a.oxFraction;
    s.oyFraction = a.oyFraction;
    s.ox = a.ox;
    s.oy = a.oy;
    s.xEnd = a.xEnd;
    s.xKind = a.xKind;
    s.x = a.x;
    s.yEnd = a.yEnd;
    s.yKind = a.yKind;
    s.y = a.y;
    return s;
  }

  private void input(ByteBuffer b, Object[] o) {
    int id = b.getInt();
    float size = b.getFloat();
    int weight = b.getInt();
    boolean italic = b.getInt() != 0;
    int color = b.getInt();
    float spacing = b.getFloat(), line = b.getFloat();
    int align = b.getInt(), ph = b.getInt();
    boolean secure = b.getInt() != 0;
    int keyboard = b.getInt();
    boolean disabled = b.getInt() != 0, rtl = b.getInt() != 0, multiline = b.getInt() != 0;
    View v = views.get(id);
    if (v instanceof InputBox)
      ((InputBox) v)
          .configure(
              size,
              weight,
              italic,
              color,
              spacing,
              line,
              align,
              ph < 0 ? "" : (String) o[ph],
              secure,
              keyboard,
              disabled,
              rtl,
              multiline,
              density);
  }

  private void inputValue(ByteBuffer b, Object[] o) {
    int id = b.getInt(), count = b.getInt(), s = b.getInt();
    View v = views.get(id);
    if (v instanceof InputBox) ((InputBox) v).setValue(count, (String) o[s]);
  }

  private void inputContent(ByteBuffer b) {
    int id = b.getInt();
    float x = b.getFloat(), y = b.getFloat(), w = b.getFloat(), h = b.getFloat();
    View v = views.get(id);
    if (v instanceof InputBox) {
      android.widget.FrameLayout.LayoutParams p =
          new android.widget.FrameLayout.LayoutParams(
              Math.round(w * density), Math.round(h * density));
      p.leftMargin = Math.round(x * density);
      p.topMargin = Math.round(y * density);
      ((InputBox) v).edit.setLayoutParams(p);
    }
  }

  private void command(ByteBuffer b) {
    int id = b.getInt(), cmd = b.getInt();
    if (cmd == 1) {
      Native.dismissKeyboard(host);
      return;
    }
    if (cmd == 2) {
      int start = b.getInt(), end = b.getInt();
      View v = views.get(id);
      if (v instanceof InputBox) ((InputBox) v).setSelection(start, end);
      return;
    }
    float x = b.getFloat(), y = b.getFloat();
    b.getInt();
    View v = views.get(id);
    if (cmd == 0 && v instanceof ScrollBox) ((ScrollBox) v).scrollToPoints(x, y, density);
  }
}
