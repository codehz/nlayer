package dev.nlayer.android;

import java.io.File;
import org.json.JSONObject;

/** `files/nlayer-test.json`, written by `nlayer test --target android`. */
final class Pin {
  final float width, height, keyboard, top, right, bottom, left;
  final String host;
  final int port;

  /** `light` or `dark`, pinned by `nlayer test`; null follows the system. */
  final String colorScheme;

  /** Set by `nlayer test`. Absent is false; the file existing is still a test session. */
  final boolean test;

  private Pin(
      float width,
      float height,
      float keyboard,
      float top,
      float right,
      float bottom,
      float left,
      String host,
      int port,
      String colorScheme,
      boolean test) {
    this.width = width;
    this.height = height;
    this.keyboard = keyboard;
    this.top = top;
    this.right = right;
    this.bottom = bottom;
    this.left = left;
    this.host = host;
    this.port = port;
    this.colorScheme = colorScheme;
    this.test = test;
  }

  static Pin read(File file) {
    if (file == null || !file.isFile()) return null;
    try {
      JSONObject json =
          new JSONObject(
              new String(
                  java.nio.file.Files.readAllBytes(file.toPath()),
                  java.nio.charset.StandardCharsets.UTF_8));
      JSONObject area = json.optJSONObject("safeArea");
      if (area == null) area = new JSONObject();
      return new Pin(
          (float) json.optDouble("width", 0),
          (float) json.optDouble("height", 0),
          (float) json.optDouble("keyboard", 0),
          (float) area.optDouble("top", 0),
          (float) area.optDouble("right", 0),
          (float) area.optDouble("bottom", 0),
          (float) area.optDouble("left", 0),
          json.optString("host", ""),
          json.optInt("port", 0),
          json.has("colorScheme") ? json.optString("colorScheme", null) : null,
          json.optBoolean("test", false));
    } catch (Exception e) {
      return null;
    }
  }
}

/** `files/nlayer-dev.json`, written by `nlayer run --target android`. */
final class DevPin {
  final String host;
  final int port;

  /** A test session keeps the overlay and patches off. */
  final boolean test;

  private DevPin(String host, int port, boolean test) {
    this.host = host;
    this.port = port;
    this.test = test;
  }

  static DevPin read(File file) {
    if (file == null || !file.isFile()) return null;
    try {
      JSONObject json =
          new JSONObject(
              new String(
                  java.nio.file.Files.readAllBytes(file.toPath()),
                  java.nio.charset.StandardCharsets.UTF_8));
      int port = json.optInt("port", 0);
      if (port <= 0 || port > 0xffff) return null;
      return new DevPin(json.optString("host", "127.0.0.1"), port, json.optBoolean("test", false));
    } catch (Exception e) {
      return null;
    }
  }
}
