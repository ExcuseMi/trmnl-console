//! SBuffer is a custom string encoding for sending the terminal display data as a string
//! for the TRMNL webhook payload. It is compressed and it contains color formatting codes.
//!
//! # Specification
//! Each SBuffer string is valid UTF-8 with each printable character representing a
//! single cell in the terminal. Exceptions are special control codes, listed below.
//!
//! Wide characters are assumed to only take up one cell.
//!
//! ## Special Control Codes
//! Non-printable characters are used to represent special control codes.
//!
//! ### U+000A (Line Feed)
//! Each row of terminal output is delimited by a newline character. If the rest of a line
//! would only contain empty space, it is omitted. "empty space" refers to cells that contain
//! a whitespace character and would render identically to the padding a renderer produces
//! for them: their background color and underline attribute match the attribute state in
//! effect when the line feed is emitted. Foreground color, boldness, dimness and italic
//! are irrelevant for this, since they are invisible on blank cells; encoders may leave
//! them set across a line feed.
//! Renderers render the rest of the line as one space character per omitted cell, using
//! the attribute state in effect when the line feed is emitted (for this the renderer may
//! need to know the intended width of a row, this is not encoded in this format, if the width
//! is not known the renderer may assume the row ends at the newline).
//! Another control code before the line feed may set or unset formatting attributes that should
//! then be applied to the rest of the "empty space" in that line.
//!
//! The last row may end with a Line Feed, but this is not required. If there is no Line Feed as
//! the last character in the SBuffer then decoders must pretend there is one and proceed as defined
//! above.
//!
//! Decoders may choose to not render the last trailing new line.
//!
//! ### U+E000
//! Every SBuffer message (version 1; this spec) contains exactly one U+E000 character at
//! the beginning of the string. Future versions may change this to another character if not
//! backwards compatible.
//!
//! ### U+E100–U+E1FF
//! Enable foreground color. The lower byte of the code represents the color index.
//!
//! ### U+E200–U+E2FF
//! Enable background color. The lower byte of the code represents the color index.
//!
//! ### U+E300
//! The next character must be interpreted as-is (as a cell, even if it is a control
//! character; including U+E300).
//!
//! Exceptions: a following Line Feed (U+000A) is not escaped — it is processed as a normal
//! Line Feed and the U+E300 has no effect. A U+E300 as the last character of the SBuffer
//! (nothing follows it) is ignored.
//!
//! ### U+E301–U+E3FF
//! The next character is output `n+1` times, where `n` is the lower byte of the code.
//! This character is always interpreted literally (as-is, see U+E300).
//! It should not be a Line Feed (U+000A); if it is, the repeat code has no effect and the
//! Line Feed is processed as normal. A repeat code as the last character of the SBuffer
//! (nothing follows it) is ignored.
//!
//! ### U+E400+
//! Attribute control characters, described by the following table:
//!
//! | Char   | Description            |
//! |--------|------------------------|
//! | U+E400 | Reset foreground color |
//! | U+E401 | Reset background color |
//! | U+E402 | Toggle boldness        |
//! | U+E403 | Toggle dimness         |
//! | U+E404 | Toggle italic          |
//! | U+E405 | Toggle underline       |
//!
//! Toggleable attributes are off by default.
//!
//! ### Reserved codes
//! All other non-defined code points in the Private Use Area U+E000-U+F8FF are reserved for future
//! use. If ANY character in this PUA area should be used as cell content, it should be prefixed
//! with U+E300.
//!
//! Decoders encountering a reserved code may either ignore it or output it as-is; this is
//! implementation-defined.
//!
//! # Implementation
//!
//! The struct [`SBuffer`] implements creating such an encoded string from wezterm_term screen
//! data and output as SBuffer string or HTML.
//!
//! Reverse video has no control code: [`SBuffer::from_terminal`] resolves it while encoding
//! by swapping the effective foreground and background colors of affected cells, substituting
//! palette index 0 (black) for the default foreground and 15 (white) for the default
//! background.
//!
//! Terminal cells holding multi-codepoint graphemes (e.g. combining marks) are reduced to a
//! single character by [`SBuffer::from_terminal`] (NFC composition, falling back to the first
//! code point), so that one encoded character always represents one cell.
//!
//! # HTML representation
//! Every SBuffer string has a specific HTML representation, which is the final content
//! to be rendered by the plugin. This implementation is provided by [`SBuffer::to_html`]
//! and another implementation exists at the receiving end for the serverless transform
//! function in the private plugin recipe (/plugin/src/transform.py).
//!
//! &, <, and > must be HTML-escaped.
//!
//! To be converted into HTML the size of the terminal must be specified, so that "empty space" can
//! be properly rendered (see "Line Feed" below and in the spec above).
//!
//! ## SBuffer-to-HTML Specification
//! The HTML representation assumes to be rendered as the content of a <pre>-Element.
//!
//! ### Attribute Handling
//! Processing control codes as specified below may apply "classes".
//!
//! Whenever a class is added, any open span element is closed and a new one is opened that has
//! a `class` attribute with all currently applied classes.
//!
//! Whenever a class is removed, any open span element is closed. If any class attribute is then
//! still currently applied a new span is opened with all currently applied classes.
//!
//! If EOF is encountered, any open span element is closed.
//!
//! A span that would not contain any characters may be omitted.
//!
//! ### Handling Special Control Codes
//! The following specifies the handling of control characters:
//!
//! #### U+000A (Line Feed)
//! Unchanged (not converted into a <br>, the HTML output is expected to be placed into
//! a <pre>). The rest of the "empty space" in the currently open row as specified by the width
//! of the terminal is rendered as a space character each. The trailing new line is not rendered.
//!
//! If a row already contains more cells than the specified width, no padding is added; the
//! full row is output regardless.
//!
//! #### U+E000
//! Stripped.
//!
//! #### U+E100–U+E1FF; U+E400
//! When one of these characters (except U+E400) is encountered, the text until the next occurrence
//! of any of these characters (including U+E400) gets the `tc--fg-<color>` class applied
//! (see `Attribute Handling`), where `<color>` is the lower byte of the code as decimal.
//!
//! #### U+E200–U+E2FF; U+E401
//! When one of these characters (except U+E401) is encountered, the text until the next occurrence
//! of any of these characters (including U+E401) gets the `tc--bg-<color>` class applied
//! (see `Attribute Handling`), where `<color>` is the lower byte of the code as decimal.
//!
//! #### U+E300; U+E301–U+E3FF
//! Handled exactly as specified in the SBuffer specification.
//!
//! #### U+E402+
//! Each of these add or remove classes to use for text rendering (see `Attribute Handling`).
//! The first occurrence each adds the respective class, the next subsequent occurrence removes it.
//! After this the next occurrence adds the class again, etc.
//!
//! | Char   | Description            | Class    |
//! |--------|------------------------|----------|
//! | U+E402 | Toggle boldness        | tc--bold |
//! | U+E403 | Toggle dimness         | tc--dim  |
//! | U+E404 | Toggle italic          | tc--ital |
//! | U+E405 | Toggle underline       | tc--undl |

