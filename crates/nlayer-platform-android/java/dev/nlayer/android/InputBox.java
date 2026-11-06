package dev.nlayer.android;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Path;
import android.graphics.Rect;
import android.graphics.drawable.ColorDrawable;
import android.os.Build;
import android.text.InputType;
import android.util.TypedValue;
import android.view.inputmethod.BaseInputConnection;
import android.widget.EditText;
import android.widget.FrameLayout;

/** An EditText in a container that carries the decoration. */
final class InputBox extends FrameLayout {
  final int id;
  final Decor decor = new Decor();
  final EditText edit;
  int count;
  boolean setting;
  String placeholder = "", keyboardName = "text";
  boolean secure, disabled, multiline;

  /** The text as last reported or set (a newer JS value waits for a composition's end). */
  String known = "";

  /** The composition last seen and the value JS set during it. */
  boolean inComposition;

  String composed = "", pendingText;
  int pendingCount;
  boolean watching;
  int pressMask;
  boolean clip;
  int textColor;
  private boolean armed;

  InputBox(Context c, int id) {
    super(c);
    this.id = id;
    setWillNotDraw(false);
    // Borders over the field, as a view's border sits over its children.
    decor.bordersAbove = true;
    setBackground(decor);
    edit =
        new EditText(c) {
          @Override
          protected void onSelectionChanged(int start, int end) {
            super.onSelectionChanged(start, end);
            if (!setting) Native.sendSelection(InputBox.this, id, start, end);
          }
        };
    edit.setSingleLine(true);
    edit.setPadding(0, 0, 0, 0);
    edit.setBackground(new ColorDrawable(0));
    edit.setIncludeFontPadding(false);
    // The line's height from its own font, as Text's StaticLayout has it.
    if (Build.VERSION.SDK_INT >= 28) edit.setFallbackLineSpacing(false);
    addView(edit, new LayoutParams(-1, -1));
    edit.setOnTouchListener(
        new android.view.View.OnTouchListener() {
          public boolean onTouch(android.view.View v, android.view.MotionEvent e) {
            return tap(e);
          }
        });
    edit.addTextChangedListener(
        new android.text.TextWatcher() {
          public void beforeTextChanged(CharSequence s, int st, int n, int a) {}

          public void onTextChanged(CharSequence s, int st, int before, int n) {}

          public void afterTextChanged(android.text.Editable e) {
            if (setting) return;
            String text = e.toString();
            // A composition that starts or changes is reported before the
            // text holding it, one that ends after the text it committed,
            // as browsers order them.
            boolean composingNow = composingStart(e) >= 0;
            watching = true;
            try {
              if (composingNow) syncComposition();
              if (!text.equals(known)) {
                known = text;
                count++;
                Native.sendInput(InputBox.this, id, text);
              }
              if (!composingNow) syncComposition();
            } finally {
              watching = false;
            }
          }
        });
    edit.setOnFocusChangeListener(
        (v, focused) -> {
          if (!focused) {
            edit.clearComposingText();
            syncComposition();
          }
        });
  }

  /** `onPress` on the field. Returning false leaves focus and typing to the `EditText`. */
  private boolean tap(android.view.MotionEvent e) {
    if ((pressMask & 1) == 0 || disabled || Mounter.blocked(getParent())) return false;
    int action = e.getActionMasked();
    if (action == android.view.MotionEvent.ACTION_DOWN) {
      armed = true;
      Mounter.pressed(this, id, true);
    } else if (action == android.view.MotionEvent.ACTION_UP) {
      if (armed) {
        armed = false;
        Mounter.pressed(this, id, false);
        Native.sendPress(this, id);
      }
    } else if (action == android.view.MotionEvent.ACTION_CANCEL) {
      armed = false;
      Mounter.pressed(this, id, false);
    }
    return false;
  }

  @Override
  protected void dispatchDraw(Canvas canvas) {
    int save = -1;
    if (clip) {
      save = canvas.save();
      Path p = new Path();
      Rect bounds = new Rect(0, 0, getWidth(), getHeight());
      Decor.roundedRect(
          p,
          new android.graphics.RectF(bounds),
          new float[] {
            decor.radii[0],
            decor.radii[0],
            decor.radii[1],
            decor.radii[1],
            decor.radii[2],
            decor.radii[2],
            decor.radii[3],
            decor.radii[3]
          },
          decor.continuous);
      canvas.clipPath(p);
    }
    super.dispatchDraw(canvas);
    if (save >= 0) canvas.restoreToCount(save);
    decor.setBounds(0, 0, getWidth(), getHeight());
    decor.drawBorders(canvas);
  }

