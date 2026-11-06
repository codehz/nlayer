package dev.nlayer.android;

import android.text.StaticLayout;
import android.util.SparseArray;
import android.view.View;
import android.view.ViewGroup;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;

final class Mounter {
  static final class ScreenOptions {
    String title;
    boolean shown;
    boolean transparent;
    boolean tintPresent;
    int tint;
    boolean backgroundPresent;
    int background;
    boolean titleColorPresent;
    int titleColor;
    boolean backVisible;
    boolean gesture;
    boolean preventBack;
    int animation;
    int left;
    int right;
    int titleView;
    float headerHeight;
  }

  static final class ModalOptions {
    int animation;
    int presentation;
    boolean closeOnEscape;
    boolean closeOnBackdropPress;
    boolean closeOnSwipe;
  }

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
      LONG_PRESS_DELAY = 19,
      SCREEN_CONFIG = 20,
      MODAL_CONFIG = 21,
      PAN_BINDING = 22,
      PINCH_BINDING = 23,
      ROTATION_BINDING = 24,
      LIST_CONTENT = 25,
      REFRESHING = 26,
      SELECTABLE = 27,
      TRACK_STATES = 28,
      POINTER_EVENTS = 29;
  final HostView host;
  final float density;
  final SparseArray<View> views = new SparseArray<>();
  final SparseArray<Integer> events = new SparseArray<>();
  final SparseArray<Integer> panBindings = new SparseArray<>();
  final SparseArray<Boolean> pinchBindings = new SparseArray<>();
  final SparseArray<Boolean> rotationBindings = new SparseArray<>();
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
  final SparseArray<ScreenOptions> screenOptions = new SparseArray<>();
  final SparseArray<ModalOptions> modalOptions = new SparseArray<>();
  final SparseArray<int[]> headerSizes = new SparseArray<>();
  final SparseArray<ModalHost> modals = new SparseArray<>();

  /** Modals whose exit motion is still drawing. Their views stay until it ends. */
  final java.util.ArrayList<ModalHost> closingModals = new java.util.ArrayList<>();

  /** Bits the core asked to hear (`active:` 1, `hover:` 2). Focus is always reported. */
  private final android.util.SparseIntArray trackedStates = new android.util.SparseIntArray();

  /** Bits currently on, including ones the core is not reading yet. */
  private final android.util.SparseIntArray currentStates = new android.util.SparseIntArray();

  static final int STATE_PRESSED = 1, STATE_HOVERED = 2, STATE_FOCUSED = 4;

  /** Presses reported because the view tracks them and does not handle `onPress` itself. */
  private final java.util.ArrayList<Integer> observedPress = new java.util.ArrayList<>();

  /** The text with a selection (one per surface). */
  TextBox selectedText;

  /** The selectable view with a selection. */
  SelectableView selectedArea;

  /** Selectable views, by view id. */
  private final SparseArray<SelectableView> selectableViews = new SparseArray<>();

  /** URLs `Text` `href` presses opened, in order. A pin records without leaving. */
  final java.util.ArrayList<String> openedUrls = new java.util.ArrayList<>();

  /** Whether view {@code id} is a selectable view. */
  boolean selectable(int id) {
    return selectableViews.get(id) != null;
  }

  /** Monotonic counter for modal show order (topmost determination). */
  int modalShowOrder;

  final java.util.LinkedHashSet<Integer> dirtyStacks = new java.util.LinkedHashSet<>();
  final java.util.LinkedHashSet<Integer> dirtyScreens = new java.util.LinkedHashSet<>();
  final java.util.LinkedHashSet<Integer> dirtyModals = new java.util.LinkedHashSet<>();
  final android.util.SparseIntArray hosts = new android.util.SparseIntArray();
  final java.util.ArrayList<Integer> stacks = new java.util.ArrayList<>();
  final java.util.HashSet<Integer> headerItems = new java.util.HashSet<>();

  Mounter(HostView h) {
    host = h;
    density = h.density;
    views.put(1, h);
  }

