//! Accessibility element, label, traits and spoken value.

use objc2::MainThreadMarker;
use objc2_foundation::NSString;
use objc2_ui_kit::{
    NSObjectUIAccessibility, UIAccessibilityTraitAdjustable, UIAccessibilityTraitButton,
    UIAccessibilityTraitHeader, UIAccessibilityTraitImage, UIAccessibilityTraitKeyboardKey,
    UIAccessibilityTraitLink, UIAccessibilityTraitNone, UIAccessibilityTraitNotEnabled,
    UIAccessibilityTraitSearchField, UIAccessibilityTraitSelected, UIAccessibilityTraitStaticText,
    UIAccessibilityTraitSummaryElement, UIAccessibilityTraitUpdatesFrequently, UIView,
};

pub(super) fn apply_accessibility(
    view: &UIView,
    a: &nlayer_core::Accessibility,
    mtm: MainThreadMarker,
) {
    use nlayer_core::AccessibilityRole;
    view.setIsAccessibilityElement(a.is_element(), mtm);
    let label = a.label.as_ref().map(|label| NSString::from_str(label));
    view.setAccessibilityLabel(label.as_deref(), mtm);
    let hint = a.hint.as_ref().map(|hint| NSString::from_str(hint));
    view.setAccessibilityHint(hint.as_deref(), mtm);
    let value = spoken_value(a).map(|value| NSString::from_str(&value));
    view.setAccessibilityValue(value.as_deref(), mtm);
    view.setAccessibilityElementsHidden(a.hidden, mtm);
    // SAFETY: immutable UIKit constants.
    let traits = unsafe {
        let mut traits = match a.role {
            AccessibilityRole::Button
            | AccessibilityRole::ImageButton
            | AccessibilityRole::Checkbox
            | AccessibilityRole::Radio
            | AccessibilityRole::Switch
            | AccessibilityRole::Tab
            | AccessibilityRole::Menuitem
            | AccessibilityRole::MenuItemCheckbox
            | AccessibilityRole::MenuItemRadio
            | AccessibilityRole::TreeItem
            | AccessibilityRole::Option => UIAccessibilityTraitButton,
            AccessibilityRole::Link => UIAccessibilityTraitLink,
            AccessibilityRole::Search => UIAccessibilityTraitSearchField,
            AccessibilityRole::Image => UIAccessibilityTraitImage,
            AccessibilityRole::KeyboardKey => UIAccessibilityTraitKeyboardKey,
            AccessibilityRole::Text => UIAccessibilityTraitStaticText,
            AccessibilityRole::Adjustable | AccessibilityRole::Spinbutton => {
                UIAccessibilityTraitAdjustable
            }
            AccessibilityRole::Header | AccessibilityRole::ColumnHeader => {
                UIAccessibilityTraitHeader
            }
            AccessibilityRole::Summary => UIAccessibilityTraitSummaryElement,
            AccessibilityRole::Progressbar | AccessibilityRole::Timer => {
                UIAccessibilityTraitUpdatesFrequently
            }
            _ => UIAccessibilityTraitNone,
        };
        if a.role == AccessibilityRole::ImageButton {
            traits |= UIAccessibilityTraitImage;
        }
        if a.state.disabled {
            traits |= UIAccessibilityTraitNotEnabled;
        }
        if a.state.selected || a.state.checked == Some(nlayer_core::Checked::True) {
            traits |= UIAccessibilityTraitSelected;
        }
        traits
    };
    view.setAccessibilityTraits(traits, mtm);
}

/// What VoiceOver reads: the value (as a percentage of its range, or the
/// text), then checked / expanded.
pub(super) fn spoken_value(a: &nlayer_core::Accessibility) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(v) = &a.value {
        if let Some(text) = &v.text {
            parts.push(text.to_string());
        } else if let Some(now) = v.now {
            match (v.min, v.max) {
                (Some(min), Some(max)) if max > min => {
                    parts.push(format!(
                        "{}%",
                        (((now - min) / (max - min)) * 100.0).round()
                    ));
                }
                _ => parts.push(format!("{now}")),
            }
        }
    }
    match a.state.checked {
        Some(nlayer_core::Checked::True) => parts.push("checked".into()),
        Some(nlayer_core::Checked::False) => parts.push("not checked".into()),
        Some(nlayer_core::Checked::Mixed) => parts.push("mixed".into()),
        None => {}
    }
    match a.state.expanded {
        Some(true) => parts.push("expanded".into()),
        Some(false) => parts.push("collapsed".into()),
        None => {}
    }
    (!parts.is_empty()).then(|| parts.join(", "))
}
