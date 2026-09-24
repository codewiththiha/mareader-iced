#!/usr/bin/env python3
"""Prove the fixture says what it claims, by driving a real Pdfium over it.

This is the fixture's own gate, kept beside the generator instead of in the
Rust tests: it runs the C API directly, so it answers questions the engine's
tests cannot — most importantly that page 3, which carries `/Rotate 90`, is
reported as 792 x 612 and renders upright when it is drawn into that box with
no rotation of our own.

    python3 tests/fixtures/verify_three_pages.py /path/to/libpdfium.so

Exit code 0 means every claim held. Nothing here is part of the build: the
Rust tests read the same file, and this is what proves the file itself.
"""

import ctypes as C
import sys
from pathlib import Path

PDF = Path(__file__).resolve().parent / "three-pages.pdf"

FPDF_ANNOT_LINK = 2  # FPDF_ANNOT_LINK in fpdf_annot.h


def load(lib: str):
    pdf = C.CDLL(lib)
    void, int_, uint, uchar, float_, ulong = (
        C.c_void_p,
        C.c_int,
        C.c_uint,
        C.c_ubyte,
        C.c_float,
        C.c_ulong,
    )

    def sig(name, restype, argtypes):
        fn = getattr(pdf, name)
        fn.restype = restype
        fn.argtypes = argtypes
        return fn

    api = {
        "init": sig("FPDF_InitLibrary", None, []),
        "load": sig("FPDF_LoadDocument", void, [C.c_char_p, C.c_char_p]),
        "count": sig("FPDF_GetPageCount", int_, [void]),
        "page": sig("FPDF_LoadPage", void, [void, int_]),
        "close_page": sig("FPDF_ClosePage", None, [void]),
        "width": sig("FPDF_GetPageWidthF", float_, [void]),
        "height": sig("FPDF_GetPageHeightF", float_, [void]),
        "rotation": sig("FPDFPage_GetRotation", int_, [void]),
        "text_page": sig("FPDFText_LoadPage", void, [void]),
        "text_count": sig("FPDFText_CountChars", int_, [void]),
        "text_get": sig("FPDFText_GetText", int_, [void, int_, int_, C.POINTER(C.c_ushort)]),
        "text_close": sig("FPDFText_ClosePage", None, [void]),
        "child": sig("FPDFBookmark_GetFirstChild", void, [void, void]),
        "sibling": sig("FPDFBookmark_GetNextSibling", void, [void, void]),
        "title": sig("FPDFBookmark_GetTitle", ulong, [void, void, ulong]),
        "dest": sig("FPDFBookmark_GetDest", void, [void, void]),
        "dest_page": sig("FPDFDest_GetDestPageIndex", int_, [void, void]),
        "annot_count": sig("FPDFPage_GetAnnotCount", int_, [void]),
        "annot": sig("FPDFPage_GetAnnot", void, [void, int_]),
        "annot_subtype": sig("FPDFAnnot_GetSubtype", int_, [void]),
        "annot_close": sig("FPDFPage_CloseAnnot", None, [void]),
        "bitmap": sig("FPDFBitmap_Create", void, [int_, int_, int_]),
        "fill": sig("FPDFBitmap_FillRect", None, [void, int_, int_, int_, int_, C.c_uint]),
        "render": sig("FPDF_RenderPageBitmap", None, [void, void, int_, int_, int_, int_, int_, int_]),
        "buffer": sig("FPDFBitmap_GetBuffer", void, [void]),
        "stride": sig("FPDFBitmap_GetStride", int_, [void]),
        "destroy": sig("FPDFBitmap_Destroy", None, [void]),
    }
    api["init"]()
    api["C"] = C
    return pdf, api


def page_text(api, page) -> str:
    text_page = api["text_page"](page)
    assert text_page, "the page has no text layer"
    count = api["text_count"](text_page)
    buf = (C.c_ushort * (count + 1))()
    written = api["text_get"](text_page, 0, count, buf)
    api["text_close"](text_page)
    # Pdfium terminates each line with a NUL; the text is what is left.
    return "".join(chr(buf[i]) for i in range(written)).replace("\x00", "")


