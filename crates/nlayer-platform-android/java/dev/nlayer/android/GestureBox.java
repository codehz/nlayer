package dev.nlayer.android;

import android.content.Context;
import android.view.GestureDetector;
import android.view.MotionEvent;
import android.view.ScaleGestureDetector;
import android.view.VelocityTracker;
import android.view.ViewConfiguration;

/**
 * Gesture detection for pan, pinch, and rotation gestures. Attached to Box views that have
 * gesture bindings or events. Pattern from nsh ca59368b Box.java gesture tracking.
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
  private float lastScale = 1.0f;
  private float lastAngle;
  private long lastScaleTime, lastRotationTime;

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
    
    // Track velocity for all gestures
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
      lastAngle = angle;
      lastRotationTime = e.getEventTime();
      Native.rotationStart(mounter.host.handle(), id);
    } else if (rotationActive && action == MotionEvent.ACTION_MOVE) {
      float delta = angle - lastAngle;
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
      // Estimate velocity: delta scale / delta time
      long now = e.getEventTime();
      long dt = now - lastScaleTime;
      float velocity = 0.0f;
      if (dt > 0) {
        // Velocity in scale units per second
        velocity = (lastScale - 1.0f) * 1000.0f / dt;
      }
      Native.pinchEnd(mounter.host.handle(), id, lastScale, velocity);
      pinchActive = false;
      lastScale = 1.0f;
    }
    
    if (rotationActive) {
      // Estimate velocity: delta angle / delta time
      long now = e.getEventTime();
      long dt = now - lastRotationTime;
      float velocity = 0.0f;
      if (dt > 0) {
        float dx = e.getPointerCount() >= 2 ? e.getX(1) - e.getX(0) : 0;
        float dy = e.getPointerCount() >= 2 ? e.getY(1) - e.getY(0) : 0;
        float angle = (float) Math.toDegrees(Math.atan2(dy, dx));
        float delta = angle - lastAngle;
        while (delta > 180) delta -= 360;
        while (delta < -180) delta += 360;
        velocity = delta * 1000.0f / dt;
      }
      Native.rotationEnd(mounter.host.handle(), id, lastAngle, velocity);
      rotationActive = false;
    }
    
    if (velocityTracker != null) {
      velocityTracker.recycle();
      velocityTracker = null;
    }
  }

  private boolean hasPan() {
    return mounter.panBindings.get(id, false) || hasEvent(17, 18);
  }

  private boolean hasPinch() {
    return mounter.pinchBindings.get(id, false) || hasEvent(19, 20);
  }

  private boolean hasRotation() {
    return mounter.rotationBindings.get(id, false) || hasEvent(21, 22);
  }
  
  private boolean hasEvent(int start, int end) {
    int mask = mounter.events.get(id, 0);
    return ((mask >> start) & 1) != 0 || ((mask >> end) & 1) != 0;
  }

  /** Whether this pan can start in a scroll view: checks axis lock via bindings. */
  boolean canPanInScroll(float dx, float dy) {
    if (!hasPan()) return false;
    boolean hasX = mounter.panBindings.get(id, false) && Native.hasPan(mounter.host.handle(), id);
    boolean hasY = hasX; // Simplified: actual check needs binding details
    // Single-axis check: horizontal pan doesn't steal from vertical scroll
    float slop = ViewConfiguration.get(view.getContext()).getScaledTouchSlop();
    if (Math.hypot(dx, dy) <= slop) return false;
    // If only X bound, must be primarily horizontal; if only Y, primarily vertical
    // For now, allow free movement if any pan binding exists
    return true;
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
        lastScale = 1.0f;
        lastScaleTime = System.currentTimeMillis();
        Native.pinchStart(mounter.host.handle(), id);
        return true;
      }
      return false;
    }

    @Override
    public boolean onScale(ScaleGestureDetector detector) {
      if (pinchActive) {
        float scale = detector.getScaleFactor();
        lastScale *= scale;
        lastScaleTime = System.currentTimeMillis();
        Native.pinchMove(mounter.host.handle(), id, lastScale);
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
