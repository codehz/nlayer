package dev.nlayer.android;

import android.graphics.Paint;
import android.graphics.Typeface;
import android.text.Layout;
import android.text.PrecomputedText;
import android.text.StaticLayout;
import android.text.TextDirectionHeuristic;
import android.text.TextDirectionHeuristics;
import android.text.TextPaint;
import android.text.TextUtils;
import android.text.SpannableString;
import android.text.Spanned;
import android.text.style.AbsoluteSizeSpan;
import android.text.style.AlignmentSpan;
import android.text.style.ForegroundColorSpan;
import android.text.style.LineHeightSpan;
import android.text.style.MetricAffectingSpan;

final class TextLayouts {
    private TextLayouts() {}
    private static final int SPAN_INTS=5, SPAN_FLOATS=2, ITALIC=1;
    static final class Prepared {
        final CharSequence text; final TextPaint paint; final Layout.Alignment alignment; final boolean justify,endAligned; final int maxLines; final TextUtils.TruncateAt ellipsize; final float density; final TextDirectionHeuristic direction;
        private int desired=-1,next; private final StaticLayout[] layouts=new StaticLayout[4];
        Prepared(CharSequence text,TextPaint paint,Layout.Alignment alignment,boolean justify,boolean endAligned,int maxLines,TextUtils.TruncateAt ellipsize,float density,TextDirectionHeuristic direction){this.text=text;this.paint=paint;this.alignment=alignment;this.justify=justify;this.endAligned=endAligned;this.maxLines=maxLines;this.ellipsize=ellipsize;this.density=density;this.direction=direction;}
        int desired(){if(desired<0){float w=android.text.TextUtils.indexOf(text,'\n')<0&&android.os.Build.VERSION.SDK_INT>=29&&text instanceof PrecomputedText?((PrecomputedText)text).getWidth(0,text.length()):Layout.getDesiredWidth(text,paint);desired=(int)Math.ceil(w);}return desired;}
        StaticLayout layout(int width){width=Math.max(0,width);for(StaticLayout l:layouts)if(l!=null&&l.getWidth()==width)return l;StaticLayout.Builder b=StaticLayout.Builder.obtain(text,0,text.length(),paint,width).setAlignment(alignment).setTextDirection(direction).setIncludePad(false).setLineSpacing(0,1).setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE).setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE);if(justify)b.setJustificationMode(Layout.JUSTIFICATION_MODE_INTER_WORD);if(maxLines>0){b.setMaxLines(maxLines);if(ellipsize!=null)b.setEllipsize(ellipsize);}StaticLayout l=b.build();layouts[next]=l;next=(next+1)%layouts.length;return l;}
    }
    static Object prepare(String text,int[] ints,float[] floats,int align,int maxLines,int ellipsize,float density){
        SpannableString s=new SpannableString(text);TextPaint paint=new TextPaint(Paint.ANTI_ALIAS_FLAG);paint.density=density;Paint.FontMetrics metrics=new Paint.FontMetrics();TextPaint measure=new TextPaint();int spans=ints.length/SPAN_INTS;
        for(int i=0;i<spans;i++){int o=i*SPAN_INTS,start=ints[o],end=Math.min(ints[o+1],text.length());if(start>=end)continue;int weight=ints[o+2],flags=ints[o+3],color=ints[o+4];float size=floats[i*SPAN_FLOATS]*density,line=floats[i*SPAN_FLOATS+1]*density;Typeface face=Typeface.create(Typeface.DEFAULT,Math.max(1,Math.min(1000,weight)),(flags&ITALIC)!=0);if(i==0){paint.setTextSize(size);paint.setTypeface(face);}int sf=Spanned.SPAN_EXCLUSIVE_EXCLUSIVE;s.setSpan(new AbsoluteSizeSpan(Math.round(size)),start,end,sf);s.setSpan(new ForegroundColorSpan(color),start,end,sf);s.setSpan(new FontSpan(face),start,end,sf);if(line>0)s.setSpan(new ExactLineHeight(line),start,end,Spanned.SPAN_INCLUSIVE_INCLUSIVE);}
        int writing=(align>>4)&3;boolean rtl=(align&0x100)!=0;align&=15;TextDirectionHeuristic direction=writing==1?TextDirectionHeuristics.LTR:writing==2?TextDirectionHeuristics.RTL:rtl?TextDirectionHeuristics.FIRSTSTRONG_RTL:TextDirectionHeuristics.FIRSTSTRONG_LTR;if(align==0&&rtl)align=3;Layout.Alignment alignment=align==2?Layout.Alignment.ALIGN_CENTER:Layout.Alignment.ALIGN_NORMAL;boolean endAligned=false;
        if(align==1||align==3){int start=0;while(start<=text.length()){int end=text.indexOf('\n',start);if(end<0)end=text.length();boolean isRtl=direction.isRtl(text,start,end-start);boolean normal=(align==1)!=isRtl;endAligned|=!normal;int spanEnd=Math.min(end+1,text.length());if(spanEnd>start)s.setSpan(new AlignmentSpan.Standard(normal?Layout.Alignment.ALIGN_NORMAL:Layout.Alignment.ALIGN_OPPOSITE),start,spanEnd,Spanned.SPAN_PARAGRAPH);start=end+1;}}
        TextUtils.TruncateAt truncate;switch(ellipsize){case 1:truncate=TextUtils.TruncateAt.START;break;case 2:truncate=TextUtils.TruncateAt.MIDDLE;break;case 3:truncate=null;break;default:truncate=TextUtils.TruncateAt.END;}
        PrecomputedText.Params params=new PrecomputedText.Params.Builder(paint).setBreakStrategy(Layout.BREAK_STRATEGY_SIMPLE).setHyphenationFrequency(Layout.HYPHENATION_FREQUENCY_NONE).setTextDirection(direction).build();return new Prepared(PrecomputedText.create(s,params),paint,alignment,align==4,endAligned,maxLines,truncate,density,direction);
    }
    static long measure(Object prepared,float maxWidthDp){Prepared p=(Prepared)prepared;int width=p.desired();if(!Float.isNaN(maxWidthDp))width=Math.min(width,(int)Math.floor(maxWidthDp*p.density));StaticLayout l=p.layout(width);int lines=l.getLineCount();if(p.maxLines>0)lines=Math.min(lines,p.maxLines);float max=0;for(int i=0;i<lines;i++)max=Math.max(max,l.getLineWidth(i));float w=(float)Math.ceil(Math.min(max,width))/p.density,h=l.getLineTop(lines)/p.density;return((long)Float.floatToRawIntBits(w)<<32)|(Float.floatToRawIntBits(h)&0xffffffffL);}
    static Object layout(Object prepared,float widthDp,float heightDp){Prepared p=(Prepared)prepared;int width=(int)Math.ceil(widthDp*p.density);if(p.alignment==Layout.Alignment.ALIGN_NORMAL&&!p.justify&&!p.endAligned&&width>=p.desired()){StaticLayout natural=p.layout(p.desired());if(natural.getParagraphDirection(0)==Layout.DIR_LEFT_TO_RIGHT)return natural;}return p.layout(width);}
    static final int LINE_FLOATS=10;
    static float[] lines(Object layout,float density){if(!(layout instanceof Layout))return new float[0];Layout l=(Layout)layout;int count=l.getLineCount();float[] out=new float[count*LINE_FLOATS];android.graphics.Rect bounds=new android.graphics.Rect();TextPaint paint=new TextPaint(l.getPaint());paint.getTextBounds("H",0,1,bounds);float cap=bounds.height();paint.getTextBounds("x",0,1,bounds);float x=bounds.height();for(int i=0;i<count;i++){int o=i*LINE_FLOATS;out[o]=l.getLineStart(i);out[o+1]=l.getLineEnd(i);out[o+2]=l.getLineLeft(i)/density;out[o+3]=l.getLineTop(i)/density;out[o+4]=l.getLineWidth(i)/density;out[o+5]=(l.getLineBottom(i)-l.getLineTop(i))/density;out[o+6]=-l.getLineAscent(i)/density;out[o+7]=l.getLineDescent(i)/density;out[o+8]=cap/density;out[o+9]=x/density;}return out;}
    static final class FontSpan extends MetricAffectingSpan {final Typeface face;FontSpan(Typeface f){face=f;}@Override public void updateMeasureState(TextPaint p){p.setTypeface(face);}@Override public void updateDrawState(TextPaint p){p.setTypeface(face);}}
    static final class ExactLineHeight implements LineHeightSpan {final int height;ExactLineHeight(float h){height=Math.round(h);}public void chooseHeight(CharSequence t,int start,int end,int spanstartv,int v,Paint.FontMetricsInt fm){int current=fm.descent-fm.ascent;if(current<=0)return;int extra=height-current;fm.ascent-=extra/2;fm.descent+=extra-extra/2;fm.top=fm.ascent;fm.bottom=fm.descent;}}
}
