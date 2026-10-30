package dev.nlayer.android;

import android.app.Activity;
import android.app.Instrumentation;
import android.content.Context;
import android.content.Intent;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.os.Bundle;
import android.view.View;
import android.view.View.MeasureSpec;
import android.view.ViewGroup;
import android.widget.FrameLayout;
import android.widget.HorizontalScrollView;
import android.widget.ScrollView;
import java.io.File;
import java.io.FileOutputStream;
import java.lang.reflect.InvocationTargetException;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.Callable;
import java.util.concurrent.atomic.AtomicReference;
import org.json.JSONArray;
import org.json.JSONObject;

/**
 * The host's Java layer, on a device: mount batches, the scroll view, the input field and the image
 * view, as {@code nlayer test --target android} drives them through a real app. Run with {@code adb
 * shell am instrument -w -r dev.nlayer.android.test/dev.nlayer.android.JavaTests}; the report is a
 * JSON object in the {@code report} key. The test APK has the app's classes on its compile
 * classpath and, at run time, in the instrumented process, so package-private members are
 * reachable.
 */
public final class JavaTests extends Instrumentation {
  private static final String[] TESTS = {
    "testMountBatchBuildsTree",
    "testMountBatchFramesAndStyles",
    "testTransformOriginFollowsResize",
    "testMountBatchStateStyle",
    "testMountBatchFocusAncestors",
    "testMountBatchDelete",
    "testScrollAxisAndOffsets",
    "testScrollContentSizing",
    "testInputConfigure",
    "testInputMultiline",
    "testInputCompositionHoldsValue",
    "testInputValueRevisions",
    "testInputTypingCounts",
    "testInputReturnDismisses",
    "testImageFitModes",
    "testImageObjectPosition",
    "testImageRadiusClips",
    "testImageAssetLoads",
  };

  @Override
  public void onCreate(Bundle arguments) {
    super.onCreate(arguments);
    start();
  }

  @Override
  public void onStart() {
    List<String> failures = new ArrayList<>();
    for (String name : TESTS) {
      try {
        JavaTests.class.getDeclaredMethod(name).invoke(this);
      } catch (InvocationTargetException error) {
        failures.add(name + ": " + describe(error.getCause()));
      } catch (Throwable error) {
        failures.add(name + ": " + describe(error));
      }
    }
    Bundle result = new Bundle();
    try {
      JSONObject report = new JSONObject();
      report.put("passed", TESTS.length - failures.size());
      report.put("total", TESTS.length);
      JSONArray failed = new JSONArray();
      for (String failure : failures) failed.put(failure);
      report.put("failures", failed);
      result.putString("report", report.toString());
    } catch (Exception error) {
      result.putString(
          "report",
          "{\"passed\":0,\"total\":"
              + TESTS.length
              + ",\"failures\":["
              + JSONObject.quote(describe(error))
              + "]}");
    }
    finish(0, result);
  }

  private static String describe(Throwable error) {
    if (error == null) return "no cause";
    StringBuilder out = new StringBuilder(String.valueOf(error));
    for (StackTraceElement frame : error.getStackTrace()) {
      out.append("\n  at ").append(frame);
    }
    return out.toString();
  }

  private Context context() {
    return getTargetContext();
  }

  /** Runs {@code work} on the UI thread and returns its value, rethrowing its failure. */
  private <T> T main(Callable<T> work) {
    AtomicReference<T> result = new AtomicReference<>();
    AtomicReference<Throwable> failure = new AtomicReference<>();
    runOnMainSync(
        () -> {
          try {
            result.set(work.call());
          } catch (Throwable error) {
            failure.set(error);
          }
        });
    Throwable error = failure.get();
    if (error instanceof RuntimeException) throw (RuntimeException) error;
    if (error instanceof Error) throw (Error) error;
    if (error != null) throw new RuntimeException(error);
    return result.get();
  }

  private static void check(boolean ok, String message) {
    if (!ok) throw new AssertionError(message);
  }

  private static void equal(Object want, Object have, String what) {
    if (want == null ? have != null : !want.equals(have)) {
      throw new AssertionError(what + ": want " + want + ", have " + have);
    }
  }

  private HostView host() {
    return main(() -> new HostView(context()));
  }

  private static Batch batch() {
    return new Batch();
  }

