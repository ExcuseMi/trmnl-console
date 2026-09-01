"""
NOTE: Previously TRMNL did not actually run this serverless transport,
since it was not supported for webhooks.

It is now supported and should run, but if for some reason it doesn't,
the decompression is instead done in JavaScript, see shared.liquid.
"""

from dataclasses import dataclass


class SBufferChar:
    START = '\uE000'
    SPACE = " "
    LINE_BREAK = "\n"
    FG_START = '\uE100'
    FG_END = '\uE200'
    BG_START = FG_END
    BG_END = '\uE300'
    ESCAPE = BG_END
    # this is on purpose, the real repeats start at E301
    REPEAT = ESCAPE
    REPEAT_END = '\uE400'
    FG_RESET = REPEAT_END
    BG_RESET = '\uE401'
    BOLD = '\uE402'
    DIM = '\uE403'
    ITAL = '\uE404'
    UNDL = '\uE405'


HTML_BOLD = "tc--bold"
HTML_DIM = "tc--dim"
HTML_ITAL = "tc--ital"
HTML_UNDL = "tc--undl"
HTML_PREFIX_FG = "tc--fg-"
HTML_PREFIX_BG = "tc--bg-"


def html_fg(color_idx: int) -> str:
    return f"{HTML_PREFIX_FG}{color_idx}"


def html_bg(color_idx: int) -> str:
    return f"{HTML_PREFIX_BG}{color_idx}"


@dataclass
class SBufferHtmlClassList:
    fg: int | None = None
    bg: int | None = None
    bold: bool = False
    dim: bool = False
    ital: bool = False
    undl: bool = False

    def is_empty(self) -> bool:
        return (
                self.fg is None
                and self.bg is None
                and not self.bold
                and not self.dim
                and not self.ital
                and not self.undl
        )

    def as_html_classes(self) -> list[str]:
        out = []
        if self.fg is not None:
            out.append(html_fg(self.fg))
        if self.bg is not None:
            out.append(html_bg(self.bg))
        if self.bold:
            out.append(HTML_BOLD)
        if self.dim:
            out.append(HTML_DIM)
        if self.ital:
            out.append(HTML_ITAL)
        if self.undl:
            out.append(HTML_UNDL)

        return out


def html_encode_handle_class_change(output: list[str], span_was_open: bool, class_list: SBufferHtmlClassList) -> bool:
    """encodes a class change by appending to output, returns whether span is now open"""
    if span_was_open:
        output.append("</span>")
    if not class_list.is_empty():
        output.append(f'<span class="{' '.join(class_list.as_html_classes())}">')
        return True
    return False


def html_push_normal_char(output: list[str], c: str) -> None:
    match c:
        case '<':
            output.append("&lt;")
        case '>':
            output.append("&gt;")
        case '&':
            output.append("&amp;")
        case c:
            output.append(c)


def html_handle_line_break(output: list[str], remaining_chars_in_line: int) -> None:
    output.append(SBufferChar.SPACE * remaining_chars_in_line)
    output.append(SBufferChar.LINE_BREAK)


def sbuffer_to_html(sbuffer: str, width: int) -> str:
    """
    This converts the custom "SBuffer" (version 1) compressed terminal data to HTML.

    `width` is the width of the terminal in cells. It is not part of the SBuffer
    format itself but required to render trailing "empty space"; it is transmitted
    separately in the webhook payload.

    See cli-client/src/sbuffer.rs in the trmnl-console repository for more info about
    this format (and the equivalent Rust implementation).
    """
    class_list = SBufferHtmlClassList()
    output = []
    span_was_open = False
    new_span_pending = False

    sb_iter = iter(sbuffer)
    try:
        if next(sb_iter) != SBufferChar.START:
            raise ValueError("Invalid SBuffer format")
    except StopIteration:
        raise ValueError("Invalid SBuffer format")

    remaining_chars_in_line = width
    for c in sb_iter:
        match c:
            case SBufferChar.LINE_BREAK:
                if new_span_pending:
                    span_was_open = html_encode_handle_class_change(output, span_was_open, class_list)
                    new_span_pending = False
                html_handle_line_break(output, remaining_chars_in_line)
                remaining_chars_in_line = width
            case c if SBufferChar.FG_START <= c < SBufferChar.FG_END:
                class_list.fg = ord(c) - ord(SBufferChar.FG_START)
                new_span_pending = True
            case SBufferChar.FG_RESET:
                class_list.fg = None
                new_span_pending = True
            case c if SBufferChar.BG_START <= c < SBufferChar.BG_END:
                class_list.bg = ord(c) - ord(SBufferChar.BG_START)
                new_span_pending = True
            case SBufferChar.BG_RESET:
                class_list.bg = None
                new_span_pending = True
            case SBufferChar.ESCAPE:
                try:
                    c = next(sb_iter)
                except StopIteration:
                    continue
                if new_span_pending:
                    span_was_open = html_encode_handle_class_change(output, span_was_open, class_list)
                    new_span_pending = False
                if c == SBufferChar.LINE_BREAK:
                    html_handle_line_break(output, remaining_chars_in_line)
                    remaining_chars_in_line = width
                else:
                    html_push_normal_char(output, c)
                    remaining_chars_in_line = max(0, remaining_chars_in_line - 1)
            case c if SBufferChar.REPEAT <= c < SBufferChar.REPEAT_END:
                n = ord(c) - ord(SBufferChar.REPEAT) + 1
                try:
                    c = next(sb_iter)
                except StopIteration:
                    continue
                if new_span_pending:
                    span_was_open = html_encode_handle_class_change(output, span_was_open, class_list)
                    new_span_pending = False
                if c == SBufferChar.LINE_BREAK:
                    html_handle_line_break(output, remaining_chars_in_line)
                    remaining_chars_in_line = width
                else:
                    for _ in range(n):
                        html_push_normal_char(output, c)
                    remaining_chars_in_line = max(0, remaining_chars_in_line - n)
            case SBufferChar.BOLD:
                class_list.bold = not class_list.bold
                new_span_pending = True
            case SBufferChar.DIM:
                class_list.dim = not class_list.dim
                new_span_pending = True
            case SBufferChar.ITAL:
                class_list.ital = not class_list.ital
                new_span_pending = True
            case SBufferChar.UNDL:
                class_list.undl = not class_list.undl
                new_span_pending = True
            case c:
                if new_span_pending:
                    span_was_open = html_encode_handle_class_change(output, span_was_open, class_list)
                    new_span_pending = False
                html_push_normal_char(output, c)
                remaining_chars_in_line = max(0, remaining_chars_in_line - 1)

    if new_span_pending:
        span_was_open = html_encode_handle_class_change(output, span_was_open, class_list)

    if len(output) > 0 and output[-1][-1] == SBufferChar.LINE_BREAK:
        output.pop()
    else:
        output.append(" " * remaining_chars_in_line)

    if span_was_open:
        output.append("</span>")

    return "".join(output)


def run(input):
    if "data" in input:
        # decode sbuffer to HTML
        if "content" in input["data"] and "width" in input["data"]:
            try:
                input["data"]["content_transformed"] = sbuffer_to_html(
                    input["data"]["content"], input["data"]["width"]
                )
                # remove the untransformed content to make sure the js doesn't try to also decode it.
                del input["data"]["content"]
            except Exception as e:
                return {"error": f"failed to decode console output: {e.__class__.__name__}: {e}"}
        else:
            return {"error": "plugin did not receive console input and width."}
    else:
        return {"error": "plugin did not receive any data yet."}
    return input
