package dev.nlayer.android;

import android.app.Activity;
import android.os.Build;
import android.os.Bundle;
import android.view.View;
import android.view.Window;
import java.io.File;
import java.nio.file.Files;

public class HostActivity extends Activity {
  private HostView host;

  @Override
  protected void onCreate(Bundle state) {
    super.onCreate(state);
    Window w = getWindow();
    if (Build.VERSION.SDK_INT >= 28) {
      w.getDecorView()
          .setSystemUiVisibility(
              View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                  | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                  | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
      w.setStatusBarColor(android.graphics.Color.TRANSPARENT);
      w.setNavigationBarColor(android.graphics.Color.TRANSPARENT);
    }
    host = new HostView(this);
    setContentView(host);
    host.setOnApplyWindowInsetsListener(
        new View.OnApplyWindowInsetsListener() {
          @Override
          public android.view.WindowInsets onApplyWindowInsets(
              View v, android.view.WindowInsets insets) {
            int top = 0, right = 0, bottom = 0, left = 0, kb = 0;
            if (Build.VERSION.SDK_INT >= 30) {
              android.graphics.Insets
                  s = insets.getInsets(android.view.WindowInsets.Type.systemBars()),
                  ime = insets.getInsets(android.view.WindowInsets.Type.ime());
              top = s.top;
              right = s.right;
              bottom = s.bottom;
              left = s.left;
              kb = Math.max(0, ime.bottom - s.bottom);
            } else {
              top = insets.getSystemWindowInsetTop();
              right = insets.getSystemWindowInsetRight();
              bottom = insets.getSystemWindowInsetBottom();
              left = insets.getSystemWindowInsetLeft();
            }
            host.setInsets(top, right, bottom, left, kb);
            return insets;
          }
        });
    host.post(
        new Runnable() {
          public void run() {
            File root = getFilesDir();
            File f = new File(root, "app.js");
            if (f.isFile())
              try {
                host.load(
                    new String(
                        Files.readAllBytes(f.toPath()), java.nio.charset.StandardCharsets.UTF_8),
                    new File(root, "assets").getAbsolutePath());
              } catch (Exception e) {
                host.reportError(e.toString());
              }
          }
        });
  }

  @Override
  protected void onDestroy() {
    if (host != null) host.destroy();
    super.onDestroy();
  }
}
