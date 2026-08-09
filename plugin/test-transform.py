#!/usr/bin/env python3
"""
Tests for src/transform.py (sbuffer_to_html and run).

Run from the plugin/ directory: python3 test-transform.py

Unlike the Rust side (which encodes real terminal screens), these tests feed
hand-written SBuffer strings, so they also cover decoder-only corner cases the
canonical Rust encoder would never emit (missing trailing line feed, redundant
resets, RLE of control characters, ...). The expected HTML pins the canonical
class order: tc--fg-N, tc--bg-N, tc--bold, tc--dim, tc--ital, tc--undl.
"""

import sys
import unittest
from os import path

sys.path.insert(0, path.join(path.dirname(path.abspath(__file__)), "src"))

from transform import run, sbuffer_to_html

V = "\ue000"
ESC = "\ue300"
RESET_FG = "\ue400"
RESET_BG = "\ue401"
BOLD = "\ue402"
DIM = "\ue403"
ITAL = "\ue404"
UNDL = "\ue405"


def fg(n: int) -> str:
    return chr(0xE100 + n)


def bg(n: int) -> str:
    return chr(0xE200 + n)


def rle(n: int) -> str:
    """RLE control code: outputs the following character n+1 times."""
    return chr(0xE300 + n)


class SBufferToHtmlTest(unittest.TestCase):
    def test_version_char_only(self):
        # No trailing LF: decoders pretend there is one, so this is one empty row.
        self.assertEqual(sbuffer_to_html(V, 5), "     ")

    def test_plain_text_padded(self):
        self.assertEqual(sbuffer_to_html(V + "hi", 4), "hi  ")

    def test_trailing_lf_is_equivalent_to_no_trailing_lf(self):
        self.assertEqual(sbuffer_to_html(V + "hi\n", 4), sbuffer_to_html(V + "hi", 4))

    def test_multiple_rows(self):
        self.assertEqual(sbuffer_to_html(V + "ab\ncd", 4), "ab  \ncd  ")

    def test_empty_middle_row(self):
        self.assertEqual(sbuffer_to_html(V + "a\n\nb", 3), "a  \n   \nb  ")

    def test_html_specials_escaped(self):
        self.assertEqual(sbuffer_to_html(V + "<&>", 3), "&lt;&amp;&gt;")

    def test_fg_span_with_reset(self):
        self.assertEqual(
            sbuffer_to_html(V + fg(1) + "ab" + RESET_FG, 4),
            '<span class="tc--fg-1">ab</span>  ',
        )

    def test_fg_span_open_at_eof_contains_padding(self):
        # The state at the (pretended) final LF still has fg 1, so the padding
        # is rendered inside the span; EOF then closes it.
        self.assertEqual(
            sbuffer_to_html(V + fg(1) + "ab", 4),
            '<span class="tc--fg-1">ab  </span>',
        )

    def test_bg_only_row_is_padded_inside_span(self):
        self.assertEqual(
            sbuffer_to_html(V + bg(4), 3), '<span class="tc--bg-4">   </span>'
        )

    def test_overlapping_fg_bg_produce_flat_spans(self):
        self.assertEqual(
            sbuffer_to_html(V + fg(1) + "a" + bg(2) + "b" + fg(3) + "c", 3),
            '<span class="tc--fg-1">a</span>'
            '<span class="tc--fg-1 tc--bg-2">b</span>'
            '<span class="tc--fg-3 tc--bg-2">c</span>',
        )

    def test_fg_change_replaces_class(self):
        self.assertEqual(
            sbuffer_to_html(V + fg(1) + "a" + fg(2) + "b", 2),
            '<span class="tc--fg-1">a</span><span class="tc--fg-2">b</span>',
        )

    def test_reset_without_open_state_is_noop(self):
        self.assertEqual(sbuffer_to_html(V + RESET_FG + RESET_BG + "x", 1), "x")

    def test_bold_toggle(self):
        self.assertEqual(
            sbuffer_to_html(V + BOLD + "B" + BOLD + "n", 2),
            '<span class="tc--bold">B</span>n',
        )

    def test_interleaved_toggles(self):
        # bold on, italic on, bold off: impossible with nested spans, the flat
        # model re-opens with the currently applied class set each time.
        self.assertEqual(
            sbuffer_to_html(V + BOLD + "a" + ITAL + "b" + BOLD + "c" + ITAL, 3),
            '<span class="tc--bold">a</span>'
            '<span class="tc--bold tc--ital">b</span>'
            '<span class="tc--ital">c</span>',
        )

    def test_class_order_is_canonical(self):
        self.assertEqual(
            sbuffer_to_html(V + fg(1) + bg(2) + BOLD + DIM + ITAL + UNDL + "x", 1),
            '<span class="tc--fg-1 tc--bg-2 tc--bold tc--dim tc--ital tc--undl">x</span>',
        )

    def test_color_index_is_decimal(self):
        self.assertEqual(
            sbuffer_to_html(V + fg(200) + "x", 1), '<span class="tc--fg-200">x</span>'
        )

    def test_color_index_zero(self):
        # Guards against treating color index 0 as falsy.
        self.assertEqual(
            sbuffer_to_html(V + fg(0) + "x", 1), '<span class="tc--fg-0">x</span>'
        )

    def test_rle_expands_n_plus_one(self):
        self.assertEqual(sbuffer_to_html(V + rle(1) + "a", 2), "aa")

    def test_rle_longer_run(self):
        self.assertEqual(sbuffer_to_html(V + rle(9) + "=", 10), "=" * 10)

    def test_rle_max_chunk(self):
        self.assertEqual(sbuffer_to_html(V + rle(255) + "=", 256), "=" * 256)

    def test_rle_expansion_is_escaped(self):
        self.assertEqual(sbuffer_to_html(V + rle(4) + "<", 5), "&lt;" * 5)

    def test_rle_repeats_control_char_literally(self):
        # The character following an RLE code is always literal, even a control code.
        self.assertEqual(sbuffer_to_html(V + rle(2) + fg(1), 3), "\ue101" * 3)

    def test_escape_makes_control_char_literal(self):
        self.assertEqual(sbuffer_to_html(V + ESC + fg(1), 1), "\ue101")

    def test_escaped_escape(self):
        self.assertEqual(sbuffer_to_html(V + ESC + ESC, 1), "\ue300")

    def test_span_continues_across_rows(self):
        # Row 0's padding is inside the span (state at LF has fg 1); the reset
        # after 'b' closes it, so row 1's padding is outside.
        self.assertEqual(
            sbuffer_to_html(V + fg(1) + "a\nb" + RESET_FG, 3),
            '<span class="tc--fg-1">a  \nb</span>  ',
        )

    def test_bg_padding_across_rows(self):
        self.assertEqual(
            sbuffer_to_html(V + bg(2) + "\nx", 3),
            '<span class="tc--bg-2">   \nx  </span>',
        )

    def test_overflowing_row_is_output_in_full(self):
        # Row of 10 cells with an advertised width of 4: no padding, the full
        # row is still output.
        self.assertEqual(sbuffer_to_html(V + rle(9) + "a", 4), "a" * 10)

    def test_escape_before_line_feed_is_a_normal_line_feed(self):
        self.assertEqual(sbuffer_to_html(V + "a" + ESC + "\nb", 3), "a  \nb  ")

    def test_rle_before_line_feed_is_a_normal_line_feed(self):
        self.assertEqual(sbuffer_to_html(V + "a" + rle(3) + "\nb", 3), "a  \nb  ")

    def test_dangling_escape_at_eof_is_ignored(self):
        self.assertEqual(sbuffer_to_html(V + "a" + ESC, 3), "a  ")

    def test_dangling_rle_at_eof_is_ignored(self):
        self.assertEqual(sbuffer_to_html(V + "a" + rle(5), 3), "a  ")

    def test_missing_version_char(self):
        self.assertRaises(ValueError, sbuffer_to_html, "hi", 3)


class RunTest(unittest.TestCase):
    def test_transforms_content_using_width(self):
        result = run({"data": {"content": V + "hi", "width": 4}})
        self.assertEqual(result, {"data": {"content_transformed": "hi  ", "width": 4}})

    def test_input_without_data_returns_error(self):
        self.assertEqual(run({"foo": 1}), {"error": "plugin did not receive any data yet."})

    def test_data_without_content_returns_error(self):
        self.assertEqual(run({"data": {"x": 1}}), {"error": "plugin did not receive console input and width."})


if __name__ == "__main__":
    unittest.main()
