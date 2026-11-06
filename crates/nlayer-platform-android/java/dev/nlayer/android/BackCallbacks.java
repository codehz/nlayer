package dev.nlayer.android;

import android.app.Activity;
import android.os.Build;
import android.window.BackEvent;
import android.window.OnBackAnimationCallback;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;

/**
 * The activity's back callback for Android 13+ (only loaded there, so {@code HostActivity} verifies
 * without these types on older versions). On Android 14+ the top screen follows the back gesture
 * (predictive back).
 */
final class BackCallbacks {
  private BackCallbacks() {}

  static Object register(Activity activity, HostView view) {
    Object callback =
        Build.VERSION.SDK_INT >= 34
            ? new Animated(view)
            : (OnBackInvokedCallback)
                () -> {
                  if (view != null) view.back();
                };
    activity
        .getOnBackInvokedDispatcher()
        .registerOnBackInvokedCallback(
            OnBackInvokedDispatcher.PRIORITY_DEFAULT, (OnBackInvokedCallback) callback);
    return callback;
  }

  static void unregister(Activity activity, Object callback) {
    activity
        .getOnBackInvokedDispatcher()
        .unregisterOnBackInvokedCallback((OnBackInvokedCallback) callback);
  }

  /** Android 14+: the screen being popped follows the finger. */
  private static final class Animated implements OnBackAnimationCallback {
    private final HostView view;
    private StackBox stack;

    Animated(HostView view) {
      this.view = view;
    }

    @Override
    public void onBackStarted(BackEvent e) {
      stack = view == null ? null : view.mounter.backStack();
      if (stack != null) stack.backStarted();
    }

    @Override
    public void onBackProgressed(BackEvent e) {
      if (stack != null) stack.backProgressed(e.getProgress());
    }

    @Override
    public void onBackCancelled() {
      if (stack != null) {
        stack.backCancelled();
        stack = null;
      }
    }

    @Override
    public void onBackInvoked() {
      stack = null;
      if (view != null) view.back();
    }
  }
}