use shadow_terminal::termwiz::color::ColorAttribute;
use shadow_terminal::wezterm_term::{CellAttributes, Intensity, Terminal, Underline};
use std::borrow::Cow;
use std::fmt::{Display, Formatter};
use unicode_normalization::UnicodeNormalization;

const START: char = '\u{E000}';
const SPACE: char = ' ';
const LINE_BREAK: char = '\n';
const FG_START: char = '\u{E100}';
const FG_END: char = char_add(FG_START, 256);
const BG_START: char = '\u{E200}';
const BG_END: char = char_add(BG_START, 256);
const ESCAPE: char = '\u{E300}';
// this is on purpose, the real repeats start at E301 - see impl & char_add.
const REPEAT: char = ESCAPE;
const REPEAT_END: char = char_add(REPEAT, 256);
const FG_RESET: char = '\u{E400}';
const BG_RESET: char = '\u{E401}';
const BOLD: char = '\u{E402}';
const DIM: char = '\u{E403}';
const ITAL: char = '\u{E404}';
const UNDL: char = '\u{E405}';

const HTML_BOLD: Cow<str> = Cow::Borrowed("tc--bold");
const HTML_DIM: Cow<str> = Cow::Borrowed("tc--dim");
const HTML_ITAL: Cow<str> = Cow::Borrowed("tc--ital");
const HTML_UNDL: Cow<str> = Cow::Borrowed("tc--undl");
const HTML_PREFIX_FG: &str = "tc--fg-";
const HTML_PREFIX_BG: &str = "tc--bg-";

fn html_fg(color_idx: u8) -> Cow<'static, str> {
    Cow::Owned(format!("{HTML_PREFIX_FG}{color_idx}"))
}

fn html_bg(color_idx: u8) -> Cow<'static, str> {
    Cow::Owned(format!("{HTML_PREFIX_BG}{color_idx}"))
}

#[inline]
const fn char_add(c: char, n: u32) -> char {
    char::from_u32(c as u32 + n).unwrap()
}

#[inline]
const fn is_reserved_char(c: char) -> bool {
    c as u32 >= 0xE000 && c as u32 <= 0xF8FF
}

const fn color_to_char(col: ColorAttribute, fg: bool) -> char {
    match col {
        ColorAttribute::PaletteIndex(i) | ColorAttribute::TrueColorWithPaletteFallback(_, i) => {
            char_add(if fg { FG_START } else { BG_START }, i as _)
        }
        ColorAttribute::Default | ColorAttribute::TrueColorWithDefaultFallback(_) => {
            if fg {
                FG_RESET
            } else {
                BG_RESET
            }
        }
    }
}

const fn reversed_effective_col(col: ColorAttribute, is_fg: bool) -> ColorAttribute {
    match col {
        // When reversing, the default colors need concrete substitutes: the default
        // fg maps to palette 0 (black), the default bg to palette 15 (white).
        ColorAttribute::Default | ColorAttribute::TrueColorWithDefaultFallback(_) => {
            ColorAttribute::PaletteIndex(if is_fg { 0 } else { 15 })
        }
        other => other,
    }
}

