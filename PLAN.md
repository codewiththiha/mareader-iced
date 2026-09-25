# Mareader — native iced port: master plan

Mareader today is a Tauri v2 shell around a Leptos/WebAssembly frontend that renders PDFs
through a vendored pdf.js. This repository rebuilds the same reader as a **native Rust
desktop application on [iced](https://github.com/iced-rs/iced) 0.14** — same features, same
look, same domain logic and same persisted data, on a lighter and faster stack:

| Concern            | Original (wasm)                          | Native port (iced)                                    |
|--------------------|------------------------------------------|-------------------------------------------------------|
| UI framework       | Leptos 0.8 (CSR, wasm-bindgen)           | iced 0.14 (Elm architecture, wgpu/tiny-skia)          |
| PDF engine         | pdf.js 6.2 vendored in the webview       | **PDFium** via `pdfium-render` (runtime-bound)        |
| Markdown           | leptos-md + md-core blocks               | `pulldown-cmark` over the same md-core blocks         |
| Text layout/measure| DOM measurement + reflow-core estimates  | `cosmic-text` 0.15 (iced's own text engine) + reflow-core |
| Font enumeration   | browser faces                            | `fontdb` 0.23 (cosmic-text's own db)                  |
| "Database"         | localStorage blobs (`mareader.settings.v1`, `mareader.library.v3`, covers, gloss) | identical serde blobs as JSON files under the OS app-data dir, same schemas + migrations |
| File dialogs       | Tauri dialog plugin                      | `rfd` (xdg-portal on Linux — no GTK build deps)       |
| Filesystem commands| Tauri invoke (`src-tauri/commands`)      | in-process `platform` services (plain `std::fs`)      |
| Appearance filters | CSS `filter:` chain + `mix-blend-mode`   | software pixel pipeline over the RGBA frames PDFium renders |
| Window chrome      | frameless webview + DOM titlebar         | `decorations(false)` + custom iced titlebar, per-OS caption clusters |
| AI gloss           | Tauri command → fm-bridge / mock         | provider trait in-process; mock everywhere, Apple Intelligence via `fm-bridge` on macOS |

The pure domain crates are **ported verbatim** (they were written host-testable on purpose),
so the library's governance, the settings schema, the zoom ladder, the pagination maths and
~1,000 unit tests carry over unchanged:

```
crates/
  reader-core/    settings schema, view modes, colour pipeline, presets, zoom maths,
                  outline shape, search model            (verbatim)
  library-core/   books, fingerprints, shelves, watched folders, rescan ledger,
                  governance, sorts, persisted blob + migrations   (verbatim)
  reflow-core/    block shape, A4 geometry, page cutter, height estimates,
                  typography resolution, search over blocks        (verbatim)
  txt-core/       plain-text normalisation + paragraph parsing     (verbatim)
  md-core/        markdown construct classification, front matter, heading outline (verbatim)
  pdf-paper/      blend backdrop colour brain: dominant-colour detection,
                  per-page palette, ladder interpolation           (verbatim)
  pdf-core/       page geometry, outline wire entries, device-pixel grid,
                  page-text search index                           (web-sys DPR read replaced
                                                                   by an app-fed ratio)
  ui-geom/        floating-surface placement + damped spring       (verbatim)
  virtual-list/   generic windowing maths                          (verbatim)
  ai-core/        gloss wire types, card geometry, marks, cache    (wasm bridge removed;
                                                                   providers live in-app)
```

What is NOT ported: `pdf-engine`, `pdf-paper`'s TS eyes, `tauri-bridge`, `app-chrome`,
`virtual-list-leptos`, the Leptos `src/` tree and the stylesheets. They are web-only glue;
their behaviour is re-implemented natively under `src/` with iced widgets, and their look is
re-expressed from the design tokens (`styles/tokens.css` → `src/theme`).

---

## 1. Constraints and ground rules

1. **No local compilation.** The development sandbox is 128 MB; a Rust/wgpu build would
   evict the workspace. GitHub Actions is the only compiler: every phase ships only when
   confident, CI reports, fixes land as small follow-ups, and WIP history is squashed
   before it is allowed to settle (`git push --force-with-lease`).
2. **Vendor-verified APIs.** iced 0.14, cosmic-text 0.15, pdfium-render 0.9.4, rfd 0.17
   sources are read locally (not compiled) so call sites match real signatures.
3. **Lightweight runtime deps.** No tokio (iced's default `thread-pool` executor), no GTK
   (rfd's xdg-portal backend), no build-time C (PDFium binds at runtime).
4. **Same data, same logic.** Persisted field names are the storage contract; the native
   app reads a blob written by the wasm app's schema byte-for-byte (modulo transport:
   localStorage → JSON files).
5. **iced-native design language.** The look (tokens, glass bars, rounded floats, amber
   current-match, layering scale) is preserved, but expressed with iced primitives —
   stacks, floats, canvas masks, animations — not DOM-shaped imitations.

## 2. Repository layout

```
Cargo.toml            workspace root = the `mareader-iced` application (bin: `mareader`)
crates/               the ported pure domain crates (above)
src/
  main.rs             boot: platform probes, pdfium bind attempt, iced application
  app.rs              Mareader state root, Message tree, update/view/subscription/theme
  route.rs            Library ⇄ Reader routing (a document being Ready is the /reader route)
  theme/              design tokens → palettes + widget styles (light/dark/dim bases, tint)
  chrome/             titlebar shell, caption clusters, icon sprite, icon buttons,
                      tooltips, menus/popovers (ui-geom placement), toasts, drag helpers
  platform/           data dir & paths, persistence (settings/library/covers/gloss, debounced),
                      filesystem services (walk, measure, store copy/delete, progress),
                      dialogs (rfd), reveal/open-url, focus rescan wiring
  state/              settings, library, reader (document/viewer/zoom/search/gloss), ui
  library/            the shelf: bar (breadcrumb, search, view/appearance menus), grid,
                      list, book & folder cards, import sheets, governance sheets,
                      context menus, selection, drag sessions, suggestions
  reader/             the viewer: mode dispatch (single, spread, continuous-v, strip-h),
                      zoom pipeline, sidebar (outline, thumbnails), controls (bottom bar,
                      page indicator, overlay scrollbar), search UI, page_host seam
  formats/
    pdf/              PDFium engine service: document lifecycle, render queue on its own
                      thread, page bitmaps, thumbnails + LRU, text extraction → pdf-core
                      SearchIndex, outline, links, selectable text layer
    reflow/           cosmic-text measurement, pagination driver (estimate → refine),
                      A4 page hosts, continuous stream, search-hit layer
    md/               pulldown-cmark spans → iced rich_text (GFM, raw HTML refused,
                      data:-URI images only)
  appearance/         native raster pipeline: CSS-filter-equivalent pixel ops
                      (invert/hue-rotate/sepia/saturate/brightness/contrast), backdrop
                      blend (multiply/screen/soft-light) driven by pdf-paper, procedural
                      paper textures + film grain, reflowable token palette → iced styles
  ai/                 gloss providers (mock; apple behind cfg+feature), selection pill,
                      word card, marks
.github/workflows/
  ci.yml              lint (clippy -D warnings), test (workspace), build (3-OS matrix)
  release.yml         v* tags → per-OS bundles with PDFium sidecar → GitHub Release
```

Message flow is plain Elm: one `Message` enum namespaced per subsystem
(`Message::Library(library::Message)` …), `update` returns `Task<Message>`, long work
(folder walks, PDF renders, AI streams) runs off-thread and reports back through tasks and
subscriptions — the same ownership discipline the Leptos effects had, in one direction.

## 3. Subsystem designs

### 3.1 Window chrome (the titlebar family)

* `decorations(false)` on every OS; window 1200×800 default, 640×480 floor (same as
  `tauri.conf.json`).
* The bar is **headless by default**: a thin hover band at the top edge reveals it; it hides
  again on exit after the same delay ladder the web app used (`use_hover_reveal`), and the
  reveal/hide is an `iced::animation` fade. A pin button holds it open; the reader's pin and
  the library's pin are separate persisted settings (`titlebar_pinned`,
  `library_titlebar_pinned`, library default pinned) — same fields, same semantics.
* The bar is **grabbable**: press on the drag band issues `window::drag(id)`; double-click
  toggles maximize.
* **Per-OS caption clusters**, chosen by `std::env::consts::OS` (no UA sniffing needed
  natively): macOS traffic lights on the left (grouped circles, glyphs on hover), Windows
  square minimize/maximize/close on the right (close flushes red), GNOME circles on the
  right — the same three families `app-chrome` drew.
* Center slot: the floating document title truncates only on genuine collision with the
  measured clusters (port of `resolve_center_slot`, pure maths from `titlebar/root.rs`),
  painted difference-blended against the local backdrop colour.
* The library bar keeps its own composition (breadcrumb / search / view+appearance menus,
  no Open button, no settings gear); the reader bar keeps the glass toolbar (sidebar
  toggle, open, title, page nav, zoom popover, view-mode switch, search, overflow).

### 3.2 Persistence (the "database")

* `directories::ProjectDirs` with qualifier `com.codewiththiha`, name `Mareader` →
  `{config|data}/mareader`:
  * `settings.json` — `reader_core::settings::Settings` (schema `mareader.settings.v1`,
    field-for-field), `sanitize()` on load (incl. the tint-curve migration), debounced
    350 ms writes.
  * `library.json` — the `library_core` blob (`mareader.library.v3` incl. `blob::migrate`
    path from v1/v2 and the retired `pdfreader.library.v1` recent-books migration).
  * `covers/{book_id}.jpg` — the cover cache (was base64 in its own localStorage key; a
    file per cover is the native equivalent, written atomically, evicted by the same LRU).
  * `gloss.json` — per-document gloss marks (ai-core's persistable JSON).
  * `Library/items/{book_id}/source.{ext}` — the library's own roof: the stored copies,
    under the app-data dir's `Library` folder the Tauri shell built, so both installs of
    the reader speak one layout (`library_core::store` rules unchanged).
* Atomic write discipline: temp file + rename, so a crash mid-write cannot corrupt a blob.

### 3.3 PDF engine (PDFium)

* `pdfium-render` 0.9.4, `default-features = false`, features `pdfium_latest` + `thread_safe`.
  `image_latest` is deliberately off: the crate's own `as_image()` is the only thing it buys,
  and the reader never wants a `DynamicImage` — `PdfBitmap::as_rgba_bytes()` hands over the
  pixels already normalised to RGBA (the render config's default `BGRA` format plus PDFium's
  byte-order reversal *is* RGBA, so the crate returns the buffer without a conversion pass),
  which is exactly what `image::Handle::from_rgba` takes. The `image` crate's codecs arrive
  later through iced's own `image` feature, when covers need JPEG. `pdfium_latest` currently
  pins Pdfium build 7881, so the sidecar is pinned to the matching bblanchon release
  (`chromium/7881`, Pdfium 151.0.7881.0), not `latest` — the bindings' API version and the
  library's must agree. iced's side is `image-without-codecs` for this phase: the widget and
  `Handle::from_rgba` are what page rasters need, and no codec is compiled until a cover is
  read back from disk.
* **The library binds once per process, not once per document.** pdfium-render keeps its
  bindings in a process-global `OnceCell`: `Pdfium::new(bindings)` initialises it, and a second
  `bind_to_library` call answers `PdfiumLibraryBindingsAlreadyInitialized`. So one owner binds
  — the engine thread, first thing — and every later use goes through the handle it hands back.
  Bind order: `MAREAEDER_PDFIUM` (a full path to the library file) → `MAREAEDER_PDFIUM_DIR`
  (a directory, which is what CI sets — deliberately *not* the crate's own
  `PDFIUM_DYNAMIC_LIB_PATH`, which its build script reads to add a link-search path and would
  be a false friend for a runtime bind) → beside the executable, its `lib/` and its `bin/`
  (the three shapes the release archives use:
  `lib/libpdfium.so`, `lib/libpdfium.dylib`, `bin/pdfium.dll`) → the working directory's same
  three → the system library. Nothing found is not a crash: the engine reports
  `unavailable` with the paths it tried, and the app keeps running — the shelf, the store and
  the text formats never needed a PDF engine.
* A dedicated engine thread owns the document (`src/formats/pdf/engine.rs`) — the native
  analogue of pdf.js's worker, with a plain request/event protocol (`src/formats/pdf/
  protocol.rs`) and no shared state with the UI. The scanner and thumbnails get no separate
  lane yet: one request at a time covers 3a, and the priority lane arrives in 3g, because a
  lane added before there is contention is a lane nobody can test.
* A document is **served inside the borrow that owns it**. `PdfDocument<'a>` borrows the
  `Pdfium` it came from, so the worker loads the document in an inner loop, serves it, and
  returns to its outer loop when the document is closed or replaced — no `'static`
  laundering, and the request that *ended* a document's life (a close, or an open that
  replaces it) is the one the outer loop acts on next, so a drop can never be lost between
  the two.
* Requests carry a **session stamp** (which open they belong to) and renders name a
  **[`FrameKey`]** — the page, and the whole number of device pixels it is to fill. The key is
  the native answer to pdf.js's `cancel_thumb` and its per-canvas bookkeeping, and it is a
  better one than a counter: it can be *compared*, so the reader asks for the key it wants
  now, drops a frame whose key has moved on where it lands, and can keep that frame without
  ever painting it over the page on screen.
* Frames render at device-pixel-grid sizes: the CSS-px box the layout asked for, times the
  live scale factor, rounded to whole device pixels by the app
  (`pdf_core::pixel_grid::snap_to_device`) and **capped** (12 000 px wide) so a deep zoom on a
  large display asks for a frame the machine can still allocate. The request carries both
  sides of that box (`PdfRenderConfig::set_target_size`), so PDFium allocates exactly the
  raster the host is about to draw: the two sides can differ only by the half-device-pixel
  the rounding moved, and the stretch that leaves is invisible — a `set_target_width`-only
  request would instead choose its own height and hand the host a bitmap whose aspect no
  longer matches the rectangle it was measured for. The host draws the frame back into that
  same box, so raster and rect agree by construction rather than by both sides rounding the
  same way. `image::Handle::from_rgba` takes the pixels as they come —
  `PdfBitmap::as_rgba_bytes()` normalises whatever channel order PDFium wrote — and the
  appearance raster pipeline (§3.5) slots in between once it exists.
* The document's **content identity** is a hash of the file itself (length, first and last
  64 KiB): pdf.js published a `fingerprint` derived from the trailer's `/ID`, and that field is
  the key a retained search index is adopted under. PDFium's equivalent
  (`FPDF_GetFileIdentifier`) is only reachable through the raw bindings and a two-call
  allocation dance, and the only thing that reads the value is our own in-process cache — the
  question is "are these the same bytes", and that is what the hash answers.
* Text: `PdfPageText` chars (loose bounds) → `pdf_core::SearchIndex` — the same Rust index
  the web app built from pdf.js text items; highlight rects, snippets and wrap-around come
  from reader-core's shared search model. PDFium reports one box per CHARACTER where pdf.js
  reported runs, so `src/formats/pdf/text.rs` flips each box into the reader's top-left space
  and groups them back into runs (new line, size change, or a gap wider than 0.4 em) — pure
  arithmetic with its own tests, no library needed.
* The fixture the engine's tests open is hand-authored and byte-stable
  (`tests/fixtures/three-pages.pdf`, written by `make_three_pages.py` beside it): three US
  Letter pages — one with a filled bar and the word "Mareader" plus both kinds of link
  annotation, one carrying a line of text, and one with `/Rotate 90` (which Pdfium reports as
  792×612 and draws upright in that box — the engine must not apply the page's own rotation a
  second time; the fixture exists partly to pin that) — a two-level chapter tree, and
  document title/author metadata. `verify_three_pages.py` drives a real Pdfium through ctypes
  and proves the fixture still carries all of it, so a fixture that quietly stopped testing
  what it names fails before the Rust suite does.
* Outline: `PdfBookmarks` flattened in document order with depth (reader-core's outline
  shape + active-section rules). Links: page link annotations → internal navigation /
  external browser (schemes vetted by reader-core's filename/url rules).
* Thumbnails: separate cheaper path, LRU 16 pair-rasters like the original; covers reuse
  the same path at shelf size and persist as JPEG.
* Selectable text layer: an iced overlay built from the same char boxes — click/drag
  selects by rect intersection, paints a translucent tint over transparent text (the
  glyphs read through, text is never doubled), double-click picks the word for the gloss,
  and the selection copies to the clipboard.

### 3.4 Reflow engine (txt/md)

* Pipeline preserved end to end: `txt-core`/`md-core` parse blocks → `reflow-core`
  estimates seed the A4 cut → **cosmic-text measures for real** (a `FontSystem` of its own,
  same version iced renders with, so measurements match pixels) → measurements refine the
  cut block by block → typography changes re-cut while keeping the reader's block.
* Rendering: blocks become iced elements — prose as `rich_text` spans (md inline via
  `pulldown-cmark`: headings, lists, tables, blockquotes, fenced code, links; raw HTML
  events refused; images only from `data:` URIs), never split mid-construct across pages.
* All four view modes work off the same page/zoom machinery as PDF (the `page_host` seam):
  single, spread (book gutters via `reflow_core::geometry`), continuous stream (no visible
  page breaks), horizontal strip.
* Fonts settings tab: default + serif/sans/mono family pickers (fontdb system faces,
  schema room for bundled fonts), size, weight, line spacing, paragraph margin, word/letter
  spacing, indent, book layout, justification (iced supports `Justified`), hyphenation
  (cosmic-text `Shaping::Advanced`), column alignment; body-ink intensity from the
  appearance menu.
* Search: `reflow_core::search` over blocks; hits paint the same amber boxes via line rects
  from the measured layout.

### 3.5 Appearance system

* The model is reader-core's (base mode + tint hue/strength + texture + noise + presets);
  only the *projection* changes:
  * **UI tokens**: `appearance::raster::tint` (OKLCH-preserving) + `reflowable` palettes →
    `iced::theme::Theme::custom` palettes and widget style functions.
  * **Raster pages**: the CSS filter chain becomes per-pixel maths on the PDFium RGBA
    frame — Light: identity; Dark: `invert(0.92) hue-rotate(180°) saturate(0.85)
    brightness(1.02)`; Dim: `brightness(0.8) saturate(0.75) contrast(0.9)`; tint:
    `sepia(min(0.55, t·0.55)) saturate(1+0.6t) hue-rotate(hue−34°)` — each implemented to
    the CSS filter spec, applied in chain order.
  * **Blend backdrop**: `pdf-paper` detects each page's paper colour (whole-page or edges,
    downsampled ≤96 px), `PagePalette` interpolates along the paint-weighted scroll
    position; the page frame is composited over that backdrop with the base mode's blend
    (multiply / screen / soft-light) — real per-pixel blending, which the webview could
    only ask a compositor to do.
  * **Textures & grain**: the five paper textures and the film grain were SVG
    feTurbulence tiles; natively they are procedural value-noise tiles generated once
    (seeded, tileable) and composited with the persisted opacity/scale; animated grain
    crawls on a frame subscription and obeys the motion switches.
  * Scrub mode: sliders repaint the pipeline from cached raw frames immediately and commit
    to settings 180 ms after the last tick, exactly like the web debounce.
* Presets: five built-ins (code, not storage) + unlimited user presets in named groups —
  reader-core's preset maths, unchanged.

### 3.6 Library

All decision logic is `library-core` (ported): two origins (read-in-place / store copy),
import options and re-shape, watched-folder rescans on launch and focus (no fs watcher —
same reasoning as the original: half-written files), ledger add/relink/skip/tombstone,
restore menus, shelves as levels (folder cards with recursive 2×2 plates, tree rows in
list density), links as rows, duplicates, governance sheets for every copy the library
makes, fuzzy search (single pass per keystroke, suggestion panel with lit matched chars,
arrow/enter/escape), manual-order sorting with numeric volume awareness, missing-book
badge + relink, selection with hold-to-enter, the app's own drag session (ghost of the
held covers, crumb resting at 420 ms → third-size, book resting 650 ms → new-shelf plate,
escape restores), breadcrumb with three crumbs + ellipsis popover, per-kind context menus,
removal receipts. The UI re-expresses these flows with iced grid/stack/float/mouse_area —
`virtual-list` drives the windowing of grid, list, thumbnail rail and continuous strips.

### 3.7 Settings, motion, shortcuts, AI

* Settings modal tabs: Layout, Appearance (mode/tint/texture/grain/paper/AI sections),
  Animations (only while the master is on), Fonts (only while a reflowable document is
  open), Gloss. The modal and the appearance menu are **route-aware**: the library's
  appearance menu has no page-texture section; the gear lives only on the reader bar.
* Motion model: `AnimationSettings` + master switch project into one `Motion` struct every
  animation site reads; iced `Animation`s (sidebar slide 300 ms, zoom tween, scroll
  glides, hover fades) collapse to their end frame when switched off — the same twelve
  motion principles from Mareader.md hold, expressed over iced's animation clock; the
  loading mark keeps its fade under every net.
* Keyboard: the original's table verbatim (Ctrl+O/F/0/1/2, +/−, arrows, PgUp/PgDn, Space,
  Shift+A, Escape ladders), via a `keyboard::on_key_press` subscription that never fires
  while a text field holds focus.