  /** Drops every view except the root. The next engine's ids start over. */
  void reset() {
    if (selectedText != null) {
      selectedText.clearSelection();
      selectedText = null;
    }
    if (selectedArea != null) {
      selectedArea.clear();
      selectedArea = null;
    }
    for (int i = selectableViews.size() - 1; i >= 0; i--) selectableViews.valueAt(i).clear();
    selectableViews.clear();
    for (int i = modals.size() - 1; i >= 0; i--) modals.valueAt(i).discard();
    for (int i = closingModals.size() - 1; i >= 0; i--) closingModals.get(i).discard();
    closingModals.clear();
    host.removeAllViews();
    View root = views.get(1);
    views.clear();
    if (root != null) views.put(1, root);
    events.clear();
    panBindings.clear();
    pinchBindings.clear();
    rotationBindings.clear();
    testIds.clear();
    accessibility.clear();
    bases.clear();
    active.clear();
    hover.clear();
    focus.clear();
    activeColor.clear();
    hoverColor.clear();
    focusColor.clear();
    longDelays.clear();
    longDown.clear();
    longFired.clear();
    for (int i = longRuns.size() - 1; i >= 0; i--) {
      Runnable run = longRuns.valueAt(i);
      if (run != null) host.removeCallbacks(run);
    }
    longRuns.clear();
    screenOptions.clear();
    modalOptions.clear();
    headerSizes.clear();
    modals.clear();
    trackedStates.clear();
    currentStates.clear();
    observedPress.clear();
    openedUrls.clear();
    dirtyStacks.clear();
    dirtyScreens.clear();
    dirtyModals.clear();
    hosts.clear();
    stacks.clear();
    headerItems.clear();
    modalShowOrder = 0;
  }