/// Emits into `codes` the control codes needed to bring the decoder's wire state
/// (`wire`) in line with the cell attributes `cell`, updating `wire` accordingly.
///
/// For blank cells (`blank`), only background color and underline are synced: all
/// other attributes are invisible on blank cells, so the wire state may keep them
/// dangling (see the "empty space" definition in the spec). `wire` therefore
/// deliberately diverges from the cell attributes for those properties.
///
/// Codes are emitted in the canonical order: fg, bg, then toggles in
/// U+E402..=U+E405 order.
fn apply_cell_attrs(
    codes: &mut Vec<char>,
    wire: &mut CellAttributes,
    cell: &CellAttributes,
    blank: bool,
) {
    let (new_fg, new_bg) = if cell.reverse() {
        (
            reversed_effective_col(cell.background(), false),
            reversed_effective_col(cell.foreground(), true),
        )
    } else {
        (cell.foreground(), cell.background())
    };
    if !blank && wire.foreground() != new_fg {
        codes.push(color_to_char(new_fg, true));
        wire.set_foreground(new_fg);
    }
    if wire.background() != new_bg {
        codes.push(color_to_char(new_bg, false));
        wire.set_background(new_bg);
    }
    if !blank {
        // Bold and dim are separate toggles of the single intensity attribute.
        let old_intensity = wire.intensity();
        let new_intensity = cell.intensity();
        if (old_intensity == Intensity::Bold) != (new_intensity == Intensity::Bold) {
            codes.push(BOLD);
        }
        if (old_intensity == Intensity::Half) != (new_intensity == Intensity::Half) {
            codes.push(DIM);
        }
        if old_intensity != new_intensity {
            wire.set_intensity(new_intensity);
        }
        let new_italic = cell.italic();
        if wire.italic() != new_italic {
            codes.push(ITAL);
            wire.set_italic(new_italic);
        }
    }
    // Underline renders on blank cells. Every underline variant maps onto the
    // single underline toggle.
    let new_underline = cell.underline() != Underline::None;
    if (wire.underline() != Underline::None) != new_underline {
        codes.push(UNDL);
        wire.set_underline(cell.underline());
    }
}

fn sbuffer_print_char(output: &mut Vec<char>, c: char, mut n: usize) {
    let utf8len = c.len_utf8();
    let is_reserved = is_reserved_char(c);
    // A literal reserved char costs an extra 3 bytes for the U+E300 escape;
    // the RLE form never needs the escape.
    let literal_len = utf8len + if is_reserved { 3 } else { 0 };
    // One RLE code covers up to 256 cells (`n+1` with `n` <= 0xFF), regardless
    // of the character's byte length.
    while n > 256 {
        output.push(char_add(REPEAT, 255));
        output.push(c);
        n -= 256;
    }
    // RLE only where it is strictly shorter than the literal run
    // (n * literal_len > 3 + utf8len bytes).
    if n * literal_len >= utf8len + 4 {
        output.push(char_add(REPEAT, (n - 1) as _));
        output.push(c);
    } else {
        for _ in 0..n {
            if is_reserved {
                output.push(ESCAPE);
            }
            output.push(c);
        }
    }
}

#[inline]
fn sbuffer_print_newline(output: &mut Vec<char>) {
    output.push(LINE_BREAK);
}

#[derive(Debug, Clone, Default)]
struct SBufferHtmlClassList {
    fg: Option<u8>,
    bg: Option<u8>,
    bold: bool,
    dim: bool,
    ital: bool,
    undl: bool,
}

impl SBufferHtmlClassList {
    fn is_empty(&self) -> bool {
        !self.fg.is_some()
            && !self.bg.is_some()
            && !self.bold
            && !self.dim
            && !self.ital
            && !self.undl
    }
    fn as_html_classes(&self) -> Vec<Cow<'static, str>> {
        let mut out = vec![];
        if let Some(fg) = self.fg {
            out.push(html_fg(fg));
        }
        if let Some(bg) = self.bg {
            out.push(html_bg(bg));
        }
        if self.bold {
            out.push(HTML_BOLD);
        }
        if self.dim {
            out.push(HTML_DIM);
        }
        if self.ital {
            out.push(HTML_ITAL);
        }
        if self.undl {
            out.push(HTML_UNDL);
        }

        out
    }
}

fn html_encode_handle_class_change(
    output: &mut Vec<char>,
    span_was_open: &mut bool,
    class_list: &SBufferHtmlClassList,
) {
    if *span_was_open {
        output.extend("</span>".chars());
    }
    if !class_list.is_empty() {
        output.extend("<span class=\"".chars());
        let mut is_first = true;
        for class in class_list.as_html_classes() {
            if !is_first {
                output.push(' ');
            }
            output.extend(class.chars());
            is_first = false;
        }
        output.extend("\">".chars());
        *span_was_open = true
    } else {
        *span_was_open = false
    }
}

fn html_push_normal_char(output: &mut Vec<char>, c: char) {
    match c {
        '<' => {
            output.extend("&lt;".chars());
        }
        '>' => {
            output.extend("&gt;".chars());
        }
        '&' => {
            output.extend("&amp;".chars());
        }
        c => {
            output.push(c);
        }
    }
}

fn html_handle_line_break(
    output: &mut Vec<char>,
    remaining_chars_in_line: &mut usize,
    cols: usize,
) {
    for _ in 0..*remaining_chars_in_line {
        output.push(SPACE);
    }
    output.push(LINE_BREAK);
    *remaining_chars_in_line = cols;
}

#[derive(Clone, Debug)]
/// SBuffer encoded terminal data, see module documentation.
///
/// To render into an actual SBuffer string, use the `Display` (or `ToString`) implementations.
pub struct SBuffer {
    string: String,
    cols: usize,
}