* AI gloss: ai-core's card geometry/spring/marks; providers behind a trait — the
  deterministic mock everywhere, Apple Intelligence through `fm-bridge` on macOS
  (same prompts + JSON schema as `src-tauri/src/ai`), streamed chunks on a subscription.

## 4. Phases (each ships green CI before the next begins)

Every phase is visually testable on its own; the app always launches.

* **P0 — Foundation.** Workspace + ported crates (their ~1,000 tests run in CI from day
  one), frameless window, the full titlebar family (hover reveal, pin, drag, per-OS
  captions), theme tokens, icon sprite, CI lanes (clippy, test, 3-OS build), lockfile
  bootstrap. *Accept: window opens on all OSes; bar hides/reveals/pins/drags; tests green.*
* **P1 — Platform & persistence.** Data dir, settings/library blob load+save (+migrations,
  debounce, atomic), fs services with progress, rfd dialogs, reveal/open-url, toasts,
  tooltips, menu/popover primitives on ui-geom. *Accept: settings survive restart; a file
  dialog opens; toast on error.*
* **P2 — Library shelf.** Routes, library bar (breadcrumb/search/menus), grid+list with
  virtual-list, book/folder cards, import sheets (folder & file), scan/ledger/governance
  wiring, context menus, selection + drag sessions, fuzzy search + suggestions, shelves,
  links, relink/restore, watched rescan on focus, placeholder covers. *Accept: import a
  folder, see the shelf, file books into shelves, search them, right-click everything.*
