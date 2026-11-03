//! Widths and font metrics for [`super::WebTextEngine`].
//!
//! On the page they come from the browser (`measure_text`, the font's
//! bounding box). Off wasm a fixed advance stands in, so the same layout
//! runs in host tests: half an em wide, ascent `0.8` em, descent `0.2` em,
//! x-height `0.5` em.

/// The width of `text` in CSS `font`, in points.
pub(crate) fn measure_text(font: &str, text: &str) -> f32 {
    host::measure_text(font, text)
}

/// The width of `text` in CSS `font` with `font-variant-numeric: numeric`.
pub(crate) fn measure_numeric_text(font: &str, numeric: &str, text: &str) -> f32 {
    host::measure_numeric_text(font, numeric, text)
}

/// Ascent and descent of CSS `font` (0 when the browser cannot tell).
pub(crate) fn font_metrics(font: &str) -> (f32, f32) {
    host::font_metrics(font)
}

/// The x height of CSS `font` (0 when the browser cannot tell).
pub(crate) fn font_x_height(font: &str) -> f32 {
    host::font_x_height(font)
}

/// Visual cluster bounds measured in the full run, in logical order.
pub(crate) fn text_cluster_boxes(
    font: &str,
    numeric: &str,
    spacing: f32,
    text: &str,
    ranges: &[[u32; 2]],
    out: &mut [[f32; 2]],
) {
    host::text_cluster_boxes(font, numeric, spacing, text, ranges, out);
}

/// The pixel size in a CSS `font` shorthand (`16px` in `400 16px …`).
fn font_size(font: &str) -> f32 {
    let Some(px) = font.find("px") else {
        return 14.0;
    };
    let start = font[..px]
        .rfind(|c: char| !c.is_ascii_digit() && c != '.')
        .map_or(0, |i| i + 1);
    font[start..px].parse().unwrap_or(14.0)
}

#[cfg(target_arch = "wasm32")]
mod host {
    #[link(wasm_import_module = "nlayer")]
    unsafe extern "C" {
        #[link_name = "measure_text"]
        fn js_measure_text(
            font: *const u8,
            font_len: usize,
            text: *const u8,
            text_len: usize,
        ) -> f64;
        #[link_name = "measure_numeric_text"]
        fn js_measure_numeric_text(
            font: *const u8,
            font_len: usize,
            numeric: *const u8,
            numeric_len: usize,
            text: *const u8,
            text_len: usize,
        ) -> f64;
        #[link_name = "text_cluster_boxes"]
        fn js_text_cluster_boxes(
            font: *const u8,
            font_len: usize,
            numeric: *const u8,
            numeric_len: usize,
            spacing: f32,
            text: *const u8,
            text_len: usize,
            ranges: *const u32,
            out: *mut f32,
            count: usize,
        );
        #[link_name = "font_ascent"]
        fn js_font_ascent(font: *const u8, font_len: usize) -> f64;
        #[link_name = "font_descent"]
        fn js_font_descent(font: *const u8, font_len: usize) -> f64;
        #[link_name = "font_x_height"]
        fn js_font_x_height(font: *const u8, font_len: usize) -> f64;
    }

    pub(super) fn measure_text(font: &str, text: &str) -> f32 {
        // SAFETY: both slices are valid for the call; JS only reads them.
        unsafe { js_measure_text(font.as_ptr(), font.len(), text.as_ptr(), text.len()) as f32 }
    }

    pub(super) fn measure_numeric_text(font: &str, numeric: &str, text: &str) -> f32 {
        // SAFETY: the slices are valid for the call; JS only reads them.
        unsafe {
            js_measure_numeric_text(
                font.as_ptr(),
                font.len(),
                numeric.as_ptr(),
                numeric.len(),
                text.as_ptr(),
                text.len(),
            ) as f32
        }
    }

    pub(super) fn font_metrics(font: &str) -> (f32, f32) {
        // SAFETY: the slice is valid for the calls; JS only reads it.
        unsafe {
            (
                js_font_ascent(font.as_ptr(), font.len()) as f32,
                js_font_descent(font.as_ptr(), font.len()) as f32,
            )
        }
    }

    pub(super) fn font_x_height(font: &str) -> f32 {
        // SAFETY: the slice is valid for the call; JS only reads it.
        unsafe { js_font_x_height(font.as_ptr(), font.len()) as f32 }
    }

    pub(super) fn text_cluster_boxes(
        font: &str,
        numeric: &str,
        spacing: f32,
        text: &str,
        ranges: &[[u32; 2]],
        out: &mut [[f32; 2]],
    ) {
        assert_eq!(ranges.len(), out.len());
        // SAFETY: strings and flat two-number arrays remain valid for the call;
        // JS reads the input and writes exactly two floats for each range.
        unsafe {
            js_text_cluster_boxes(
                font.as_ptr(),
                font.len(),
                numeric.as_ptr(),
                numeric.len(),
                spacing,
                text.as_ptr(),
                text.len(),
                ranges.as_ptr().cast(),
                out.as_mut_ptr().cast(),
                ranges.len(),
            );
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod host {
    pub(super) fn measure_text(font: &str, text: &str) -> f32 {
        super::font_size(font) * 0.5 * text.chars().count() as f32
    }

    pub(super) fn measure_numeric_text(font: &str, _numeric: &str, text: &str) -> f32 {
        measure_text(font, text)
    }

    pub(super) fn font_metrics(font: &str) -> (f32, f32) {
        let size = super::font_size(font);
        (size * 0.8, size * 0.2)
    }

    pub(super) fn font_x_height(font: &str) -> f32 {
        super::font_size(font) * 0.5
    }

    pub(super) fn text_cluster_boxes(
        font: &str,
        numeric: &str,
        _spacing: f32,
        text: &str,
        ranges: &[[u32; 2]],
        out: &mut [[f32; 2]],
    ) {
        let width = if numeric.is_empty() {
            measure_text(font, text)
        } else {
            measure_numeric_text(font, numeric, text)
        };
        let units = text.encode_utf16().count().max(1) as f32;
        let advance = width / units;
        for (range, slot) in ranges.iter().zip(out.iter_mut()) {
            *slot = [range[0] as f32 * advance, range[1] as f32 * advance];
        }
    }
}