impl SBuffer {
    /// Captures the current screen contents (including cell attributes) of the given terminal.
    pub fn from_terminal(term: &Terminal) -> Self {
        let size = term.get_size();
        let mut screen = term.screen().clone();
        let mut encoded: Vec<char> = vec![START];
        // The attribute state a decoder reconstructs (see apply_cell_attrs).
        let mut wire_attr = CellAttributes::default();
        let mut current_char: Option<(char, usize)> = None;
        // Small buffer for pending attribute codes
        let mut codes: Vec<char> = Vec::new();
        let default_attr = CellAttributes::default();

        for y in 0..size.rows {
            for x in 0..size.cols {
                let maybe_cell = screen.get_cell(x, y.try_into().unwrap());
                // Never-touched cells are default-attributed blanks; they must take
                // part in attribute syncing like any other cell.
                let (attrs, text) = match maybe_cell {
                    Some(cell) => (cell.attrs(), cell.str()),
                    None => (&default_attr, " "),
                };
                codes.clear();
                apply_cell_attrs(&mut codes, &mut wire_attr, attrs, text == " ");
                if !codes.is_empty() {
                    // The codes must sit between the previous cells and this one:
                    // flush the pending run first.
                    if let Some((c, n)) = current_char.take() {
                        sbuffer_print_char(&mut encoded, c, n);
                    }
                    encoded.extend_from_slice(&codes);
                }
                // A cell must encode as exactly one character. Multi-scalar
                // graphemes (combining marks) are NFC-composed; if no precomposed
                // form exists, only the first scalar survives. Empty cells
                // (wide-char spacers) emit nothing.
                let cell_char = if text.chars().nth(1).is_some() {
                    text.nfc().next()
                } else {
                    text.chars().next()
                };
                if let Some(c) = cell_char {
                    match current_char {
                        Some((cur_c, ref mut n)) if c == cur_c => *n += 1,
                        _ => {
                            if let Some((cur_c, n)) = current_char.take() {
                                sbuffer_print_char(&mut encoded, cur_c, n);
                            }
                            current_char = Some((c, 1));
                        }
                    }
                }
            }
            // End of line: the trailing whitespace run is trimmed. Its background
            // and underline have already been synced by the blank-cell handling.
            if let Some((c, n)) = current_char.take()
                && c != SPACE
            {
                sbuffer_print_char(&mut encoded, c, n);
            }
            sbuffer_print_newline(&mut encoded);
        }

        Self {
            string: encoded.into_iter().collect(),
            cols: size.cols,
        }
    }

    /// Outputs an HTML representation of the SBuffer, as described in the module documentation.
    pub fn to_html(&self) -> String {
        let mut class_list = SBufferHtmlClassList::default();
        let mut output: Vec<char> = vec![];
        let mut span_was_open = false;
        let mut new_span_pending = false;

        let mut iter = self.string.chars();
        assert_eq!(Some(START), iter.next());

        let mut remaining_chars_in_line = self.cols;
        while let Some(c) = iter.next() {
            match c {
                LINE_BREAK => {
                    if new_span_pending {
                        html_encode_handle_class_change(
                            &mut output,
                            &mut span_was_open,
                            &class_list,
                        );
                        new_span_pending = false;
                    }
                    html_handle_line_break(&mut output, &mut remaining_chars_in_line, self.cols);
                }
                c if c >= FG_START && c < FG_END => {
                    class_list.fg = Some((c as u32 - FG_START as u32) as _);
                    new_span_pending = true;
                }
                FG_RESET => {
                    class_list.fg = None;
                    new_span_pending = true;
                }
                c if c >= BG_START && c < BG_END => {
                    class_list.bg = Some((c as u32 - BG_START as u32) as _);
                    new_span_pending = true;
                }
                BG_RESET => {
                    class_list.bg = None;
                    new_span_pending = true;
                }
                ESCAPE => {
                    let c = iter.next();
                    if let Some(c) = c {
                        if new_span_pending {
                            html_encode_handle_class_change(
                                &mut output,
                                &mut span_was_open,
                                &class_list,
                            );
                            new_span_pending = false;
                        }
                        if c == LINE_BREAK {
                            html_handle_line_break(
                                &mut output,
                                &mut remaining_chars_in_line,
                                self.cols,
                            );
                        } else {
                            html_push_normal_char(&mut output, c);
                            remaining_chars_in_line = remaining_chars_in_line.saturating_sub(1);
                        }
                    }
                }
                c if c >= REPEAT && c < REPEAT_END => {
                    let n = c as u32 - REPEAT as u32 + 1;
                    let c = iter.next();
                    if let Some(c) = c {
                        if new_span_pending {
                            html_encode_handle_class_change(
                                &mut output,
                                &mut span_was_open,
                                &class_list,
                            );
                            new_span_pending = false;
                        }
                        if c == LINE_BREAK {
                            html_handle_line_break(
                                &mut output,
                                &mut remaining_chars_in_line,
                                self.cols,
                            );
                        } else {
                            for _ in 0..n {
                                html_push_normal_char(&mut output, c);
                            }
                            remaining_chars_in_line =
                                remaining_chars_in_line.saturating_sub(n as _);
                        }
                    }
                }
                BOLD => {
                    class_list.bold = !class_list.bold;
                    new_span_pending = true;
                }
                DIM => {
                    class_list.dim = !class_list.dim;
                    new_span_pending = true;
                }
                ITAL => {
                    class_list.ital = !class_list.ital;
                    new_span_pending = true;
                }
                UNDL => {
                    class_list.undl = !class_list.undl;
                    new_span_pending = true;
                }
                c => {
                    if new_span_pending {
                        html_encode_handle_class_change(
                            &mut output,
                            &mut span_was_open,
                            &class_list,
                        );
                        new_span_pending = false;
                    }
                    html_push_normal_char(&mut output, c);
                    remaining_chars_in_line = remaining_chars_in_line.saturating_sub(1);
                }
            }
        }

        if new_span_pending {
            html_encode_handle_class_change(&mut output, &mut span_was_open, &class_list);
        }

        if output.last().copied() == Some(LINE_BREAK) {
            output.pop();
        } else {
            output.extend(vec![SPACE; remaining_chars_in_line]);
        }

        if span_was_open {
            output.extend("</span>".chars());
        }

        output.into_iter().collect()
    }
}