* **P3 — PDF reader.** Engine service + bind strategy, open pipeline (dialog, drag-drop,
  last-path), single view + page turn, zoom pipeline (ladder/fits/auto-resize/shrink),
  continuous + horizontal + spread, page indicator/bottom bar/overlay scrollbar, sidebar
  (outline + virtualized thumbnails + LRU), full-text search + highlights, links, text
  selection layer, covers from real frames. *Accept: read a PDF in all four modes, search
  it, browse its outline, select and copy text.*
* **P4 — Reflow reader.** Measurement service, pagination driver, A4 page hosts, txt & md
  block rendering (pulldown-cmark spans), continuous stream, heading outline, reflow
  search hits, Fonts tab live. *Accept: open a .md/.txt, paginate, change every typography
  knob and watch the re-cut keep your place.*
* **P5 — Appearance.** UI token pipeline, raster filter chain, blend backdrop (pdf-paper),
  textures + grain, presets + user groups, scrub/commit. *Accept: the five built-ins read
  like the web app's; tint sweeps continuously; grain crawls until switched off.*
* **P6 — Settings, motion, shortcuts.** Settings modal (route-aware tabs), appearance
  menus both routes, app/overflow menus (fullscreen, shortcut reference, reload window),
  motion switches projected everywhere, keyboard table, reduced-motion analogue.
  *Accept: every switch in the README's Motion section demonstrably gates its motion.*
