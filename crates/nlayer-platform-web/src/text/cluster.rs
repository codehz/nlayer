//! The end of a grapheme cluster, so a line never breaks inside one and a
//! caret never stands inside one.
//!
//! Extracted from `nsh-core/src/text_edit.rs` (`next_cluster` only).

/// True when `c` joins the character before it into one cluster: marks,
/// variation selectors, emoji modifiers and tags, and what follows a
/// zero-width joiner.
fn extends(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F
        | 0x0483..=0x0489
        | 0x0591..=0x05BD
        | 0x0610..=0x061A
        | 0x064B..=0x065F
        | 0x0E31 | 0x0E34..=0x0E3A | 0x0E47..=0x0E4E
        | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF
        | 0x200C..=0x200D
        | 0x20D0..=0x20FF
        | 0x3099..=0x309A
        | 0xFE00..=0xFE0F
        | 0xFE20..=0xFE2F
        | 0x1F3FB..=0x1F3FF
        | 0xE0020..=0xE007F
        | 0xE0100..=0xE01EF)
}

fn regional(c: char) -> bool {
    (0x1F1E6..=0x1F1FF).contains(&(c as u32))
}

/// The end of the cluster starting at `at`.
pub(crate) fn next_cluster(s: &str, at: usize) -> usize {
    let mut chars = s[at..].char_indices().peekable();
    let Some((_, first)) = chars.next() else {
        return s.len();
    };
    let mut prev = first;
    // Two regional indicators make one flag.
    let mut flag = regional(first);
    while let Some(&(i, c)) = chars.peek() {
        let joined = prev == '\u{200D}' || extends(c) || (flag && regional(c));
        if !joined {
            return at + i;
        }
        if regional(c) {
            flag = false;
        }
        prev = c;
        chars.next();
    }
    s.len()
}
