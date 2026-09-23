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
  * `store/` — the library's own roof: one folder per stored book under its stable id
    (`library_core::store` rules unchanged).
* Atomic write discipline: temp file + rename, so a crash mid-write cannot corrupt a blob.

### 3.3 PDF engine (PDFium)

* `pdfium-render` 0.9 (default features: `pdfium_latest`, `image_latest`, `thread_safe`;
  objects are `Send + Sync` in 0.9). Bind order: library beside the executable (the release
  bundle's sidecar) → system library → friendly "PDF engine unavailable" state (the app
  still runs: library + text formats work).
* A dedicated engine thread owns documents and the render queue (page renders, thumbnails,
  text extraction, outline/links) — the native analogue of pdf.js's worker. The UI posts
  requests; results return as messages with RGBA frames.
* Frames render at device-pixel-grid sizes (`pdf_core::snap_to` with the live scale factor),
  BGRA→RGBA, then through the appearance raster pipeline (§3.5) before becoming
  `image::Handle::from_rgba` for the page host.
* Text: `PdfPageText` chars (loose bounds) → `pdf_core::SearchIndex` — the same Rust index
  the web app built from pdf.js text items; highlight rects, snippets and wrap-around come
  from reader-core's shared search model.
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

## 5. CI/CD

`ci.yml` (every push + PR, concurrency-cancelled):

* **lint** (ubuntu): system deps for winit (`libxkbcommon-dev`, `libwayland-dev`),
  `cargo clippy --workspace --all-targets --locked -- -D warnings`. No rustfmt gate — the
  ported crates are hand-formatted exactly as upstream (same deliberate gap the original
  documented); new code is written fmt-clean regardless.
* **test** (ubuntu): `cargo test --workspace --locked` — the ported suites are the parity
  proof; new native modules add their own tests (pixel pipeline, measurement, engine
  protocol) as they land.
* **build** (matrix ubuntu / macos / windows): `cargo build --locked` proves the app
  compiles where it ships. Separate rust-cache keys per lane (two lanes sharing a key race
  and GitHub keeps only the first save).
* **lockfile bootstrap**: the first runs generate `Cargo.lock` and upload it as an
  artifact; it is committed back and the lanes switch to `--locked`.

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
* **P2 — in flight.** The shelf surface has landed: the library bar (breadcrumb, search
  pill, view menu, appearance button), grid + list layouts with book/folder/link cards
  and the add door, the empty state, shelf navigation, live query filtering, shelf
  creation, the file picker's import flow, view persistence (layout/columns/cover/sort)
  and reload-from-disk. The folder walk now imports: it mints a shelf for the folder at
  the level on screen, nests a shelf per subfolder, files each document as a linked
  book on the ledger's say-so, and reports itself live from a dock pill. Governance
  has landed too: the shelf's own menu off the last crumb (rename in place, take
  apart), right-click menus for books, links and folder shelves, a sheet primitive
  carrying the rename and remove questions, and OS reveal-in-folder. Next: the
  import sheet (formats, threshold, link-or-copy, grouping), watched-folder
  governance (rescan, restore), selection and drag, duplicates.
