package dev.nlayer.android;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.BitmapFactory;
import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.Rect;
import android.graphics.RectF;
import android.view.View;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;

final class ImageBox extends View {
  final int id;
  final Decor decor = new Decor();
  Bitmap bitmap;
  int mode, pressMask;
  boolean disabled;
  float xValue = .5f, yValue = .5f;
  int xKind = 1, yKind = 1;
  boolean xEnd, yEnd;
  String uri;
  private final Path clip = new Path();

  ImageBox(Context c, int id) {
    super(c);
    this.id = id;
    setBackground(decor);
  }

  void setPosition(boolean xe, int xk, float xv, boolean ye, int yk, float yv) {
    xEnd = xe;
    xKind = xk;
    xValue = xv;
    yEnd = ye;
    yKind = yk;
    yValue = yv;
    invalidate();
  }

  @Override
  public boolean onTouchEvent(android.view.MotionEvent e) {
    if (disabled || Mounter.blocked(getParent())) {
      if (e.getAction() == android.view.MotionEvent.ACTION_DOWN) Mounter.pressed(this, id, false);
      return true;
    }
    if ((pressMask & 1) == 0) return false;
    int action = e.getAction();
    if (action == android.view.MotionEvent.ACTION_DOWN) Mounter.pressed(this, id, true);
    else if (action == android.view.MotionEvent.ACTION_UP) {
      Mounter.pressed(this, id, false);
      Native.sendPress(this, id);
    } else if (action == android.view.MotionEvent.ACTION_CANCEL) Mounter.pressed(this, id, false);
    else return true;
    return true;
  }

  void load(final String source, int mode, final HostView host) {
    this.uri = source;
    this.mode = mode;
    if (source == null) {
      bitmap = null;
      invalidate();
      return;
    }
    new Thread(
            new Runnable() {
              public void run() {
                Bitmap result = null;
                try {
                  if (source.startsWith("asset:/")) {
                    String rest = source.substring(7);
                    if (rest.contains("..")) return;
                    InputStream in = host.openAsset(rest);
                    try {
                      result = BitmapFactory.decodeStream(in);
                    } finally {
                      in.close();
                    }
                  } else if (source.startsWith("http://") || source.startsWith("https://")) {
                    HttpURLConnection c = (HttpURLConnection) new URL(source).openConnection();
                    c.setInstanceFollowRedirects(true);
                    c.setConnectTimeout(15000);
                    c.setReadTimeout(20000);
                    try {
                      InputStream in = c.getInputStream();
                      try {
                        result = BitmapFactory.decodeStream(in);
                      } finally {
                        in.close();
                      }
                    } finally {
                      c.disconnect();
                    }
                  }
                } catch (Exception ignored) {
                }
                final Bitmap decoded = result;
                post(
                    new Runnable() {
                      public void run() {
                        if (source.equals(uri)) {
                          bitmap = decoded;
                          invalidate();
                        }
                      }
                    });
              }
            })
        .start();
  }

  private float place(float free, boolean end, int kind, float value) {
    float offset = kind == 0 ? value : value * free;
    return end ? free - offset : offset;
  }

  @Override
  protected void onDraw(Canvas c) {
    super.onDraw(c);
    if (bitmap == null) return;
    float vw = getWidth(),
        vh = getHeight(),
        bw = bitmap.getWidth(),
        bh = bitmap.getHeight(),
        sx = vw / bw,
        sy = vh / bh;
    if (mode == 0) sx = sy = Math.max(sx, sy);
    else if (mode == 1) sx = sy = Math.min(sx, sy);
    else if (mode == 3) sx = sy = 1;
    else if (mode == 4) sx = sy = Math.min(1, Math.min(sx, sy));
    float w = bw * sx, h = bh * sy, freeX = vw - w, freeY = vh - h;
    RectF dst =
        new RectF(place(freeX, xEnd, xKind, xValue), place(freeY, yEnd, yKind, yValue), 0, 0);
    dst.right = dst.left + w;
    dst.bottom = dst.top + h;
    c.save();
    c.clipPath(decor.clip(new Rect(0, 0, Math.round(vw), Math.round(vh)), clip));
    c.drawBitmap(bitmap, null, dst, new Paint(Paint.FILTER_BITMAP_FLAG | Paint.ANTI_ALIAS_FLAG));
    c.restore();
    decor.setBounds(0, 0, getWidth(), getHeight());
    decor.drawBorders(c);
  }
}
