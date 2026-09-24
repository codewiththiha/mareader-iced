#!/usr/bin/env python3
"""Write the engine's test fixture: tests/fixtures/three-pages.pdf.

Hand-authored rather than exported from a word processor, so its bytes are a
matter of record and the engine's assertions can be exact. Three US Letter
pages:

* page 1 — a filled bar and the word "Mareader" in Helvetica, plus two link
  annotations: one jumping to page 2 inside the document, one opening a URL;
* page 2 — a line of text;
* page 3 — the same sheet with `/Rotate 90`, so Pdfium reports it as 792 × 612
  and renders it upright: the one page whose geometry is not the MediaBox's.

The chapter tree has a nested level — Chapter One (page 1) with Section One
(page 2) under it, and Chapter Two (page 3) as Chapter One's sibling — and the
document information carries a title and an author, so the open pipeline has
something to prefer over the file name.

Nothing is compressed and nothing is random: two runs produce identical bytes.

    python3 tests/fixtures/make_three_pages.py

`verify_three_pages.py` drives a real Pdfium over the result and is the proof
that the fixture still says what this docstring says.
"""

from pathlib import Path

PAGE_W, PAGE_H = 612, 792
OUT = Path(__file__).resolve().parent / "three-pages.pdf"

# The object numbers, named, because a hand-written xref is where a fixture
# stops matching its own contents.
CATALOG = 1
PAGES = 2
PAGE_ONE = 3
PAGE_TWO = 4
OUTLINES = 5
FONT = 6
CONTENT_ONE = 7
CONTENT_TWO = 8
CHAPTER_ONE = 9
INFO = 10
PAGE_THREE = 11
CHAPTER_TWO = 12
CONTENT_THREE = 13
SECTION_ONE = 14
LINK_INTERNAL = 15
LINK_EXTERNAL = 16


def stream(body: bytes) -> bytes:
    return b"<< /Length %d >>\nstream\n" % len(body) + body + b"\nendstream"


# The three content streams' bodies. A filled bar as well as text on page 1:
# a raster with no ink is a raster of blank paper, which is exactly what a
# size assertion cannot see.
ONE_BODY = (
    b"0 0 0 rg\n72 636 240 10 re f\n"
    b"BT\n/F1 24 Tf\n72 700 Td\n(Mareader) Tj\nET\n"
)
TWO_BODY = b"BT\n/F1 24 Tf\n72 700 Td\n(page two) Tj\nET\n"
# One filled square at the top-left of the UNROTATED media box: the page's own
# quarter turn carries it to the top-right of the landscape view, which is
# where the verifier looks for it.
THREE_BODY = b"0 0 0 rg\n72 620 100 100 re f\n"


def page(number: int, contents: int, rotate: int | None, annots: str = "") -> bytes:
    body = (
        b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 %d %d] "
        b"/Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R"
        % (PAGES, PAGE_W, PAGE_H, FONT, contents)
    )
    if rotate is not None:
        body += b" /Rotate %d" % rotate
    if annots:
        body += b" /Annots [" + annots + b"]"
    return body + b" >>"


OBJECTS = {
    CATALOG: b"<< /Type /Catalog /Pages %d 0 R /Outlines %d 0 R >>" % (PAGES, OUTLINES),
    PAGES: b"<< /Type /Pages /Kids [%d 0 R %d 0 R %d 0 R] /Count 3 >>"
    % (PAGE_ONE, PAGE_TWO, PAGE_THREE),
    PAGE_ONE: page(PAGE_ONE, CONTENT_ONE, None, b"%d 0 R %d 0 R" % (LINK_INTERNAL, LINK_EXTERNAL)),
    PAGE_TWO: page(PAGE_TWO, CONTENT_TWO, None),
    PAGE_THREE: page(PAGE_THREE, CONTENT_THREE, 90),
    OUTLINES: b"<< /Type /Outlines /First %d 0 R /Last %d 0 R /Count 2 >>"
    % (CHAPTER_ONE, CHAPTER_TWO),
    FONT: b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    CONTENT_ONE: stream(ONE_BODY),
    CONTENT_TWO: stream(TWO_BODY),
    CONTENT_THREE: stream(THREE_BODY),
    # A `/Next` chain lives at the level its objects sit on: Chapter Two is
    # Chapter One's sibling, and Section One is Chapter One's child. Putting
    # the `/Next` on the child instead makes Chapter Two read as the child's
    # own sibling, which is a fixture bug that looks like an engine bug.
    CHAPTER_ONE: b"<< /Title (Chapter One) /Parent %d 0 R /Next %d 0 R /First %d 0 R "
    b"/Last %d 0 R /Count 1 /Dest [%d 0 R /Fit] >>"
    % (OUTLINES, CHAPTER_TWO, SECTION_ONE, SECTION_ONE, PAGE_ONE),
    SECTION_ONE: b"<< /Title (Section One) /Parent %d 0 R /Dest [%d 0 R /Fit] >>"
    % (CHAPTER_ONE, PAGE_TWO),
    CHAPTER_TWO: b"<< /Title (Chapter Two) /Parent %d 0 R /Prev %d 0 R /Dest [%d 0 R /Fit] >>"
    % (OUTLINES, CHAPTER_ONE, PAGE_THREE),
    INFO: b"<< /Title (Three Pages) /Author (Mareader) /Producer (make_three_pages.py) >>",
    LINK_INTERNAL: b"<< /Type /Annot /Subtype /Link /Rect [72 620 300 660] /Border [0 0 0] "
    b"/Dest [%d 0 R /Fit] >>" % PAGE_TWO,
    LINK_EXTERNAL: b"<< /Type /Annot /Subtype /Link /Rect [72 560 300 600] /Border [0 0 0] "
    b"/A << /S /URI /URI (https://example.com/mareader) >> >>",
}


def build() -> bytes:
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = {}
    for number in sorted(OBJECTS):
        assert number == len(offsets) + 1, f"object {number} is out of order"
        offsets[number] = len(out)
        out += b"%d 0 obj\n" % number + OBJECTS[number] + b"\nendobj\n"
    xref_at = len(out)
    size = len(OBJECTS) + 1  # the free head, then one entry per object
    out += b"xref\n0 %d\n" % size
    out += b"0000000000 65535 f \n"
    for number in sorted(OBJECTS):
        out += b"%010d 00000 n \n" % offsets[number]
    out += b"trailer\n<< /Size %d /Root %d 0 R /Info %d 0 R >>\n" % (size, CATALOG, INFO)
    out += b"startxref\n%d\n%%%%EOF\n" % xref_at
    return bytes(out)


def main() -> None:
    data = build()
    OUT.write_bytes(data)
    print(f"wrote {OUT} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
