package dev.nlayer.android;

import android.content.Context;
import android.view.GestureDetector;
import android.view.MotionEvent;
import android.view.ScaleGestureDetector;
import android.view.VelocityTracker;

/**
 * Gesture detection for pan, pinch, and rotation gestures. Attached to Box views that have
 * gesture bindings. Based on nsh's gesture detection pattern.
 */
final class GestureBox {
  private final Box view;
  private final int id;
  private final Mounter mounter;
  private final GestureDetector panDetector;
  private final ScaleGestureDetector scaleDetector;
  private VelocityTracker velocityTracker;
  
  private boolean panActive, pinchActive, rotationActive;
  private float panStartX, panStartY;
  private float initialAngle;
  private long lastRotationTime;

  GestureBox(Box view, int id, Mounter mounter) {
    this.view = view;
    this.id = id;
    this.mounter = mounter;
    
    Context ctx = view.getContext();
    panDetector = new GestureDetector(ctx, new PanListener());
    scaleDetector = new ScaleGestureDetector(ctx, new ScaleListener());
  }

  boolean onTouchEvent(MotionEvent e) {
    int action = e.getActionMasked();
    int pointerCount = e.getPointerCount();
    
    // Track velocity for pan gestures
    if (velocityTracker == null) {
      velocityTracker = VelocityTracker.obtain();
    }
    velocityTracker.addMovement(e);
    
    // Handle rotation (two-finger twist)
    if (pointerCount == 2 && hasRotation()) {
      handleRotation(e, action);
    }
    
    // Handle pinch (two-finger scale)
    if (pointerCount >= 2 && hasPinch()) {
      scaleDetector.onTouchEvent(e);
    }
    
    // Handle pan (one or two finger drag)
    if (hasPan()) {
      panDetector.onTouchEvent(e);
    }
    
    // End gestures on pointer up or cancel
    if (action == MotionEvent.ACTION_UP || action == MotionEvent.ACTION_CANCEL) {
      endGestures(e);
    }
    
    return panActive || pinchActive || rotationActive;
  }

  private void handleRotation(MotionEvent e, int action) {
    if (e.getPointerCount() < 2) return;
    
    float dx = e.getX(1) - e.getX(0);
    float dy = e.getY(1) - e.getY(0);
    float angle = (float) Math.toDegrees(Math.atan2(dy, dx));
    
    if (action == MotionEvent.ACTION_POINTER_DOWN && !rotationActive) {
      rotationActive = true;
      initialAngle = angle;
      lastRotationTime = e.getEventTime();
      Native.rotationStart(mounter.host.handle(), id);
    } else if (rotationActive && action == MotionEvent.ACTION_MOVE) {
      float delta = angle - initialAngle;
      // Normalize to [-180, 180]
      while (delta > 180) delta -= 360;
      while (delta < -180) delta += 360;
      Native.rotationMove(mounter.host.handle(), id, delta);
      lastRotationTime = e.getEventTime();
    }
  }

  private void endGestures(MotionEvent e) {
    float d = mounter.density;
    
    if (panActive) {
      velocityTracker.computeCurrentVelocity(1000);
      float vx = velocityTracker.getXVelocity() / d;
      float vy = velocityTracker.getYVelocity() / d;
      float tx = (e.getX() - panStartX) / d;
      float ty = (e.getY() - panStartY) / d;
      Native.panEnd(mounter.host.handle(), id, tx, ty, vx, vy);
      panActive = false;
    }
    
    if (pinchActive) {
      // Estimate velocity from recent scale changes
      velocityTracker.computeCurrentVelocity(1000);
      float velocity = 0.0f; // Scale velocity is complex; approximate as 0
      float scale = 1.0f; // Final scale from last move
      Native.pinchEnd(mounter.host.handle(), id, scale, velocity);
      pinchActive = false;
    }
    
    if (rotationActive) {
      float velocity = 0.0f; // Approximate rotation velocity
      float angle = 0.0f; // Final angle from last move
      Native.rotationEnd(mounter.host.handle(), id, angle, velocity);
      rotationActive = false;
    }
    
    if (velocityTracker != null) {
      velocityTracker.recycle();
      velocityTracker = null;
    }
  }

  private boolean hasPan() {
    return Native.hasPan(mounter.host.handle(), id);
  }

  private boolean hasPinch() {
    int mask = mounter.events.get(id, 0);
    return (mask & (1 << 19)) != 0 || (mask & (1 << 20)) != 0; // PinchStart or PinchEnd
  }

  private boolean hasRotation() {
    int mask = mounter.events.get(id, 0);
    return (mask & (1 << 21)) != 0 || (mask & (1 << 22)) != 0; // RotationStart or RotationEnd
  }

  private final class PanListener extends GestureDetector.SimpleOnGestureListener {
    @Override
    public boolean onDown(MotionEvent e) {
      if (hasPan()) {
        panStartX = e.getX();
        panStartY = e.getY();
        panActive = true;
        Native.panStart(mounter.host.handle(), id);
        return true;
      }
      return false;
    }

    @Override
    public boolean onScroll(MotionEvent e1, MotionEvent e2, float distanceX, float distanceY) {
      if (panActive && e2 != null) {
        float d = mounter.density;
        float tx = (e2.getX() - panStartX) / d;
        float ty = (e2.getY() - panStartY) / d;
        Native.panMove(mounter.host.handle(), id, tx, ty);
        return true;
      }
      return false;
    }
  }

  private final class ScaleListener extends ScaleGestureDetector.SimpleOnScaleGestureListener {
    @Override
    public boolean onScaleBegin(ScaleGestureDetector detector) {
      if (hasPinch()) {
        pinchActive = true;
        Native.pinchStart(mounter.host.handle(), id);
        return true;
      }
      return false;
    }

    @Override
    public boolean onScale(ScaleGestureDetector detector) {
      if (pinchActive) {
        float scale = detector.getScaleFactor();
        Native.pinchMove(mounter.host.handle(), id, scale);
        return true;
      }
      return false;
    }

    @Override
    public void onScaleEnd(ScaleGestureDetector detector) {
      // Handled in endGestures
    }
  }
}
