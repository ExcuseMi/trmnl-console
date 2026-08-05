#!/usr/bin/env python3
"""Generate the ANSI-256 -> TRMNL framework color CSS for plugin/src/shared.liquid.

Maps every color of the xterm 256-color palette (what the SBuffer color
indices refer to) to the perceptually nearest color token of the TRMNL
framework palette (v3.2), and prints CSS rules that bind the `tc--fg-<n>` /
`tc--bg-<n>` classes emitted by the SBuffer->HTML decoders to the
framework-owned paint variables (`--text-<token>-*` / `--bg-<token>-*`).
Those variables are resolved by the framework's screen-mode engine, which
takes care of reduced-palette and 1-bit/2-bit dithered rendering.

The framework palette below is from framework_colors.resolved.json of
trmnl-framework @ b05a845a (framework 3.2.0) plus the grayscale variables
from _variables_root.scss. Nearest-color matching happens in OKLab space.

Usage: python3 bin/gen-ansi-colors.py  (paste output into shared.liquid)
"""

# --- framework palette -------------------------------------------------------

FRAMEWORK_HUE_BASES = {
    "red": "#FF0000",
    "orange": "#FF8000",
    "yellow": "#FFFF00",
    "lime": "#80FF00",
    "green": "#00FF00",
    "cyan": "#00FFFF",
    "blue": "#0000FF",
    "violet": "#8000FF",
    "purple": "#FF00FF",
    "pink": "#FF0080",
}

# Per-hue (r, g, b) direction of the ramp at full intensity, encoded via the
# resolved shade table: shade-10..40 scale the base channels 0x22..0xEE,
# shade-45..75 blend towards white (0x11 steps on the off channels).
FRAMEWORK_SHADE_STEPS = (10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75)


def _shade(base_hex: str, step: int) -> str:
    r, g, b = _rgb(base_hex)
    if step <= 40:
        # 10 -> 0x22, 15 -> 0x44 ... 40 -> 0xEE (scale each channel)
        level = (step - 5) // 5 * 0x22
        return _hex(tuple(round(c / 255 * level) for c in (r, g, b)))
    # 45..75 blend towards white: zero channels walk 0x11, 0x33 ... 0xDD
    blend = (17 + 34 * ((step - 45) // 5)) / 255
    return _hex(tuple(round(c + (255 - c) * blend) for c in (r, g, b)))


def _rgb(hex_color: str) -> tuple[int, int, int]:
    hex_color = hex_color.lstrip("#")
    return tuple(int(hex_color[i : i + 2], 16) for i in (0, 2, 4))


def _hex(rgb: tuple[int, int, int]) -> str:
    return "#{:02X}{:02X}{:02X}".format(*rgb)


def framework_palette() -> dict[str, str]:
    palette = {"black": "#000000", "white": "#FFFFFF"}
    for step in FRAMEWORK_SHADE_STEPS:
        palette[f"gray-{step}"] = _hex((step // 5 * 0x11 - 0x11,) * 3)
    for hue, base in FRAMEWORK_HUE_BASES.items():
        palette[hue] = base
        for step in FRAMEWORK_SHADE_STEPS:
            palette[f"{hue}-{step}"] = _shade(base, step)
    return palette


# Resolved values from framework_colors.resolved.json to guard against the
# ramp derivation above drifting from upstream.
FRAMEWORK_PALETTE_SPOT_CHECKS = {
    "red-25": "#880000",
    "red-50": "#FF3333",
    "orange-30": "#AA5500",
    "orange-60": "#FFBB77",
    "lime-45": "#88FF11",
    "violet-70": "#DDBBFF",
    "pink-15": "#440022",
    "gray-10": "#111111",
    "gray-75": "#EEEEEE",
}


# --- xterm 256-color palette -------------------------------------------------

ANSI_BASE_16 = (
    "#000000", "#800000", "#008000", "#808000",
    "#000080", "#800080", "#008080", "#C0C0C0",
    "#808080", "#FF0000", "#00FF00", "#FFFF00",
    "#0000FF", "#FF00FF", "#00FFFF", "#FFFFFF",
)

CUBE_LEVELS = (0, 95, 135, 175, 215, 255)


def ansi_palette() -> list[str]:
    colors = [c for c in ANSI_BASE_16]
    for r in CUBE_LEVELS:
        for g in CUBE_LEVELS:
            for b in CUBE_LEVELS:
                colors.append(_hex((r, g, b)))
    for i in range(24):
        colors.append(_hex((8 + 10 * i,) * 3))
    return colors


# --- OKLab nearest-color matching --------------------------------------------


def _oklab(rgb: tuple[int, int, int]) -> tuple[float, float, float]:
    def lin(c: int) -> float:
        c /= 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    r, g, b = (lin(c) for c in rgb)
    l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b) ** (1 / 3)
    m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b) ** (1 / 3)
    s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b) ** (1 / 3)
    return (
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    )


def nearest_token(hex_color: str, palette: dict[str, str]) -> str:
    target = _oklab(_rgb(hex_color))
    return min(
        palette,
        key=lambda token: sum(
            (a - b) ** 2 for a, b in zip(_oklab(_rgb(palette[token])), target)
        ),
    )


# --- CSS generation ----------------------------------------------------------


def fg_rule(token: str) -> str:
    return (
        f"--tc-fg-color: var(--text-{token}-color, var(--{token})); "
        f"--tc-fg-image: var(--text-{token}-image, none); "
        f"--tc-fg-clip: var(--text-{token}-clip, border-box); "
        f"--tc-fg-under: var(--text-{token}-under, transparent);"
    )

def bg_rule(token: str) -> str:
    return (
        f"--tc-bg-color: var(--bg-{token}-color, var(--{token})); "
        f"--tc-bg-image: var(--bg-{token}-image, none);"
    )


def main() -> None:
    palette = framework_palette()
    for token, expected in FRAMEWORK_PALETTE_SPOT_CHECKS.items():
        assert palette[token] == expected, (token, palette[token], expected)

    token_to_indices: dict[str, list[int]] = {}
    for idx, hex_color in enumerate(ansi_palette()):
        token_to_indices.setdefault(nearest_token(hex_color, palette), []).append(idx)

    for prefix, rule in (("fg", fg_rule), ("bg", bg_rule)):
        for token, indices in token_to_indices.items():
            selectors = ", ".join(f".tc--{prefix}-{i}" for i in indices)
            print(f"    {selectors} {{ {rule(token)} }}")
        print()


if __name__ == "__main__":
    main()
