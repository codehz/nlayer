package dev.nlayer.android;

import android.content.Context;
import android.graphics.Color;
import android.text.InputType;
import android.text.TextWatcher;
import android.widget.EditText;
import android.widget.FrameLayout;

final class InputBox extends FrameLayout {
    final int id; final EditText edit; int count; boolean setting;
    String placeholder = "", keyboardName = "text"; boolean secure, disabled; int pressMask;
    private boolean armed;
    InputBox(Context c,int id){super(c);this.id=id;edit=new EditText(c);edit.setSingleLine(true);edit.setPadding(0,0,0,0);edit.setBackgroundColor(Color.TRANSPARENT);addView(edit,new LayoutParams(-1,-1));edit.setOnTouchListener(new android.view.View.OnTouchListener(){public boolean onTouch(android.view.View v,android.view.MotionEvent e){return tap(e);}});edit.addTextChangedListener(new android.text.TextWatcher(){public void beforeTextChanged(CharSequence s,int st,int n,int a){}public void onTextChanged(CharSequence s,int st,int before,int n){}public void afterTextChanged(android.text.Editable e){if(!setting){count++;Native.sendInput(InputBox.this,id,e.toString());}}});}
    /** `onPress` on the field. Returning false leaves focus and typing to the `EditText`. */
    private boolean tap(android.view.MotionEvent e){if((pressMask&1)==0||disabled)return false;int action=e.getActionMasked();if(action==android.view.MotionEvent.ACTION_DOWN){armed=true;Mounter.pressed(this,id,true);}else if(action==android.view.MotionEvent.ACTION_UP){if(armed){armed=false;Mounter.pressed(this,id,false);Native.sendPress(this,id);}}else if(action==android.view.MotionEvent.ACTION_CANCEL){armed=false;Mounter.pressed(this,id,false);}return false;}
    void configure(float size,int weight,boolean italic,int color,float spacing,float lineHeight,int align,String placeholder,boolean secure,int keyboard,boolean disabled,boolean rtl){this.placeholder=placeholder==null?"":placeholder;this.secure=secure;this.disabled=disabled;this.keyboardName=keyboard==1?"email":keyboard==2?"number":keyboard==3?"phone":"text";edit.setTextSize(size);edit.setTextColor(color);edit.setTypeface(android.graphics.Typeface.create(android.graphics.Typeface.DEFAULT,Math.max(1,Math.min(1000,weight)),italic));edit.setHint(placeholder);edit.setEnabled(!disabled);int type=secure?InputType.TYPE_CLASS_TEXT|InputType.TYPE_TEXT_VARIATION_PASSWORD:keyboard==1?InputType.TYPE_CLASS_TEXT|InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS:keyboard==2?InputType.TYPE_CLASS_NUMBER|InputType.TYPE_NUMBER_FLAG_DECIMAL:keyboard==3?InputType.TYPE_CLASS_PHONE:InputType.TYPE_CLASS_TEXT;edit.setInputType(type);edit.setGravity((align==2?android.view.Gravity.CENTER_HORIZONTAL:align==3?android.view.Gravity.RIGHT:android.view.Gravity.LEFT)|android.view.Gravity.CENTER_VERTICAL);edit.setLayoutDirection(rtl?LAYOUT_DIRECTION_RTL:LAYOUT_DIRECTION_LTR);}
    void setValue(int n,String value){if(n<count)return;count=n;setting=true;try{if(!value.contentEquals(edit.getText()))edit.setText(value);}finally{setting=false;}}
}