  // --- mount batches -------------------------------------------------------

  void testMountBatchBuildsTree() {
    HostView host = host();
    Batch b = batch();
    b.word(0).word(2).word(0); // create a box
    b.word(0).word(3).word(3); // create a vertical scroll
    b.word(1).word(1).word(2).word(-1); // insert the box into the root
    b.word(1).word(1).word(3).word(-1); // insert the scroll into the root
    b.word(1).word(3).word(2).word(-1); // move the box into the scroll's content
    b.word(16).word(2).string("box");
    b.word(7).word(2).word(3).word(0).word(0).word(0); // press and scroll bits
    main(
        () -> {
          host.mounter.apply(b.bytes(), b.objects());
          return null;
        });

    Box box = (Box) host.mounter.views.get(2);
    ScrollBox scroll = (ScrollBox) host.mounter.views.get(3);
    check(box != null && scroll != null, "views were not created");
    check(box.getParent() == scroll.content, "a child of a scroll mounts into its content");
    check(scroll.getParent() == host, "the scroll mounts into the root");
    equal("box", host.mounter.testIds.get(2), "testID");
    equal(3, host.mounter.events.get(2), "event mask");
    check(box.isClickable(), "a press makes the view clickable");
  }

  void testMountBatchFramesAndStyles() {
    HostView host = host();
    float d = host.density;
    Batch b = batch();
    b.word(0).word(2).word(0);
    b.word(1).word(1).word(2).word(-1);
    b.word(4).word(2).real(4).real(6).real(20).real(10);
    b.word(5).word(2);
    style(b, 0xff112233, 8, 0.5f, true);
    main(
        () -> {
          host.mounter.apply(b.bytes(), b.objects());
          return null;
        });

    Box box = (Box) host.mounter.views.get(2);
    float[] frame = (float[]) box.getTag();
    equal(4f, frame[0], "frame x");
    equal(6f, frame[1], "frame y");
    equal(20f, frame[2], "frame width");
    equal(10f, frame[3], "frame height");
    ViewGroup.MarginLayoutParams lp = (ViewGroup.MarginLayoutParams) box.getLayoutParams();
    equal(Math.round(4 * d), lp.leftMargin, "layout left");
    equal(Math.round(6 * d), lp.topMargin, "layout top");
    equal(Math.round(20 * d), box.getWidth(), "laid-out width");
    equal(Math.round(10 * d), box.getHeight(), "laid-out height");
    equal(0.5f, box.getAlpha(), "opacity");
    equal(View.INVISIBLE, box.getVisibility(), "hidden paints invisible");
    equal(0xff112233, box.decor.fill, "fill");
    equal(8 * d, box.decor.radii[0], "radius in pixels");
    equal(box.getWidth() / 2f, box.getPivotX(), "default pivot follows the size");
  }

  void testTransformOriginFollowsResize() {
    HostView host = host();
    Batch b = batch();
    b.word(0).word(2).word(0);
    b.word(1).word(1).word(2).word(-1);
    b.word(4).word(2).real(0).real(0).real(20).real(10);
    b.word(5).word(2);
    styleOrigin(b, 0xff112233, 0, 1, false, 0.25f, 0.75f);
    main(
        () -> {
          host.mounter.apply(b.bytes(), b.objects());
          return null;
        });

    Box box = (Box) host.mounter.views.get(2);
    equal(box.getWidth() * 0.25f, box.getPivotX(), "percentage origin x");
    equal(box.getHeight() * 0.75f, box.getPivotY(), "percentage origin y");

    Batch resize = batch();
    resize.word(4).word(2).real(0).real(0).real(40).real(20);
    main(
        () -> {
          host.mounter.apply(resize.bytes(), resize.objects());
          return null;
        });
    equal(box.getWidth() * 0.25f, box.getPivotX(), "origin x follows a resize");
    equal(box.getHeight() * 0.75f, box.getPivotY(), "origin y follows a resize");
  }

