package dev.nlayer.android;

import android.app.Activity;
import android.content.res.Configuration;
import android.graphics.Color;
import android.graphics.Rect;
import android.graphics.drawable.ColorDrawable;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.ViewTreeObserver;
import android.view.Window;
import android.view.WindowInsets;
import java.io.File;
import java.nio.file.Files;

public class HostActivity extends Activity {
  private HostView host;
  private int safeTop, safeRight, safeBottom, safeLeft, keyboard = -1;
  private boolean dark;

  @Override
  protected void onCreate(Bundle state) {
    super.onCreate(state);
    Window w = getWindow();
    dark = isDark(getResources().getConfiguration());
    applyChrome(dark);
    // API 28–29 have no WindowInsets.Type.ime(). adjustResize is the only
    // window mode whose content insets include the keyboard there; the
    // layout flags above keep the surface at the full window, so the app
    // still lays out under the keyboard and only the inset changes.
    if (Build.VERSION.SDK_INT < 30)
      w.setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
    host = new HostView(this);
    host.dark = dark;
    setContentView(host);
    host.setOnApplyWindowInsetsListener(
        new View.OnApplyWindowInsetsListener() {
          @Override
          public WindowInsets onApplyWindowInsets(View v, WindowInsets insets) {
            publish(v, insets);
            return insets;
          }
        });
    // API 28–29 do not redispatch insets when the keyboard opens under
    // adjustNothing. adjustResize moves the visible frame.
    if (Build.VERSION.SDK_INT < 30) {
      host.getViewTreeObserver()
          .addOnGlobalLayoutListener(
              new ViewTreeObserver.OnGlobalLayoutListener() {
                @Override
                public void onGlobalLayout() {
                  publish(host, host.getRootWindowInsets());
                }
              });
    }
    host.post(
        new Runnable() {
          public void run() {
            File root = getFilesDir();
            File f = new File(root, "app.js");
            try {
              if (f.isFile()) {
                host.load(
                    new String(
                        Files.readAllBytes(f.toPath()), java.nio.charset.StandardCharsets.UTF_8),
                    new File(root, "assets").getAbsolutePath());
              } else {
                // A packaged app: the bundle and images are in the APK.
                java.io.InputStream in = getAssets().open("app.js");
                java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();
                try {
                  byte[] buffer = new byte[8192];
                  int n;
                  while ((n = in.read(buffer)) > 0) out.write(buffer, 0, n);
                } finally {
                  in.close();
                }
                host.load(
                    new String(out.toByteArray(), java.nio.charset.StandardCharsets.UTF_8),
                    null,
                    true);
              }
            } catch (Exception e) {
              host.reportError(e.toString());
            }
          }
        });
  }

  static boolean isDark(Configuration config) {
    return (config.uiMode & Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES;
  }

  /** The root background and the status / navigation bar icons follow the scheme. */
  private void applyChrome(boolean dark) {
    Window w = getWindow();
    int background = dark ? Color.BLACK : Color.WHITE;
    w.setBackgroundDrawable(new ColorDrawable(background));
    w.getDecorView().setBackgroundColor(background);
    w.setStatusBarColor(Color.TRANSPARENT);
    w.setNavigationBarColor(Color.TRANSPARENT);
    int flags =
        View.SYSTEM_UI_FLAG_LAYOUT_STABLE
            | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION;
    if (Build.VERSION.SDK_INT >= 23 && !dark) flags |= View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR;
    if (Build.VERSION.SDK_INT >= 26 && !dark) flags |= View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR;
    w.getDecorView().setSystemUiVisibility(flags);
  }

  @Override
  public void onConfigurationChanged(Configuration config) {
    super.onConfigurationChanged(config);
    boolean next = isDark(config);
    if (next != dark) {
      dark = next;
      applyChrome(dark);
      if (host != null) host.setColorScheme(dark);
    }
  }

  @Override
  public void onBackPressed() {
    if (host != null && host.back()) return;
    super.onBackPressed();
  }

  private void publish(View v, WindowInsets insets) {
    int top = safeTop, right = safeRight, bottom = safeBottom, left = safeLeft, kb = 0;
    if (insets != null && Build.VERSION.SDK_INT >= 30) {
      android.graphics.Insets s = insets.getInsets(WindowInsets.Type.systemBars()),
          ime = insets.getInsets(WindowInsets.Type.ime());
      top = s.top;
      right = s.right;
      bottom = s.bottom;
      left = s.left;
      // How far the IME reaches into this edge-to-edge surface. The
      // navigation bar under the keyboard is part of that overlap, as
      // on iOS. Subtracting systemBars leaves that strip under the keys.
      kb = Math.max(0, ime.bottom);
    } else if (insets != null) {
      // Stable insets are the system bars: with adjustResize the system
      // window inset at the bottom also carries the keyboard, which is
      // `env(keyboard-inset-height)`, not the safe area.
      top = insets.getStableInsetTop();
      right = insets.getStableInsetRight();
      bottom = insets.getStableInsetBottom();
      left = insets.getStableInsetLeft();
      kb = keyboardOverlap(v, insets);
    } else if (Build.VERSION.SDK_INT < 30) {
      // No navigation-bar size yet: a closed keyboard can look like one.
      return;
    }
    if (top == safeTop
        && right == safeRight
        && bottom == safeBottom
        && left == safeLeft
        && kb == keyboard) return;
    safeTop = top;
    safeRight = right;
    safeBottom = bottom;
    safeLeft = left;
    keyboard = kb;
    host.setInsets(top, right, bottom, left, kb);
  }

  /**
   * API 28–29: {@code Type.ime()} does not exist. The visible frame stops at the keys. An overlap
   * no larger than the navigation bar is not the keyboard. A larger overlap is the full IME height,
   * including the strip over the navigation bar, matching {@code Type.ime()} on API 30.
   */
  static int keyboardOverlap(View v, WindowInsets insets) {
    View root = v.getRootView();
    int height = root.getHeight();
    if (height <= 0) return 0;
    Rect visible = new Rect();
    root.getWindowVisibleDisplayFrame(visible);
    int nav = insets == null ? 0 : insets.getStableInsetBottom();
    return keyboardHeight(height, visible.bottom, nav);
  }

  static int keyboardHeight(int rootHeight, int visibleBottom, int navBottom) {
    int overlap = rootHeight - visibleBottom;
    if (overlap <= navBottom) return 0;
    return overlap;
  }

  @Override
  protected void onDestroy() {
    if (host != null) host.destroy();
    super.onDestroy();
  }
}
