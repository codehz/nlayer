package dev.nlayer.android;

import android.graphics.Paint;
import android.graphics.Typeface;
import android.text.Layout;
import android.text.PrecomputedText;
import android.text.SpannableString;
import android.text.Spanned;
import android.text.StaticLayout;
import android.text.TextDirectionHeuristic;
import android.text.TextDirectionHeuristics;
import android.text.TextPaint;
import android.text.TextUtils;
import android.text.style.AbsoluteSizeSpan;
import android.text.style.AlignmentSpan;
import android.text.style.ForegroundColorSpan;
import android.text.style.LineHeightSpan;
import android.text.style.MetricAffectingSpan;

final class TextLayouts {
  private TextLayouts() {}

  private static final int SPAN_INTS = 5, SPAN_FLOATS = 2, ITALIC = 1;

  static final class Prepared {
    final CharSequence text;

    /**
     * What layouts are built from. Not {@link #text}: a {@link StaticLayout} over a {@link
     * PrecomputedText} reports every caret at the line's end, which breaks span frames.
     */
    final CharSequence source;

    final TextPaint paint;
    final Layout.Alignment alignment;
    final boolean justify, endAligned;
    final int maxLines;
    final TextUtils.TruncateAt ellipsize;
    final float density;
    final TextDirectionHeuristic direction;
    private int desired = -1, next;
    private final StaticLayout[] layouts = new StaticLayout[4];

    Prepared(
        CharSequence text,
        CharSequence source,
        TextPaint paint,
        Layout.Alignment alignment,
        boolean justify,
        boolean endAligned,
        int maxLines,
        TextUtils.TruncateAt ellipsize,
        float density,
        TextDirectionHeuristic direction) {
      this.text = text;
      this.source = source;
      this.paint = paint;
      this.alignment = alignment;
      this.justify = justify;
      this.endAligned = endAligned;
      this.maxLines = maxLines;
      this.ellipsize = ellipsize;
      this.density = density;
      this.direction = direction;
    }

    int desired() {
      if (desired < 0) {
        float w =
            android.text.TextUtils.indexOf(text, '\n') < 0
                    && android.os.Build.VERSION.SDK_INT >= 29
                    && text instanceof PrecomputedText
                ? ((PrecomputedText) text).getWidth(0, text.length())
                : Layout.getDesiredWidth(text, paint);
        desired = (int) Math.ceil(w);
      }
      return desired;
    }

