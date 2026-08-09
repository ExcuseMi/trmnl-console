#!/usr/bin/env node
/*
 * Tests for the client-side SBuffer decoder in src/shared.liquid (the
 * JavaScript sbuffer_to_html that runs during render when the serverless
 * transform did not).
 *
 * Run from the plugin/ directory: node test-shared-liquid.js
 *
 * These are the same test vectors as test-transform.py (which tests the Python
 * sbuffer_to_html in src/transform.py) — the two decoders must stay
 * equivalent, so keep the test lists in sync; test names match the Python
 * methods 1:1. The RunTest cases have no JavaScript counterpart: the
 * equivalent wiring in shared.liquid is Liquid + DOM glue that cannot run
 * outside the device.
 *
 * The decoder is extracted verbatim from shared.liquid: everything between
 * the `<script type="text/javascript" class="tc--decompress-script">` tag and the `// ---` separator line
 * must be pure JavaScript (no Liquid markup, no DOM access) so that it can be
 * evaluated here in Node.
 */
"use strict";

const assert = require("node:assert/strict");
const {test} = require("node:test");
const fs = require("node:fs");
const path = require("node:path");

const LIQUID_PATH = path.join(__dirname, "src", "shared.liquid");
const SCRIPT_OPEN = '<script type="text/javascript" class="tc--decompress-script">';
const DECODER_END = "// ---";

function extractDecoderSource() {
    const liquid = fs.readFileSync(LIQUID_PATH, "utf8");
    const start = liquid.indexOf(SCRIPT_OPEN);
    assert.notEqual(start, -1, `${SCRIPT_OPEN} not found in shared.liquid`);
    const end = liquid.indexOf(DECODER_END, start);
    assert.notEqual(
        end,
        -1,
        `"${DECODER_END}" separator not found after the script tag in shared.liquid`,
    );
    const source = liquid.slice(start + SCRIPT_OPEN.length, end);
    for (const marker of ["{{", "{%"]) {
        assert.ok(
            !source.includes(marker),
            `the decoder part of the script block must be pure JavaScript, found Liquid marker "${marker}"`,
        );
    }
    return source;
}

const sbuffer_to_html = new Function(
    `"use strict";\n${extractDecoderSource()}\nreturn sbuffer_to_html;`,
)();

const V = "\uE000";
const ESC = "\uE300";
const RESET_FG = "\uE400";
const RESET_BG = "\uE401";
const BOLD = "\uE402";
const DIM = "\uE403";
const ITAL = "\uE404";
const UNDL = "\uE405";

const fg = (n) => String.fromCharCode(0xe100 + n);
const bg = (n) => String.fromCharCode(0xe200 + n);
// RLE control code: outputs the following character n+1 times.
const rle = (n) => String.fromCharCode(0xe300 + n);

test("test_version_char_only", () => {
    // No trailing LF: decoders pretend there is one, so this is one empty row.
    assert.equal(sbuffer_to_html(V, 5), "     ");
});

test("test_plain_text_padded", () => {
    assert.equal(sbuffer_to_html(V + "hi", 4), "hi  ");
});

test("test_trailing_lf_is_equivalent_to_no_trailing_lf", () => {
    assert.equal(sbuffer_to_html(V + "hi\n", 4), sbuffer_to_html(V + "hi", 4));
});

test("test_multiple_rows", () => {
    assert.equal(sbuffer_to_html(V + "ab\ncd", 4), "ab  \ncd  ");
});

test("test_empty_middle_row", () => {
    assert.equal(sbuffer_to_html(V + "a\n\nb", 3), "a  \n   \nb  ");
});

test("test_html_specials_escaped", () => {
    assert.equal(sbuffer_to_html(V + "<&>", 3), "&lt;&amp;&gt;");
});

test("test_fg_span_with_reset", () => {
    assert.equal(
        sbuffer_to_html(V + fg(1) + "ab" + RESET_FG, 4),
        '<span class="tc--fg-1">ab</span>  ',
    );
});

test("test_fg_span_open_at_eof_contains_padding", () => {
    // The state at the (pretended) final LF still has fg 1, so the padding
    // is rendered inside the span; EOF then closes it.
    assert.equal(
        sbuffer_to_html(V + fg(1) + "ab", 4),
        '<span class="tc--fg-1">ab  </span>',
    );
});

test("test_bg_only_row_is_padded_inside_span", () => {
    assert.equal(sbuffer_to_html(V + bg(4), 3), '<span class="tc--bg-4">   </span>');
});

test("test_overlapping_fg_bg_produce_flat_spans", () => {
    assert.equal(
        sbuffer_to_html(V + fg(1) + "a" + bg(2) + "b" + fg(3) + "c", 3),
        '<span class="tc--fg-1">a</span>' +
        '<span class="tc--fg-1 tc--bg-2">b</span>' +
        '<span class="tc--fg-3 tc--bg-2">c</span>',
    );
});