  void apply(byte[] bytes, Object[] objects) {
    ByteBuffer b = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN);
    while (b.hasRemaining()) {
      int opcode = b.getInt();
      switch (opcode) {
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
          command(b, objects);
          break;
        case ACCESSIBILITY:
          accessibility(b, objects);
          break;
        case LONG_PRESS_DELAY:
          longDelay(b.getInt(), b.getFloat());
          break;
        case SCREEN_CONFIG:
          screenConfig(b, objects);
          break;
        case MODAL_CONFIG:
          modalConfig(b);
          break;
        case PAN_BINDING:
          panBinding(b);
          break;
        case PINCH_BINDING:
          pinchBinding(b);
          break;
        case ROTATION_BINDING:
          rotationBinding(b);
          break;
        case LIST_CONTENT:
          listContent(b);
          break;
        case REFRESHING:
          refreshing(b);
          break;
        case SELECTABLE:
          selectable(b.getInt(), b.getInt() != 0);
          break;
        case TRACK_STATES:
          trackStates(b.getInt(), b.getInt());
          break;
        case POINTER_EVENTS:
          pointerEvents(b.getInt(), b.getInt() != 0);
          break;
        default:
          throw new IllegalArgumentException("Unknown nlayer opcode " + opcode);
      }
    }
    finishNavigation();
  }

  private void events(ByteBuffer b) {
    int id = b.getInt(), mask = b.getInt();
    b.getInt();
    b.getInt();
    b.getInt();
    events.put(id, mask);
    View v = views.get(id);
    if (v == null) return;

    // Update gesture box based on events and bindings
    if (v instanceof Box) {
      updateGestureBox(id);
    }

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
    if (v instanceof TextBox) ((TextBox) v).noteSelectable();
    if ((mask & (1 << 11)) != 0) {
      v.setOnTouchListener(
          (view, event) -> {
            longTouch(id, event);
            return false;
          });
    } else {
      v.setOnTouchListener(null);
    }
  }

  private void panBinding(ByteBuffer b) {
    int id = b.getInt();
    int axis = b.getInt(); // 0=none, 1=x-only, 2=y-only, 3=both
    if (axis == 0) {
      panBindings.remove(id);
    } else {
      panBindings.put(id, axis);
    }
    // Set panAxis on Box: 0=free, 1=horizontal, 2=vertical. The binding can
    // arrive before the view exists; updateGestureBox sets it then.
    View v = views.get(id);
    if (v instanceof Box) {
      Box box = (Box) v;
      box.panAxis = (axis == 1) ? 1 : (axis == 2) ? 2 : 0;
    }
    updateGestureBox(id);
  }

  private void pinchBinding(ByteBuffer b) {
    int id = b.getInt();
    boolean hasScale = b.getInt() != 0;
    if (hasScale) {
      pinchBindings.put(id, true);
    } else {
      pinchBindings.remove(id);
    }
    updateGestureBox(id);
  }

  private void rotationBinding(ByteBuffer b) {
    int id = b.getInt();
    boolean hasAngle = b.getInt() != 0;
    if (hasAngle) {
      rotationBindings.put(id, true);
    } else {
      rotationBindings.remove(id);
    }
    updateGestureBox(id);
  }

  private void updateGestureBox(int id) {
    View v = views.get(id);
    if (!(v instanceof Box)) return;
    Box box = (Box) v;
    int panAxis = panBindings.get(id, 0);
    box.panAxis = (panAxis == 1) ? 1 : (panAxis == 2) ? 2 : 0;

    // Gesture box needed if: bindings OR events (17-22)
    int mask = events.get(id, 0);
    boolean hasEvents = (mask & (0x3F << 17)) != 0;
    boolean hasBindings =
        panAxis != 0 || pinchBindings.get(id, false) || rotationBindings.get(id, false);

    if ((hasEvents || hasBindings) && box.gestures == null) {
      box.gestures = new Gestures(box, id, this);
    } else if (!hasEvents && !hasBindings && box.gestures != null) {
      box.gestures.reset();
      box.gestures = null;
    }
  }

  private void longDelay(int id, float delay) {
    if (delay < 0) longDelays.remove(id);
    else longDelays.put(id, delay);
  }

  private void longTouch(int id, android.view.MotionEvent event) {
    // `none` is not the target. Descendants keep their own listeners.
    View self = views.get(id);
    if (self != null && ignoresPointer(self)) return;
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

  void cancelLong(int id) {
    longDown.put(id, false);
    cancelRun(id);
  }

  /** A finger went down: a selection that does not contain the point ends. */
  void touchDown(float rawX, float rawY) {
    if (selectedText != null && !selectedText.selectionContains(rawX, rawY))
      selectedText.clearSelection();
    if (selectedArea != null && !selectedArea.selectionContains(rawX, rawY)) selectedArea.clear();
  }

  /** The outermost selectable view {@code text} is in, or null. */
  SelectableView selectableViewOf(TextBox text) {
    if (selectableViews.size() == 0) return null;
    SelectableView outer = null;
    for (Object v = text; v instanceof View; v = ((View) v).getParent()) {
      for (int i = 0; i < selectableViews.size(); i++) {
        SelectableView area = selectableViews.valueAt(i);
        if (area.view == v) outer = area;
      }
    }
    return outer;
  }

  private void selectable(int id, boolean on) {
    View v = views.get(id);
    SelectableView area = selectableViews.get(id);
    if (on && v != null && (area == null || area.view != v)) {
      if (area != null) area.clear();
      selectableViews.put(id, new SelectableView(this, v));
    } else if (!on && area != null) {
      area.clear();
      selectableViews.remove(id);
    }
    if (v != null) refreshTexts(v);
  }

  private void refreshTexts(View v) {
    if (v instanceof TextBox) ((TextBox) v).noteSelectable();
    if (v instanceof ViewGroup) {
      ViewGroup group = (ViewGroup) v;
      for (int i = 0; i < group.getChildCount(); i++) refreshTexts(group.getChildAt(i));
    }
  }

  /** The system clipboard. Selection menus write here; there is no JS API. */
  static void copyText(android.content.Context context, String text) {
    Object service = context.getSystemService(android.content.Context.CLIPBOARD_SERVICE);
    if (!(service instanceof android.content.ClipboardManager)) return;
    ((android.content.ClipboardManager) service)
        .setPrimaryClip(android.content.ClipData.newPlainText("text", text == null ? "" : text));
  }

  /**
   * `Command::OpenUrl`: one `ACTION_VIEW`, no event and no grant. A pinned ({@code nlayer test})
   * app records the address instead of leaving.
   */
  void openUrl(String url) {
    if (url == null || url.isEmpty()) return;
    openedUrls.add(url);
    if (host.pin != null) return;
    android.content.Context context = host.getContext();
    android.content.Intent intent =
        new android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(url));
    if (!(context instanceof android.app.Activity)) {
      intent.addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK);
    }
    try {
      context.startActivity(intent);
    } catch (android.content.ActivityNotFoundException | SecurityException ignored) {
      // Nothing handles it, or the platform refused. The press already ran.
    }
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
        v = new TextBox(host.getContext(), this, id);
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
      case 5:
        v = new StackBox(host.getContext(), this, id);
        stacks.add(id);
        break;
      case 6:
        v = new StackBox.ScreenBox(host.getContext(), this, id);
        break;
      case 7:
        ModalHost modal = new ModalHost(host.getContext(), this, id);
        modals.put(id, modal);
        v = modal.root;
        break;
      case 8:
        v = new Box(host.getContext(), this, id);
        headerItems.add(id);
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
            boolean hovered = e.getAction() != android.view.MotionEvent.ACTION_HOVER_EXIT;
            view.setHovered(hovered);
            paint(vid);
            reportState(vid, STATE_HOVERED, hovered);
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
    // Modal into host
    if (modals.get(child) != null) {
      hosts.put(child, parent);
      dirtyModals.add(child);
      return;
    }
    // ScreenBox into StackBox
    if (c instanceof StackBox.ScreenBox && p instanceof StackBox) {
      StackBox stack = (StackBox) p;
      if (before >= 0) {
        int index = stack.screens.indexOf(before);
        if (index >= 0) {
          stack.screens.remove((Integer) child);
          stack.screens.add(index, child);
        } else {
          stack.screens.remove((Integer) child);
          stack.screens.add(child);
        }
      } else {
        stack.screens.remove((Integer) child);
        stack.screens.add(child);
      }
      hosts.put(child, parent);
      dirtyStacks.add(parent);
      return;
    }
    // Header item into ScreenBox
    if (headerItems.contains(child) && p instanceof StackBox.ScreenBox) {
      hosts.put(child, parent);
      dirtyScreens.add(parent);
      return;
    }
    // Generic view insertion
    if (p instanceof ScrollBox) p = ((ScrollBox) p).content;
    if (p instanceof Box) p = ((Box) p).container();
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
    refreshTexts(c);
  }

  private void remove(int id) {
    // Handle hosted children (screens in stacks, header items in screens, modals)
    if (hosts.indexOfKey(id) >= 0) {
      int parentId = hosts.get(id);
      View parent = views.get(parentId);
      if (parent instanceof StackBox) {
        StackBox stack = (StackBox) parent;
        stack.screens.remove((Integer) id);
        dirtyStacks.add(parentId);
      }
      if (modals.get(id) != null) {
        dirtyModals.add(id);
      }
      if (headerItems.contains(id)) {
        View v = views.get(id);
        if (v != null && v.getParent() instanceof ViewGroup) {
          ((ViewGroup) v.getParent()).removeView(v);
        }
        dirtyScreens.add(parentId);
      }
      hosts.delete(id);
      return;
    }
    // Generic view removal. A dialog that is leaving keeps the view: the
    // element is already gone, and the exit motion still has to show it.
    View v = views.get(id);
    if (v != null && keptForModalExit(v)) return;
    if (v != null && v.getParent() instanceof ViewGroup) ((ViewGroup) v.getParent()).removeView(v);
  }

  void retainClosing(ModalHost modal) {
    if (!closingModals.contains(modal)) closingModals.add(modal);
  }

  void releaseClosing(ModalHost modal) {
    closingModals.remove(modal);
  }

  /**
   * True when `v` belongs to a modal that is playing its exit, or that this batch already unhosted
   * and will dismiss once the batch ends.
   */
  private boolean keptForModalExit(View v) {
    for (int i = closingModals.size() - 1; i >= 0; i--) {
      if (closingModals.get(i).holds(v)) return true;
    }
    for (int id : dirtyModals) {
      if (hosts.indexOfKey(id) >= 0) continue;
      ModalHost modal = modals.get(id);
      if (modal != null && modal.holds(v)) return true;
    }
    return false;
  }

  private void delete(int id) {
    if (id == 1) return;
    ModalHost modal = modals.get(id);
    if (modal != null) {
      modal.dismiss();
      modals.remove(id);
      modalOptions.remove(id);
    }
    remove(id);
    SelectableView area = selectableViews.get(id);
    if (area != null) {
      area.clear();
      selectableViews.remove(id);
    }
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
    screenOptions.remove(id);
    headerSizes.remove(id);
    headerItems.remove(id);
    stacks.remove((Integer) id);
    trackedStates.delete(id);
    currentStates.delete(id);
    observedPress.remove(Integer.valueOf(id));
  }

  private void frame(int id, float x, float y, float w, float h) {
    View v = views.get(id);
    if (v == null || id == 1) return;
    v.setTag(new float[] {x, y, w, h});
    int px = Math.round(x * density),
        py = Math.round(y * density),
        pw = Math.round(w * density),
        ph = Math.round(h * density);
    // Header items: store size but don't set margins/layout
    if (headerItems.contains(id)) {
      headerSizes.put(id, new int[] {pw, ph});
      return;
    }
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

    /** Box shadows (px): dx, dy, blur, spread, inset (1) per layer. */
    float[] shadows = new float[0];

    int[] shadowColors = new int[0];
    Decor.Gradient[] gradients = new Decor.Gradient[0];
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
    readEffects(b, s);
    return s;
  }

  /**
   * Effects after the transform ops. Word layout is {@code encode.rs} {@code view_style}. Lengths
   * are points; pixels are {@code points * density}.
   */
  private void readEffects(ByteBuffer b, Style s) {
    int shadows = b.getInt();
    s.shadows = new float[shadows * 5];
    s.shadowColors = new int[shadows];
    for (int i = 0; i < shadows; i++) {
      for (int k = 0; k < 4; k++) s.shadows[i * 5 + k] = b.getFloat() * density;
      s.shadowColors[i] = b.getInt();
      s.shadows[i * 5 + 4] = b.getInt() != 0 ? 1 : 0;
    }
    int gradients = b.getInt();
    s.gradients = new Decor.Gradient[gradients];
    for (int i = 0; i < gradients; i++) {
      Decor.Gradient g = new Decor.Gradient();
      g.kind = b.getInt();
      g.repeating = b.getInt() != 0;
      g.circle = b.getInt() != 0;
      g.size = b.getInt();
      float start = b.getFloat();
      float angle = b.getFloat();
      g.x = b.getFloat();
      g.y = b.getFloat();
      if (g.kind == 1) {
        g.right = start != 0;
        g.bottom = angle != 0;
      } else if (g.kind == 3) {
        g.angle = start * 360f;
      } else {
        g.angle = angle * 360f;
      }
      int stops = b.getInt();
      g.colors = new int[stops];
      g.positions = new float[stops];
      g.units = new int[stops];
      g.values = new float[stops];
      for (int k = 0; k < stops; k++) {
        g.colors[k] = b.getInt();
        g.units[k] = b.getInt();
        float v = b.getFloat();
        g.values[k] = g.units[k] == 2 ? v * density : v;
      }
      // A transparent stop takes its neighbor's hue: fades do not darken.
      for (int k = 0; k < stops; k++) {
        if ((g.colors[k] >>> 24) != 0) continue;
        for (int m = 1; m < stops; m++) {
          int j = k + m < stops ? k + m : k - m;
          if (j >= 0 && j < stops && (g.colors[j] >>> 24) != 0) {
            g.colors[k] = g.colors[j] & 0x00ffffff;
            break;
          }
        }
      }
      s.gradients[i] = g;
    }
  }

  private Decor decor(View v) {
    if (v instanceof StackBox) return ((StackBox) v).decor;
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
      d.boxShadows = s.shadows;
      d.boxShadowColors = s.shadowColors;
      d.gradients = s.gradients;
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
    if (v instanceof Box) {
      Box box = (Box) v;
      box.clip = s.overflow;
      box.updateClip();
    }
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
    int selectable = b.getInt();
    Integer selectionColor = b.getInt() != 0 ? b.getInt() : null;
    int hrefCount = b.getInt();
    int[] hrefs = new int[Math.max(0, hrefCount) * 3];
    for (int i = 0; i < hrefCount; i++) {
      hrefs[i * 3] = b.getInt();
      hrefs[i * 3 + 1] = b.getInt();
      hrefs[i * 3 + 2] = b.getInt();
    }
    View v = views.get(id);
    if (v instanceof TextBox && li >= 0 && li < o.length && o[li] instanceof StaticLayout) {
      TextBox text = (TextBox) v;
      text.setText((StaticLayout) o[li], x, y, w, h);
      text.setLinks(links, presses);
      text.setIds(idStart, idEnd, ids);
      text.setRuns(runStart, runEnd, runWeight, runItalic, runColor, runSize);
      text.setSelectable(selectable, selectionColor);
      text.setHrefs(hrefs);
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
    float refreshIndicator = b.getFloat();
    boolean paging = b.getInt() != 0;
    float snapInterval = b.getFloat();
    int offsetsLen = b.getInt();
    float[] snapOffsets = null;
    if (offsetsLen >= 0) {
      snapOffsets = new float[offsetsLen];
      for (int i = 0; i < offsetsLen; i++) {
        snapOffsets[i] = b.getFloat();
      }
    }
    int snapAlignment = b.getInt();
    boolean disableIntervalMomentum = b.getInt() != 0;
    float deceleration = b.getFloat();

    if (v instanceof ScrollBox) {
      ScrollBox s = (ScrollBox) v;
      s.contentPointsW = w;
      s.contentPointsH = h;
      s.size(Math.round(w * density), Math.round(h * density));

      // Handle refresh indicator
      if (refreshIndicator >= 0) {
        s.setRefresh(true, refreshIndicator);
      } else {
        s.setRefresh(false, 0);
      }

      // Set snap behavior
      s.setSnapBehavior(
          paging, snapInterval, snapOffsets, snapAlignment, disableIntervalMomentum, deceleration);
    }
  }

  private void scrollBy(ByteBuffer b) {
    View v = views.get(b.getInt());
    float x = b.getFloat(), y = b.getFloat();
    if (v instanceof ScrollBox) ((ScrollBox) v).scrollByPoints(x, y, density);
  }

  private void listContent(ByteBuffer b) {
    int id = b.getInt();
    int stickyCount = b.getInt();
    int[] stickyIndices = new int[stickyCount];
    for (int i = 0; i < stickyCount; i++) {
      stickyIndices[i] = b.getInt();
    }
    boolean inverted = b.getInt() != 0;
    boolean viewportParent = b.getInt() != 0;

    // Viewability config
    boolean hasViewability = b.getInt() != 0;
    if (hasViewability) {
      b.getFloat(); // view_area_coverage_threshold
      b.getFloat(); // item_visible_threshold
    }

    // MaintainPosition config
    boolean hasMaintain = b.getInt() != 0;
    if (hasMaintain) {
      b.getInt(); // min_index_for_visible
      b.getFloat(); // autoscroll_to_bottom_threshold
    }

    // Core handles sticky/inverted via frame calculations
    // Platform just applies the frames
  }

  private void refreshing(ByteBuffer b) {
    int id = b.getInt();
    boolean refreshing = b.getInt() != 0;
    View v = views.get(id);
    if (v instanceof ScrollBox) {
      ((ScrollBox) v).setRefreshing(refreshing);
    }
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
    HostView host = hostOf(v);
    if (host == null) return;
    host.mounter.paint(id);
    // A disabled view is not pressed. Clearing a press still reports.
    if (down && viewDisabled(v)) return;
    host.mounter.reportState(id, STATE_PRESSED, down);
  }

  /**
   * The surface that owns {@code view}. A dialog window is not a child of the activity content, so
   * the walk also accepts a {@link Box} or {@link StackBox} (they keep the mounter).
   */
  static HostView hostOf(View view) {
    for (View v = view; v != null; ) {
      if (v instanceof HostView) return (HostView) v;
      if (v instanceof Box) return ((Box) v).mounter.host;
      if (v instanceof StackBox) return ((StackBox) v).mounter.host;
      android.view.ViewParent parent = v.getParent();
      v = parent instanceof View ? (View) parent : null;
    }
    return null;
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
    View node = origin;
    int focusedId = nodeId(origin);
    if (focusedId < 0 && origin.getParent() instanceof InputBox) {
      node = (InputBox) origin.getParent();
      focusedId = ((InputBox) node).id;
    }
    if (focusedId >= 0) {
      boolean on =
          node.isFocused() || (node instanceof InputBox && ((InputBox) node).edit.isFocused());
      reportState(focusedId, STATE_FOCUSED, on);
    }
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

  private int nodeId(View v) {
    int id = idOf(v);
    if (id >= 0) return id;
    if (v instanceof StackBox) return ((StackBox) v).id;
    return -1;
  }

  private static boolean viewDisabled(View v) {
    if (v instanceof Box) return ((Box) v).disabled;
    if (v instanceof TextBox) return ((TextBox) v).disabled;
    if (v instanceof ImageBox) return ((ImageBox) v).disabled;
    if (v instanceof ScrollBox) return ((ScrollBox) v).disabled;
    if (v instanceof InputBox) return ((InputBox) v).disabled;
    return false;
  }

  /** The view's own touch handler calls {@link #pressed}. The observer must not also. */
  private static boolean ownsPress(View v) {
    if (v instanceof Box) return (((Box) v).pressMask & 1) != 0;
    if (v instanceof ImageBox) return (((ImageBox) v).pressMask & 1) != 0;
    if (v instanceof TextBox) return (((TextBox) v).pressMask & 1) != 0;
    if (v instanceof ScrollBox) return (((ScrollBox) v).pressMask & 1) != 0;
    if (v instanceof InputBox) return (((InputBox) v).pressMask & 1) != 0;
    return false;
  }

  /** `pointerEvents: none`: this view is not the hit target. Children still are. */
  private static boolean ignoresPointer(View v) {
    return pointerValue(v) == 1;
  }

  /** `0` auto, `1` none. Views that are not hit targets still default to auto. */
  private static int pointerValue(View v) {
    if (v instanceof Box) return ((Box) v).pointerEvents;
    if (v instanceof TextBox) return ((TextBox) v).pointerEvents;
    if (v instanceof ImageBox) return ((ImageBox) v).pointerEvents;
    if (v instanceof ScrollBox) return ((ScrollBox) v).pointerEvents;
    if (v instanceof InputBox) return ((InputBox) v).pointerEvents;
    return 0;
  }

  /** Stores computed `pointer-events`: `auto` is 0, `none` is 1. */
  private void pointerEvents(int id, boolean auto) {
    View v = views.get(id);
    if (v == null) return;
    int value = auto ? 0 : 1;
    if (v instanceof Box) ((Box) v).pointerEvents = value;
    else if (v instanceof TextBox) ((TextBox) v).pointerEvents = value;
    else if (v instanceof ImageBox) ((ImageBox) v).pointerEvents = value;
    else if (v instanceof ScrollBox) ((ScrollBox) v).pointerEvents = value;
  }

  private void trackStates(int id, int states) {
    trackedStates.put(id, states);
    if ((states & STATE_PRESSED) == 0 && observedPress.remove(Integer.valueOf(id))) {
      View v = views.get(id);
      if (v != null) pressed(v, id, false);
    }
  }

  /**
   * An interaction state of {@code id} changed. Press and hover are sent only when the last {@code
   * TrackStates} includes the bit. Focus is always sent, so the core can keep {@code focus-within}.
   */
  private void reportState(int id, int bit, boolean on) {
    int states = currentStates.get(id);
    int next = on ? states | bit : states & ~bit;
    if (next == states) return;
    currentStates.put(id, next);
    int reported = trackedStates.get(id) | STATE_FOCUSED;
    if ((reported & bit) == 0) return;
    long handle = host.handle();
    if (handle != 0) Native.states(handle, id, next & reported);
  }

  /**
   * A finger on {@code root}. Views that track press but do not handle {@code onPress} report it
   * here, without taking the touch (a scroll around them still scrolls).
   */
  void observePress(View root, android.view.MotionEvent e) {
    int action = e.getActionMasked();
    if (action == android.view.MotionEvent.ACTION_DOWN) {
      endObservedPress();
      float rawX = e.getRawX(), rawY = e.getRawY();
      for (View v = hitNode(root, rawX, rawY); v != null; ) {
        int id = nodeId(v);
        if (id >= 0) {
          if (ownsPress(v)) break;
          if (!ignoresPointer(v)
              && !viewDisabled(v)
              && !blocked(v.getParent())
              && (trackedStates.get(id) & STATE_PRESSED) != 0) {
            pressed(v, id, true);
            observedPress.add(id);
          }
        }
        if (v == root) break;
        android.view.ViewParent parent = v.getParent();
        v = parent instanceof View ? (View) parent : null;
      }
    } else if (action == android.view.MotionEvent.ACTION_UP
        || action == android.view.MotionEvent.ACTION_CANCEL) {
      endObservedPress();
    }
  }

  private void endObservedPress() {
    for (int i = 0; i < observedPress.size(); i++) {
      int id = observedPress.get(i);
      View v = views.get(id);
      if (v != null) pressed(v, id, false);
    }
    observedPress.clear();
  }

  /**
   * The frontmost `auto` node at the screen point, or null. A `none` node is not a target; its
   * descendants still are, including ones drawn outside it.
   */
  static View hitNode(View root, float rawX, float rawY) {
    if (root == null) return null;
    return hitNode(root, rawX, rawY, new int[2]);
  }

  private static View hitNode(View v, float rawX, float rawY, int[] at) {
    if (v.getVisibility() != View.VISIBLE) return null;
    boolean inside = contains(v, rawX, rawY, at);
    // A clipped node does not hit outside its border. Overflow visible still does.
    if ((!clipsPointer(v) || inside) && v instanceof ViewGroup) {
      ViewGroup group = (ViewGroup) v;
      for (int i = group.getChildCount() - 1; i >= 0; i--) {
        View hit = hitNode(group.getChildAt(i), rawX, rawY, at);
        if (hit != null) return hit;
      }
    }
    if (!inside || !pointerNode(v) || ignoresPointer(v)) return null;
    return v;
  }

  private static boolean clipsPointer(View v) {
    if (v instanceof Box) return ((Box) v).clip;
    if (v instanceof ScrollBox) return true;
    if (v instanceof InputBox) return ((InputBox) v).clip;
    return false;
  }

  /** Nodes that store `pointer-events`. Chrome around them is not a target. */
  private static boolean pointerNode(View v) {
    return v instanceof Box
        || v instanceof TextBox
        || v instanceof ImageBox
        || v instanceof ScrollBox
        || v instanceof InputBox;
  }

  private static boolean contains(View v, float rawX, float rawY, int[] at) {
    v.getLocationOnScreen(at);
    float x = rawX - at[0], y = rawY - at[1];
    return x >= 0 && y >= 0 && x < v.getWidth() && y < v.getHeight();
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
    if (over.shadowColors.length > 0) {
      s.shadows = over.shadows;
      s.shadowColors = over.shadowColors;
    }
    if (over.gradients.length > 0) s.gradients = over.gradients;
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
    s.shadows = a.shadows.clone();
    s.shadowColors = a.shadowColors.clone();
    s.gradients = a.gradients.clone();
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

  private void command(ByteBuffer b, Object[] objects) {
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
    if (cmd == 3) {
      int index = b.getInt();
      if (index >= 0 && index < objects.length && objects[index] instanceof String) {
        openUrl((String) objects[index]);
      }
      return;
    }
    float x = b.getFloat(), y = b.getFloat();
    b.getInt();
    View v = views.get(id);
    if (cmd == 0 && v instanceof ScrollBox) ((ScrollBox) v).scrollToPoints(x, y, density);
  }

  private void screenConfig(ByteBuffer b, Object[] objects) {
    int id = b.getInt();
    ScreenOptions opts = new ScreenOptions();
    int titleIndex = b.getInt();
    opts.title = titleIndex < 0 ? null : (String) objects[titleIndex];
    opts.shown = b.getInt() != 0;
    opts.transparent = b.getInt() != 0;
    opts.tintPresent = b.getInt() != 0;
    if (opts.tintPresent) opts.tint = b.getInt();
    opts.backgroundPresent = b.getInt() != 0;
    if (opts.backgroundPresent) opts.background = b.getInt();
    opts.titleColorPresent = b.getInt() != 0;
    if (opts.titleColorPresent) opts.titleColor = b.getInt();
    opts.backVisible = b.getInt() != 0;
    opts.gesture = b.getInt() != 0;
    opts.preventBack = b.getInt() != 0;
    opts.animation = b.getInt();
    opts.left = b.getInt();
    opts.right = b.getInt();
    opts.titleView = b.getInt();
    opts.headerHeight = b.getFloat();
    screenOptions.put(id, opts);
    dirtyScreens.add(id);
    int parentId = hosts.get(id, -1);
    if (parentId >= 0) {
      dirtyStacks.add(parentId);
    }
  }

  private void modalConfig(ByteBuffer b) {
    int id = b.getInt();
    ModalOptions opts = new ModalOptions();
    opts.animation = b.getInt();
    opts.presentation = b.getInt();
    opts.closeOnEscape = b.getInt() != 0;
    opts.closeOnBackdropPress = b.getInt() != 0;
    opts.closeOnSwipe = b.getInt() != 0;
    modalOptions.put(id, opts);
    ModalHost modal = modals.get(id);
    if (modal != null) {
      modal.configure(
          opts.animation,
          opts.presentation,
          opts.closeOnEscape,
          opts.closeOnBackdropPress,
          opts.closeOnSwipe);
    }
  }

  private void finishNavigation() {
    // Mark screens of dirty stacks as dirty
    for (int stackId : dirtyStacks) {
      View v = views.get(stackId);
      if (v instanceof StackBox) {
        StackBox stack = (StackBox) v;
        for (int screenId : stack.screens) {
          dirtyScreens.add(screenId);
        }
      }
    }
    // Configure dirty screens BEFORE reconciling (so animation fields are fresh)
    for (int screenId : dirtyScreens) {
      View v = views.get(screenId);
      if (!(v instanceof StackBox.ScreenBox)) continue;
      StackBox.ScreenBox screen = (StackBox.ScreenBox) v;
      ScreenOptions opts = screenOptions.get(screenId);
      if (opts == null) continue;
      // Find the screen's index in its stack to determine canGoBack
      int parentId = hosts.get(screenId, -1);
      boolean canGoBack = false;
      if (parentId >= 0) {
        View parent = views.get(parentId);
        if (parent instanceof StackBox) {
          StackBox stack = (StackBox) parent;
          int index = stack.screens.indexOf(screenId);
          canGoBack = index > 0;
        }
      }
      screen.configure(
          this,
          opts.title,
          opts.shown,
          opts.transparent,
          opts.tintPresent ? opts.tint : 0,
          opts.background,
          opts.backgroundPresent,
          opts.titleColorPresent ? opts.titleColor : 0,
          opts.backVisible,
          opts.gesture,
          opts.preventBack,
          opts.animation,
          opts.left,
          opts.right,
          opts.titleView,
          opts.headerHeight,
          canGoBack);
    }
    dirtyScreens.clear();
    // Reconcile dirty stacks AFTER screen configs are applied
    for (int stackId : dirtyStacks) {
      View v = views.get(stackId);
      if (v instanceof StackBox) {
        StackBox stack = (StackBox) v;
        stack.reconcile();
      }
    }
    dirtyStacks.clear();
    // Handle dirty modals
    for (int modalId : dirtyModals) {
      ModalHost modal = modals.get(modalId);
      if (modal == null) continue;
      ModalOptions opts = modalOptions.get(modalId);
      if (opts != null) {
        modal.configure(
            opts.animation,
            opts.presentation,
            opts.closeOnEscape,
            opts.closeOnBackdropPress,
            opts.closeOnSwipe);
      }
      if (hosts.indexOfKey(modalId) >= 0) {
        modal.show();
      } else {
        modal.dismiss();
      }
    }
    dirtyModals.clear();
  }

  StackBox backStack() {
    // Return null when any modal is shown (no predictive back under dialogs)
    for (int i = 0; i < modals.size(); i++) {
      ModalHost modal = modals.valueAt(i);
      if (modal.shown()) return null;
    }
    for (int i = stacks.size() - 1; i >= 0; i--) {
      View v = views.get(stacks.get(i));
      if (v instanceof StackBox) {
        StackBox stack = (StackBox) v;
        if (stack.canSwipe()) {
          return stack;
        }
      }
    }
    return null;
  }
}
