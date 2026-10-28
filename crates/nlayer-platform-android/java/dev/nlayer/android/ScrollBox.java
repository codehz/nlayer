package dev.nlayer.android;

import android.content.Context;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.HorizontalScrollView;
import android.widget.ScrollView;

final class ScrollBox extends FrameLayout {
    final int id;final Decor decor=new Decor();boolean horizontal;ViewGroup scroller;final Box content;int contentW,contentH;
    ScrollBox(Context c,Mounter m,int id){super(c);this.id=id;setBackground(decor);content=new Box(c,m,id);build(c);}
    private void build(Context c){scroller=horizontal?new HorizontalScrollView(c):new ScrollView(c);scroller.setClipToPadding(false);scroller.setVerticalScrollBarEnabled(!horizontal);scroller.setHorizontalScrollBarEnabled(horizontal);scroller.addView(content,new ViewGroup.LayoutParams(-2,-2));addView(scroller,0,new LayoutParams(-1,-1));size(contentW,contentH);}
    void configure(boolean h,boolean indicator,Context c,Mounter m){if(horizontal!=h){horizontal=h;if(scroller!=null){scroller.removeView(content);removeView(scroller);}build(c);}scroller.setVerticalScrollBarEnabled(indicator&&!horizontal);scroller.setHorizontalScrollBarEnabled(indicator&&horizontal);}
    void size(int w,int h){contentW=w;contentH=h;if(content==null)return;int cw=Math.max(w,getWidth()),ch=Math.max(h,getHeight());content.measure(MeasureSpec.makeMeasureSpec(cw,MeasureSpec.EXACTLY),MeasureSpec.makeMeasureSpec(ch,MeasureSpec.EXACTLY));content.layout(0,0,cw,ch);}
    void scrollDp(int dx,int dy){if(horizontal)scroller.scrollBy(dx,0);else scroller.scrollBy(0,dy);}
    void scrollToDp(int x,int y,boolean animated){if(horizontal){if(animated)((HorizontalScrollView)scroller).smoothScrollTo(x,0);else scroller.scrollTo(x,0);}else{if(animated)((ScrollView)scroller).smoothScrollTo(0,y);else scroller.scrollTo(0,y);}}
}