  void testMountBatchStateStyle() {
    HostView host = host();
    Batch b = batch();
    b.word(0).word(2).word(0);
    b.word(1).word(1).word(2).word(-1);
    b.word(4).word(2).real(0).real(0).real(20).real(10);
    b.word(5).word(2);
    style(b, 0xff0000ff, 0, 1, false);
    b.word(12).word(2).word(0).word(1); // active style
    style(b, 0xffff0000, 0, 0.25f, false);
    b.bool(false); // no text color
    main(
        () -> {
          host.mounter.apply(b.bytes(), b.objects());
          return null;
        });

    Box box = (Box) host.mounter.views.get(2);
    check(!box.isPressed(), "not pressed at first");
    equal(1f, box.getAlpha(), "base opacity");
    main(
        () -> {
          box.setPressed(true);
          host.mounter.paint(2);
          return null;
        });
    equal(0.25f, box.getAlpha(), "active opacity");
    equal(0xffff0000, box.decor.fill, "active fill");
    main(
        () -> {
          box.setPressed(false);
          host.mounter.paint(2);
          return null;
        });
    equal(1f, box.getAlpha(), "base opacity returns");
    equal(0xff0000ff, box.decor.fill, "base fill returns");
  }

  void testMountBatchFocusAncestors() {
    Activity activity =
        startActivitySync(
            new Intent(context(), HostActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
    try {
      HostView host = main(() -> new HostView(activity));
      Batch b = batch();
      b.word(0).word(2).word(0); // a box ancestor with focusStyle
      b.word(0).word(4).word(0); // a box between it and the field
      b.word(0).word(3).word(4); // an input field inside the inner box
      b.word(1).word(1).word(2).word(-1); // root <- outer box
      b.word(1).word(2).word(4).word(-1); // outer <- inner box
      b.word(1).word(4).word(3).word(-1); // inner <- input
      b.word(4).word(2).real(0).real(0).real(200).real(100);
      b.word(4).word(4).real(10).real(10).real(180).real(80);
      b.word(4).word(3).real(0).real(0).real(200).real(40);
      b.word(5).word(2); // base paint
      style(b, 0xff0000ff, 0, 1, false);
      b.word(5).word(3);
      style(b, 0xffffffff, 0, 1, false);
      b.word(12).word(2).word(2).word(1); // the ancestor's focus paint
      style(b, 0, 0, 0.25f, false);
      b.bool(false);
      b.word(12).word(3).word(2).word(1); // the field's own focus paint
      style(b, 0xffff0000, 0, 1, false);
      b.bool(false);
      main(
          () -> {
            activity.setContentView(host);
            host.mounter.apply(b.bytes(), b.objects());
            return null;
          });
      waitForIdleSync();
      Box box = (Box) host.mounter.views.get(2);
      InputBox input = (InputBox) host.mounter.views.get(3);
      check(box != null && input != null, "views were not created");
      // A window in non-touch mode may focus the first field as it attaches.
      main(
          () -> {
            host.requestFocus();
            return null;
          });
      waitForIdleSync();
      check(!host.mounter.chainFocused(box), "no field has focus yet");
      equal(1f, box.getAlpha(), "base opacity");
      equal(0xff0000ff, box.decor.fill, "base fill");

      main(
          () -> {
            input.edit.requestFocusFromTouch();
            return null;
          });
      waitForIdleSync();
      check(input.edit.isFocused(), "the field took focus");
      check(host.mounter.chainFocused(box), "the ancestor sees the field");
      equal(0.25f, box.getAlpha(), "ancestor focus opacity");
      equal(0xffff0000, input.decor.fill, "field focus fill");
      String tree = main(() -> Tree.snapshot(host));
      check(focused(tree, 2), "the tree reports the ancestor focused");
      check(focused(tree, 3), "the tree reports the field focused");

      main(
          () -> {
            input.edit.clearFocus();
            return null;
          });
      waitForIdleSync();
      check(!host.mounter.chainFocused(box), "the ancestor no longer sees focus");
      check(!input.edit.isFocused(), "the field lost focus");
      equal(1f, box.getAlpha(), "base opacity returns");
      equal(0xff0000ff, box.decor.fill, "base fill returns");
      tree = main(() -> Tree.snapshot(host));
      check(!focused(tree, 2), "the tree clears the ancestor focus");
    } finally {
      main(
          () -> {
            activity.finish();
            return null;
          });
      waitForIdleSync();
    }
  }

  /** Whether the tree snapshot marks node {@code id} focused. */
  private static boolean focused(String tree, int id) {
    try {
      JSONArray nodes = new JSONObject(tree).getJSONArray("nodes");
      for (int i = 0; i < nodes.length(); i++) {
        JSONObject node = nodes.getJSONObject(i);
        if (node.optInt("id") == id) return node.optBoolean("focused");
      }
    } catch (Exception error) {
      throw new RuntimeException(error);
    }
    throw new AssertionError("no node " + id + " in " + tree);
  }

  void testMountBatchDelete() {
    HostView host = host();
    Batch b = batch();
    b.word(0).word(2).word(0);
    b.word(1).word(1).word(2).word(-1);
    b.word(16).word(2).string("gone");
    b.word(3).word(2);
    main(
        () -> {
          host.mounter.apply(b.bytes(), b.objects());
          return null;
        });
    check(host.mounter.views.get(2) == null, "delete drops the view");
    check(host.mounter.testIds.get(2) == null, "delete drops the testID");
    check(host.getChildCount() == 0, "delete removes it from its parent");
    check(host.mounter.views.get(1) == host, "the root survives");
  }

  // --- ScrollBox -----------------------------------------------------------

  void testScrollAxisAndOffsets() {
    HostView host = host();
    float d = host.density;
    Batch b = batch();
    b.word(0).word(3).word(3);
    b.word(1).word(1).word(3).word(-1);
    b.word(4).word(3).real(0).real(0).real(200).real(300);
    b.word(10).word(3).real(600).real(900);
    main(
        () -> {
          host.mounter.apply(b.bytes(), b.objects());
          return null;
        });
    ScrollBox box = (ScrollBox) host.mounter.views.get(3);
    main(
        () -> {
          box.measure(
              MeasureSpec.makeMeasureSpec(Math.round(200 * d), MeasureSpec.EXACTLY),
              MeasureSpec.makeMeasureSpec(Math.round(300 * d), MeasureSpec.EXACTLY));
          box.layout(0, 0, Math.round(200 * d), Math.round(300 * d));
          return null;
        });
    check(box.scroller instanceof ScrollView, "vertical by default");
    check(!box.horizontal, "not horizontal by default");
    equal(600f, box.contentPointsW, "content width");
    equal(900f, box.contentPointsH, "content height");

    main(
        () -> {
          box.scrollToPoints(40, 100, d);
          return null;
        });
    equal(0, box.scroller.getScrollX(), "a vertical scroll leaves x at zero");
    equal(Math.round(100 * d), box.scroller.getScrollY(), "y offset in pixels");
    equal(40f, box.offsetX, "x offset in points");
    equal(100f, box.offsetY, "y offset in points");

    main(
        () -> {
          box.configure(true, 0, 0, true, context(), host.mounter);
          box.measure(
              MeasureSpec.makeMeasureSpec(Math.round(200 * d), MeasureSpec.EXACTLY),
              MeasureSpec.makeMeasureSpec(Math.round(300 * d), MeasureSpec.EXACTLY));
          box.layout(0, 0, Math.round(200 * d), Math.round(300 * d));
          return null;
        });
    check(box.scroller instanceof HorizontalScrollView, "horizontal after configure");
    check(box.horizontal, "horizontal flag");
    equal(600f, box.contentPointsW, "content width survives the rebuild");

    main(
        () -> {
          box.scrollToPoints(40, 100, d);
          return null;
        });
    equal(Math.round(40 * d), box.scroller.getScrollX(), "horizontal x offset");
    equal(0, box.scroller.getScrollY(), "a horizontal scroll leaves y at zero");
  }

  void testScrollContentSizing() {
    HostView host = host();
    float d = host.density;
    Batch b = batch();
    b.word(0).word(3).word(3);
    b.word(1).word(1).word(3).word(-1);
    b.word(4).word(3).real(0).real(0).real(200).real(300);
    b.word(10).word(3).real(150).real(200);
    main(
        () -> {
          host.mounter.apply(b.bytes(), b.objects());
          return null;
        });
    ScrollBox box = (ScrollBox) host.mounter.views.get(3);
    equal(150f, box.contentPointsW, "content points are the app's size");
    equal(200f, box.contentPointsH, "content points are the app's size");
    equal(Math.round(200 * d), box.content.getWidth(), "content is at least the viewport wide");
    equal(Math.round(300 * d), box.content.getHeight(), "content is at least the viewport tall");
  }

  // --- InputBox ------------------------------------------------------------

  void testInputConfigure() {
    HostView host = host();
    InputBox box = main(() -> new InputBox(context(), 5));
    main(
        () -> {
          box.configure(
              16,
              700,
              true,
              0xff112233,
              1,
              20,
              3,
              "Name",
              true,
              1,
              true,
              true,
              false,
              host.density);
          return null;
        });
    equal("Name", box.placeholder, "placeholder");
    equal("email", box.keyboardName, "keyboard name");
    check(box.secure, "secure");
    check(box.disabled, "disabled");
    check(!box.edit.isEnabled(), "the field follows disabled");
    equal("Name", box.edit.getHint().toString(), "hint");
    equal(0x99112233, box.edit.getCurrentHintTextColor(), "muted hint");
    equal(0xff112233, box.edit.getCurrentTextColor(), "text color");
    check(
        (box.edit.getInputType() & android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD) != 0,
        "password variation");
    equal(16 * host.density, box.edit.getTextSize(), "text size in pixels");
    equal(View.LAYOUT_DIRECTION_RTL, box.edit.getLayoutDirection(), "rtl");
  }

  void testInputMultiline() {
    HostView host = host();
    InputBox box = main(() -> new InputBox(context(), 5));
    main(
        () -> {
          box.configure(
              16,
              400,
              false,
              0xff000000,
              0,
              0,
              0,
              "Body",
              false,
              0,
              false,
              false,
              true,
              host.density);
          box.setValue(0, "a\nb");
          box.setSelection(1, 9);
          return null;
        });
    check(
        (box.edit.getInputType() & android.text.InputType.TYPE_TEXT_FLAG_MULTI_LINE) != 0,
        "multiline input type");
    check(!box.edit.isSingleLine(), "not single line");
    equal("a\nb", box.edit.getText().toString(), "the text keeps its break");
    equal(1, box.edit.getSelectionStart(), "the selection start");
    equal(3, box.edit.getSelectionEnd(), "a selection clamps its end to the text");
  }

  void testInputCompositionHoldsValue() {
    HostView host = host();
    InputBox box = main(() -> new InputBox(context(), 5));
    main(
        () -> {
          box.configure(
              16, 400, false, 0xff000000, 0, 0, 0, "", false, 0, false, false, true, host.density);
          box.setValue(0, "a");
          box.inComposition = true;
          box.setValue(1, "held");
          return null;
        });
    equal("a", box.edit.getText().toString(), "a value from JS waits for the composition");
    main(
        () -> {
          box.inComposition = false;
          box.applyPending();
          return null;
        });
    equal("held", box.edit.getText().toString(), "the held value applies once it ends");
  }

  void testInputValueRevisions() {
    InputBox box = main(() -> new InputBox(context(), 5));
    main(
        () -> {
          box.setValue(0, "first");
          return null;
        });
    equal("first", box.edit.getText().toString(), "first value");
    equal(0, box.count, "count follows the value");
    main(
        () -> {
          box.setValue(1, "second");
          return null;
        });
    equal("second", box.edit.getText().toString(), "second value");
    equal(1, box.count, "newer count accepted");
    main(
        () -> {
          box.setValue(0, "stale");
          return null;
        });
    equal("second", box.edit.getText().toString(), "stale value ignored");
    equal(1, box.count, "stale count ignored");
  }

  void testInputTypingCounts() {
    InputBox box = main(() -> new InputBox(context(), 5));
    main(
        () -> {
          box.edit.setText("typed");
          return null;
        });
    equal("typed", box.edit.getText().toString(), "typed text");
    check(box.count >= 1, "typing counts a change");
    check(!box.setting, "typing is not a programmatic set");
  }

  void testInputReturnDismisses() {
    Activity activity =
        startActivitySync(
            new Intent(context(), HostActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
    try {
      int[] keyboards = {0, 1, 2};
      String[] names = {"text", "email", "number"};
      boolean[] resigns = {true, true, false};
      InputBox[] boxes = new InputBox[keyboards.length];
      HostView host =
          main(
              () -> {
                HostView view =
                    (HostView)
                        ((FrameLayout) activity.findViewById(android.R.id.content)).getChildAt(0);
                for (int i = 0; i < boxes.length; i++) {
                  InputBox box = new InputBox(activity, 10 + i);
                  box.configure(
                      16,
                      400,
                      false,
                      0xff000000,
                      0,
                      0,
                      0,
                      "",
                      false,
                      keyboards[i],
                      false,
                      false,
                      false,
                      view.density);
                  view.addView(box, new FrameLayout.LayoutParams(200, 40));
                  boxes[i] = box;
                }
                return view;
              });

      for (int i = 0; i < boxes.length; i++) {
        InputBox box = boxes[i];
        main(
            () -> {
              box.edit.requestFocus();
              return null;
            });
        check(main(() -> box.edit.hasFocus()), names[i] + " field took focus");
        main(
            () -> {
              box.edit.onEditorAction(android.view.inputmethod.EditorInfo.IME_ACTION_DONE);
              return null;
            });
        equal(!resigns[i], main(() -> box.edit.hasFocus()), names[i] + " focus after Return");
        if (resigns[i]) {
          check(main(() -> host.findFocus() != box.edit), names[i] + " keyboard focus");
        }
      }
    } finally {
      main(
          () -> {
            activity.finish();
            return null;
          });
      waitForIdleSync();
    }
  }

  // --- ImageBox ------------------------------------------------------------

  private static Bitmap image(int color) {
    Bitmap bitmap = Bitmap.createBitmap(4, 2, Bitmap.Config.ARGB_8888);
    bitmap.eraseColor(color);
    return bitmap;
  }

  private Bitmap draw(ImageBox box, int size) {
    return main(
        () -> {
          box.measure(
              MeasureSpec.makeMeasureSpec(size, MeasureSpec.EXACTLY),
              MeasureSpec.makeMeasureSpec(size, MeasureSpec.EXACTLY));
          box.layout(0, 0, size, size);
          Bitmap out = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888);
          box.draw(new Canvas(out));
          return out;
        });
  }

  private static void hasColor(Bitmap bitmap, int x, int y, int color, String what) {
    equal(color, bitmap.getPixel(x, y), what + " at " + x + "," + y);
  }

  void testImageFitModes() {
    Bitmap source = image(Color.RED);
    ImageBox box = main(() -> new ImageBox(context(), 6));
    main(
        () -> {
          box.bitmap = source;
          box.mode = 0;
          return null;
        });
    Bitmap cover = draw(box, 20);
    hasColor(cover, 0, 0, Color.RED, "cover fills the box");
    hasColor(cover, 19, 19, Color.RED, "cover fills the box");

    main(
        () -> {
          box.mode = 1;
          return null;
        });
    Bitmap contain = draw(box, 20);
    hasColor(contain, 10, 0, Color.TRANSPARENT, "contain leaves the top free");
    hasColor(contain, 10, 10, Color.RED, "contain centers the image");

    main(
        () -> {
          box.mode = 3;
          return null;
        });
    Bitmap none = draw(box, 20);
    hasColor(none, 0, 0, Color.TRANSPARENT, "none keeps the source size");
    hasColor(none, 9, 10, Color.RED, "none centers the image");
  }

  void testImageObjectPosition() {
    Bitmap source = image(Color.RED);
    ImageBox box = main(() -> new ImageBox(context(), 6));
    main(
        () -> {
          box.bitmap = source;
          box.mode = 1;
          box.setPosition(false, 1, 0, true, 0, 0);
          return null;
        });
    Bitmap end = draw(box, 20);
    hasColor(end, 10, 2, Color.TRANSPARENT, "end-aligned image leaves the top free");
    hasColor(end, 10, 15, Color.RED, "end-aligned image sits at the bottom");

    main(
        () -> {
          box.setPosition(false, 1, 0, false, 0, 0);
          return null;
        });
    Bitmap start = draw(box, 20);
    hasColor(start, 10, 2, Color.RED, "start-aligned image sits at the top");
    hasColor(start, 10, 18, Color.TRANSPARENT, "start-aligned image leaves the bottom free");
  }

  void testImageRadiusClips() {
    Bitmap source = image(Color.RED);
    ImageBox box = main(() -> new ImageBox(context(), 6));
    main(
        () -> {
          box.bitmap = source;
          box.mode = 0;
          for (int i = 0; i < 4; i++) box.decor.radii[i] = 10;
          return null;
        });
    Bitmap clipped = draw(box, 20);
    hasColor(clipped, 0, 0, Color.TRANSPARENT, "the rounded corner is clipped");
    hasColor(clipped, 10, 10, Color.RED, "the middle is painted");
  }

  void testImageAssetLoads() throws Exception {
    Bitmap source = image(Color.RED);
    File file = new File(context().getFilesDir(), "java-tests.png");
    try (FileOutputStream out = new FileOutputStream(file)) {
      check(source.compress(Bitmap.CompressFormat.PNG, 100, out), "wrote the image");
    }
    // View.post queues on a detached view; a window is what runs it.
    Activity activity =
        startActivitySync(
            new Intent(context(), HostActivity.class).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
    try {
      ImageBox box = main(() -> new ImageBox(activity, 6));
      main(
          () -> {
            HostView host =
                (HostView)
                    ((FrameLayout) activity.findViewById(android.R.id.content)).getChildAt(0);
            host.addView(box, new FrameLayout.LayoutParams(20, 20));
            box.load("asset:/java-tests.png", 0, context().getFilesDir().getAbsolutePath());
            return null;
          });
      long deadline = System.currentTimeMillis() + 5000;
      Bitmap decoded = main(() -> box.bitmap);
      while (decoded == null && System.currentTimeMillis() < deadline) {
        Thread.sleep(20);
        decoded = main(() -> box.bitmap);
      }
      check(decoded != null, "asset image decoded (file=" + file.length() + ")");
      equal(4, decoded.getWidth(), "decoded width");
      equal(2, decoded.getHeight(), "decoded height");
      equal("asset:/java-tests.png", box.uri, "source URI");
    } finally {
      main(
          () -> {
            activity.finish();
            return null;
          });
      waitForIdleSync();
      file.delete();
    }
  }

  // --- mount batch words ---------------------------------------------------

  /** The little-endian words {@link Mounter} reads, with its object table. */
  private static final class Batch {
    private byte[] bytes = new byte[256];
    private int used;
    private final List<String> strings = new ArrayList<>();

    Batch word(int value) {
      ensure(4);
      write(value & 0xff);
      write((value >>> 8) & 0xff);
      write((value >>> 16) & 0xff);
      write((value >>> 24) & 0xff);
      return this;
    }

    Batch real(float value) {
      return word(Float.floatToRawIntBits(value));
    }

    Batch bool(boolean value) {
      return word(value ? 1 : 0);
    }

    Batch string(String value) {
      strings.add(value);
      return word(strings.size() - 1);
    }

    private void ensure(int more) {
      if (used + more <= bytes.length) return;
      byte[] bigger = new byte[Math.max(bytes.length * 2, used + more)];
      System.arraycopy(bytes, 0, bigger, 0, used);
      bytes = bigger;
    }

    private void write(int value) {
      bytes[used++] = (byte) value;
    }

    byte[] bytes() {
      byte[] out = new byte[used];
      System.arraycopy(bytes, 0, out, 0, used);
      return out;
    }

    Object[] objects() {
      return strings.toArray();
    }
  }

  /**
   * A view style as {@code encode.rs} writes it: fill, four radii, continuous, four border widths,
   * four border colors, border style, opacity, overflow, eight transform values, hidden, both
   * transform-origin lengths and both object positions, and no transform ops.
   */
  private static void style(Batch b, int fill, float radius, float opacity, boolean hidden) {
    styleOrigin(b, fill, radius, opacity, hidden, 0.5f, 0.5f);
  }

  private static void styleOrigin(
      Batch b, int fill, float radius, float opacity, boolean hidden, float ox, float oy) {
    b.word(fill);
    for (int i = 0; i < 4; i++) b.real(radius);
    b.bool(false);
    for (int i = 0; i < 4; i++) b.real(0);
    for (int i = 0; i < 4; i++) b.word(0);
    b.word(0);
    b.real(opacity);
    b.bool(false);
    b.real(0).real(0).real(1).real(1).real(0).real(0).real(0).real(0);
    b.bool(hidden);
    length(b, ox);
    length(b, oy);
    position(b, false, 0.5f);
    position(b, false, 0.5f);
    b.word(0);
  }

  private static void length(Batch b, float fraction) {
    b.word(1).real(fraction);
  }

  private static void position(Batch b, boolean end, float fraction) {
    b.bool(end);
    length(b, fraction);
  }
}