test("test_fg_change_replaces_class", () => {
    assert.equal(
        sbuffer_to_html(V + fg(1) + "a" + fg(2) + "b", 2),
        '<span class="tc--fg-1">a</span><span class="tc--fg-2">b</span>',
    );
});

test("test_reset_without_open_state_is_noop", () => {
    assert.equal(sbuffer_to_html(V + RESET_FG + RESET_BG + "x", 1), "x");
});

test("test_bold_toggle", () => {
    assert.equal(
        sbuffer_to_html(V + BOLD + "B" + BOLD + "n", 2),
        '<span class="tc--bold">B</span>n',
    );
});

test("test_interleaved_toggles", () => {
    // bold on, italic on, bold off: impossible with nested spans, the flat
    // model re-opens with the currently applied class set each time.
    assert.equal(
        sbuffer_to_html(V + BOLD + "a" + ITAL + "b" + BOLD + "c" + ITAL, 3),
        '<span class="tc--bold">a</span>' +
        '<span class="tc--bold tc--ital">b</span>' +
        '<span class="tc--ital">c</span>',
    );
});

test("test_class_order_is_canonical", () => {
    assert.equal(
        sbuffer_to_html(V + fg(1) + bg(2) + BOLD + DIM + ITAL + UNDL + "x", 1),
        '<span class="tc--fg-1 tc--bg-2 tc--bold tc--dim tc--ital tc--undl">x</span>',
    );
});

test("test_color_index_is_decimal", () => {
    assert.equal(sbuffer_to_html(V + fg(200) + "x", 1), '<span class="tc--fg-200">x</span>');
});

test("test_color_index_zero", () => {
    // Guards against treating color index 0 as falsy.
    assert.equal(sbuffer_to_html(V + fg(0) + "x", 1), '<span class="tc--fg-0">x</span>');
});

test("test_rle_expands_n_plus_one", () => {
    assert.equal(sbuffer_to_html(V + rle(1) + "a", 2), "aa");
});

test("test_rle_longer_run", () => {
    assert.equal(sbuffer_to_html(V + rle(9) + "=", 10), "=".repeat(10));
});

test("test_rle_max_chunk", () => {
    assert.equal(sbuffer_to_html(V + rle(255) + "=", 256), "=".repeat(256));
});

test("test_rle_expansion_is_escaped", () => {
    assert.equal(sbuffer_to_html(V + rle(4) + "<", 5), "&lt;".repeat(5));
});

test("test_rle_repeats_control_char_literally", () => {
    // The character following an RLE code is always literal, even a control code.
    assert.equal(sbuffer_to_html(V + rle(2) + fg(1), 3), "\uE101".repeat(3));
});

test("test_escape_makes_control_char_literal", () => {
    assert.equal(sbuffer_to_html(V + ESC + fg(1), 1), "\uE101");
});

test("test_escaped_escape", () => {
    assert.equal(sbuffer_to_html(V + ESC + ESC, 1), "\uE300");
});

test("test_span_continues_across_rows", () => {
    // Row 0's padding is inside the span (state at LF has fg 1); the reset
    // after 'b' closes it, so row 1's padding is outside.
    assert.equal(
        sbuffer_to_html(V + fg(1) + "a\nb" + RESET_FG, 3),
        '<span class="tc--fg-1">a  \nb</span>  ',
    );
});

test("test_bg_padding_across_rows", () => {
    assert.equal(
        sbuffer_to_html(V + bg(2) + "\nx", 3),
        '<span class="tc--bg-2">   \nx  </span>',
    );
});

test("test_overflowing_row_is_output_in_full", () => {
    // Row of 10 cells with an advertised width of 4: no padding, the full
    // row is still output.
    assert.equal(sbuffer_to_html(V + rle(9) + "a", 4), "a".repeat(10));
});

test("test_escape_before_line_feed_is_a_normal_line_feed", () => {
    assert.equal(sbuffer_to_html(V + "a" + ESC + "\nb", 3), "a  \nb  ");
});

test("test_rle_before_line_feed_is_a_normal_line_feed", () => {
    assert.equal(sbuffer_to_html(V + "a" + rle(3) + "\nb", 3), "a  \nb  ");
});

test("test_dangling_escape_at_eof_is_ignored", () => {
    assert.equal(sbuffer_to_html(V + "a" + ESC, 3), "a  ");
});

test("test_dangling_rle_at_eof_is_ignored", () => {
    assert.equal(sbuffer_to_html(V + "a" + rle(5), 3), "a  ");
});

test("test_missing_version_char", () => {
    assert.throws(() => sbuffer_to_html("hi", 3));
});
