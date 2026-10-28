package dev.nlayer.android;

import android.content.Context;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.HorizontalScrollView;
import android.widget.ScrollView;

final class ScrollBox extends FrameLayout {
    final int id;final Decor decor=new Decor();boolean horizontal;ViewGroup scroller;final Box content;int contentW,contentH;float contentPointsW,contentPointsH,offsetX,offsetY;
    ScrollBox(Context c,Mounter m,int id){super(c);this.id=id;setBackground(decor);content=new Box(c,m,id);build(c);}
    private void build(Context c){scroller=horizontal?new HorizontalScrollView(c):new ScrollView(c);scroller.setClipToPadding(false);scroller.setVerticalScrollBarEnabled(!horizontal);scroller.setHorizontalScrollBarEnabled(horizontal);scroller.addView(content,new ViewGroup.LayoutParams(-2,-2));addView(scroller,0,new LayoutParams(-1,-1));size(contentW,contentH);}
    void configure(boolean h,boolean indicator,Context c,Mounter m){if(horizontal!=h){horizontal=h;if(scroller!=null){scroller.removeView(content);removeView(scroller);}build(c);}scroller.setVerticalScrollBarEnabled(indicator&&!horizontal);scroller.setHorizontalScrollBarEnabled(indicator&&horizontal);}
    void size(int w,int h){contentW=w;contentH=h;if(content==null)return;int cw=Math.max(w,getWidth()),ch=Math.max(h,getHeight());content.measure(MeasureSpec.makeMeasureSpec(cw,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(ch,MeasureSpec.EXACTLY));content.layout(0,0,cw,ch);}
    void scrollByPoints(float dx,float dy,float density){offsetX+=dx;offsetY+=dy;if(horizontal)scroller.scrollBy(Math.round(dx*density),0);else scroller.scrollBy(0,Math.round(dy*density));}
    void scrollToPoints(float x,float y,float density){offsetX=x;offsetY=y;int px=Math.round(x*density),py=Math.round(y*density);if(horizontal)scroller.scrollTo(px,0);else scroller.scrollTo(0,py);}
}
