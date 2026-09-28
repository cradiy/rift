# Rift

Rift is a GPUI file manager in development. It uses the host file system for
directory contents, standard user locations and mounted storage devices while
preserving the Finder-inspired visual direction.

```bash
cargo run -p rift
```

Rift creates its configuration on first launch. On Linux the default path is
`~/.config/rift/config.toml`; set `RIFT_CONFIG` to use another file. The current
settings cover the application font, logging and persistent browser preferences
such as view mode, sorting, hidden files and sidebar visibility. See
[`config.toml.example`](config.toml.example) for the complete schema.
`RUST_LOG` can temporarily override the configured logging level and filters.

Current browser interactions include virtualized grid and list views, marquee
selection, Shift/Ctrl multi-selection, Finder-style sorting and kind grouping,
hidden-file toggling, back/forward/up navigation and live mounted-device
discovery. File commands cover inline rename, create, copy, paste, move, Trash
and permanent deletion from Trash. Raster and SVG thumbnails are available in
both views, and the provider-based Quick Look currently previews images.

Keyboard navigation always supports arrow keys, Shift+arrow range selection,
Enter/Ctrl+O to open, Alt+Up for the parent directory, and Alt+Left/Alt+Right
for directory history. Optional Vim mode is disabled by default and can be
toggled from the toolbar `…` menu or with `browser.vim_mode` in the TOML file.
It adds `h/j/k/l`, `o`, `u`, `-`, `gg`, `G`, Ctrl+U/Ctrl+D, `a`, `d` and `D`.
The `a` dialog creates a file normally or a folder when the name ends with
`/`. Lowercase `d` waits for `y` confirmation before moving the selection to
Trash; uppercase `D` moves it immediately. Plain-letter bindings remain
disabled while searching or renaming.

Rift currently targets Linux. GPUI, GPUI Effects, GPUI Platform and UIC are
pulled from the same remote GPUI revision pinned in the workspace manifest.

See [docs/architecture.md](docs/architecture.md) for module boundaries and the
state/effect flow.
