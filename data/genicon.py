"""Draw Uta's app icon in Aizome's icon grammar.

The tile, palette and grid are Aizome's (aizome/icons/genicons.py, after
aizome-design/logo/svg/aizome-app-icon.svg): a 60-unit kon square on a 64
grid, a 1-unit bevel highlight, a kachi edge and a hard 4-unit shadow down
and right. The glyph sits in the tile's inner 40 units on a 2-unit grid,
drawn only with the dye ramp and one amber accent, from rectangles, so it
stays crisp at every integer scale.

Uta 歌 is "song": two beamed notes on a faint staff, the first kamenozoki,
the second hanada, joined by an amber beam.

    python3 data/genicon.py data
"""
import pathlib
import sys

PALE, MID = "#a9c4ff", "#7aa2f7"
AMBER, KON, KACHI, LINE2 = "#e0af68", "#24283b", "#0b0c12", "#3b4261"
APP_ID, TITLE = "org.aizome.Uta", "Uta (Music)"


def R(x, y, w, h, fill, extra=""):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill}"{extra}/>'


def tile(title, glyph):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64" role="img" aria-label="{title}">'
        f"<title>{title}</title>"
        + R(4, 4, 60, 60, KACHI)
        + R(0, 0, 60, 60, KON)
        + R(1, 1, 58, 1, "#ffffff", ' fill-opacity=".08"')
        + f'<rect x=".5" y=".5" width="59" height="59" fill="none" stroke="{KACHI}"/>'
        + glyph
        + "</svg>\n"
    )


def head(x, y, fill):
    # A note head 14 wide and 10 tall, stepped so it leans as an engraved one does.
    return R(x + 4, y, 10, 2, fill) + R(x, y + 2, 14, 6, fill) + R(x, y + 8, 10, 2, fill)


def uta():
    g = "".join(R(10, y, 40, 2, LINE2) for y in (18, 26, 34, 42))
    # Each stem is flush with its head's right edge, as in engraving.
    g += R(20, 10, 4, 34, PALE) + head(10, 40, PALE)
    g += R(46, 10, 4, 30, MID) + head(36, 36, MID)
    g += R(20, 10, 30, 6, AMBER)
    return g


out = pathlib.Path(sys.argv[1])
out.mkdir(parents=True, exist_ok=True)
(out / f"{APP_ID}.svg").write_text(tile(TITLE, uta()))
print(f"wrote {out / APP_ID}.svg")
