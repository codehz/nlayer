package dev.nlayer.android;

import android.content.Context;
import android.graphics.Canvas;
import android.text.Layout;
import android.text.StaticLayout;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;

final class TextBox extends ViewGroup {
    final int id; final Decor decor=new Decor(); StaticLayout layout; float x,y,w,h; int[] links=new int[0]; boolean[] linkPress=new boolean[0]; boolean clip;
    TextBox(Context c,int id){super(c);this.id=id;setWillNotDraw(false);setBackground(decor);}
    void setText(StaticLayout l,float x,float y,float w,float h){layout=l;this.x=x;this.y=y;this.w=w;this.h=h;invalidate();}
    @Override protected void onMeasure(int ws,int hs){setMeasuredDimension(MeasureSpec.getSize(ws),MeasureSpec.getSize(hs));}
    @Override protected void onLayout(boolean c,int l,int t,int r,int b){}
    @Override protected void onDraw(Canvas c){super.onDraw(c);if(layout==null)return;int s=c.save();c.translate(x,y);c.clipRect(0,0,w,h);layout.draw(c);c.restoreToCount(s);}
    void setLinks(int[] data,boolean[] presses){links=data;linkPress=presses;setClickable(data.length>0);}
    void setTextColor(int color){if(layout==null)return;CharSequence text=layout.getText();if(text instanceof android.text.Spannable){android.text.Spannable s=(android.text.Spannable)text;android.text.style.ForegroundColorSpan[] spans=s.getSpans(0,s.length(),android.text.style.ForegroundColorSpan.class);for(android.text.style.ForegroundColorSpan span:spans)s.setSpan(new android.text.style.ForegroundColorSpan(color),s.getSpanStart(span),s.getSpanEnd(span),s.getSpanFlags(span));}invalidate();}
    @Override public boolean onTouchEvent(MotionEvent e){if(e.getAction()==MotionEvent.ACTION_UP&&layout!=null){int line=layout.getLineForVertical((int)(e.getY()-y));int off=layout.getOffsetForHorizontal(line,e.getX()-x);for(int i=0;i<linkPress.length;i++)if(linkPress[i]&&off>=links[i*3]&&off<links[i*3+1]){Native.sendPress(this,links[i*3+2]);return true;}}return true;}
}
