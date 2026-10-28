package dev.nlayer.android;

final class Native {
  static {
    System.loadLibrary("nlayer");
  }

  static native long create(HostView view, float density);

  static native void destroy(long handle);

  static native void load(long handle, byte[] code, String assetsDir);

  static native void resize(long handle, float widthDp, float heightDp);

  static native void insets(
      long handle, float top, float right, float bottom, float left, float keyboard);

  static native void serve(long handle, String host, int port);

  static native void press(long handle, int id);

  /** `kind` is an `EventKind` discriminant (`onScroll` is 1). */
  static native void event(long handle, int id, int kind);

  static native void input(long handle, int id, String text);

  private Native() {}

  static void sendPress(android.view.View view, int id) {
    HostView host = host(view);
    if (host != null) press(host.handle(), id);
  }

  static void sendInput(android.view.View view, int id, String text) {
    HostView host = host(view);
    if (host != null) input(host.handle(), id, text);
  }

  static void send(android.view.View view, int id, int kind) {
    HostView host = host(view);
    if (host != null) event(host.handle(), id, kind);
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