* **P7 — AI gloss.** Providers (mock/apple), selection pill, word card + spring, marks and
  anchors for both format families, gloss settings. *Accept: double-click a word in a PDF
  and in a reflowed page; the card lands by the same placement rule.*
* **P8 — Release & polish.** Release workflow (3-OS artifacts + PDFium sidecar pinned to a
  bblanchon build, icons, version sync), README rewrite, docs pass, history squash to
  clean conventional commits, force-push. *Accept: tagged build downloads and runs on all
  three OSes.*

### 4.1 P3 — the PDF reader, in increments

P3 is the first phase whose subject is a whole surface, so it ships as seven increments, each
one CI-green, each one visually testable on the day it lands. The order is the order a reader
meets the app: engine, then the page, then everything that moves or measures it.

* **3a — The engine and the first page.** The Pdfium bind strategy and its degradation path
  (`MAREAEDER_PDFIUM` file → `MAREAEDER_PDFIUM_DIR` directory → beside the executable and
  its `lib/`+`bin/` → the working directory's same three → the system library), the engine
  thread with its request/event protocol, its session stamp and its frame keys, the
  per-character text extraction grouped into runs, and the open pipeline (picker, drop, last
  path, library row) with the status ladder `Idle → Opening → Ready/Error`; page size, page
  count, title and author read off the file; single view at the reader's startup fit, painted
  from a real frame; page turn by keyboard and by the bar's own prev/next; close back to the
  shelf with the reading position flushed into the row's read point. *Accept: open a PDF, read
  it, turn pages, go back — and open it again where you left off.*
* **3b — Zoom.** The web app's `src/zoom/*` and its three scales, ported into one module,
  `src/reader/zoom.rs`: `Command` (ladder `Step`, `Refit`, `Constrain`, `Follow`) resolved
  against the window, the mode and the sheet under the reader's eyes to a single scale; the
  three scales the pipeline keeps apart — `desired` (what the reader asked for, the ceiling a
  manual zoom resolves to), `display` (what is on screen this frame) and `committed` (what the
  mounted raster is crisp at); a transition tweened over 120 ms on an out-cubic curve (the web
  app's `ease_out_cubic`: it covers ground early and lands on the target rather than stopping
  dead on it), with the target re-resolved from the in-flight target so `+ +` advances two
  rungs, and a container follow landing in the frame it was asked for with its crisp commit
  held until the space around the page has been quiet for 180 ms. The fit arithmetic is the
  `FitDims` the reader already carries (one definition for the seed scale and the live
  refit); resolution records intent —
  a step writes `desired` and clears the fit, a refit writes `desired` from the fit, a
  constraint leaves `desired` alone so a hand-picked zoom overflows and scrolls instead of
  snapping back to fit. The reference's own doors, and no others: the reading bar's zoom
  cluster — the ladder's two steps with the readout between them, and the Fit Width / Fit
  Page choices beside it, which is the web app's reader menu kept where the reader's hands
  already are — the `+`/`=` and `-`/`_` keys as plain presses (a modified key belongs to the
  window's shortcuts, exactly as in the reference; there is no wheel-zoom door to port), the
  window's own follow posted on every resize report, and the page-turn re-fit while Auto
  Resize is on. Every one of them lands through the same resolver, and every scale moves
  through the same transition. *Accept: every zoom door lands on the same scale, the page
  stays crisp once a zoom settles, and a window drag moves the page continuously without a
  raster per frame.*
* **3c — The scrolling modes.** The continuous strip on the ported `virtual-list` windowing
  with `PAGE_GAP` and page margin, the horizontal strip, spread's gutter arithmetic, the
  scroll→page sync (`scroll_fraction` ↔ `fraction_offset`) and the mount anchor that lands the
  resume point, the first-paint gate. *Accept: all four modes read, and closing/reopening in
  each resumes where you were.*
* **3d — The sidebar.** The outline panel fed by PDFium's bookmarks through `pdf-core`'s
  outline shape with the active chapter following the page, and the thumbnail rail on a
  virtualized `lazy` rail with the page-pair LRU the web app kept (16 entries), prefetch
  around the reader, and the cover path reusing it. *Accept: browse a book's chapters and
  pages without waiting for a rail to repaint.*
* **3e — Search.** Per-page text extraction behind the engine (PDFium chars grouped into the
  runs `pdf_core::SearchIndex` expects), the index built lazily on the first search and
  retained per content identity, the search pill and hits list, the amber highlight layer,
  the overlay scrollbar and the page indicator, and the persistence of the reading position.
  *Accept: search a book, walk the hits, watch the page light up on the match.*
* **3f — Links, selection, covers.** Link annotations (internal jump, external open through
  the platform's URL opener), the selectable text layer built from the same char boxes
  (click/drag selection painting a tint over the page, double-click picking the word, copy to
  the clipboard), and covers rendered from real first frames into `covers/{book_id}.jpg` —
  including the backfill the shelf has been documenting and the prune a departure owes.
  *Accept: copy a sentence out of a PDF and find the covers on the shelf.*
**Carried out of P2, and where it lands.** One gap opened the moment the reader became real:
the shelf knows which books are missing, but a click on one had nowhere to go — the web app
answers it with the Find-again question, which re-points the row (a linked book takes the new
address; a stored book takes a fresh copy, measured before the row is written). 3a gates the
click instead: a row the library knows is dead does not open onto a document at all, and the
toast says so. The sheet, its folder walk and the copy land in their own increment before 3f,
because a bookshelf that cannot find a book it has lost is the one thing P2 promised and has
not yet delivered.

* **The refactor queue.** The codebase review (`review/`, outside the repo) lands its findings as
    small refactor passes rather than a big rewrite, and keeps the plan as the record: each pass is
    one CI-green commit, taken between phases so the reader work is never interrupted mid-feature.
    Passes 1–7 are in — the theme's one alpha helper and one home for the wall clock, the dead
    shell weight and one title rule; the `Cow` menu rows; `theme::Elevation` and its rules; the
    `chrome::desktop` / `platform::os` seam; the floating-geometry helpers; the control
    vocabulary's shared tests; the ghost button's one home, with its eight callers rewired — and
    the review has begun on the library surface, file by file: the shelf's cell module is now the
    kit plus one file per cell (the paths stay `card::book_card` and its neighbours), and the
    grid's cell arithmetic has one home that the metrics, the layout, the fit report and the view
    all read instead of re-deriving. `app.rs` followed the same way: 7,814 lines around one `impl`
    became eighteen modules under `src/app/`, each owning one subject — the walk, the landing, the
    copies, the moves, the sheets, the choosing, the ghosts, the rows, the shelves, the view, the
    reading route, the session, the events — with `mod.rs` left as the vocabulary they share and
    `update.rs` as the dispatch, and `crate::app`'s surface unchanged. The queue's remaining
    entries — the chrome's tests, `crates/library-core`'s larger files — follow the same shape. The
    dialog controls closed as no change: the three filled buttons the review suspected of one
    recipe are three looks, each with a single call site.
* **3g — The paper seam.** The render-queue priorities and cancellations under a zoom
  gesture (one lane, two priorities; a superseded render is dropped before the raster, not
  after), the memory ceilings (drop-on-close, the thumbnail LRU, the frame cache), and the
  hooks `pdf-paper` and the appearance pipeline dock into — a no-op until P5 fills them, so
  the reader is never rebuilt to receive them. *Accept: a page-flipping session's memory is
  flat, and a zoom gesture never queues a raster it will throw away.*



`ci.yml` (every push + PR, concurrency-cancelled):

* **lint** (ubuntu): system deps for winit (`libxkbcommon-dev`, `libwayland-dev`),
  `cargo clippy --workspace --all-targets --locked -- -D warnings`. No rustfmt gate — the
  ported crates are hand-formatted exactly as upstream (same deliberate gap the original
  documented); new code is written fmt-clean regardless.
* **test** (ubuntu): `cargo test --workspace --locked` — the ported suites are the parity
  proof; new native modules add their own tests (pixel pipeline, measurement, engine
  protocol) as they land. From P3, the lane also fetches the pinned Pdfium build
  (`bblanchon/pdfium-binaries`, release `chromium/7881`, `pdfium-linux-x64.tgz` → `lib/libpdfium.so`)
  into the runner's workspace and exports `MAREAEDER_PDFIUM_DIR` plus
  `MAREAEDER_REQUIRE_PDFIUM=1`: the engine's tests then run against a real PDFium instead of
  skipping, and the variable turns a missing library from a silent skip into a failure —
  a lane that passes because the engine was absent proves nothing. The download is cached by
  build number, so warm runs pay nothing for it.
* **Engine tests without a library.** Every test that needs Pdfium checks availability first
  and returns early with a printed note when the library is absent (a contributor on a clean
  machine still gets green tests); `MAREAEDER_REQUIRE_PDFIUM=1` upgrades that skip to a
  failure inside CI. The fixture they open is a hand-authored, byte-stable PDF committed
  under `tests/fixtures/` — three pages, a text run, a bookmark, a link — so the assertions
  can be exact (page count, page 1's size, a match for a known word, a chapter title).
* **build** (matrix ubuntu / macos / windows): `cargo build --locked` proves the app compiles
  where it ships, and `mareader --smoke` boots the whole app state in-process — settings, the
  library blob, both surfaces built as widget trees — and exits 0. No window is opened and no
  window server is needed, which is the point: the lane proves the app *starts* on the three
  systems it ships to without asking a runner for a display. The smoke list is the app's own
  shape, so a phase that adds a surface without adding it there fails the lane that exists to
  notice. Separate rust-cache keys per lane (two lanes sharing a key race and GitHub keeps
  only the first save).
* **lockfile bootstrap**: the first runs generate `Cargo.lock` and upload it as an
  artifact; it is committed back and the lanes switch to `--locked`.

* **The smoke lane's coverage is the app's own shape**: it boots the state (settings, library)
  and builds the surfaces in-process, and reports whether the surface list it was handed is
  complete — so a phase that adds a surface to the app without adding it to the smoke list
  fails the lane that exists to notice. No feature gymnastics are involved: the smoke path is
  a flag on the same binary every other lane builds, because an app whose *store* half is
  behind a feature is an app two builds disagree about. The Linux lanes need no GTK headers:
  `rfd`'s default backends on Linux are the XDG portal (pure-Rust `zbus` at build time, the
  system's own dialog at run time) and wayland, and the installed system deps stay the winit
  pair (`libxkbcommon-dev`, `libwayland-dev`) plus `pkg-config`.

`release.yml` (tags `v*`): resolve version → build per OS → fetch the pinned PDFium
release for the target → lay binary + sidecar (+ assets) into an archive → publish a
GitHub Release with the matching `release-notes/` file as the body. Prerelease tags
(`vX.Y.Z-*`) publish as prereleases.

## 6. Risks and answers

* **No local compiler** → vendor-read every API before use; small, self-contained pushes;
  the CI loop (push → watch → fix → squash) is the accepted rhythm.
* **PDFium availability at runtime** → sidecar in release bundles; graceful degradation in
  dev; system-library fallback.
* **iced gaps vs DOM** (no backdrop blur, no mix-blend-mode in canvas, no text selection on
  images) → translucent tokens for glass; software compositing where we own the pixels;
  a purpose-built selection overlay from PDFium char boxes. Each gap gets a native answer
  that keeps the behaviour, not an imitation of the CSS.
* **Big-bang risk** → phases are independently shippable; the app is useful from P2 and
  complete-looking from P5.

## 7. Progress

* **P0 — shipped.** Workspace, the ten ported crates, the frameless window and the full
  titlebar family (hover reveal, per-route pin, drag band, per-OS captions), tokens, the
  53-icon sprite, CI lanes green on clippy/test/3-OS build.
* **P1 — shipped.** Data dir, settings + library blob load/save (atomic writes, JSON),
  the folder walk with throttled progress beats, rfd pickers, toasts, the popover
  vocabulary on ui-geom, theme mix/wash.
* **P2 — shipped.** The shelf surface has landed: the library bar (breadcrumb, search
  pill, view menu, appearance button), grid + list layouts with book/folder/link cards
  and the add door, the empty state, shelf navigation, live query filtering, shelf
  creation, view persistence (layout/columns/cover/sort) and reload-from-disk. The
  folder import now runs on the ledger: the import sheet answers how the books are held
  (copied into the store, read at place, or read at place and watched) alongside the
  formats, the include/exclude rule, the size threshold and the structure; a walk diffs
  its findings against the watched folder's ledger (placements, tombstones, per-rung
  tracking), mints the shelf chain its rungs name, copies through the store with
  per-file results and each copy wearing its own measurement, and lands linked books at
  their addresses when the tree reads in place. Loose files from the picker or a drop
  land as the library's own copies, except where a read-at-place tree answers for them —
  those come back as that tree's books. Boot and every regained focus measure the
  library's addresses first, then walk the folders that owe a rescan; runs claim their
  folder's root so an ask and a rescan never race one ledger row. Removing a book now
  pays the whole debt: the tombstone that keeps a watched folder quiet, and — for a copy
  the library owns — the byte in the store. Folder cards and list rows say what they
  hold: the badge for where the books live, the watch dot, the books-and-shelves summary
  and the recursive 2×2 plate. The folder shelf now answers for its watch seat: the
  context menu's toggle row flips the rung the shelf holds, the add menu lists what the
  ledger remembers — removed books a click gives back (copies restored through the store,
  each wearing a fresh measurement) and books that moved, offered as show-here-also or
  go-and-look — and the import sheet seeds its answers from the ground's own tree, writing
  the watch answer back onto the rung the pick names. Selection has landed: a 450ms hold
  on any cell starts the choosing mode (Shift+Enter is its keyboard twin), taps toggle
  memberships, the set wears the ring, the check chip, the tint and the step back the web
  shelf wore, and one bar answers for it — All, Add to shelf (every shelf the whole set
  may nest onto, or a new one minted for the filing), Remove (one sheet, one receipt, one
  persist), Done. Right-clicks answer per target: a member of the set asks the selection's
  menu, the level's own floor asks for a shelf or the select-all door. The membership edits
  live in library/arrange.rs — filings, unfilings, bulk moves, nestings and sibling
  reorders on the id lists alone, with the web app's own lift-and-reinsert arithmetic and
  its tests; a move never touches a file the reader owns. The drag session rides those
  same facts: a press that travels 6px lifts its cell — or the whole set when the cell
  is in the selection — and a ghost follows the pointer, a fan of cover tiles carrying
  the count on its corner, swapping to the plate of the shelf a brewing fold will make.
  One pure table (library/drag.rs, with the web effect table's own tests) answers the
  gesture: a book cell offers the seam it would write — the bottom half lands after,
  the top before — and a 650ms rest over an unheld book brews the fold; a folder cell
  takes the hold inside on its middle band and reorders siblings on its edges, only
  where the shelf graph does not refuse; the empty level files what is held onto
  itself. Held cells fade, the target wears the seam, the ring or the tint, and list
  rows and grid cards alike report their bands through sensor zones. The release
  commits through arrange's own primitives — the insert index resolved against the
  anchor's container, and only while the view reorders by drag — and one persist covers
  the drop; Escape or a release over a refusal cancels and leaves the selection exactly
  as it was. The bar's crumbs are armed for the drag: every crumb — Home, with the
  library's own spelling of "no shelf", included — is a filing target from anywhere in
  the library, the crumb under the hold wears the accent under it rather than around
  it, and a 420ms rest sinks the ghost to a third of its size on the crumb, because
  the crumb is the one target smaller than the ghost and the shrink is what keeps the
  name being aimed at readable. The breadcrumb now folds: one pure module
  (library/fold.rs — the web fold's own arithmetic, carried over with its tests)
  decides the split against the bar's estimated budget; the web bar measured its
  crumbs with a hidden probe, and the native bar estimates the same boxes from the
  face's glyph step, the crumb's cap and the chevron — the one deviation, documented
  where the arithmetic lives. The ellipsis stands for the hidden levels: a hover
  opens its panel one beat behind the pointer and a 220ms grace closes it — a leave
  that lands inside the panel's own box arms nothing, because the layers queue the
  panel's enter before the ellipsis's exit, and the geometry, not the message order,
  decides who is right. The panel packs the elided chain into the rows the window's
  width decides, every crumb in it a way back and a filing target, and a drag opens
  the panel by hover alone: the ellipsis is a hover target and never a drop, the
  effect table's own refusal. Duplicates have landed with it: the pure half of the
  web service (library/duplicate.rs, with its tests) plans one entry at a time — a
  book, or a link at one, asks the store for a copy that lands beside the row the
  reader pointed at; a shelf asks for a whole fresh subtree of copies spliced in
  behind the original; a link at a shelf stays a pointer and lands on the spot —
  and the menus grew the web's own rows (Select, Duplicate) in the web's own order,
  the set's disabled Duplicate row waking up with the queue that walks it. The
  highlights' half of a duplicate waits on the reader's marks store, documented
  where the landing lives. The departure's gate has arrived around the moves: one
  pure module (`library/departure.rs`, with the web screen's own tests) says which
  rows a move takes into the store — a read-at-place book leaving the ground that
  made it, and only that — and the sheet asks the cost before anything moves. The
  answer rides a store batch of its own (one card for the whole gesture), the
  copies come home as the library's own with their moved-out logs written FIRST —
  the tombstone wears the book's original fingerprint — and the interrupted
  gesture resumes with what landed: a copy that failed costs that book its move
  and nothing else. Returns bind by address: a stored row landing where its source
  file stood spends the folder's moved-out log, and a departure's own landing
  never binds the log it just wrote. The covers a departure prunes wait on the
  engines, documented where the web does them. The level's name screen now rides
  the same moves: one pure module (`library/conflicts.rs`, the wiring around the
  core's own collision rule) screens every hand-move, filing and lift-out — what
  collides waits on the sheet, what does not lands now — and a queue holds the
  questions a busy sheet cannot take yet. The sheet asks which of three things
  the reader meant, in the web's own sentences: merge folds the arrival into the
  row that is there and inherits its shelves but the level it left; replace
  purges the displaced row through the removal's own sweep and seats the arrival
  in its slot on every shelf it held — a read-at-place arrival riding its own
  copy run first; "as new" mints the next free name, renames and moves; and
  where neither side is the reader's to destroy — a dragged read-at-place book
  meeting the library's own copy of its file — the sheet offers a link in place
  of replace, dissolves the dragged row and writes the folder's moved-out log
  bound to the survivor. The import's own questions (a merged folder's per-file
  ask, a covered file, the folder-name collision, the already-imported note) and
  the reveal's own light arrived with the walk and the gate, further down. The rung's and the removal's copy doors have arrived on the same
  sheet: taking a shelf apart now asks first whenever the level reads books in
  place — the question names the cost, the ground, and the way up (the nearest
  rung of the folder still standing, or the library's top level) — and "Copy and
  take apart" rides a store batch of its own, with the level coming apart only
  once every book it owed is safe; a book the store refused leaves the shelf
  standing so the reader can ask again. Removing shelves screens the same way:
  a shelf off the list that reads books in place buys their copies first, and
  the sheet's second answer — let the folder make the level again on its next
  import — removes without them; the removal itself runs whatever the copies
  did, deepest shelf first, because a shelf dissolved first is a shelf no
  sweep reaches. Underneath both, the take-apart is one pure primitive
  (`arrange::dismantle`, the web's `delete_shelf` with its own tests): the
  children re-hang on the parent, the folder's own rungs re-hang the way its
  next scan would hang them — a rung whose level is gone takes the nearest one
  still standing — the folder lets the rung go in its map, the books standing
  on the level come up exactly one level onto the nearest rung the tree still
  stands on, and the dead shelf links drop. The simplified dismantle this
  replaces left the folder's rungs hanging beside their tree and stranded the
  level's books; the menus' take-apart now rides the same primitive and the
  same door. The shelf's own move door has arrived on the same sheet: a drag
  onto a level, a bulk filing and a sibling seam all ride one screen — the
  core's own departing rule says which movers leave a read-at-place seat — and
  the question counts the books of every departing shelf in one row, promises
  each copy the level's next free name so the folder's own name stays free for
  the original, and offers a way home instead of a copy for a drop inside the
  mover's own family: an off-seat rung reseats under the shelf its directory
  names, and a displaced folder's root shelf folds back into the family tree —
  hung on the rung its directory names, its own rungs become the tree's, its
  ledger retires into the tree's, and a tree a walk holds is the one fold
  refused. The bought landing converts the departing rungs before any shelf
  write, marks the tree that rode along as hand-placed, lets the folder's map
  go of the departed zone, names the copies as promised, and seats what landed
  through the very gesture the screen wrapped; a shelf whose books the store
  refused entire stays where it was, with its own sentence. The import's
  book questions have arrived on the same sheet: one kind per question —
  the name collision, the folder's merge, the covered ground and the
  library's own copy elsewhere — and the screen, the words and the answers
  all dispatch on it. A loose file inside a tree the library reads in
  place, or one whose content the library already holds, now raises the
  two-answer question instead of landing silently beside its twin: import
  a copy of one's own here, or go to the one the reader has. A file whose
  name the level holds raises the name question the moves ride, and its
  three answers now cover the import's own half — "add as new" rides a
  single-file copy through the store, minted under the name the sheet
  showed, "make link" files a pointer at the row that is there, and
  "already imported" navigates to it. The questions ride their run's plan
  and are asked only once the copies have landed, because a sheet answered
  mid-flight would land beside a ghost. The merged folder's per-file
  question is described and answered here too — two names, three answers,
  a twin of one linked file withholding "as new" on both the sheet and
  the write, the merge healing the measurement only when the arriving
  file IS the row's file, the replace seating the copy in the purged
  row's slot — and the walk's own screen raises it, from the shelf map the
  merge is landing into. The batch
  has its switch: the apply-to-all row, shown only while questions of the
  sheet's own kind wait, sends one answer down the queue to every twin
  question and stops at the first different shape. The import's walk now
  answers the gate in front of it: a folder whose name the root level
  already holds is a question before it is an import, asked with the
  words the web's own sheet speaks — a stored arrival hearing the level's
  own three answers, a read-at-place one hearing the pointer and the
  merge, because a second read of one ground is the one thing the family
  gate exists to prevent. The answers decide what the walk is for: *show
  it* opens the shelf that is here; *make link* files a pointer at the
  root without a second shelf; *merge* threads a plan through the run so
  the folder files into the shelf the level already held — its root rung
  re-anchored onto that shelf, scanned tree minting fresh rungs under it,
  and every file named the shelf already held asked one question at a
  time down the import's own compact sheet; *as new* mints the walk a
  fresh row under the counter name, so a second run of one ground never
  spends the first run's answers; and *replace* sweeps the shelf's own
  rows out through the removal's own pass first, so the copies land in
  the names the shelves showed. Ground a tree still reads is the one case
  the bound walk cannot take: its copies land beside the tree, unbound —
  a scan-and-copy run with no ledger behind it, one card and no row of
  its own, the tree's shelved family stepping out of the way it never
  entered — while the rung folders' group answer cuts the same chain a
  bound walk would under the shelf the run mints for them. The replaced
  shelf's books leave through the same sweep, the answered ground never
  races a run that already holds it, and the queued ask behind a root now
  remembers the whole intent rather than only the ground. The covered
  re-import's own note has landed: not a question — an answer — naming
  the shelf its pick meant, its subline and sentence the web's own
  ("Nothing new to import" — the import walked the folder again and
  found nothing new — every book is already on the shelf), and every way
  out of it — the button, the close, the scrim, the Escape — ends on the
  same light. Under it the reveal itself: one write says what to light
  and a nonce to answer a second reveal of the same thing, and the light
  is the whole of the web's scroll-into-view and flash rendered on the
  native floor — `library/reveal.rs` reads the level and the offset off
  the library and the layout's own constants (the web measured its
  hidden probe; the native arithmetic stands where arithmetic lives,
  next to the constants it trusts), the shelf's scroll wears the
  reveal's identity and reports its own bounds so the offset is the
  viewport's own geometry, and the cell wears the membership's own ring
  for the flash's own sixteen-hundred millisecond beat. The covered
  walk's quiet answer fires as well: the covered continuation lit a
  landing that found nothing new asks the patch of no work at all, and
  a landing that found something answers by lighting where it
  stands — a run that found its pick answers the reader with their
  pick. The last three answers the import owed have landed with
  them. A found file whose folder's moved-out log is bound to a LIVING
  row is a book the reader already has: the walk and the loose drop
  both lift those files out of their findings before the ledger ever
  sees them, count them with the run's own report, and light the first
  — the log remembered that row, so the reader is taken to it rather
  than handed a second book beside it. A copies import of ground a
  read-at-place tree still reads now lands BESIDE the tree even in a
  bound run: the copy list is the ledger's own pure table
  (`copy_over_paths`), every address on it owes a book of its own, the
  heal-by-address pass skips it — a second instance is never a heal —
  and the mint pushes the row outright with its own adopted
  measurement, because `add_book`'s one-row-per-fingerprint rule is
  right for a walk and wrong for the instance the reader just asked
  for. And a plan that renames or merges now RE-SEATS what is already
  here: the walk's screen collects the rows the destination does not
  already wear a name for, the landing walks each one's rung onto the
  planned tree — the same `chain_for` the mints ride — so *merge into
  it* moves the folder's books instead of copying them again, and the
  rung a file lands on decides its seat, not the tree's root. What P3
  opens on is the engine the shelf has been promising all along: the
  PDFium service and bind strategy, the open pipeline, and the first
  reading surface the marks, covers and kept reading data all wait
  on.
* **P3 — 3a, 3b and 3c shipped (plus seven refactor passes); 3d (the sidebar) next.** The engine has landed: `pdfium-render` 0.9.4 binds the
  shared library at run time through `MAREAEDER_PDFIUM`/`MAREAEDER_PDFIUM_DIR`,
  beside the executable and its `lib`/`bin`, then the working directory's same three,
  then the system's loader — and a machine with no Pdfium gets a sentence naming
  every place that was looked rather than a crash. Binding is process-global, so the
  engine's own thread owns it: it starts on the first request, loads one document at
  a time inside the borrow that owns it, and answers a plain request/event protocol —
  a session stamp on every message, and a `FrameKey` (page, whole device pixels) on
  every raster, so a frame for a page the reader has left is dropped where it lands
  and a frame for the page on screen is kept and stretched until its replacement
  arrives. Text extraction flips Pdfium's per-character boxes into the reader's own
  space and groups them into runs with the same seams pdf.js split at. The reading
  surface itself: the open pipeline (the shelf's rows, a drop, a re-open on boot)
  gate the address, settle the row, read the resume point before the engine is asked
  for anything and clamp it to the book that actually opened; the status ladder
  `Idle → Opening → Ready/Error` paints a placeholder sheet in the meantime; page 1
  arrives as a real raster at the startup fit (the reader's own `FitDims`, byte-for-
  byte the web app's fit arithmetic, resolved once at the seed so the first frame
  lands where the fit is going); page turns come from the bar's pill, the arrow keys
  and PageUp/PageDown, land on the sheet's own box even where the book changes shape,
  and report the new position so the library's rows keep it; and leaving flush the
  read point into the rows the same `rows_for_read` rule names, with the shelf's own
  record of the name and author written the moment the document answers.
  And 3c opened the whole book at once: the continuous strip mounts on the ported
  windowing (`virtual-list`'s own `Strip` — one extent per page at the live scale, the
  fixed chrome between them, a window kept to a screenful of read-ahead and about one
  page either side of it, nothing off screen built), and a book reads in all four
  modes: the column, the horizontal strip whose pages are inset by the reader's margin
  rather than gapped and which centres the page it lands on, the paginated single, and
  the spread. Opening or flipping into a strip mounts it on the page the reader is on
  and aims the scroll, re-posting the aim until the surface answers — a post to a
  widget that is not built yet is nothing, so the cover over the open holds the posts
  until the resume page has painted, and the mount lands on the reader's page rather
  than on page one walking down to it. A turn within two viewports glides the strip,
  so the pages it passes are asked for as they come into view; a longer jump lands at
  once, aimed rather than shouted because one post can be clamped away by a surface
  that has just changed size; and a zoom holds the point under the middle of the
  window across the new geometry, because the pages scale and the chrome does not. The
  reader's own scroll is theirs: the surface's reports decide the page once they have
  gone quiet for twelve frames, every progress write goes through one door a turn or a
  jump has usually already been through, and a wheel notch — which iced hands to the
  axis it points at, so a horizontal strip never hears it — is translated by the app
  into the reader's own step along that strip.
  Refactor passes run between 3b and 3c, ahead of that queue, one CI-green commit each.
  The queue's own record lives in `review/` (outside the repo); what has landed there:
  the library's cell kit was split out of `card.rs`, the grid's cell arithmetic now has
  one derivation (`content_width`/`grid_metrics`) instead of four, the cells' rules
  (hover, selection, drag bands, layering) are shared between the cards and the list
  rows, the level's 32px rhythm is named once, the suites build their fixtures from
  `library_core::testkit`, the bar reads the fold's one packing (`fold::pack_elided`),
  the menus' decisions are pure functions with tests, and the two remaining god files
  in the shelf are gone: `conflicts.rs` is `conflicts/{mod,planned,naming,words}.rs` and
  `departure.rs` is `departure/{mod,leaving,returning,shelves,log,tests}.rs`. The eight split
  folders lost their `tests.rs` after that: every case moved into the file that covers it, its
  fixtures into that folder's `kit.rs` — 284 tests before and after, the same names and the
  same assertions — so a subject and the cases that prove it now share one file. Next in the
  queue is `src/app.rs` — 7,833 lines behind a frozen `Message`/`update`/`view` surface.
  The first took the shell's dead weight out: `theme::fade` was a byte-identical twin of
  `theme::wash` (a colour at a fraction of its own alpha) and 90 sites spelled it both
  ways — one helper now, `wash`; `theme::danger` names its one literal; `now_ms` was
  defined three times over and lives in `platform` (the stamps are persisted and read
  back on another run, so it is a wall clock on purpose); the app's own
  `folder_shelf_of` was a second copy of `library::departure`'s; `Covered.rel` was carried
  and never read; `Route::title`'s two arms returned the same string — the app's name is
  one constant now, and the window title and the bar's centre read it through one
  `shown_title()`.
  The second made one builder for each menu row: `item`/`owned_item`,
  `danger_item`/`owned_danger_item` and `section`/`owned_section` were the same bodies
  written twice over `&str` against `String`, and the sheet's `panel_owned` was
  `panel_sized` with an owned title — each pair is one builder over `impl Into<Cow<str>>`
  now, and the danger row takes the item's icon slot, which is what its 24px inset was
  imitating. The third named the shadows: sixteen `Shadow` literals in eight files, seven
  blur radii and four alphas with nothing saying which surface sits above which, are
  `theme::Elevation`'s ladder now — Thumb, Badge, Bar, Pill, Hover, Page, Ring, Float,
  Plate, Toast, Sheet, Chrome — carrying the port's numbers unchanged. The fourth settled
  the layer that had two names: `chrome::platform` is `chrome::desktop` (the bar's
  numbers) and the OS identity moved to `platform::{Os, os()}`, so `platform` means the
  OS layer only and `platform::fs` no longer reaches into the chrome for a fact about
  the machine it is already running on.
  The god file itself then went: `app.rs` is `src/app/` — eighteen modules under a frozen
  `Message`/`update`/`view` surface, the largest 799 lines — and `walk.rs` followed, the folder
  walk now `app/walk/{mod,plan,claim,rungs,checks,run,watch}.rs`: the plan, the root claim, the
  rungs, the checks and the one filesystem run each in their own file, the dwell and geometry
  constants on the reader that is their only reason to exist.
  Zoom is the web app's pipeline, ported whole into `reader/zoom.rs` and wired to one
  owner: `Command` (Step / Refit / Constrain / Follow) resolved against the window, the
  mode and the sheet under the reader's eyes; the three scales kept apart — `desired`,
  the reader's own and the ceiling a hand-picked zoom resolves to, `display`, what the
  painter reads this frame, and `committed`, the only scale a raster is ever asked at;
  The frame followed the god files: `app/view.rs` is `app/view/{mod,bar,layers,dock,select}.rs`
  now — the route's own view, the titlebar, the overlays, the runs' dock and the selection's bar —
  and the toggle the sheet's option rows and the view menu's pills both draw is one builder again,
  wearing the chip the web's `ToggleButton` calls its default.
    The sheets followed: `app/sheets.rs` is `app/sheets/{mod,rename,remove,import,copy,conflict}.rs`,
    one file per panel with the dispatcher keeping one line per `Sheet` variant, and `app/update.rs` —
    a single 727-line `update` over 93 arms — is
    `app/update/{mod,shell,reading,bar,view,rows,selection,sheets,add}.rs`: the dispatcher is a match
    of one-line delegations, and each subject file holds the handlers its arms call.
  and one transition, 120 ms on an out-cubic curve, retargeted from wherever the eye is
  when a second press lands mid-flight. A window drag is a *follow*: the layout is in
  the new window on the frame the size was reported — a scale that waited for the drag
  to end would leave the page wider than its box for the whole burst — while the crisp
  raster waits out a 180 ms quiet, so a drag costs one render instead of one per frame.
  A page turn re-resolves the same way when Auto Resize is on; a chosen fit answers in
  the frame it lands; a manual step drops the fit and becomes the ceiling, so a page
  zoomed in on stays there and overflows rather than snapping back. The bar's readout is
  the *display* scale, its step buttons are enabled by asking the resolver whether the
  ladder has anywhere to go, and the app drives the whole thing from its own frames
  subscription — alive exactly while a transition is, so a still reader costs no
  redraws. The name the bar's centre, the window title and the error card show is
  resolved once per move of the identity — the document's own `/Title` when it is one worth showing, else the
  row's display name, else the address's stem — and kept, because the bar asks for it
  every frame. All five CI lanes are green on the increment; the text lane (PDFium's
  per-character boxes, grouped into runs) is ported and driven by the engine's own
  test, and waits on the search that will ask it for a page.

## 8. The refactor pass (after the audit)

The codebase review (`review/`, kept outside this repo's history as its own
working set) read every folder and file and queued its findings as sections
S1…S8. The shelf section closed with the library split, and the reader area has
now had its own pass: the two `RETIRED_*` storage keys went (nothing native
reads localStorage — `settings.json` and `library.json` are the whole contract,
and the four surviving key constants now say so), `reflow-core`'s typography
stopped resolving into CSS custom properties and became what a native painter
needs (an ordered stack per family, plus the body font's average advance for the
pagination estimate), `reader-core`'s view model lost two constants no reader
named and the generic easing nothing eased with (its `virtual-list` dependency
went with them), and five helpers whose only caller was their own test were
deleted outright. The last duplicated fixtures — a fingerprint builder and three
copies of the page-box fixture — are one call each now. What remains flagged
rather than changed is the appearance pipelines' CSS-shaped output, which the
appearance phase will re-shape in iced terms, and the spread/preset rules that
are ported and tested ahead of the phases that consume them.
