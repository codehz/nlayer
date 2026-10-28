package dev.nlayer.android;

import android.content.Context;
import android.graphics.Canvas;
import android.graphics.Path;
import android.graphics.Rect;
import android.graphics.drawable.ColorDrawable;
import android.os.Build;
import android.text.InputType;
import android.util.TypedValue;
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
  boolean secure, disabled;
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
    edit = new EditText(c);
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
            if (!setting) {
              count++;
              Native.sendInput(InputBox.this, id, e.toString());
            }
          }
        });
  }

  /** `onPress` on the field. Returning false leaves focus and typing to the `EditText`. */
  private boolean tap(android.view.MotionEvent e) {
    if ((pressMask & 1) == 0 || disabled) return false;
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
      float density) {
    this.placeholder = placeholder == null ? "" : placeholder;
    this.secure = secure;
    this.disabled = disabled;
    this.textColor = color;
    this.keyboardName =
        keyboard == 1 ? "email" : keyboard == 2 ? "number" : keyboard == 3 ? "phone" : "text";
    edit.setTextSize(TypedValue.COMPLEX_UNIT_PX, size * density);
    edit.setTextColor(color);
    if (size > 0) edit.setLetterSpacing(spacing / size);
    if (lineHeight > 0 && Build.VERSION.SDK_INT >= 28)
      edit.setLineHeight(Math.round(lineHeight * density));
    else edit.setLineSpacing(0, 1);
    edit.setHint(placeholder);
    edit.setEnabled(!disabled);
    int type =
        secure
            ? InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD
            : keyboard == 1
                ? InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
                : keyboard == 2
                    ? InputType.TYPE_CLASS_NUMBER | InputType.TYPE_NUMBER_FLAG_DECIMAL
                    : keyboard == 3 ? InputType.TYPE_CLASS_PHONE : InputType.TYPE_CLASS_TEXT;
    edit.setInputType(type);
    // Password input types reset the typeface. Set it after.
    edit.setTypeface(
        android.graphics.Typeface.create(
            android.graphics.Typeface.DEFAULT, Math.max(1, Math.min(1000, weight)), italic));
    edit.setGravity(
        (align == 2
                ? android.view.Gravity.CENTER_HORIZONTAL
                : align == 3 ? android.view.Gravity.RIGHT : android.view.Gravity.LEFT)
            | android.view.Gravity.CENTER_VERTICAL);
    edit.setLayoutDirection(rtl ? LAYOUT_DIRECTION_RTL : LAYOUT_DIRECTION_LTR);
  }

  void setValue(int n, String value) {
    if (n < count) return;
    count = n;
    setting = true;
    try {
      if (!value.contentEquals(edit.getText())) edit.setText(value);
    } finally {
      setting = false;
    }
  }
}