  void configure(
      float size,
      int weight,
      boolean italic,
      int color,
      float spacing,
      float lineHeight,
      int align,
      String placeholder,
      boolean secure,
      int keyboard,
      boolean disabled,
      boolean rtl,
      boolean multiline,
      float density) {
    this.placeholder = placeholder == null ? "" : placeholder;
    this.secure = secure;
    this.disabled = disabled;
    this.multiline = multiline;
    this.textColor = color;
    this.keyboardName =
        keyboard == 1 ? "email" : keyboard == 2 ? "number" : keyboard == 3 ? "phone" : "text";
    edit.setTextSize(TypedValue.COMPLEX_UNIT_PX, size * density);
    applyTextColor(color);
    if (size > 0) edit.setLetterSpacing(spacing / size);
    if (lineHeight > 0 && Build.VERSION.SDK_INT >= 28)
      edit.setLineHeight(Math.round(lineHeight * density));
    else edit.setLineSpacing(0, 1);
    edit.setHint(placeholder);
    edit.setEnabled(!disabled);
    int type =
        secure && !multiline
            ? InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD
            : keyboard == 1
                ? InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
                : keyboard == 2
                    ? InputType.TYPE_CLASS_NUMBER | InputType.TYPE_NUMBER_FLAG_DECIMAL
                    : keyboard == 3 ? InputType.TYPE_CLASS_PHONE : InputType.TYPE_CLASS_TEXT;
    if (multiline) type |= InputType.TYPE_TEXT_FLAG_MULTI_LINE;
    edit.setSingleLine(!multiline);
    if (multiline) edit.setMaxLines(Integer.MAX_VALUE);
    edit.setInputType(type);
    final boolean resign = !multiline && (keyboard == 0 || keyboard == 1);
    edit.setImeOptions(
        resign
            ? android.view.inputmethod.EditorInfo.IME_ACTION_DONE
            : android.view.inputmethod.EditorInfo.IME_ACTION_NONE);
    edit.setOnEditorActionListener(
        new android.widget.TextView.OnEditorActionListener() {
          public boolean onEditorAction(
              android.widget.TextView v, int action, android.view.KeyEvent event) {
            if (!resign) return false;
            if (action == android.view.inputmethod.EditorInfo.IME_ACTION_DONE
                || (event != null && event.getKeyCode() == android.view.KeyEvent.KEYCODE_ENTER)) {
              Native.dismissKeyboard(InputBox.this);
              return true;
            }
            return false;
          }
        });
    // Password input types reset the typeface. Set it after.
    edit.setTypeface(
        android.graphics.Typeface.create(
            android.graphics.Typeface.DEFAULT, Math.max(1, Math.min(1000, weight)), italic));
    edit.setGravity(
        (align == 2
                ? android.view.Gravity.CENTER_HORIZONTAL
                : align == 3 ? android.view.Gravity.RIGHT : android.view.Gravity.LEFT)
            | (multiline ? android.view.Gravity.TOP : android.view.Gravity.CENTER_VERTICAL));
    edit.setLayoutDirection(rtl ? LAYOUT_DIRECTION_RTL : LAYOUT_DIRECTION_LTR);
  }

  /** `selectedRange`: clamps to the text and applies without reporting back a stale value. */
  void setSelection(int start, int end) {
    int length = edit.getText().length();
    boolean was = setting;
    setting = true;
    try {
      edit.setSelection(Math.min(start, length), Math.min(Math.max(end, start), length));
    } finally {
      setting = was;
    }
  }

  /** The value JS set. During a composition it waits for the end. */
  void setValue(int n, String value) {
    if (inComposition) {
      pendingText = value;
      pendingCount = n;
      return;
    }
    pendingText = null;
    if (n < count) return;
    count = n;
    if (value.contentEquals(edit.getText())) {
      known = value;
      return;
    }
    setting = true;
    known = value;
    int selection = edit.getSelectionEnd();
    try {
      edit.setText(value);
      edit.setSelection(Math.min(Math.max(selection, 0), value.length()));
    } finally {
      setting = false;
    }
  }

  private static int composingStart(android.text.Editable text) {
    return BaseInputConnection.getComposingSpanStart(text);
  }

  /** Reports a change of the composition and applies a value JS set meanwhile. */
  private void syncComposition() {
    android.text.Editable text = edit.getText();
    int start = text == null ? -1 : BaseInputConnection.getComposingSpanStart(text);
    int end = text == null ? -1 : BaseInputConnection.getComposingSpanEnd(text);
    boolean now = start >= 0 && end >= 0 && start != end;
    String composedNow =
        now ? text.subSequence(Math.min(start, end), Math.max(start, end)).toString() : "";
    if (now == inComposition && composedNow.equals(composed)) return;
    inComposition = now;
    composed = composedNow;
    Native.sendComposition(this, id, now, composedNow);
    if (!now && pendingText != null) {
      if (watching) post(this::applyPending);
      else applyPending();
    }
  }

  void applyPending() {
    if (inComposition || pendingText == null) return;
    String text = pendingText;
    int n = pendingCount;
    pendingText = null;
    setValue(n, text);
  }

  /**
   * The widget style's text appearance is the inverse one, so its hint is light. A light hint
   * disappears on a light field. Mute the text color instead.
   */
  static int hintColor(int color) {
    return (color & 0x00ffffff) | 0x99000000;
  }

  void applyTextColor(int color) {
    if (edit.getCurrentTextColor() != color) edit.setTextColor(color);
    int hint = hintColor(color);
    if (edit.getCurrentHintTextColor() != hint) edit.setHintTextColor(hint);
  }
}
