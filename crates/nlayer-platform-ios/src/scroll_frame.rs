//! `onScroll` is at most once per frame. A host notes every offset and
//! delivers the latest when the frame ends. An end, a drag, or momentum
//! flushes that view first so the last offset is not reported after it.

/// Latest scroll payload per view, armed until the host flushes the frame.
#[derive(Debug, Default)]
pub(crate) struct ScrollFrames<T> {
    pending: Vec<(u32, T)>,
    armed: bool,
}

impl<T> ScrollFrames<T> {
    /// Remembers `value` for `id`. `true` the first time this frame: the
    /// host schedules one flush.
    pub(crate) fn note(&mut self, id: u32, value: T) -> bool {
        if let Some(slot) = self
            .pending
            .iter_mut()
            .find(|(existing, _)| *existing == id)
        {
            slot.1 = value;
        } else {
            self.pending.push((id, value));
        }
        let arm = !self.armed;
        self.armed = true;
        arm
    }

    /// The views waiting, and the frame is no longer armed.
    pub(crate) fn take(&mut self) -> Vec<(u32, T)> {
        self.armed = false;
        std::mem::take(&mut self.pending)
    }

    /// That view's latest offset, if one is waiting. The frame stays armed
    /// while another view is still waiting.
    pub(crate) fn take_id(&mut self, id: u32) -> Option<T> {
        let index = self
            .pending
            .iter()
            .position(|(existing, _)| *existing == id)?;
        let (_, value) = self.pending.remove(index);
        if self.pending.is_empty() {
            self.armed = false;
        }
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::ScrollFrames;

    #[test]
    fn two_offsets_in_one_frame_deliver_the_latest_once() {
        let mut frames = ScrollFrames::default();
        assert!(frames.note(5, 10));
        assert!(!frames.note(5, 24));
        assert_eq!(frames.take(), vec![(5, 24)]);
        assert!(frames.note(5, 1));
    }

    #[test]
    fn an_end_flushes_that_view_and_leaves_the_other_armed() {
        let mut frames = ScrollFrames::default();
        assert!(frames.note(5, 10));
        assert!(!frames.note(6, 3));
        assert_eq!(frames.take_id(5), Some(10));
        assert_eq!(frames.take(), vec![(6, 3)]);
        assert!(frames.note(5, 11));
    }
}
