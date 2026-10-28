package dev.nlayer.android;

import java.io.File;
import org.json.JSONObject;

/** `files/nlayer-test.json`, written by `nlayer test --target android`. */
final class Pin {
  final float width, height, keyboard, top, right, bottom, left;
  final String host;
  final int port;

  private Pin(
      float width,
      float height,
      float keyboard,
      float top,
      float right,
      float bottom,
      float left,
      String host,
      int port) {
    this.width = width;
    this.height = height;
    this.keyboard = keyboard;
    this.top = top;
    this.right = right;
    this.bottom = bottom;
    this.left = left;
    this.host = host;
    this.port = port;
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
          json.optInt("port", 0));
    } catch (Exception e) {
      return null;
    }
  }
}
