// Copyright 2026 the nlayer authors. All rights reserved. Apache-2.0 license.
//
// Selection derived from nsh (com.longbridge.nsh.TextBox):
// Copyright 2024 Longbridge Whale Wealth Ltd.

package dev.nlayer.android;

import android.annotation.SuppressLint;
import android.content.Context;
import android.graphics.Canvas;
import android.text.StaticLayout;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;

/**
 * Draws a StaticLayout made on the engine thread. A group: the views of its attachments are its
 * children, placed on their placeholders by the core.
 *
 * <p>Selection ({@code userSelect: "text"}): a long press selects the word under the finger (the
 * shared model's word, not a Java breaker) and shows the floating toolbar (Copy, Select all,
 * Share); the handles move its ends. The long press is this view's timer. {@link Mounter}'s press
 * timer is cancelled when the gesture starts, so a selection does not also report {@code
 * onLongPress}, and the release does not report {@code onPress} or open an {@code href}. Text
 * inside a selectable view is selected by that view, not on its own.
 */
final class TextBox extends ViewGroup {
  final int id;
  final Decor decor = new Decor();
  final Mounter mounter;
  StaticLayout layout;
  float x, y, w, h;
  int[] links = new int[0];
  boolean[] linkPress = new boolean[0];
  int[] idStart = new int[0], idEnd = new int[0];
  String[] idName = new String[0];
  int[] runStart = new int[0], runEnd = new int[0], runWeight = new int[0], runColor = new int[0];
  boolean[] runItalic = new boolean[0];
  float[] runSize = new float[0];
  boolean clip;
  int pressMask;
  boolean disabled;

  /** {@code userSelect: "text"}. */
  private boolean selectable;

  /** {@code userSelect: "none"}: out of selectable views too. */
  boolean excluded;

  /** {@code userSelect} as written: 1 text, 0 none, 2 unset. */
  int userSelect = 2;

  /** The selectable view whose gesture this touch is, if any. */
  private SelectableView areaGesture;

  /** What a selectable view has selected here (UTF-16 offsets), or -1. */
  private int areaStart = -1, areaEnd = -1;

  /** {@code selectionColor}: the highlight and the handles' tint; null: the theme's. */
  Integer selectionColor;

  /** The selection (UTF-16 offsets), or -1 when there is none. */
  private int selStart = -1, selEnd = -1;

  private android.view.ActionMode mode;
  private SelectionHandle startHandle, endHandle;
  private android.graphics.Paint highlightPaint;
  private final android.graphics.Path selectionPath = new android.graphics.Path();
  private float downX, downY;

  /** The long press selected: the rest of the gesture moves the selection's end. */
  private boolean extending;

  /** The word the long press selected. */
  private int anchorStart, anchorEnd;

  private final Runnable longPress = this::selectWordAtDown;
  private final android.view.ViewTreeObserver.OnPreDrawListener follow =
      () -> {
        placeHandles();
        return true;
      };

  /** {@code href} ranges: start, end, node. */
  private int[] hrefs = new int[0];

  private int pressedLink = -1;

  /** This gesture is a selection: the release is not a press and does not open {@code href}. */
  private boolean selectionGesture;

  TextBox(Context c, Mounter mounter, int id) {
    super(c);
    this.mounter = mounter;
    this.id = id;
    setWillNotDraw(false);
    setClipChildren(false);
    setBackground(decor);
  }

  void setText(StaticLayout l, float x, float y, float w, float h) {
    boolean changed =
        layout == null || !android.text.TextUtils.equals(layout.getText(), l.getText());
    // New text: the selection's offsets mean nothing in it.
    if (selStart >= 0 && changed) clearSelection();
    if (changed && mounter.selectedArea != null) mounter.selectedArea.textChanged(this);
    layout = l;
    this.x = x;
    this.y = y;
    this.w = w;
    this.h = h;
    invalidate();
  }

  void setLinks(int[] data, boolean[] presses) {
    links = data;
    linkPress = presses;
    noteSelectable();
  }

