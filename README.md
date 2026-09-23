# Mareader (native)

A desktop document reader for long-form reading — the native rebuild of
[Mareader](https://github.com/codewiththiha/mareader) on
[iced](https://github.com/iced-rs/iced) 0.14.

The web app (Tauri + Leptos/WebAssembly + pdf.js) and this one are the same
reader: same features, same look, same domain logic and the same persisted
data — on a lighter stack.

* **PDF** renders through **PDFium** (`pdfium-render`, bound at runtime — a
  completely different engine from pdf.js).
* **Plain text and Markdown** reflow through the app's own pagination on
  **cosmic-text** measurement, with **pulldown-cmark** painting the Markdown.
* **The domain is ported, not rewritten**: `crates/` carries the original's
  pure cores (settings schema, library governance, pagination maths, search
  index, colour pipeline) with their ~1,000 tests, so the library's logic and
  the persisted "database" are literally the same.
* **The window chrome is native per OS**: macOS wears AppKit's own traffic
  lights over a transparent full-size-content titlebar; Windows and Linux
  run frameless with the app's caption clusters. The bar is headless until
  hovered, pinnable per route, and grabbable.

See [PLAN.md](PLAN.md) for the architecture, the parity map and the phase
plan this repository is built by.

## Status

Under construction, phase by phase — each phase lands green on all three
platforms in CI before the next begins. Today: the foundation (window,
chrome, theme, ported domain cores).

## Development

```bash
cargo run
```

Prerequisites: a stable Rust toolchain; on Linux, `libxkbcommon-dev` and
`libwayland-dev`. PDF rendering additionally wants a `libpdfium` dynamic
library beside the binary in development (the release bundles ship one);
without it, the app still runs and says so.

## License

MIT — see [LICENSE](LICENSE).
