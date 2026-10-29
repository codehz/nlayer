package dev.nlayer.android;

import android.os.Build;
import android.view.View;
import android.view.accessibility.AccessibilityNodeInfo;

/**
 * Accessibility of one view: `role`, `accessibilityLabel` / `accessibilityHint`,
 * `accessibilityState`, `accessibilityValue` and whether the subtree is hidden.
 *
 * <p>Cut from nsh's `A11y.java`: roles are the v2 set, by name; there are no custom actions, live
 * regions or level.
 */
final class A11y extends View.AccessibilityDelegate {
  static final class Info {
    String role;
    String label;
    String hint;
    boolean hidden;
    boolean disabled;
    boolean selected;

    /** 0 none, 1 false, 2 true, 3 mixed. */
    int checked;

    /** 0 none, 1 false, 2 true. */
    int expanded;

    float min = Float.NaN, max = Float.NaN, now = Float.NaN;
    String valueText;
  }

  final Info info;

  private A11y(Info info) {
    this.info = info;
  }

  /** Applies the view-level settings; the rest happens when a node is queried. */
  static void apply(View v, Info info) {
    v.setAccessibilityDelegate(new A11y(info));
    if (info.hidden) {
      v.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_NO_HIDE_DESCENDANTS);
    } else if (info.label != null || info.role != null) {
      v.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_YES);
    } else {
      v.setImportantForAccessibility(View.IMPORTANT_FOR_ACCESSIBILITY_AUTO);
    }
    v.setContentDescription(info.label);
    if (Build.VERSION.SDK_INT >= 28 && "header".equals(info.role)) {
      v.setAccessibilityHeading(true);
    }
  }

  @Override
  public void onInitializeAccessibilityNodeInfo(View host, AccessibilityNodeInfo node) {
    super.onInitializeAccessibilityNodeInfo(host, node);
    String className = className(info.role);
    if (className != null) node.setClassName(className);
    String description = roleDescription(info.role);
    if (description != null && Build.VERSION.SDK_INT >= 30) {
      node.getExtras().putCharSequence("AccessibilityNodeInfo.roleDescription", description);
    }
    if (info.hint != null && Build.VERSION.SDK_INT >= 26) node.setHintText(info.hint);
    node.setEnabled(!info.disabled);
    node.setSelected(info.selected);
    StringBuilder state = new StringBuilder();
    if (info.checked != 0) {
      node.setCheckable(true);
      node.setChecked(info.checked == 2);
      if (info.checked == 3) state.append("mixed");
    }
    if (info.expanded != 0) {
      if (state.length() > 0) state.append(", ");
      state.append(info.expanded == 2 ? "expanded" : "collapsed");
    }
    if (info.valueText != null) {
      if (state.length() > 0) state.append(", ");
      state.append(info.valueText);
    } else if (!Float.isNaN(info.now) && !Float.isNaN(info.min) && !Float.isNaN(info.max)) {
      node.setRangeInfo(
          AccessibilityNodeInfo.RangeInfo.obtain(
              AccessibilityNodeInfo.RangeInfo.RANGE_TYPE_FLOAT, info.min, info.max, info.now));
    }
    if (state.length() > 0 && Build.VERSION.SDK_INT >= 30) {
      node.setStateDescription(state);
    }
  }

  private static String className(String role) {
    if (role == null) return null;
    switch (role) {
      case "button":
        return "android.widget.Button";
      case "imagebutton":
        return "android.widget.ImageButton";
      case "image":
        return "android.widget.ImageView";
      case "search":
        return "android.widget.EditText";
      case "text":
        return "android.widget.TextView";
      case "adjustable":
      case "spinbutton":
        return "android.widget.SeekBar";
      case "checkbox":
        return "android.widget.CheckBox";
      case "combobox":
        return "android.widget.Spinner";
      case "progressbar":
      case "timer":
        return "android.widget.ProgressBar";
      case "radio":
        return "android.widget.RadioButton";
      case "radiogroup":
        return "android.widget.RadioGroup";
      case "switch":
        return "android.widget.Switch";
      case "tablist":
        return "android.widget.TabWidget";
      case "toolbar":
        return "android.widget.Toolbar";
      case "list":
        return "android.widget.AbsListView";
      case "grid":
        return "android.widget.GridView";
      case "listbox":
        return "android.widget.ListView";
      default:
        return null;
    }
  }

  private static String roleDescription(String role) {
    if (role == null) return null;
    switch (role) {
      case "link":
        return "link";
      case "summary":
        return "summary";
      case "alert":
        return "alert";
      case "menu":
        return "menu";
      case "menubar":
        return "menu bar";
      case "menuitem":
      case "menuitemcheckbox":
      case "menuitemradio":
        return "menu item";
      case "scrollbar":
        return "scroll bar";
      case "tab":
        return "tab";
      case "tablist":
        return "tab list";
      case "timer":
        return "timer";
      case "row":
        return "row";
      case "cell":
        return "cell";
      case "columnheader":
        return "column header";
      case "rowgroup":
        return "row group";
      case "option":
        return "option";
      case "region":
        return "region";
      case "navigation":
        return "navigation";
      case "tree":
        return "tree";
      case "treeitem":
        return "tree item";
      default:
        return null;
    }
  }
}