  /**
   * {@code selectable}: 1 text, 0 none (excluded from an area too), 2 unset (not selectable on its
   * own; an area still includes it).
   */
  void setSelectable(int selectable, Integer selectionColor) {
    boolean on = selectable == 1;
    if (!on) clearSelection();
    boolean recolor = !java.util.Objects.equals(this.selectionColor, selectionColor);
    this.selectable = on;
    this.excluded = selectable == 0;
    this.userSelect = selectable;
    this.selectionColor = selectionColor;
    if (recolor) {
      highlightPaint = null;
      if (startHandle != null) startHandle.tint(selectionColor);
      if (endHandle != null) endHandle.tint(selectionColor);
      if (selStart >= 0 || areaStart >= 0) invalidate();
    }
    noteSelectable();
  }

  void setHrefs(int[] hrefs) {
    this.hrefs = hrefs;
    noteSelectable();
  }

  Integer selectionColor() {
    return selectionColor;
  }

  /** What a selectable view selected here: [{@code start}, {@code end}), or -1, -1. */
  void setAreaSelection(int start, int end) {
    if (start == areaStart && end == areaEnd) return;
    areaStart = start;
    areaEnd = end;
    invalidate();
  }

  /** Clickable when a press, an {@code href}, or a selection gesture can start here. */
  void noteSelectable() {
    boolean area = !excluded && mounter.selectableViewOf(this) != null;
    setClickable(selectable || area || hasPressLink() || (pressMask & 1) != 0 || hrefs.length > 0);
  }

  /** A nested span takes {@code onPress}. That wins over the text view's own. */
  boolean hasPressLink() {
    for (boolean press : linkPress) if (press) return true;
    return false;
  }

  void setIds(int[] start, int[] end, String[] names) {
    idStart = start;
    idEnd = end;
    idName = names;
  }

  void setRuns(int[] start, int[] end, int[] weight, boolean[] italic, int[] color, float[] size) {
    runStart = start;
    runEnd = end;
    runWeight = weight;
    runItalic = italic;
    runColor = color;
    runSize = size;
  }

  @Override
  protected void onMeasure(int ws, int hs) {
    setMeasuredDimension(MeasureSpec.getSize(ws), MeasureSpec.getSize(hs));
  }

  @Override
  protected void onLayout(boolean c, int l, int t, int r, int b) {}

  @Override
  protected void onDetachedFromWindow() {
    clearSelection();
    if (mounter.selectedArea != null) mounter.selectedArea.textChanged(this);
    super.onDetachedFromWindow();
  }

  @Override
  protected void onDraw(Canvas c) {
    super.onDraw(c);
    if (layout == null) return;
    int s = c.save();
    c.translate(x, y);
    c.clipRect(0, 0, w, h);
    if (selStart >= 0) highlight(c, selStart, selEnd);
    else if (areaStart >= 0)
      highlight(
          c,
          Math.min(areaStart, layout.getText().length()),
          Math.min(areaEnd, layout.getText().length()));
    layout.draw(c);
    c.restoreToCount(s);
  }

  /** Draws the selection's highlight over [{@code start}, {@code end}). */
  private void highlight(Canvas canvas, int start, int end) {
    if (start >= end || layout == null) return;
    if (highlightPaint == null) {
      highlightPaint = new android.graphics.Paint();
      int color;
      if (selectionColor != null) {
        color = (selectionColor & 0x00ffffff) | 0x55000000;
      } else {
        android.content.res.TypedArray a =
            getContext().obtainStyledAttributes(new int[] {android.R.attr.textColorHighlight});
        color = a.getColor(0, 0x553b82f6);
        a.recycle();
      }
      highlightPaint.setColor(color);
    }
    layout.getSelectionPath(start, end, selectionPath);
    canvas.drawPath(selectionPath, highlightPaint);
  }