    StaticLayout layout(int width) {
      width = Math.max(0, width);
      for (StaticLayout l : layouts) if (l != null && l.getWidth() == width) return l;
      // The same break strategy, hyphenation and direction the PrecomputedText measured with. The
      // characters come from `source`, not the PrecomputedText: its layout would report every caret
      // at the line's end, and a span's frame would have no width.
      StaticLayout.Builder b =
          StaticLayout.Builder.obtain(source, 0, source.length(), paint, width)
              .setAlignment(alignment)
              .setTextDirection(direction)
              .setIncludePad(false)
              .setLineSpacing(0, 1)
              .setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE)
              .setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE);
      if (justify) b.setJustificationMode(Layout.JUSTIFICATION_MODE_INTER_WORD);
      if (maxLines > 0) {
        b.setMaxLines(maxLines);
        if (ellipsize != null) b.setEllipsize(ellipsize);
      }
      StaticLayout l = b.build();
      layouts[next] = l;
      next = (next + 1) % layouts.length;
      return l;
    }
  }

  static Object prepare(
      String text,
      int[] ints,
      float[] floats,
      int align,
      int maxLines,
      int ellipsize,
      float density,
      float[] attachments) {
    SpannableString s = new SpannableString(text);
    TextPaint paint = new TextPaint(Paint.ANTI_ALIAS_FLAG);
    paint.density = density;
    Paint.FontMetrics metrics = new Paint.FontMetrics();
    TextPaint measure = new TextPaint();
    int spans = ints.length / SPAN_INTS;
    for (int i = 0; i < spans; i++) {
      int o = i * SPAN_INTS, start = ints[o], end = Math.min(ints[o + 1], text.length());
      if (start >= end) continue;
      int weight = ints[o + 2], flags = ints[o + 3], color = ints[o + 4];
      float size = floats[i * SPAN_FLOATS] * density, line = floats[i * SPAN_FLOATS + 1] * density;
      Typeface face =
          Typeface.create(
              Typeface.DEFAULT, Math.max(1, Math.min(1000, weight)), (flags & ITALIC) != 0);
      if (i == 0) {
        paint.setTextSize(size);
        paint.setTypeface(face);
      }
      int sf = Spanned.SPAN_EXCLUSIVE_EXCLUSIVE;
      s.setSpan(new AbsoluteSizeSpan(Math.round(size)), start, end, sf);
      s.setSpan(new ForegroundColorSpan(color), start, end, sf);
      s.setSpan(new FontSpan(face), start, end, sf);
      if (line > 0)
        s.setSpan(new ExactLineHeight(line), start, end, Spanned.SPAN_INCLUSIVE_INCLUSIVE);
    }
    int writing = (align >> 4) & 3;
    boolean rtl = (align & 0x100) != 0;
    align &= 15;
    TextDirectionHeuristic direction =
        writing == 1
            ? TextDirectionHeuristics.LTR
            : writing == 2
                ? TextDirectionHeuristics.RTL
                : rtl
                    ? TextDirectionHeuristics.FIRSTSTRONG_RTL
                    : TextDirectionHeuristics.FIRSTSTRONG_LTR;
    if (align == 0 && rtl) align = 3;
    Layout.Alignment alignment =
        align == 2 ? Layout.Alignment.ALIGN_CENTER : Layout.Alignment.ALIGN_NORMAL;
    boolean endAligned = false;
    if (align == 1 || align == 3) {
      int start = 0;
      while (start <= text.length()) {
        int end = text.indexOf('\n', start);
        if (end < 0) end = text.length();
        boolean isRtl = direction.isRtl(text, start, end - start);
        boolean normal = (align == 1) != isRtl;
        endAligned |= !normal;
        int spanEnd = Math.min(end + 1, text.length());
        if (spanEnd > start)
          s.setSpan(
              new AlignmentSpan.Standard(
                  normal ? Layout.Alignment.ALIGN_NORMAL : Layout.Alignment.ALIGN_OPPOSITE),
              start,
              spanEnd,
              Spanned.SPAN_PARAGRAPH);
        start = end + 1;
      }
    }
    TextUtils.TruncateAt truncate;
    switch (ellipsize) {
      case 1:
        truncate = TextUtils.TruncateAt.START;
        break;
      case 2:
        truncate = TextUtils.TruncateAt.MIDDLE;
        break;
      case 3:
        truncate = null;
        break;
      default:
        truncate = TextUtils.TruncateAt.END;
    }
    if (attachments != null) {
      for (int i = 0; i + 3 < attachments.length; i += 4) {
        int at = (int) attachments[i];
        if (at < 0 || at >= text.length()) continue;
        s.setSpan(
            new Attachment(
                Math.round(attachments[i + 1] * density),
                Math.round(attachments[i + 2] * density),
                (int) attachments[i + 3]),
            at,
            at + 1,
            Spanned.SPAN_EXCLUSIVE_EXCLUSIVE);
      }
    }
    PrecomputedText.Params params =
        new PrecomputedText.Params.Builder(paint)
            .setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE)
            .setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE)
            .setTextDirection(direction)
            .build();
    return new Prepared(
        PrecomputedText.create(s, params),
        s,
        paint,
        alignment,
        align == 4,
        endAligned,
        maxLines,
        truncate,
        density,
        direction);
  }

  /**
   * A width in dp to whole pixels, down ({@code up} false) or up. Layout lands widths on device
   * pixels (a text measured 64 pixels is 64 / 2.625 dp), but a width that went through the layout's
   * arithmetic (a box's width less its padding) comes back a hair off: 63.99999 pixels, which
   * floored is a pixel narrower than the text and breaks its last word onto a line of its own.
   * Within a thousandth of a pixel is that pixel.
   */
  private static int toPixels(float dp, float density, boolean up) {
    float px = dp * density;
    int nearest = Math.round(px);
    if (Math.abs(px - nearest) < 1e-3f) return nearest;
    return (int) (up ? Math.ceil(px) : Math.floor(px));
  }

  static long measure(Object prepared, float maxWidthDp) {
    Prepared p = (Prepared) prepared;
    int width = p.desired();
    if (!Float.isNaN(maxWidthDp)) {
      // Text that fits lays out as at its natural width: same layout.
      width = Math.min(width, toPixels(maxWidthDp, p.density, false));
    }
    StaticLayout l = p.layout(width);
    int lines = l.getLineCount();
    if (p.maxLines > 0) lines = Math.min(lines, p.maxLines);
    float max = 0;
    for (int i = 0; i < lines; i++) max = Math.max(max, l.getLineWidth(i));
    float w = (float) Math.ceil(Math.min(max, width)) / p.density,
        h = l.getLineTop(lines) / p.density;
    return ((long) Float.floatToRawIntBits(w) << 32) | (Float.floatToRawIntBits(h) & 0xffffffffL);
  }

  static Object layout(Object prepared, float widthDp, float heightDp) {
    Prepared p = (Prepared) prepared;
    int width = toPixels(widthDp, p.density, true);
    // Left-aligned LTR text that fits draws the same in a wider box: keep the measured layout. Not
    // text a paragraph span aligns to the right: laid out at its natural width, it would stay on
    // the left.
    if (p.alignment == Layout.Alignment.ALIGN_NORMAL
        && !p.justify
        && !p.endAligned
        && width >= p.desired()) {
      StaticLayout natural = p.layout(p.desired());
      if (natural.getParagraphDirection(0) == Layout.DIR_LEFT_TO_RIGHT) return natural;
    }
    return p.layout(width);
  }

  static final int LINE_FLOATS = 10;

  static float[] lines(Object layout, float density) {
    if (!(layout instanceof Layout)) return new float[0];
    Layout l = (Layout) layout;
    int count = l.getLineCount();
    float[] out = new float[count * LINE_FLOATS];
    android.graphics.Rect bounds = new android.graphics.Rect();
    TextPaint paint = new TextPaint(l.getPaint());
    paint.getTextBounds("H", 0, 1, bounds);
    float cap = bounds.height();
    paint.getTextBounds("x", 0, 1, bounds);
    float x = bounds.height();
    for (int i = 0; i < count; i++) {
      int o = i * LINE_FLOATS;
      out[o] = l.getLineStart(i);
      out[o + 1] = l.getLineEnd(i);
      out[o + 2] = l.getLineLeft(i) / density;
      out[o + 3] = l.getLineTop(i) / density;
      out[o + 4] = l.getLineWidth(i) / density;
      out[o + 5] = (l.getLineBottom(i) - l.getLineTop(i)) / density;
      out[o + 6] = -l.getLineAscent(i) / density;
      out[o + 7] = l.getLineDescent(i) / density;
      out[o + 8] = cap / density;
      out[o + 9] = x / density;
    }
    return out;
  }

  static final class FontSpan extends MetricAffectingSpan {
    final Typeface face;

    FontSpan(Typeface f) {
      face = f;
    }

    @Override
    public void updateMeasureState(TextPaint p) {
      p.setTypeface(face);
    }

    @Override
    public void updateDrawState(TextPaint p) {
      p.setTypeface(face);
    }
  }

  static final class ExactLineHeight implements LineHeightSpan {
    final int height;

    ExactLineHeight(float h) {
      height = Math.round(h);
    }

    public void chooseHeight(
        CharSequence t, int start, int end, int spanstartv, int v, Paint.FontMetricsInt fm) {
      int current = fm.descent - fm.ascent;
      if (current <= 0) return;
      int extra = height - current;
      fm.ascent -= extra / 2;
      fm.descent += extra - extra / 2;
      fm.top = fm.ascent;
      fm.bottom = fm.descent;
    }
  }

  /**
   * Where a final layout put its attachments, in text order: x, y (top left, dp) each, NaN for ones
   * cut off (lines past the limit, an ellipsis).
   */
  static float[] attachments(Object layout, float density) {
    if (!(layout instanceof Layout)) return new float[0];
    Layout l = (Layout) layout;
    CharSequence text = l.getText();
    if (!(text instanceof Spanned)) return new float[0];
    Spanned spanned = (Spanned) text;
    Attachment[] spans = spanned.getSpans(0, text.length(), Attachment.class);
    java.util.Arrays.sort(spans, (a, b) -> spanned.getSpanStart(a) - spanned.getSpanStart(b));
    float[] out = new float[spans.length * 2];
    int lines = l.getLineCount();
    for (int i = 0; i < spans.length; i++) {
      Attachment a = spans[i];
      int at = spanned.getSpanStart(a);
      int line = l.getLineForOffset(at);
      boolean shown = lines > 0 && at < l.getLineEnd(lines - 1);
      if (shown && l.getEllipsisCount(line) > 0)
        shown = at < l.getLineStart(line) + l.getEllipsisStart(line);
      if (!shown) {
        out[i * 2] = Float.NaN;
        out[i * 2 + 1] = Float.NaN;
        continue;
      }
      float x = l.getPrimaryHorizontal(at);
      if (l.isRtlCharAt(at)) x -= a.width;
      out[i * 2] = x / density;
      out[i * 2 + 1] = (l.getLineBaseline(line) - a.ascent) / density;
    }
    return out;
  }

  /** A hit: the offset in the low 32 bits, bit 32 for a caret at the end of a wrapped line. */
  private static long packHit(int offset, boolean upstream) {
    return (upstream ? 1L << 32 : 0L) | (offset & 0xffffffffL);
  }

  /** The offset (UTF-16) of a {@link #hitTest} result. */
  static int hitOffset(long hit) {
    return (int) hit;
  }

  /**
   * Whether a {@link #hitTest} result is the end of a wrapped line, on that line ({@code
   * Affinity::Upstream}).
   */
  static boolean hitUpstream(long hit) {
    return (hit >>> 32) != 0;
  }

  /**
   * The caret nearest ({@code x}, {@code y}) of a final layout. {@code density}: px per unit of
   * {@code x} and {@code y} (1 when the caller already has pixels).
   */
  static long hitTest(Object layout, float x, float y, float density) {
    if (!(layout instanceof Layout)) return 0;
    Layout l = (Layout) layout;
    int lines = l.getLineCount();
    if (lines == 0) return 0;
    x *= density;
    y *= density;
    int line = l.getLineForVertical((int) y);
    int offset = l.getOffsetForHorizontal(line, x);
    int end = l.getLineEnd(line);
    if (line < lines - 1
        && end > l.getLineStart(line)
        && breakStart(l, line) == end
        && l.isRtlCharAt(end - 1) == (l.getParagraphDirection(line) == Layout.DIR_RIGHT_TO_LEFT)) {
      float endX = wrappedEndX(l, line);
      if (offset == l.getOffsetForHorizontal(line, endX)
          && Math.abs(x - endX) < Math.abs(x - l.getPrimaryHorizontal(offset))) {
        return packHit(end, true);
      }
    }
    return packHit(offset, false);
  }

  private static int breakStart(Layout l, int line) {
    int start = l.getLineStart(line), end = l.getLineEnd(line);
    if (end <= start) return end;
    CharSequence text = l.getText();
    char c = text.charAt(end - 1);
    if (c != '\n' && c != '\r' && c != '\u0085' && c != '\u2028' && c != '\u2029') return end;
    return c == '\n' && end - 1 > start && text.charAt(end - 2) == '\r' ? end - 2 : end - 1;
  }

  private static float wrappedEndX(Layout l, int line) {
    float width = l.getLineWidth(line);
    float start = l.getPrimaryHorizontal(l.getLineStart(line));
    return l.getParagraphDirection(line) == Layout.DIR_RIGHT_TO_LEFT
        ? start - width
        : start + width;
  }

  /**
   * An attachment's placeholder: its size, placed in its line by its {@code verticalAlign} (as
   * {@code TextAttachment::extent} places it, with the run's font metrics). Draws nothing.
   */
  static final class Attachment extends android.text.style.ReplacementSpan {
    static final int BASELINE = 0, TEXT_TOP = 1, TEXT_BOTTOM = 2, MIDDLE = 3;
    final int width, height, align;
    int ascent;

    Attachment(int width, int height, int align) {
      this.width = width;
      this.height = height;
      this.align = align;
      this.ascent = height;
    }

    @Override
    public int getSize(
        Paint paint, CharSequence text, int start, int end, Paint.FontMetricsInt fm) {
      if (align != BASELINE) {
        Paint.FontMetrics font = paint.getFontMetrics();
        switch (align) {
          case TEXT_TOP:
            ascent = Math.round(-font.ascent);
            break;
          case TEXT_BOTTOM:
            ascent = Math.round(height - font.descent);
            break;
          default:
            {
              android.graphics.Rect x = new android.graphics.Rect();
              paint.getTextBounds("x", 0, 1, x);
              ascent = Math.round((height - x.top) / 2f);
              break;
            }
        }
      }
      if (fm != null) {
        Paint.FontMetricsInt own = paint.getFontMetricsInt();
        fm.ascent = Math.min(own.ascent, -Math.max(0, ascent));
        fm.top = Math.min(own.top, fm.ascent);
        fm.descent = Math.max(own.descent, height - ascent);
        fm.bottom = Math.max(own.bottom, fm.descent);
        fm.leading = own.leading;
      }
      return width;
    }

    @Override
    public void draw(
        android.graphics.Canvas canvas,
        CharSequence text,
        int start,
        int end,
        float x,
        int top,
        int y,
        int bottom,
        Paint paint) {}
  }
}