impl Display for SBuffer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.string.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    //! The SBuffer spec deliberately leaves encoders freedom (when to use RLE, code
    //! ordering, …). The `Display` tests below assert exact strings and thereby pin the
    //! canonical encoder policy for this implementation:
    //!
    //! - The output starts with U+E000; every row, including the last, ends with U+000A.
    //! - Attribute codes are emitted immediately before the first cell that needs them,
    //!   in the order: fg (U+E1xx/U+E400), bg (U+E2xx/U+E401), then toggles in
    //!   U+E402..=U+E405 order. Codes are only emitted when the state actually changes.
    //! - The trailing run of whitespace cells of a row is trimmed. Before the line
    //!   feed, background color and underline are normalized to the trimmed cells'
    //!   attributes, because they render on blank padding; foreground color, bold,
    //!   dim and italic may dangle across line feeds (invisible on blanks).
    //! - RLE is used for a run iff it is strictly shorter in UTF-8 bytes than the
    //!   literal run: length >= 5 for 1-byte chars, >= 3 for 2- and 3-byte chars.
    //!   The literal cost of PUA content includes its U+E300 escape (3 bytes per
    //!   cell), so reserved chars are RLE'd from runs of 2. Runs longer than 256
    //!   cells are chunked greedily (256 first).
    //! - Cells containing a PUA code point (U+E000..=U+F8FF) are escaped with U+E300.
    //! - Cells holding multi-scalar graphemes are reduced to one character: NFC
    //!   composition, falling back to the first scalar.
    //!
    //! Wide characters are deliberately not tested: their encoding is underspecified
    //! in spec v1 ("assumed to only take up one cell").
    //!
    //! The test terminal is a real in-process `wezterm_term::Terminal`; input is fed
    //! as raw pty output bytes, so cell contents/attributes come from real ANSI
    //! escape-sequence processing.

    use super::SBuffer;
    use shadow_terminal::wezterm_term::color::ColorPalette;
    use shadow_terminal::wezterm_term::{Terminal, TerminalConfiguration, TerminalSize};
    use std::sync::Arc;

    #[derive(Debug)]
    struct TestConfig;

    impl TerminalConfiguration for TestConfig {
        fn color_palette(&self) -> ColorPalette {
            ColorPalette::default()
        }
    }

    fn sbuffer(cols: usize, rows: usize, ansi: &str) -> SBuffer {
        let mut term = Terminal::new(
            TerminalSize {
                cols,
                rows,
                ..TerminalSize::default()
            },
            Arc::new(TestConfig),
            "trmnl-console-test",
            "1",
            Box::new(Vec::new()),
        );
        term.advance_bytes(ansi.as_bytes());
        SBuffer::from_terminal(&term)
    }

    fn enc(cols: usize, rows: usize, ansi: &str) -> String {
        sbuffer(cols, rows, ansi).to_string()
    }

    fn html(cols: usize, rows: usize, ansi: &str) -> String {
        sbuffer(cols, rows, ansi).to_html()
    }

    mod from_terminal {
        use super::enc;

        #[test]
        fn captures_device_sized_terminal() {
            // From-capture is infallible; smoke test on the real device dimensions.
            let s = enc(114, 32, "");
            assert!(s.starts_with('\u{E000}'));
            assert_eq!(s.matches('\n').count(), 32);
        }
    }

    mod display {
        use super::enc;

        #[test]
        fn empty_terminal() {
            assert_eq!(enc(4, 2, ""), "\u{E000}\n\n");
        }

        #[test]
        fn plain_text() {
            assert_eq!(enc(10, 2, "hi"), "\u{E000}hi\n\n");
        }

        #[test]
        fn trailing_whitespace_is_trimmed() {
            assert_eq!(enc(10, 1, "hi   "), "\u{E000}hi\n");
        }

        #[test]
        fn inner_whitespace_is_kept() {
            assert_eq!(enc(10, 1, "a b"), "\u{E000}a b\n");
        }

        #[test]
        fn text_on_later_row() {
            assert_eq!(enc(10, 2, "ab\r\ncd"), "\u{E000}ab\ncd\n");
        }

        #[test]
        fn html_specials_are_not_escaped_in_sbuffer() {
            assert_eq!(enc(10, 1, "<&>"), "\u{E000}<&>\n");
        }

        #[test]
        fn no_rle_below_saving_threshold_ascii() {
            // 4 literal bytes vs 4 RLE bytes: not strictly smaller, stays literal.
            assert_eq!(enc(10, 1, "===="), "\u{E000}====\n");
        }

        #[test]
        fn rle_at_saving_threshold_ascii() {
            // 5 cells => n = 4 (code outputs n+1 repetitions).
            assert_eq!(enc(10, 1, "====="), "\u{E000}\u{E304}=\n");
        }

        #[test]
        fn rle_longer_run() {
            assert_eq!(enc(12, 1, "=========="), "\u{E000}\u{E309}=\n");
        }

        #[test]
        fn no_rle_below_saving_threshold_multibyte() {
            // '█' is 3 UTF-8 bytes: 2 literals (6 bytes) == RLE (6 bytes), stays literal.
            assert_eq!(enc(10, 1, "██"), "\u{E000}██\n");
        }

        #[test]
        fn rle_at_saving_threshold_multibyte() {
            assert_eq!(enc(10, 1, "███"), "\u{E000}\u{E302}█\n");
        }

        #[test]
        fn rle_chunks_greedily_at_256() {
            let run = "=".repeat(300);
            // 256 cells (n = 255) + 44 cells (n = 43).
            assert_eq!(enc(300, 1, &run), "\u{E000}\u{E3FF}=\u{E32B}=\n");
        }

        #[test]
        fn rle_chunks_multibyte_in_cells_not_bytes() {
            // The 256-cell limit of one RLE code is independent of the char's
            // UTF-8 length: 300 cells chunk as 256 + 44, same as for ASCII.
            let run = "█".repeat(300);
            assert_eq!(enc(300, 1, &run), "\u{E000}\u{E3FF}█\u{E32B}█\n");
        }

        #[test]
        fn rle_applies_to_inner_spaces() {
            assert_eq!(enc(20, 1, "a          b"), "\u{E000}a\u{E309} b\n");
        }

        #[test]
        fn fg_color_dangles_at_line_end() {
            // Foreground is invisible on blank cells, so it is not reset before
            // the line feed; decoders pad using the still-open state.
            assert_eq!(enc(10, 1, "\x1b[31mab"), "\u{E000}\u{E101}ab\n");
        }

        #[test]
        fn fg_code_is_emitted_only_on_state_change() {
            assert_eq!(enc(10, 1, "\x1b[31ma\x1b[31mb"), "\u{E000}\u{E101}ab\n");
        }

        #[test]
        fn attr_codes_follow_a_pending_run() {
            // Regression: attribute codes must not be emitted before the still
            // accumulating run of previous (default-attribute) cells.
            assert_eq!(enc(10, 1, "aa\x1b[31mbb"), "\u{E000}aa\u{E101}bb\n");
        }

        #[test]
        fn attr_change_splits_a_same_char_run() {
            // Regression: an attribute change must split the run even when the
            // character stays the same.
            assert_eq!(enc(10, 1, "aa\x1b[31ma"), "\u{E000}aa\u{E101}a\n");
        }

        #[test]
        fn bright_fg_color() {
            assert_eq!(enc(10, 1, "\x1b[91mx"), "\u{E000}\u{E109}x\n");
        }

        #[test]
        fn indexed_256_fg_color() {
            assert_eq!(enc(10, 1, "\x1b[38;5;200mx"), "\u{E000}\u{E1C8}x\n");
        }

        #[test]
        fn bg_color() {
            // Background renders on blank padding, so it must be normalized to
            // the (default) trailing cells before the line feed.
            assert_eq!(enc(10, 1, "\x1b[44mx"), "\u{E000}\u{E204}x\u{E401}\n");
        }

        #[test]
        fn bold_toggles_on_and_off() {
            assert_eq!(
                enc(10, 1, "\x1b[1mB\x1b[0mn"),
                "\u{E000}\u{E402}B\u{E402}n\n"
            );
        }

        #[test]
        fn dim_italic_underline_toggles() {
            // Per-cell changes are emitted in U+E402..=U+E405 order
            // (e.g. dim-off before italic-on for cell 'b'). Underline renders on
            // blank padding, so it is toggled off again before the line feed.
            assert_eq!(
                enc(10, 1, "\x1b[2ma\x1b[0m\x1b[3mb\x1b[0m\x1b[4mc"),
                "\u{E000}\u{E403}a\u{E403}\u{E404}b\u{E404}\u{E405}c\u{E405}\n"
            );
        }

        #[test]
        fn code_emission_order_is_fg_bg_toggles() {
            // At the line end only the bg needs normalizing; fg and bold dangle.
            assert_eq!(
                enc(10, 1, "\x1b[31;44;1mx"),
                "\u{E000}\u{E101}\u{E204}\u{E402}x\u{E401}\n"
            );
        }

        #[test]
        fn reverse_video_swaps_default_colors() {
            // Reverse video has no SBuffer code; the encoder resolves it by
            // swapping the effective colors. Default colors substitute their
            // standard palette indices: default-fg -> 0 (black), default-bg
            // -> 15 (white), so reversed default text is white-on-black.
            assert_eq!(
                enc(10, 1, "\x1b[7mab"),
                "\u{E000}\u{E10F}\u{E200}ab\u{E401}\n"
            );
        }

        #[test]
        fn reverse_video_swaps_palette_colors() {
            // Red-on-blue reversed encodes as blue-on-red: fg <- 4, bg <- 1.
            assert_eq!(
                enc(10, 1, "\x1b[31;44;7mx"),
                "\u{E000}\u{E104}\u{E201}x\u{E401}\n"
            );
        }

        #[test]
        fn full_row_keeps_state_across_line_feed() {
            // Row 0 is completely filled: no trailing empty space. The dangling
            // fg state is invisible on row 1's blank cells, so no reset is needed
            // there either.
            assert_eq!(enc(3, 2, "\x1b[31mabc"), "\u{E000}\u{E101}abc\n\n");
        }

        #[test]
        fn bg_painted_empty_row_keeps_attributes() {
            // Erase-to-EOL paints the row with the current background (BCE). The
            // all-whitespace row is trimmed to nothing, but must carry bg 4 so that
            // decoders pad it correctly; row 1 is default again, so the bg must be
            // normalized back before its line feed.
            assert_eq!(enc(4, 2, "\x1b[44m\x1b[K"), "\u{E000}\u{E204}\n\u{E401}\n");
        }

        #[test]
        fn pua_content_is_escaped() {
            assert_eq!(enc(10, 1, "\u{E005}"), "\u{E000}\u{E300}\u{E005}\n");
        }

        #[test]
        fn rle_threshold_accounts_for_escape_overhead() {
            // A literal reserved char costs 3 (escape) + 3 (char) bytes per cell,
            // so RLE (6 bytes total) already wins for a run of 2.
            assert_eq!(enc(10, 1, "\u{E005}\u{E005}"), "\u{E000}\u{E301}\u{E005}\n");
        }

        #[test]
        fn combining_mark_grapheme_is_nfc_composed() {
            // A cell may hold a multi-scalar grapheme (combining mark attached to
            // the previous char). One cell must encode as ONE character, or all
            // following columns shift. NFC composition preserves the glyph here:
            // "e" + U+0301 -> "é" (U+00E9).
            assert_eq!(enc(10, 1, "e\u{301}"), "\u{E000}é\n");
        }

        #[test]
        fn non_composable_combining_mark_is_dropped() {
            // "x" + U+0332 (combining low line) has no precomposed form: only the
            // first scalar is emitted so the cell grid stays intact.
            assert_eq!(enc(10, 1, "x\u{332}"), "\u{E000}x\n");
        }

        #[test]
        fn escape_char_as_content_is_escaped() {
            // The trickiest content char: U+E300 itself (escape == RLE base).
            assert_eq!(enc(10, 1, "\u{E300}"), "\u{E000}\u{E300}\u{E300}\n");
        }

        #[test]
        fn rle_at_saving_threshold_4byte() {
            // U+1D400 (MATHEMATICAL BOLD CAPITAL A) is 4 UTF-8 bytes and width 1:
            // 2 literals (8 bytes) > RLE (7 bytes), so RLE already wins at 2.
            assert_eq!(
                enc(10, 1, "\u{1D400}\u{1D400}"),
                "\u{E000}\u{E301}\u{1D400}\n"
            );
        }

        #[test]
        fn truecolor_degrades_to_default() {
            // Known v1 limitation: 24-bit SGR colors arrive as
            // TrueColorWithDefaultFallback and degrade to the default color.
            // The E400 is redundant (wire state was already default) but
            // harmless: the wire state tracks the ColorAttribute, not the
            // emitted code. Candidate for a byte-saving optimization later.
            assert_eq!(enc(10, 1, "\x1b[38;2;100;100;100mx"), "\u{E000}\u{E400}x\n");
        }
    }

    mod to_html {
        use super::SBuffer;
        use super::html;

        #[test]
        fn plain_text_is_padded_to_width() {
            assert_eq!(html(4, 2, "hi"), "hi  \n    ");
        }

        #[test]
        fn html_specials_are_escaped() {
            assert_eq!(html(5, 1, "<&>"), "&lt;&amp;&gt;  ");
        }

        #[test]
        fn fg_span_open_at_eof_contains_padding() {
            // The encoder lets fg dangle at the line end, so the state at the
            // (final) line feed still has fg 1 and the padding is rendered inside
            // the span. Mirrors test_fg_span_open_at_eof_contains_padding on the
            // Python side.
            assert_eq!(
                html(4, 1, "\x1b[31mab"),
                "<span class=\"tc--fg-1\">ab  </span>"
            );
        }

        #[test]
        fn bg_span_contains_row_padding() {
            assert_eq!(
                html(4, 1, "\x1b[44m\x1b[K"),
                "<span class=\"tc--bg-4\">    </span>"
            );
        }

        #[test]
        fn overlapping_fg_bg_produce_flat_spans() {
            assert_eq!(
                html(3, 1, "\x1b[31ma\x1b[44mb\x1b[33mc"),
                "<span class=\"tc--fg-1\">a</span>\
                 <span class=\"tc--fg-1 tc--bg-4\">b</span>\
                 <span class=\"tc--fg-3 tc--bg-4\">c</span>"
            );
        }

        #[test]
        fn bold_span() {
            assert_eq!(
                html(6, 1, "\x1b[1mB\x1b[0mn"),
                "<span class=\"tc--bold\">B</span>n    "
            );
        }

        #[test]
        fn rle_is_expanded() {
            assert_eq!(html(6, 1, "======"), "======");
        }

        #[test]
        fn rle_expansion_is_escaped() {
            assert_eq!(html(6, 1, "<<<<<<"), "&lt;&lt;&lt;&lt;&lt;&lt;");
        }

        #[test]
        fn span_continues_across_rows() {
            assert_eq!(
                html(3, 2, "\x1b[44m\x1b[K\r\n\x1b[K"),
                "<span class=\"tc--bg-4\">   \n   </span>"
            );
        }

        #[test]
        fn pua_content_is_raw() {
            assert_eq!(html(3, 1, "\u{E005}"), "\u{E005}  ");
        }

        // The following decode inputs the encoder never produces; they are built
        // directly instead of via a terminal capture.
        fn html_raw(sbuffer: &str, cols: usize) -> String {
            SBuffer {
                string: sbuffer.into(),
                cols,
            }
            .to_html()
        }

        #[test]
        fn overflowing_row_is_output_in_full() {
            // Row of 10 cells with an advertised width of 4: no padding, the
            // full row is still output.
            assert_eq!(html_raw("\u{E000}\u{E309}a", 4), "aaaaaaaaaa");
        }

        #[test]
        fn escape_before_line_feed_is_a_normal_line_feed() {
            assert_eq!(html_raw("\u{E000}a\u{E300}\nb", 3), "a  \nb  ");
        }

        #[test]
        fn rle_before_line_feed_is_a_normal_line_feed() {
            assert_eq!(html_raw("\u{E000}a\u{E303}\nb", 3), "a  \nb  ");
        }

        #[test]
        fn dangling_escape_at_eof_is_ignored() {
            assert_eq!(html_raw("\u{E000}a\u{E300}", 3), "a  ");
        }

        #[test]
        fn dangling_rle_at_eof_is_ignored() {
            assert_eq!(html_raw("\u{E000}a\u{E305}", 3), "a  ");
        }

        #[test]
        fn dim_span() {
            assert_eq!(
                html(6, 1, "\x1b[2mD\x1b[0mn"),
                "<span class=\"tc--dim\">D</span>n    "
            );
        }

        #[test]
        fn ital_span() {
            assert_eq!(
                html(6, 1, "\x1b[3mI\x1b[0mn"),
                "<span class=\"tc--ital\">I</span>n    "
            );
        }

        #[test]
        fn undl_span() {
            assert_eq!(
                html(6, 1, "\x1b[4mU\x1b[0mn"),
                "<span class=\"tc--undl\">U</span>n    "
            );
        }

        #[test]
        fn reverse_video_spans() {
            // Encodes as fg-15/bg-0 with the bg reset before the LF; the
            // dangling fg-15 keeps the padding in its own span.
            assert_eq!(
                html(4, 1, "\x1b[7mab"),
                "<span class=\"tc--fg-15 tc--bg-0\">ab</span>\
                 <span class=\"tc--fg-15\">  </span>"
            );
        }

        // Mirrors of the Python decoder tests in plugin/test-transform.py
        // (shared expected-HTML vectors; both decoders must agree).

        #[test]
        fn version_char_only() {
            assert_eq!(html_raw("\u{E000}", 5), "     ");
        }

        #[test]
        fn trailing_lf_is_equivalent_to_no_trailing_lf() {
            assert_eq!(html_raw("\u{E000}hi\n", 4), html_raw("\u{E000}hi", 4));
        }

        #[test]
        fn empty_middle_row() {
            assert_eq!(html_raw("\u{E000}a\n\nb", 3), "a  \n   \nb  ");
        }

        #[test]
        fn fg_change_replaces_class() {
            assert_eq!(
                html_raw("\u{E000}\u{E101}a\u{E102}b", 2),
                "<span class=\"tc--fg-1\">a</span><span class=\"tc--fg-2\">b</span>"
            );
        }

        #[test]
        fn reset_without_open_state_is_noop() {
            assert_eq!(html_raw("\u{E000}\u{E400}\u{E401}x", 1), "x");
        }

        #[test]
        fn interleaved_toggles() {
            // bold on, italic on, bold off: the flat model re-opens with the
            // currently applied class set each time.
            assert_eq!(
                html_raw("\u{E000}\u{E402}a\u{E404}b\u{E402}c\u{E404}", 3),
                "<span class=\"tc--bold\">a</span>\
                 <span class=\"tc--bold tc--ital\">b</span>\
                 <span class=\"tc--ital\">c</span>"
            );
        }

        #[test]
        fn class_order_is_canonical() {
            assert_eq!(
                html_raw(
                    "\u{E000}\u{E101}\u{E202}\u{E402}\u{E403}\u{E404}\u{E405}x",
                    1
                ),
                "<span class=\"tc--fg-1 tc--bg-2 tc--bold tc--dim tc--ital tc--undl\">x</span>"
            );
        }

        #[test]
        fn color_class_is_decimal() {
            assert_eq!(
                html_raw("\u{E000}\u{E1C8}x", 1),
                "<span class=\"tc--fg-200\">x</span>"
            );
        }

        #[test]
        fn color_class_zero_index() {
            // Guards against treating color index 0 as falsy in either decoder.
            assert_eq!(
                html_raw("\u{E000}\u{E100}x", 1),
                "<span class=\"tc--fg-0\">x</span>"
            );
        }

        #[test]
        fn escape_outputs_control_char_raw() {
            assert_eq!(html_raw("\u{E000}\u{E300}\u{E101}", 1), "\u{E101}");
        }

        #[test]
        fn escaped_escape() {
            assert_eq!(html_raw("\u{E000}\u{E300}\u{E300}", 1), "\u{E300}");
        }

        #[test]
        fn rle_repeats_control_char_literally() {
            assert_eq!(
                html_raw("\u{E000}\u{E302}\u{E101}", 3),
                "\u{E101}\u{E101}\u{E101}"
            );
        }

        #[test]
        fn rle_max_chunk() {
            assert_eq!(html_raw("\u{E000}\u{E3FF}=", 256), "=".repeat(256));
        }

        #[test]
        fn fg_span_across_rows_with_reset() {
            // Row 0's padding is inside the span (state at LF has fg 1); the
            // reset after 'b' closes it, so row 1's padding is outside.
            assert_eq!(
                html_raw("\u{E000}\u{E101}a\nb\u{E400}", 3),
                "<span class=\"tc--fg-1\">a  \nb</span>  "
            );
        }

        #[test]
        fn bg_padding_across_rows() {
            assert_eq!(
                html_raw("\u{E000}\u{E202}\nx", 3),
                "<span class=\"tc--bg-2\">   \nx  </span>"
            );
        }
    }
}