  void setTextColor(int color) {
    if (layout == null) return;
    CharSequence text = layout.getText();
    if (text instanceof android.text.Spannable) {
      android.text.Spannable s = (android.text.Spannable) text;
      android.text.style.ForegroundColorSpan[] spans =
          s.getSpans(0, s.length(), android.text.style.ForegroundColorSpan.class);
      for (android.text.style.ForegroundColorSpan span : spans) {
        s.setSpan(
            new android.text.style.ForegroundColorSpan(color),
            s.getSpanStart(span),
            s.getSpanEnd(span),
            s.getSpanFlags(span));
        s.removeSpan(span);
      }
    }
    invalidate();
  }

  private int hitLink(MotionEvent e) {
    return linkAt(e.getX(), e.getY());
  }

  /** A pressable span under a point in this view's coordinates. */
  boolean pressLinkAt(float x, float y) {
    return linkAt(x, y) >= 0;
  }

  private int linkAt(float x, float y) {
    if (layout == null) return -1;
    int line = layout.getLineForVertical((int) (y - this.y));
    if (line < 0 || line >= layout.getLineCount()) return -1;
    int off = layout.getOffsetForHorizontal(line, x - this.x);
    for (int i = 0; i < linkPress.length; i++) {
      if (linkPress[i] && off >= links[i * 3] && off < links[i * 3 + 1]) return i;
    }
    return -1;
  }

  /** The caret nearest to ({@code x}, {@code y}) (view pixels). */
  private long hitAt(float x, float y) {
    return TextLayouts.hitTest(layout, x - this.x, y - this.y, 1f);
  }

  /** The character offset nearest to ({@code x}, {@code y}) (view pixels), -1 without text. */
  private int offsetAt(float x, float y) {
    if (layout == null) return -1;
    return TextLayouts.hitOffset(hitAt(x, y));
  }

  /** The innermost {@code href} node at {@code offset}, or -1. */
  private int hrefAt(int offset) {
    if (offset < 0) return -1;
    int best = -1, span = Integer.MAX_VALUE;
    for (int i = 0; i + 2 < hrefs.length; i += 3) {
      int start = hrefs[i], end = hrefs[i + 1];
      if (offset < start || offset >= end || end - start >= span) continue;
      span = end - start;
      best = hrefs[i + 2];
    }
    return best;
  }

  /**
   * Who a short press reports: the pressable span when one is hit, else the innermost {@code href}
   * (so the engine opens it), else this text when it has {@code onPress}.
   */
  private int pressTarget(float x, float y) {
    int link = linkAt(x, y);
    if (link >= 0) return links[link * 3 + 2];
    if (layout != null) {
      int line = layout.getLineForVertical((int) (y - this.y));
      if (line >= 0 && line < layout.getLineCount()) {
        int off = layout.getOffsetForHorizontal(line, x - this.x);
        int href = hrefAt(off);
        if (href >= 0) return href;
      }
    }
    return (pressMask & 1) != 0 ? id : -1;
  }

