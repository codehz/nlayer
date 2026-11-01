package dev.nlayer.android;

import android.content.Context;
import android.view.MotionEvent;
import android.view.VelocityTracker;
import android.view.ViewConfiguration;

/**
 * Gesture tracking for Box views. Transplanted from nsh ca59368b Box.java lines 395-648.
 * Handles pan (with axis locking), pinch, and rotation gestures.
 */
final class Gestures {
  private final Box view;
  private final int id;
  private final Mounter mounter;
  
  // Gesture state (nsh Box.java lines 395-407)
  private boolean panning, tracking, transforming;
  private VelocityTracker velocity;
  private float downRawX, downRawY;
  private float pressX, pressY;
  
  // Two-finger gesture: start span/angle, last values for velocity (nsh lines 403-407)
  private float startSpan, startAngle, lastScale, lastAngle;
  private long lastTime;
  private float scaleVelocity, angleVelocity;

  Gestures(Box view, int id, Mounter mounter) {
    this.view = view;
    this.id = id;
    this.mounter = mounter;
  }

  // nsh Box.java line 416-423
  private boolean pannable() {
    int mask = mounter.events.get(id, 0);
    return ((mask >> 17) & 1) != 0 || ((mask >> 18) & 1) != 0 || mounter.panBindings.get(id, 0) != 0;
  }

  private boolean twoFinger() {
    int mask = mounter.events.get(id, 0);
    boolean pinchable = mounter.pinchBindings.get(id, false) || ((mask >> 19) & 1) != 0 || ((mask >> 20) & 1) != 0;
    boolean rotatable = mounter.rotationBindings.get(id, false) || ((mask >> 21) & 1) != 0 || ((mask >> 22) & 1) != 0;
    return pinchable || rotatable;
  }

  // nsh Box.java lines 425-433: track velocity in screen coordinates
  private void track(MotionEvent e) {
    if (velocity == null) return;
    MotionEvent screen = MotionEvent.obtain(e);
    screen.setLocation(e.getRawX(), e.getRawY());
    velocity.addMovement(screen);
    screen.recycle();
  }

  // nsh Box.java lines 435-446: pan starts check with axis gating
  private boolean panStarts(float dx, float dy, float slop) {
    if (Math.hypot(dx, dy) <= slop) return false;
    int panAxis = view.panAxis;
    if (panAxis == 1) return Math.abs(dx) > Math.abs(dy); // horizontal
    if (panAxis == 2) return Math.abs(dy) > Math.abs(dx); // vertical
    return true; // free
  }

  // Check if inside a ScrollView (for press delay)
  private boolean insideScrollView() {
    android.view.ViewParent p = view.getParent();
    while (p != null) {
      if (p instanceof android.widget.ScrollView || p instanceof android.widget.HorizontalScrollView)
        return true;
      p = p.getParent();
    }
    return false;
  }

  // nsh Box.java lines 460-507: intercept to start pan on child
  boolean onInterceptTouchEvent(MotionEvent e) {
    int action = e.getActionMasked();
    if (action == MotionEvent.ACTION_DOWN) {
      downRawX = e.getRawX();
      downRawY = e.getRawY();
    }
    if (action == MotionEvent.ACTION_POINTER_DOWN && twoFinger() && e.getPointerCount() == 2) {
      // Second finger: take over for pinch/rotation
      tracking = false;
      return true;
    }
    // Take over drag that starts on child
    if (pannable() && action == MotionEvent.ACTION_MOVE && !panning) {
      float slop = ViewConfiguration.get(view.getContext()).getScaledTouchSlop();
      float dx = e.getRawX() - downRawX, dy = e.getRawY() - downRawY;
      if (panStarts(dx, dy, slop)) {
        // DOWN went to child: pan starts with this move
        tracking = false;
        if (velocity != null) velocity.recycle();
        velocity = VelocityTracker.obtain();
        track(e);
        panning = true;
        view.getParent().requestDisallowInterceptTouchEvent(true);
        float density = mounter.density;
        startPan(dx / density, dy / density);
        return true;
      }
    }
    return false;
  }

