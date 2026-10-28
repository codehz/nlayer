package dev.nlayer.android;

import android.content.Context;
import android.graphics.Color;
import android.os.Handler;
import android.os.Looper;
import android.util.AttributeSet;
import android.view.Gravity;
import android.view.View;
import android.widget.FrameLayout;

public final class HostView extends FrameLayout {
    final float density; final Mounter mounter; private final Handler main=new Handler(Looper.getMainLooper()); private long handle; private String code,assetsDir; private int safeTop,safeRight,safeBottom,safeLeft,keyboard;
    public HostView(Context c){this(c,null);} public HostView(Context c,AttributeSet a){super(c,a);density=getResources().getDisplayMetrics().density;setFocusable(true);mounter=new Mounter(this);}
    void start(){if(handle==0)handle=Native.create(this,density);}
    public void submit(final byte[] bytes,final Object[] objects){main.post(new Runnable(){public void run(){mounter.apply(bytes,objects==null?new Object[0]:objects);}});}
    public void reportError(final String message){main.post(new Runnable(){public void run(){removeAllViews();android.widget.TextView error=new android.widget.TextView(getContext());error.setText(message);error.setTextColor(Color.WHITE);error.setBackgroundColor(0xff8b0000);error.setPadding(16,16,16,16);addView(error,new LayoutParams(LayoutParams.MATCH_PARENT,LayoutParams.WRAP_CONTENT,Gravity.TOP));}});}
    public void load(String code,String assetsDir){this.code=code;this.assetsDir=assetsDir;start();if(getWidth()>0&&getHeight()>0){Native.load(handle,code.getBytes(java.nio.charset.StandardCharsets.UTF_8),assetsDir);this.code=null;}}
    String assetsPath(){return assetsDir==null?getContext().getFilesDir().getAbsolutePath()+"/assets":assetsDir;}
    @Override protected void onSizeChanged(int w,int h,int ow,int oh){super.onSizeChanged(w,h,ow,oh);if(w<=0||h<=0)return;start();Native.resize(handle,w/density,h/density);Native.insets(handle,safeTop/density,safeRight/density,safeBottom/density,safeLeft/density,keyboard/density);if(code!=null){Native.load(handle,code.getBytes(java.nio.charset.StandardCharsets.UTF_8),assetsDir);code=null;}}
    void setInsets(int t,int r,int b,int l,int k){safeTop=t;safeRight=r;safeBottom=b;safeLeft=l;keyboard=k;if(handle!=0)Native.insets(handle,t/density,r/density,b/density,l/density,k/density);}
    long handle(){return handle;} void destroy(){if(handle!=0){Native.destroy(handle);handle=0;}}
}
