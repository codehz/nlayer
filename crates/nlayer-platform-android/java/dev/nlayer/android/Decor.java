package dev.nlayer.android;

import android.graphics.Canvas;
import android.graphics.DashPathEffect;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.drawable.Drawable;

final class Decor extends Drawable {
  int fill;
  final float[] radii = new float[4], widths = new float[4];
  final int[] colors = new int[4];
  boolean continuous;
  boolean bordersAbove;
  int borderStyle;
  private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
  private final Path path = new Path(), inner = new Path(), side = new Path();
  private final RectF rect = new RectF();
  private final float[] corners = new float[8], fitted = new float[4], innerRadii = new float[8];
  boolean hidden;
  float originX = .5f, originY = .5f;
  boolean originXFraction = true, originYFraction = true;

  private float[] fit(Rect b) {
    float f = 1, w = b.width(), h = b.height();
    f = Math.min(f, ratio(w, radii[0], radii[1]));
    f = Math.min(f, ratio(w, radii[3], radii[2]));
    f = Math.min(f, ratio(h, radii[0], radii[3]));
    f = Math.min(f, ratio(h, radii[1], radii[2]));
    for (int i = 0; i < 4; i++) fitted[i] = radii[i] * f;
    return fitted;
  }

  private static float ratio(float side, float a, float b) {
    return a + b > side ? side / (a + b) : 1;
  }

  private Path shape(Rect b, float inset, Path out) {
    rect.set(b.left + inset, b.top + inset, b.right - inset, b.bottom - inset);
    for (int i = 0; i < 4; i++) {
      float r = Math.max(0, fit(b)[i] - inset);
      corners[i * 2] = corners[i * 2 + 1] = r;
    }
    out.reset();
    roundedRect(out, rect, corners, continuous);
    return out;
  }

  private static final double SMOOTHING = .6;

  static void roundedRect(Path out, RectF r, float[] c, boolean continuous) {
    if (!continuous) {
      out.addRoundRect(r, c, Path.Direction.CW);
      return;
    }
    double x0 = r.left, y0 = r.top, x1 = r.right, y1 = r.bottom, w = x1 - x0, h = y1 - y0;
    double[][] s = {
      {x1, y0, 1, 0, 0, 1, c[2], c[3], share(w, c[2], c[0]), share(h, c[3], c[5])},
      {x1, y1, 0, 1, -1, 0, c[5], c[4], share(h, c[5], c[3]), share(w, c[4], c[6])},
      {x0, y1, -1, 0, 0, -1, c[6], c[7], share(w, c[6], c[4]), share(h, c[7], c[1])},
      {x0, y0, 0, -1, 1, 0, c[0], c[1], share(h, c[0], c[6]), share(w, c[1], c[2])}
    };
    double[] p = new double[2];
    for (int i = 0; i < 4; i++) {
      double[] a = s[i];
      double radius = Math.max(a[6], a[7]);
      if (radius <= 0) {
        if (i == 0) out.moveTo((float) a[0], (float) a[1]);
        else out.lineTo((float) a[0], (float) a[1]);
        continue;
      }
      double su = a[6] / radius,
          sv = a[7] / radius,
          budget = Math.max(radius, Math.min(a[8] / su, a[9] / sv)),
          smooth = Math.max(0, Math.min(SMOOTHING, budget / radius - 1)),
          len = Math.min((1 + smooth) * radius, budget),
          arc = Math.PI / 2 * (1 - smooth),
          arcLen = Math.sin(arc / 2) * radius * Math.sqrt(2),
          alpha = (Math.PI / 2 - arc) / 2,
          p34 = radius * Math.tan(alpha / 2),
          beta = Math.PI / 4 * smooth,
          cc = p34 * Math.cos(beta),
          d = cc * Math.tan(beta),
          b = Math.max(0, (len - arcLen - cc - d) / 3),
          aa = 2 * b;
      map(a, su, sv, -len, 0, p);
      if (i == 0) out.moveTo((float) p[0], (float) p[1]);
      else out.lineTo((float) p[0], (float) p[1]);
      boolean transitions = aa + b + cc > 1e-9 * radius;
      if (transitions) cubic(out, a, su, sv, -len + aa, 0, -len + aa + b, 0, -len + aa + b + cc, d);
      double start = -Math.PI / 4 - arc / 2,
          end = -Math.PI / 4 + arc / 2,
          k = 4.0 / 3 * Math.tan(arc / 4) * radius,
          p0x = -radius + radius * Math.cos(start),
          p0y = radius + radius * Math.sin(start),
          p3x = -radius + radius * Math.cos(end),
          p3y = radius + radius * Math.sin(end);
      cubic(
          out,
          a,
          su,
          sv,
          p0x - k * Math.sin(start),
          p0y + k * Math.cos(start),
          p3x + k * Math.sin(end),
          p3y - k * Math.cos(end),
          p3x,
          p3y);
      if (transitions) cubic(out, a, su, sv, 0, len - aa - b, 0, len - aa, 0, len);
    }
    out.close();
  }