  // nsh Box.java lines 510-648: main touch handling
  boolean onTouchEvent(MotionEvent e) {
    boolean pannable = pannable(), twoFinger = twoFinger();
    if (!pannable && !twoFinger) return false;
    
    float density = mounter.density;
    switch (e.getActionMasked()) {
      case MotionEvent.ACTION_DOWN:
        downRawX = e.getRawX();
        downRawY = e.getRawY();
        tracking = true;
        panning = false;
        transforming = false;
        if (velocity != null) velocity.recycle();
        velocity = VelocityTracker.obtain();
        track(e);
        pressX = e.getX();
        pressY = e.getY();
        return true;

      case MotionEvent.ACTION_POINTER_DOWN:
        if (twoFinger && e.getPointerCount() == 2) {
          tracking = false;
          panning = false;
          transforming = true;
          float dx = e.getX(1) - e.getX(0);
          float dy = e.getY(1) - e.getY(0);
          startSpan = (float) Math.hypot(dx, dy);
          startAngle = (float) Math.toDegrees(Math.atan2(dy, dx));
          lastScale = 1.0f;
          lastAngle = 0.0f;
          lastTime = e.getEventTime();
          scaleVelocity = 0.0f;
          angleVelocity = 0.0f;
          Native.pinchStart(mounter.host.handle(), id);
          Native.rotationStart(mounter.host.handle(), id);
        }
        return true;

      case MotionEvent.ACTION_MOVE:
        track(e);
        if (panning) {
          float dx = e.getRawX() - downRawX, dy = e.getRawY() - downRawY;
          Native.panMove(mounter.host.handle(), id, dx / density, dy / density);
          mounter.host.flushAnimation();
        } else if (transforming && e.getPointerCount() >= 2) {
          updateTransform(e);
        } else if (tracking && !panning && pannable) {
          float slop = ViewConfiguration.get(view.getContext()).getScaledTouchSlop();
          float dx = e.getRawX() - downRawX, dy = e.getRawY() - downRawY;
          if (panStarts(dx, dy, slop)) {
            panning = true;
            tracking = false;
            view.getParent().requestDisallowInterceptTouchEvent(true);
            startPan(dx / density, dy / density);
          }
        }
        return true;

      case MotionEvent.ACTION_POINTER_UP:
        if (transforming && e.getPointerCount() == 2) {
          updateTransform(e);
          endTransform(1);
        }
        return true;

      case MotionEvent.ACTION_UP:
        track(e);
        if (panning) {
          endPan(e);
        } else if (transforming) {
          endTransform(1);
        }
        tracking = false;
        return true;

      case MotionEvent.ACTION_CANCEL:
        if (panning) endPan(e);
        if (transforming) endTransform(2);
        tracking = false;
        panning = false;
        transforming = false;
        return true;
    }
    return false;
  }

  // nsh Box.java lines 560-576: start pan
  private void startPan(float dx, float dy) {
    Native.panStart(mounter.host.handle(), id);
    Native.panMove(mounter.host.handle(), id, dx, dy);
    mounter.host.flushAnimation();
  }

  // nsh Box.java lines 578-605: end pan with velocity
  private void endPan(MotionEvent e) {
    if (!panning) return;
    panning = false;
    float density = mounter.density;
    float dx = e.getRawX() - downRawX, dy = e.getRawY() - downRawY;
    
    // Velocity: pixels/sec -> points/sec (nsh line 587-593)
    velocity.computeCurrentVelocity(1000);
    float vx = velocity.getXVelocity() / density;
    float vy = velocity.getYVelocity() / density;
    
    Native.panEnd(mounter.host.handle(), id, dx / density, dy / density, vx, vy);
    
    if (velocity != null) {
      velocity.recycle();
      velocity = null;
    }
  }

  // nsh Box.java lines 607-635: update transform with span/angle
  private void updateTransform(MotionEvent e) {
    if (e.getPointerCount() < 2) return;
    
    float dx = e.getX(1) - e.getX(0);
    float dy = e.getY(1) - e.getY(0);
    float span = (float) Math.hypot(dx, dy);
    float angle = (float) Math.toDegrees(Math.atan2(dy, dx));
    
    float scale = span / startSpan;
    float rotation = angle - startAngle;
    // Normalize to [-180, 180]
    while (rotation > 180) rotation -= 360;
    while (rotation < -180) rotation += 360;
    
    // Velocity: change per millisecond -> change per second (nsh lines 621-625)
    long now = e.getEventTime();
    float dt = Math.max(1, now - lastTime) / 1000.0f;
    scaleVelocity = (scale - lastScale) / dt;
    angleVelocity = (rotation - lastAngle) / dt;
    
    lastScale = scale;
    lastAngle = rotation;
    lastTime = now;
    
    Native.pinchMove(mounter.host.handle(), id, scale);
    Native.rotationMove(mounter.host.handle(), id, rotation);
    mounter.host.flushAnimation();
  }

  // nsh Box.java lines 637-648: end transform
  private void endTransform(int state) {
    if (!transforming) return;
    transforming = false;
    
    Native.pinchEnd(mounter.host.handle(), id, lastScale, scaleVelocity);
    Native.rotationEnd(mounter.host.handle(), id, lastAngle, angleVelocity);
  }

  void reset() {
    if (velocity != null) {
      velocity.recycle();
      velocity = null;
    }
    panning = false;
    tracking = false;
    transforming = false;
  }
}