def ink(api, page, width, height, rotate):
    """The bounding box of dark pixels, or None when the raster is blank."""
    bitmap = api["bitmap"](width, height, 1)
    assert bitmap, "no bitmap"
    api["fill"](bitmap, 0, 0, width, height, 0xFFFFFFFF)
    api["render"](bitmap, page, 0, 0, width, height, rotate, 0x01)  # FPDF_ANNOT
    stride = api["stride"](bitmap)
    ptr = C.cast(api["buffer"](bitmap), C.POINTER(C.c_ubyte))
    xs, ys = [], []
    for row in range(height):
        for col in range(width):
            if ptr[row * stride + col * 4] < 0x80:
                xs.append(col)
                ys.append(row)
    api["destroy"](bitmap)
    if not xs:
        return None
    return (min(xs), min(ys), max(xs), max(ys), len(xs))


def bookmark_title(api, node) -> str:
    length = api["title"](node, None, 0)
    buf = (C.c_ushort * (length // 2))()
    api["title"](node, buf, length)
    return "".join(chr(buf[i]) for i in range(len(buf)) if buf[i])


def outline(api, doc, node, depth, out):
    """The tree in the order the engine walks it: a node, then its children,
    then its next sibling — depth-first, prefix order."""
    while node:
        dest = api["dest"](doc, node)
        page = api["dest_page"](doc, dest) + 1 if dest else 0
        out.append((bookmark_title(api, node), page, depth))
        outline(api, doc, api["child"](doc, node), depth + 1, out)
        node = api["sibling"](doc, node)


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    pdf, api = load(sys.argv[1])
    assert PDF.is_file(), f"run make_three_pages.py first: {PDF} is missing"
    doc = api["load"](str(PDF).encode(), None)
    assert doc, "the fixture did not load"
    checks = []

    count = api["count"](doc)
    checks.append(("page count", count, 3))

    sizes = []
    for index in range(count):
        page = api["page"](doc, index)
        sizes.append((round(api["width"](page)), round(api["height"](page)), api["rotation"](page)))
        api["close_page"](page)
    checks.append(("page 1 box", sizes[0][:2], (612, 792)))
    checks.append(("page 2 box", sizes[1][:2], (612, 792)))
    checks.append(("page 3 box (rotated)", sizes[2][:2], (792, 612)))
    checks.append(("page 3 rotation", sizes[2][2], 1))

    page1 = api["page"](doc, 0)
    text = page_text(api, page1)
    checks.append(("page 1 text", text, "Mareader"))
    annots = api["annot_count"](page1)
    links = 0
    for index in range(annots):
        annot = api["annot"](page1, index)
        if api["annot_subtype"](annot) == FPDF_ANNOT_LINK:
            links += 1
        api["annot_close"](annot)
    checks.append(("page 1 links", links, 2))
    page1_ink = ink(api, page1, 612, 792, 0)
    checks.append(("page 1 has ink", page1_ink is not None and page1_ink[4] > 500, True))
    api["close_page"](page1)

    page3 = api["page"](doc, 2)
    landscape = ink(api, page3, 792, 612, 0)
    # The square was drawn at the top-left of the unrotated sheet; the page's
    # own turn carries it to the right-hand half of the landscape view. If the
    # engine rotated it a second time, this box would sit on the left.
    right_half = landscape is not None and landscape[0] > 792 / 2
    checks.append(("page 3 draws upright in its own box", right_half, True))
    api["close_page"](page3)

    entries = []
    outline(api, doc, api["child"](doc, None), 0, entries)
    checks.append(
        (
            "outline",
            entries,
            [("Chapter One", 1, 0), ("Section One", 2, 1), ("Chapter Two", 3, 0)],
        )
    )

    failed = 0
    for name, got, want in checks:
        ok = got == want
        failed += not ok
        print(f"{'ok  ' if ok else 'FAIL'} {name}: {got!r}" + ("" if ok else f" (wanted {want!r})"))
    print(f"\n{len(checks) - failed} of {len(checks)} claims held")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