  private static double share(double len, double r, double o) {
    return r + o > 0 ? len * r / (r + o) : 0;
  }

  private static void map(double[] s, double u, double v, double a, double b, double[] p) {
    p[0] = s[0] + s[2] * a * u + s[4] * b * v;
    p[1] = s[1] + s[3] * a * u + s[5] * b * v;
  }

  private static void cubic(
      Path out,
      double[] s,
      double u,
      double v,
      double a,
      double b,
      double c,
      double d,
      double e,
      double f) {
    double[] p = new double[2];
    map(s, u, v, a, b, p);
    float x1 = (float) p[0], y1 = (float) p[1];
    map(s, u, v, c, d, p);
    float x2 = (float) p[0], y2 = (float) p[1];
    map(s, u, v, e, f, p);
    out.cubicTo(x1, y1, x2, y2, (float) p[0], (float) p[1]);
  }

  @Override
  public void draw(Canvas c) {
    Rect b = getBounds();
    if ((fill >>> 24) != 0) {
      paint.setStyle(Paint.Style.FILL);
      paint.setColor(fill);
      c.drawPath(shape(b, 0, path), paint);
    }
    if (!bordersAbove) drawBorders(c);
  }

  boolean hasBorder() {
    for (int i = 0; i < 4; i++) if (widths[i] > 0 && (colors[i] >>> 24) > 0) return true;
    return false;
  }

  void drawBorders(Canvas canvas) {
    if (!hasBorder()) return;
    Rect b = getBounds();
    boolean same = true;
    for (int i = 1; i < 4; i++) same &= widths[i] == widths[0] && colors[i] == colors[0];
    if (same) {
      float w = widths[0];
      paint.setStyle(Paint.Style.STROKE);
      paint.setStrokeWidth(w);
      paint.setColor(colors[0]);
      if (borderStyle == 1) paint.setPathEffect(new DashPathEffect(new float[] {w * 3, w * 3}, 0));
      else if (borderStyle == 2) {
        paint.setPathEffect(new DashPathEffect(new float[] {0, w * 2}, 0));
        paint.setStrokeCap(Paint.Cap.ROUND);
      }
      canvas.drawPath(shape(b, w / 2, path), paint);
      paint.setPathEffect(null);
      paint.setStrokeCap(Paint.Cap.BUTT);
      return;
    }
    shape(b, 0, path);
    float t = widths[0], r = widths[1], bo = widths[2], l = widths[3];
    float[][] sides = {
      {0, 0, b.width(), 0, b.width() - r, t, l, t},
      {b.width(), 0, b.width(), b.height(), b.width() - r, b.height() - bo, b.width() - r, t},
      {b.width(), b.height(), 0, b.height(), l, b.height() - bo, b.width() - r, b.height() - bo},
      {0, b.height(), 0, 0, l, t, l, b.height() - bo}
    };
    for (int i = 0; i < 4; i++) {
      if (widths[i] <= 0 || (colors[i] >>> 24) == 0) continue;
      float[] p = sides[i];
      side.reset();
      side.moveTo(b.left + p[0], b.top + p[1]);
      for (int k = 2; k < 8; k += 2) side.lineTo(b.left + p[k], b.top + p[k + 1]);
      side.close();
      canvas.save();
      canvas.clipPath(side);
      paint.setColor(colors[i]);
      canvas.drawPath(path, paint);
      canvas.restore();
    }
  }

  void applyPivot(android.view.View v) {
    if (originXFraction && originYFraction && originX == .5f && originY == .5f) return;
    v.setPivotX(originXFraction ? originX * v.getWidth() : originX);
    v.setPivotY(originYFraction ? originY * v.getHeight() : originY);
  }

  @Override
  public void setAlpha(int alpha) {}

  @Override
  public void setColorFilter(android.graphics.ColorFilter f) {}

  @Override
  public int getOpacity() {
    return PixelFormat.TRANSLUCENT;
  }
}
