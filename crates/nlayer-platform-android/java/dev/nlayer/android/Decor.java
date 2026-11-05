package dev.nlayer.android;

import android.graphics.Canvas;
import android.graphics.DashPathEffect;
import android.graphics.LinearGradient;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.PixelFormat;
import android.graphics.RadialGradient;
import android.graphics.Rect;
import android.graphics.RectF;
import android.graphics.Shader;
import android.graphics.SweepGradient;
import android.graphics.drawable.Drawable;
import android.os.Build;

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

  /** The view's painted shape. Image drawing clips to this, fitted to `b`. */
  Path clip(Rect b, Path out) {
    return shape(b, 0, out);
  }

  /** Fitted corner radii, in the same order as `radii` (px). */
  float[] fitted(Rect b) {
    float[] src = fit(b);
    return new float[] {src[0], src[1], src[2], src[3]};
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
    drawOuterShadows(c, b);
    if ((fill >>> 24) != 0) {
      paint.setStyle(Paint.Style.FILL);
      paint.setColor(fill);
      c.drawPath(shape(b, 0, path), paint);
    }
    drawGradients(c, b);
    drawInsetShadows(c, b);
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
    // A fraction is of the current size. The default center stays unset so
    // Android keeps tracking it, unless a previous origin left a pixel pivot.
    if (originXFraction && originYFraction && originX == .5f && originY == .5f) {
      float cx = v.getWidth() / 2f, cy = v.getHeight() / 2f;
      if (v.getPivotX() != cx || v.getPivotY() != cy) {
        v.setPivotX(cx);
        v.setPivotY(cy);
      }
      return;
    }
    float x = originXFraction ? originX * v.getWidth() : originX;
    float y = originYFraction ? originY * v.getHeight() : originY;
    if (v.getPivotX() != x) v.setPivotX(x);
    if (v.getPivotY() != y) v.setPivotY(y);
  }

  /** True if an outer box shadow draws outside the box. */
  boolean hasOuterShadows() {
    for (int n = 0; n < boxShadowColors.length; n++) {
      if (boxShadows[n * 5 + 4] == 0 && (boxShadowColors[n] >>> 24) != 0) return true;
    }
    return false;
  }

  // Box shadows (px): dx, dy, blur, spread, inset (1) per layer, top layer
  // first (CSS order), and their colors.
  float[] boxShadows = new float[0];
  int[] boxShadowColors = new int[0];
  // Gradients, top layer first.
  Gradient[] gradients = new Gradient[0];

  /** A gradient background (`encode.rs` effects). */
  static final class Gradient {
    /** 0 linear (angle), 1 to a corner, 2 radial, 3 conic. */
    int kind;

    /** Degrees. 0 is up, clockwise (CSS). Kind 1 recomputes this from the box. */
    float angle;

    boolean right, bottom, circle;

    /** Radial: 0 closest side, 1 closest corner, 2 farthest side, 3 farthest corner. */
    int size;

    float x = 0.5f, y = 0.5f;
    int[] colors = new int[0];

    /** Fractions of the gradient line, placed by {@link #resolve} when {@link #units} is set. */
    float[] positions = new float[0];

    /** Stops as written: 0 spread, 1 fraction, 2 pixels. Null once already resolved. */
    int[] units;

    float[] values;

    /** Repeats past the last stop (`repeating-*-gradient`). */
    boolean repeating;

    /** The stops {@link #place} made for the shader: colors, and positions within 0..1. */
    int[] drawnColors;

    float[] drawnPositions;

    /** What {@link #drawnColors} and {@link #drawnPositions} were placed for. */
    private float placedLine = Float.NaN, placedReach = Float.NaN;

    /** Most stops {@link #place} repeats a gradient's into, as the core's cap. */
    private static final int MAX_UNROLLED_STOPS = 16384;

    /**
     * Places the stops along a gradient line {@code line} px long ({@link #resolve}) for a shader
     * that repeats nothing and draws them over {@code 0..reach} of the line (1: the whole line of a
     * linear or conic gradient; a radial gradient's rings go on past its ending shape, to the box's
     * farthest corner): positions as fractions of {@code reach}, a repeating gradient's stops
     * repeated over all of it with period last − first, both ways (CSS Images 3), and cut at both
     * ends with the colors there.
     */
    void place(float line, float reach) {
      if (drawnColors != null && line == placedLine && reach == placedReach) return;
      placedLine = line;
      placedReach = reach;
      resolve(line);
      reach = Math.max(reach, 1e-6f);
      int n = positions.length;
      float period = n > 0 ? positions[n - 1] - positions[0] : 0;
      double start = 0, end = 0;
      if (period > 1e-4f) {
        start = Math.floor(-positions[n - 1] / period);
        end = Math.ceil((reach - positions[0]) / period);
      }
      drawnColors = colors;
      if (!repeating || period <= 1e-4f || (end - start + 1) * n > MAX_UNROLLED_STOPS) {
        drawnPositions = positions;
        if (reach != 1) {
          drawnPositions = new float[n];
          for (int k = 0; k < n; k++) drawnPositions[k] = positions[k] / reach;
        }
        return;
      }
      int copies = (int) (end - start + 1), m = copies * n;
      int[] tiledColors = new int[m];
      float[] tiled = new float[m];
      float max = Float.NEGATIVE_INFINITY;
      for (int c = 0; c < copies; c++) {
        float shift = (float) (start + c) * period;
        for (int k = 0; k < n; k++) {
          // A copy's first stop may round to a hair before the last one's.
          max = Math.max(max, positions[k] + shift);
          tiled[c * n + k] = max;
          tiledColors[c * n + k] = colors[k];
        }
      }
      int inside = 0;
      for (float p : tiled) if (p > 0 && p < reach) inside++;
      drawnColors = new int[inside + 2];
      drawnPositions = new float[inside + 2];
      drawnColors[0] = colorAt(tiledColors, tiled, 0);
      int j = 1;
      for (int i = 0; i < m; i++) {
        if (tiled[i] <= 0 || tiled[i] >= reach) continue;
        drawnColors[j] = tiledColors[i];
        drawnPositions[j++] = tiled[i] / reach;
      }
      drawnColors[j] = colorAt(tiledColors, tiled, reach);
      drawnPositions[j] = 1;
    }

    /** The color at {@code t} of ascending stops (at a hard edge, the later stop). */
    private static int colorAt(int[] colors, float[] positions, float t) {
      int i = positions.length - 1;
      while (i >= 0 && positions[i] > t) i--;
      if (i < 0) return colors[0];
      if (i + 1 < positions.length && positions[i + 1] > positions[i]) {
        return mix(
            colors[i], colors[i + 1], (t - positions[i]) / (positions[i + 1] - positions[i]));
      }
      return colors[i];
    }

    /** {@code a} mixed with {@code b} by {@code t} (0: a), premultiplied, as CSS mixes. */
    private static int mix(int a, int b, float t) {
      float alphaA = a >>> 24, alphaB = b >>> 24;
      float alpha = alphaA + (alphaB - alphaA) * t;
      int out = Math.round(Math.max(0, Math.min(255, alpha))) << 24;
      if (alpha <= 0) return out;
      for (int shift = 0; shift <= 16; shift += 8) {
        float x = ((a >>> shift) & 0xff) * alphaA, y = ((b >>> shift) & 0xff) * alphaB;
        out |= Math.round(Math.max(0, Math.min(255, (x + (y - x) * t) / alpha))) << shift;
      }
      return out;
    }

    /**
     * Places the stops along a gradient line {@code line} px long, as `Gradient::resolve_stops`:
     * lengths as fractions of the line, missing positions spread evenly, each at least the one
     * before it.
     */
    void resolve(float line) {
      if (units == null) return;
      int n = positions.length;
      if (n == 0) return;
      for (int k = 0; k < n; k++) {
        switch (units[k]) {
          case 0:
            positions[k] = Float.NaN;
            break;
          case 1:
            positions[k] = values[k];
            break;
          default:
            positions[k] = line > 0 ? values[k] / line : values[k] > 0 ? 1 : 0;
        }
      }
      if (Float.isNaN(positions[0])) positions[0] = 0;
      if (Float.isNaN(positions[n - 1])) positions[n - 1] = 1;
      // From 0, as the core's Gradient::resolve_stops: negative stops resolve to 0.
      float max = 0;
      for (int k = 0; k < n; k++) {
        if (Float.isNaN(positions[k])) continue;
        max = Math.max(max, positions[k]);
        positions[k] = max;
      }
      int i = 1;
      while (i < n) {
        if (!Float.isNaN(positions[i])) {
          i++;
          continue;
        }
        int start = i - 1, end = i;
        while (Float.isNaN(positions[end])) end++;
        float a = positions[start], b = positions[end];
        for (int k = i; k < end; k++) positions[k] = a + (b - a) * (k - start) / (end - start);
        i = end;
      }
    }
  }

  /** Distance shadow casters are moved away to draw only their shadows. */
  private static final float AWAY = 1 << 14;

  private final Path shadowPath = new Path();

  /** CSS blur (twice the deviation) as a Skia shadow radius. */
  private static float shadowRadius(float blur) {
    // Skia: sigma = radius / sqrt(3) + 0.5.
    return Math.max(0.01f, (blur / 2 - 0.5f) * 1.7320508f);
  }

  /** Outer box shadows: only outside the box, as CSS draws them. */
  private void drawOuterShadows(Canvas canvas, Rect b) {
    if (!hasOuterShadows()) return;
    canvas.save();
    if (Build.VERSION.SDK_INT >= 26) canvas.clipOutPath(shape(b, 0, path));
    paint.setStyle(Paint.Style.FILL);
    // Bottom layer first.
    for (int n = boxShadowColors.length - 1; n >= 0; n--) {
      int i = n * 5;
      if (boxShadows[i + 4] != 0) continue;
      int color = boxShadowColors[n];
      if ((color >>> 24) == 0) continue;
      float spread = boxShadows[i + 3];
      // The caster sits far away; only its shadow lands here.
      paint.setColor(0xff000000);
      paint.setShadowLayer(
          shadowRadius(boxShadows[i + 2]), boxShadows[i] + AWAY, boxShadows[i + 1], color);
      shadowPath.reset();
      grown(b, spread, shadowPath);
      shadowPath.offset(-AWAY, 0);
      canvas.drawPath(shadowPath, paint);
    }
    paint.clearShadowLayer();
    canvas.restore();
  }

  /** Inset box shadows: inside the padding box, above the background. */
  private void drawInsetShadows(Canvas canvas, Rect b) {
    boolean any = false;
    for (int n = 0; n < boxShadowColors.length; n++)
      any |= boxShadows[n * 5 + 4] != 0 && (boxShadowColors[n] >>> 24) != 0;
    if (!any) return;
    float t = widths[0], r = widths[1], bo = widths[2], l = widths[3];
    Rect pad =
        new Rect(
            Math.round(b.left + l),
            Math.round(b.top + t),
            Math.round(b.right - r),
            Math.round(b.bottom - bo));
    canvas.save();
    canvas.clipPath(shape(pad, 0, path));
    paint.setStyle(Paint.Style.FILL);
    for (int n = boxShadowColors.length - 1; n >= 0; n--) {
      int i = n * 5;
      if (boxShadows[i + 4] == 0) continue;
      int color = boxShadowColors[n];
      if ((color >>> 24) == 0) continue;
      float dx = boxShadows[i],
          dy = boxShadows[i + 1],
          blur = boxShadows[i + 2],
          spread = boxShadows[i + 3];
      // A frame around the shifted, shrunk inner shape casts the shadow inwards.
      float reach = Math.abs(dx) + Math.abs(dy) + Math.abs(spread) + blur * 2 + 4;
      shadowPath.reset();
      shadowPath.setFillType(Path.FillType.EVEN_ODD);
      shadowPath.addRect(
          pad.left - reach,
          pad.top - reach,
          pad.right + reach,
          pad.bottom + reach,
          Path.Direction.CW);
      Path hole = new Path();
      grown(pad, -spread, hole);
      hole.offset(dx, dy);
      shadowPath.addPath(hole);
      shadowPath.offset(-AWAY, 0);
      paint.setColor(0xff000000);
      paint.setShadowLayer(shadowRadius(blur), AWAY, 0, color);
      canvas.drawPath(shadowPath, paint);
    }
    paint.clearShadowLayer();
    canvas.restore();
  }

  /** The box's rounded shape grown by `d` px (corners grow with it). */
  private void grown(Rect b, float d, Path out) {
    float[] fitted = fit(b);
    rect.set(b.left - d, b.top - d, b.right + d, b.bottom + d);
    for (int i = 0; i < 4; i++) {
      float radius = fitted[i] > 0 ? Math.max(0, fitted[i] + d) : 0;
      corners[i * 2] = radius;
      corners[i * 2 + 1] = radius;
    }
    out.reset();
    roundedRect(out, rect, corners, continuous);
  }

  private final android.graphics.Matrix gradientMatrix = new android.graphics.Matrix();

  private void drawGradients(Canvas canvas, Rect b) {
    if (gradients.length == 0) return;
    float w = Math.max(1, b.width()), h = Math.max(1, b.height());
    paint.setStyle(Paint.Style.FILL);
    Path box = shape(b, 0, path);
    for (int i = gradients.length - 1; i >= 0; i--) {
      Gradient g = gradients[i];
      if (g.colors == null || g.colors.length < 2) continue;
      Shader shader;
      switch (g.kind) {
        case 2:
          {
            float cx = b.left + g.x * w, cy = b.top + g.y * h;
            float ax = g.x * w, ay = g.y * h;
            float dx, dy;
            switch (g.size) {
              case 0:
                dx = Math.min(ax, w - ax);
                dy = Math.min(ay, h - ay);
                break;
              case 2:
                dx = Math.max(ax, w - ax);
                dy = Math.max(ay, h - ay);
                break;
              case 1:
                dx = Math.min(ax, w - ax) * 1.4142135f;
                dy = Math.min(ay, h - ay) * 1.4142135f;
                break;
              default:
                dx = Math.max(ax, w - ax) * 1.4142135f;
                dy = Math.max(ay, h - ay) * 1.4142135f;
            }
            if (g.circle) {
              float radius =
                  g.size == 0
                      ? Math.min(dx, dy)
                      : g.size == 2
                          ? Math.max(dx, dy)
                          : (float) Math.hypot(dx / 1.4142135f, dy / 1.4142135f);
              dx = dy = radius;
            }
            // The farthest corner, in radii of the ending shape: repeating rings go on to it.
            float reach =
                g.repeating && dx > 0 && dy > 0
                    ? Math.max(
                        1, (float) Math.hypot(Math.max(ax, w - ax) / dx, Math.max(ay, h - ay) / dy))
                    : 1;
            // Lengths are along the ray to the right.
            g.place(dx, reach);
            shader =
                new RadialGradient(
                    cx,
                    cy,
                    Math.max(0.5f, dx) * reach,
                    g.drawnColors,
                    g.drawnPositions,
                    Shader.TileMode.CLAMP);
            // An ellipse: the circle scaled vertically.
            gradientMatrix.setScale(1, dy / Math.max(0.5f, dx), cx, cy);
            shader.setLocalMatrix(gradientMatrix);
            break;
          }
        case 3:
          {
            float cx = b.left + g.x * w, cy = b.top + g.y * h;
            g.place(0, 1);
            shader = new SweepGradient(cx, cy, g.drawnColors, g.drawnPositions);
            // Sweeps start right; CSS starts up.
            gradientMatrix.setRotate(g.angle - 90, cx, cy);
            shader.setLocalMatrix(gradientMatrix);
            break;
          }
        default:
          {
            float angle = g.angle;
            if (g.kind == 1) {
              float a = (float) Math.toDegrees(Math.atan2(h, w));
              angle = g.right ? (g.bottom ? 90 + a : 90 - a) : (g.bottom ? 270 - a : 270 + a);
            }
            double rad = Math.toRadians(angle);
            float sin = (float) Math.sin(rad), cos = (float) Math.cos(rad);
            float len = Math.abs(w * sin) + Math.abs(h * cos);
            float cx = b.left + w / 2, cy = b.top + h / 2;
            float dx = sin * len / 2, dy = -cos * len / 2;
            g.place(len, 1);
            shader =
                new LinearGradient(
                    cx - dx,
                    cy - dy,
                    cx + dx,
                    cy + dy,
                    g.drawnColors,
                    g.drawnPositions,
                    Shader.TileMode.CLAMP);
          }
      }
      paint.setShader(shader);
      paint.setColor(0xff000000);
      canvas.drawPath(box, paint);
      paint.setShader(null);
    }
  }

  @Override
  public void setAlpha(int alpha) {}

  @Override
  public void setColorFilter(android.graphics.ColorFilter f) {}

  @Override
  @SuppressWarnings("deprecation") // Abstract on Drawable; not used for drawing anymore.
  public int getOpacity() {
    return PixelFormat.TRANSLUCENT;
  }
}
