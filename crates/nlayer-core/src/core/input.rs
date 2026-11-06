//! Single-line and multiline inputs.

use super::*;

/// Shortens a long run without a line-break opportunity so that measuring a
/// multiline value stays linear in its length. A 474 KB minified JSON takes
/// seconds to measure at every edit; broken into runs of 1024 it is fast.
/// The input shows and reports its own text; the measured height can only
/// grow by a line per break, and text without such runs is measured as is.
pub(super) fn broken_runs(content: &str) -> std::borrow::Cow<'_, str> {
    let mut broken: Option<String> = None;
    let (mut start, mut from) = (0, 0);
    for (end, _) in unicode_linebreak::linebreaks(content) {
        let mut run = 0;
        for (offset, c) in content[start..end].char_indices() {
            let i = start + offset;
            // The final character may itself provide the break (space or
            // newline). Do not introduce another line immediately before it.
            if run == MEASURED_RUN && i + c.len_utf8() < end {
                let broken = broken.get_or_insert_with(|| {
                    String::with_capacity(content.len() + content.len() / MEASURED_RUN)
                });
                broken.push_str(&content[from..i]);
                broken.push('\n');
                (run, from) = (0, i);
            }
            run += 1;
        }
        start = end;
    }
    match broken {
        Some(mut broken) => {
            broken.push_str(&content[from..]);
            broken.into()
        }
        None => content.into(),
    }
}

impl<M: TextEngine> Core<M> {
    /// The user edited input `id` (before JS sees the edit): keeps the text
    /// for layout and for stale-value detection.
    pub fn input_changed(&mut self, id: NodeId, text: &str, count: u32) {
        let Ok(key) = self.key(id) else { return };
        let Some(input) = self.nodes[key as usize].input.as_mut() else {
            return;
        };
        input.count = count;
        if input.value != text {
            input.value.clear();
            input.value.push_str(text);
            self.measure_input(key);
        }
    }

    /// Rebuilds the text an input is measured with: its value, else its
    /// placeholder, else an empty line.
    pub(super) fn measure_input(&mut self, key: Key) {
        let node = &self.nodes[key as usize];
        let (Some(input), Some(layout)) = (&node.input, node.layout) else {
            return;
        };
        let placeholder = input.value.is_empty() && !input.config.placeholder.is_empty();
        let text = self.input_text(
            key,
            if placeholder {
                &input.config.placeholder
            } else {
                &input.value
            },
        );
        let cx = self.text_cx(layout);
        let same = cx.text.layout_eq(&text);
        let input = self.nodes[key as usize]
            .input
            .as_mut()
            .expect("input state");
        // A value that reads as the placeholder measures the same, but
        // it is not the placeholder: laid out again so the two stay distinct.
        if same && input.placeholder_measured == placeholder {
            return;
        }
        input.placeholder_measured = placeholder;
        if !same {
            *self.text_cx(layout) = TextContext::new(Arc::new(text));
        }
        self.layout.mark_dirty(layout);
        self.needs_layout = true;
    }

    /// `content` in the text style of input `key`, as it is measured. A
    /// single line has `max_lines` 1; several wrap at the width, and an
    /// empty string or a trailing line break still takes a line.
    fn input_text(&self, key: Key, content: &str) -> AttributedText {
        let node = &self.nodes[key as usize];
        let multiline = node.input.as_ref().is_some_and(|i| i.config.multiline);
        let mut text = AttributedText {
            max_lines: if multiline { 0 } else { 1 },
            ..AttributedText::default()
        };
        let style = self.inherited_text_style(key).inherit(&node.text_style);
        if multiline {
            text.push(&broken_runs(content), style);
        } else {
            text.push(content, style);
        }
        if content.is_empty() || (multiline && content.ends_with('\n')) {
            text.push("\u{200b}", style);
        }
        text
    }

    pub(super) fn emit_input(&mut self, key: Key, id: NodeId, flags: u32) {
        let text_style = self.nodes[key as usize].text_style;
        let rtl = self.nodes[key as usize].rtl;
        let style = self.inherited_text_style(key).inherit(&text_style);
        let align = super::text::directed_align(
            text_style.text_align.unwrap_or_default(),
            rtl && !text_style.text_align_physical,
        );
        let writing = text_style.writing_direction.unwrap_or(rtl);
        let config = (flags & dirty::INPUT != 0).then(|| {
            let input = self.nodes[key as usize]
                .input
                .as_ref()
                .expect("input state");
            let mut config = input.config.clone();
            config.style = style;
            config.align = align;
            config.rtl = writing;
            config
        });
        let value = (flags & dirty::INPUT_VALUE != 0).then(|| {
            let input = self.nodes[key as usize]
                .input
                .as_ref()
                .expect("input state");
            (input.value.clone(), input.count)
        });
        if let Some(config) = config {
            self.mount.push(MountItem::Input {
                id,
                config: Box::new(config),
            });
        }
        if let Some((text, count)) = value {
            self.mount.push(MountItem::InputValue { id, text, count });
        }
    }
}
