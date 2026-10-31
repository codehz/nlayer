package dev.nlayer.android;

final class Native {
  static {
    System.loadLibrary("nlayer");
  }

  static native long create(HostView view, float density);

  static native void destroy(long handle);

  static native void load(long handle, byte[] code, String assetsDir, String dataDir, boolean dark);

  static native void resize(long handle, float widthDp, float heightDp);

  /** Whether the animation graph runs, or a binding still needs its first paint. */
  static native boolean animating(long handle);

  /** One animation frame at `nanos`; the encoded styles to apply (may be empty). */
  static native byte[] tick(long handle, long nanos);

  static native void insets(
      long handle, float top, float right, float bottom, float left, float keyboard);

  static native void serve(long handle, String host, int port);

  /** The system's color scheme changed. */
  static native void colorScheme(long handle, boolean dark);

  /** The back button: `BackHandler` listeners, then the platform default. */
  static native void back(long handle);

  /** A display frame began after {@code HostView.requestFrame}. */
  static native void frame(long handle);

  static native void press(long handle, int id);

  /** A press stayed down long enough (`onLongPress`). */
  static native void longPress(long handle, int id);

  /** `kind` is an `EventKind` discriminant (`onScroll` is 1). Lengths are points. */
  static native void scroll(
      long handle,
      int id,
      int kind,
      float x,
      float y,
      float contentW,
      float contentH,
      float width,
      float height);

  static native void input(long handle, int id, String text);

  static native void selection(long handle, int id, int start, int end);

  static native void composition(long handle, int id, boolean composing, String text);

  static native void screenHeader(long handle, int id, float height);

  static native void rootInsets(
      long handle, int id, float top, float right, float bottom, float left);

  private Native() {}

  static void sendPress(android.view.View view, int id) {
    HostView host = host(view);
    if (host != null) {
      if (host.mounter.longPressSuppresses(id)) return;
      press(host.handle(), id);
    }
  }

  static void sendInput(android.view.View view, int id, String text) {
    HostView host = host(view);
    if (host != null) input(host.handle(), id, text);
  }

  static void sendSelection(android.view.View view, int id, int start, int end) {
    HostView host = host(view);
    if (host != null) selection(host.handle(), id, start, end);
  }

  static void sendComposition(android.view.View view, int id, boolean composing, String text) {
    HostView host = host(view);
    if (host != null) composition(host.handle(), id, composing, text);
  }

  static void sendScroll(
      android.view.View view,
      int id,
      int kind,
      float x,
      float y,
      float contentW,
      float contentH,
      float width,
      float height) {
    HostView host = host(view);
    if (host != null) scroll(host.handle(), id, kind, x, y, contentW, contentH, width, height);
  }

  static void dismissKeyboard(android.view.View view) {
    HostView host = host(view);
    if (host != null) host.dismissKeyboard();
  }

  private static HostView host(android.view.View view) {
    for (android.view.View v = view; v != null; ) {
      if (v instanceof HostView) return (HostView) v;
      android.view.ViewParent p = v.getParent();
      v = p instanceof android.view.View ? (android.view.View) p : null;
    }
    return null;
  }
}