  @SuppressLint("ClickableViewAccessibility")
  @Override
  public boolean onTouchEvent(MotionEvent e) {
    if (disabled || Mounter.blocked(getParent())) {
      if (e.getActionMasked() == MotionEvent.ACTION_DOWN) Mounter.pressed(this, id, false);
      return true;
    }
    int action = e.getActionMasked();
    if (action == MotionEvent.ACTION_DOWN) {
      areaGesture = excluded ? null : mounter.selectableViewOf(this);
    }
    if (areaGesture != null) {
      if (areaGesture.selecting()) {
        pressedLink = -1;
        Mounter.pressed(this, id, false);
      } else if (action == MotionEvent.ACTION_DOWN) {
        // The area's timer is the long press. This text's `onLongPress` does not also run.
        mounter.cancelLong(id);
        pressedLink = hitLink(e);
        if (pressTarget(e.getX(), e.getY()) >= 0) Mounter.pressed(this, id, true);
      }
      boolean handled = areaGesture.onTextTouch(this, e);
      if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) {
        if (areaGesture.tookSelection()) {
          pressedLink = -1;
          Mounter.pressed(this, id, false);
        } else if (action == MotionEvent.ACTION_UP) releasePress(e);
        else {
          pressedLink = -1;
          Mounter.pressed(this, id, false);
        }
      }
      return handled;
    }
    if (selectable) return selectionTouch(e);
    return pressTouch(e);
  }

  private boolean selectionTouch(MotionEvent e) {
    switch (e.getActionMasked()) {
      case MotionEvent.ACTION_DOWN:
        downX = e.getX();
        downY = e.getY();
        extending = false;
        selectionGesture = false;
        clearSelection();
        // One timer: the selection's. The press timer must not also fire.
        mounter.cancelLong(id);
        postDelayed(longPress, android.view.ViewConfiguration.getLongPressTimeout());
        pressedLink = hitLink(e);
        if (pressTarget(e.getX(), e.getY()) >= 0) Mounter.pressed(this, id, true);
        return true;
      case MotionEvent.ACTION_MOVE:
        {
          if (extending) {
            int offset = offsetAt(e.getX(), e.getY());
            if (offset >= 0 && offset < anchorStart) setSelection(offset, anchorEnd, false);
            else if (offset >= 0) setSelection(anchorStart, Math.max(offset, anchorEnd), false);
            break;
          }
          float slop = android.view.ViewConfiguration.get(getContext()).getScaledTouchSlop();
          if (Math.hypot(e.getX() - downX, e.getY() - downY) > slop) removeCallbacks(longPress);
          break;
        }
      case MotionEvent.ACTION_UP:
      case MotionEvent.ACTION_CANCEL:
        removeCallbacks(longPress);
        if (extending || selectionGesture) {
          extending = false;
          if (e.getActionMasked() == MotionEvent.ACTION_UP) showToolbar();
          pressedLink = -1;
          Mounter.pressed(this, id, false);
          // The release is not `onPress` and does not open `href`.
          break;
        }
        if (e.getActionMasked() == MotionEvent.ACTION_UP) releasePress(e);
        else {
          pressedLink = -1;
          Mounter.pressed(this, id, false);
        }
        break;
      default:
        break;
    }
    return true;
  }

  private boolean pressTouch(MotionEvent e) {
    int action = e.getActionMasked();
    if (action == MotionEvent.ACTION_DOWN) {
      pressedLink = hitLink(e);
      if (pressTarget(e.getX(), e.getY()) < 0) return false;
      Mounter.pressed(this, id, true);
      return true;
    }
    if (pressTarget(e.getX(), e.getY()) < 0 && pressedLink < 0 && (pressMask & 1) == 0)
      return false;
    if (action == MotionEvent.ACTION_UP) releasePress(e);
    else if (action == MotionEvent.ACTION_CANCEL) {
      pressedLink = -1;
      Mounter.pressed(this, id, false);
    }
    return true;
  }

  private void releasePress(MotionEvent e) {
    int target = pressedLink >= 0 ? links[pressedLink * 3 + 2] : pressTarget(e.getX(), e.getY());
    pressedLink = -1;
    Mounter.pressed(this, id, false);
    if (target >= 0) Native.sendPress(this, target);
  }

  /** The long press: selects the word under the finger. */
  private void selectWordAtDown() {
    if (layout == null || layout.getText().length() == 0) return;
    long hit = hitAt(downX, downY);
    // The character under the finger: the one before a caret at a wrapped line's end.
    int offset = TextLayouts.hitOffset(hit) - (TextLayouts.hitUpstream(hit) ? 1 : 0);
    if (offset < 0) return;
    int length = layout.getText().length();
    int at = Math.min(offset, length - 1);
    int[] word = SelectableView.word(layout.getText().toString(), at);
    if (word.length < 2 || word[1] <= word[0]) return;
    performHapticFeedback(android.view.HapticFeedbackConstants.LONG_PRESS);
    if (getParent() != null) getParent().requestDisallowInterceptTouchEvent(true);
    mounter.cancelLong(id);
    extending = true;
    selectionGesture = true;
    anchorStart = word[0];
    anchorEnd = word[1];
    pressedLink = -1;
    Mounter.pressed(this, id, false);
    setSelection(word[0], word[1], false);
  }

  /** Selects [{@code start}, {@code end}); with {@code toolbar}, shows the toolbar too. */
  private void setSelection(int start, int end, boolean toolbar) {
    if (layout == null) return;
    int length = layout.getText().length();
    start = Math.max(0, Math.min(start, length));
    end = Math.max(start, Math.min(end, length));
    if (start == end) {
      clearSelection();
      return;
    }
    boolean was = selStart >= 0;
    selStart = start;
    selEnd = end;
    if (!was) {
      if (startHandle == null) {
        SelectionHandle.Owner owner =
            new SelectionHandle.Owner() {
              @Override
              public void handleDown() {
                if (mode != null)
                  mode.hide(android.view.ViewConfiguration.getDefaultActionModeHideDuration());
              }

              @Override
              public void handleMoved(boolean start, float x, float y) {
                dragHandle(start, x, y);
              }

              @Override
              public void handleUp() {
                showToolbar();
              }
            };
        startHandle = new SelectionHandle(this, true, selectionColor, owner);
        endHandle = new SelectionHandle(this, false, selectionColor, owner);
      }
      getViewTreeObserver().addOnPreDrawListener(follow);
      // One selection per surface.
      if (mounter.selectedArea != null) mounter.selectedArea.clear();
      mounter.selectedText = this;
    }
    placeHandles();
    if (toolbar) showToolbar();
    else if (mode != null) mode.invalidateContentRect();
    invalidate();
  }

  /** Ends the selection, its handles and its toolbar. */
  void clearSelection() {
    removeCallbacks(longPress);
    if (selStart < 0) return;
    selStart = selEnd = -1;
    extending = false;
    if (startHandle != null) {
      startHandle.dismiss();
      endHandle.dismiss();
    }
    getViewTreeObserver().removeOnPreDrawListener(follow);
    if (mounter.selectedText == this) mounter.selectedText = null;
    android.view.ActionMode m = mode;
    mode = null;
    if (m != null) m.finish();
    invalidate();
  }

  /** Whether the screen point is on this view or one of its handles. */
  boolean selectionContains(float rawX, float rawY) {
    if (startHandle != null && (startHandle.contains(rawX, rawY) || endHandle.contains(rawX, rawY)))
      return true;
    int[] at = new int[2];
    getLocationOnScreen(at);
    return rawX >= at[0]
        && rawX < at[0] + getWidth()
        && rawY >= at[1]
        && rawY < at[1] + getHeight();
  }

  /** The selected text, attachments left out, from the shared model. */
  private String selectedText() {
    if (layout == null || selStart < 0) return "";
    return SelectableView.copy(layout.getText().toString(), selStart, selEnd);
  }

  private void showToolbar() {
    if (selStart < 0) return;
    if (mode != null) {
      mode.invalidateContentRect();
      mode.hide(0);
      return;
    }
    mode =
        startActionMode(
            new android.view.ActionMode.Callback2() {
              @Override
              public boolean onCreateActionMode(android.view.ActionMode m, android.view.Menu menu) {
                menu.add(android.view.Menu.NONE, android.R.id.copy, 0, android.R.string.copy)
                    .setAlphabeticShortcut('c');
                menu.add(
                        android.view.Menu.NONE,
                        android.R.id.selectAll,
                        1,
                        android.R.string.selectAll)
                    .setAlphabeticShortcut('a');
                // The system's own "Share" label (TextView's), not in the public resources;
                // without it there is no item.
                android.content.res.Resources system = android.content.res.Resources.getSystem();
                int share = system.getIdentifier("share", "string", "android");
                if (share != 0)
                  menu.add(
                      android.view.Menu.NONE, android.R.id.shareText, 2, system.getString(share));
                return true;
              }

              @Override
              public boolean onPrepareActionMode(
                  android.view.ActionMode m, android.view.Menu menu) {
                boolean all =
                    layout != null && selStart == 0 && selEnd == layout.getText().length();
                menu.findItem(android.R.id.selectAll).setVisible(!all);
                return true;
              }

              @Override
              public boolean onActionItemClicked(
                  android.view.ActionMode m, android.view.MenuItem item) {
                if (selStart < 0) return false;
                int itemId = item.getItemId();
                if (itemId == android.R.id.copy) {
                  Mounter.copyText(getContext(), selectedText());
                  clearSelection();
                  return true;
                }
                if (itemId == android.R.id.selectAll) {
                  setSelection(0, layout.getText().length(), false);
                  m.invalidate();
                  return true;
                }
                if (itemId == android.R.id.shareText) {
                  android.content.Intent send =
                      new android.content.Intent(android.content.Intent.ACTION_SEND)
                          .setType("text/plain")
                          .putExtra(android.content.Intent.EXTRA_TEXT, selectedText());
                  android.content.Intent chooser = android.content.Intent.createChooser(send, null);
                  if (!(getContext() instanceof android.app.Activity)) {
                    chooser.addFlags(android.content.Intent.FLAG_ACTIVITY_NEW_TASK);
                  }
                  try {
                    getContext().startActivity(chooser);
                  } catch (android.content.ActivityNotFoundException ignored) {
                    // Nothing can share text.
                  }
                  clearSelection();
                  return true;
                }
                return false;
              }

              @Override
              public void onDestroyActionMode(android.view.ActionMode m) {
                if (mode == m) {
                  mode = null;
                  clearSelection();
                }
              }

              @Override
              @SuppressWarnings("deprecation") // computeBounds(RectF) is @FlaggedApi, not in 35.
              public void onGetContentRect(
                  android.view.ActionMode m, View view, android.graphics.Rect out) {
                if (layout == null || selStart < 0) {
                  out.set(0, 0, getWidth(), getHeight());
                  return;
                }
                android.graphics.RectF bounds = new android.graphics.RectF();
                layout.getSelectionPath(selStart, selEnd, selectionPath);
                selectionPath.computeBounds(bounds, true);
                out.set(
                    Math.round(x + bounds.left),
                    Math.round(y + bounds.top),
                    Math.round(x + bounds.right),
                    Math.round(y + bounds.bottom));
              }
            },
            android.view.ActionMode.TYPE_FLOATING);
  }

  /** Puts the handles at the selection's ends (or hides them while the text is off screen). */
  private void placeHandles() {
    if (selStart < 0 || layout == null || !isAttachedToWindow() || startHandle == null) return;
    if (!isShown()) {
      startHandle.dismiss();
      endHandle.dismiss();
      return;
    }
    int[] at = new int[2];
    getLocationInWindow(at);
    int startLine = layout.getLineForOffset(selStart);
    startHandle.place(
        at[0] + x + layout.getPrimaryHorizontal(selStart),
        at[1] + y + layout.getLineBottom(startLine));
    int endLine = layout.getLineForOffset(selEnd);
    float endX;
    if (endLine > 0 && selEnd == layout.getLineStart(endLine)) {
      endLine--;
      endX = layout.getLineRight(endLine);
    } else {
      endX = layout.getPrimaryHorizontal(selEnd);
    }
    endHandle.place(at[0] + x + endX, at[1] + y + layout.getLineBottom(endLine));
  }

  /** A handle moved to the window point ({@code x}, {@code y}): its line bottom. */
  private void dragHandle(boolean start, float x, float y) {
    int[] at = new int[2];
    getLocationInWindow(at);
    int offset = offsetAt(x - at[0], y - at[1] - 2 * getResources().getDisplayMetrics().density);
    if (offset < 0 || selStart < 0) return;
    if (start) setSelection(Math.min(offset, selEnd - 1), selEnd, false);
    else setSelection(selStart, Math.max(offset, selStart + 1), false);
  }
}
