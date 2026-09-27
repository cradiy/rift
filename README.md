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

Rift currently targets Linux. GPUI, GPUI Effects, GPUI Platform and UIC are
pulled from the same remote GPUI revision pinned in the workspace manifest.

See [docs/architecture.md](docs/architecture.md) for module boundaries and the
state/effect flow.
