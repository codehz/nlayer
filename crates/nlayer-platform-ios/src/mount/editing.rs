//! Input edits, selection, composition and focus.

use nlayer_core::NodeId;

use super::*;

impl Mounter {
    pub(super) fn report_input(&self, id: NodeId, text: String) {
        if let Some(control) = &self.control {
            control.input_changed(id, text);
        }
    }

    /// The editor reported an edit with `marked` (its composition, if any).
    /// A composition that starts or changes is reported before the text
    /// holding it, and one that ends after the text it committed, as
    /// browsers order them.
    pub(super) fn input_edited(&mut self, id: NodeId, marked: Option<String>) {
        let update = match self.native(id) {
            Some(Native::Input(field)) => field.edited(),
            Some(Native::Area(area)) => area.edited(),
            _ => return,
        };
        let change = if marked.is_some() {
            self.compose(id, marked.clone())
        } else {
            None
        };
        if let Some(change) = change {
            self.report_composition(id, change);
        }
        if let Some(crate::input::TextUpdate::Report(text)) = update {
            self.report_input(id, text);
        }
        if marked.is_none()
            && let Some(change) = self.compose(id, None)
        {
            self.report_composition(id, change);
        }
    }

    /// The input's selection moved (and its composition may have ended).
    pub(super) fn input_selection(
        &mut self,
        id: NodeId,
        selection: Option<(u32, u32)>,
        marked: Option<String>,
    ) {
        let selected = match self.native(id) {
            Some(Native::Input(field)) => {
                selection.and_then(|(start, end)| field.selected(start, end))
            }
            Some(Native::Area(area)) => {
                selection.and_then(|(start, end)| area.selected(start, end))
            }
            _ => None,
        };
        if let Some((start, end)) = selected
            && let Some(control) = &self.control
        {
            control.selection_changed(id, start, end);
        }
        let change = match self.native(id) {
            Some(Native::Input(field)) => field.compose_if_known(marked),
            Some(Native::Area(area)) => area.compose_if_known(marked),
            _ => None,
        };
        if let Some(change) = change {
            self.report_composition(id, change);
        }
    }

    pub(super) fn compose(&mut self, id: NodeId, marked: Option<String>) -> Option<(bool, String)> {
        match self.native(id) {
            Some(Native::Input(field)) => field.compose(marked),
            Some(Native::Area(area)) => area.compose(marked),
            _ => None,
        }
    }

    pub(super) fn report_composition(&self, id: NodeId, (composing, text): (bool, String)) {
        if let Some(control) = &self.control {
            control.composition_changed(id, composing, text);
        }
    }

    pub(super) fn begin_focus(&mut self, id: NodeId) {
        if self.focused == Some(id) {
            return;
        }
        if let Some(old) = self.focused {
            let ids = self.chain(old);
            self.mark_focused(&ids, false);
        }
        let ids = self.chain(id);
        self.mark_focused(&ids, true);
        self.focused = Some(id);
    }

    pub(super) fn end_focus(&mut self, id: NodeId) {
        if self.focused != Some(id) {
            return;
        }
        let ids = self.chain(id);
        self.focused = None;
        self.mark_focused(&ids, false);
    }
}
