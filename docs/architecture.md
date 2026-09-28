# Rift architecture

Rift is a Linux-first GPUI file manager. Its architecture separates browser
policy, operating-system I/O and GPUI presentation so that new views and file
types do not require filesystem logic to move into the UI.

The dependency direction is intentionally one-way:

```text
                         ┌─────────────┐
                         │ rift-config │
                         └──────▲──────┘
                                │
┌───────────────┐       ┌───────┴────────┐       ┌───────────────┐
│ rift-platform │ <──── │      app       │ ────> │    rift-fs    │
└───────────────┘       │ GPUI + UIC UI  │       └───────┬───────┘
                        └───────┬────────┘               │
                                └──────────┬──────────────┘
                                           ▼
                                    ┌─────────────┐
                                    │  rift-core  │
                                    └─────────────┘
```

`rift-core` has no dependency on GPUI or concrete filesystem APIs.
`rift-fs` implements its ports, while `app` is the composition root.

## Workspace crates

### `rift-core`

Owns framework-independent file-manager policy:

- domain models such as `Entry`, `EntryKind`, `EntryCategory` and `Location`;
- `BrowserState`, directory history, selection, sorting, hidden-file state and
  grid/list view mode;
- `BrowserMessage` inputs and `BrowserEffect::ReadDirectory`;
- `NavigationState` and its stale-request protection;
- `FileSystem` and `NavigationSource` ports;
- filesystem command types such as `FileOperation` and structured errors.

Directory reads use request IDs. Results from an older asynchronous request
cannot replace a newer directory or navigation snapshot. The selected paths,
active item and range-selection anchor live in `BrowserState`, including atomic
selection replacement for marquee and keyboard range selection.

### `rift-fs`

Implements the core ports with the host filesystem. It owns:

- directory enumeration and metadata conversion;
- standard Linux user-directory discovery;
- `/proc/self/mountinfo` parsing and removable-device discovery;
- rename, create, recursive copy, move and collision-safe copy naming;
- freedesktop Trash integration and permanent deletion from Trash.

It does not know about GPUI, visual components, modals or browser history.
Filesystem failures are returned as `FileSystemError` values rather than being
presented directly.

### `rift-platform`

Contains small operating-system integration points that are not filesystem
policy. It currently opens files with the system-associated application and
provides platform detection helpers. Keeping this separate prevents process or
desktop-launch APIs from leaking into `rift-core`.

### `rift-config`

Owns the strict TOML schema, validation, default-file creation, path resolution
and atomic replacement when settings are saved. The configuration covers fonts,
logging and durable browser preferences. It does not initialize the logger or
depend on GPUI, so future binaries can reuse it.

### `app`

The desktop composition root wires concrete adapters to state and renders the
interface:

- `app.rs` loads configuration, initializes logging/UIC, creates the window and
  injects `LocalFileSystem` plus `SystemNavigationSource`;
- `config.rs` maps persisted browser preferences to core state and serializes
  updates through a dedicated background writer so UI interactions never wait
  on configuration I/O;
- `presentation/controller.rs` owns `BrowserState`, executes directory effects
  and filesystem commands away from the GPUI thread, and stores the in-process
  copy buffer;
- `presentation/navigation.rs` loads sidebar locations and refreshes mounted
  devices every five seconds;
- `presentation/browser.rs` converts core entries into display-ready items;
- `ui/file_browser/` owns browser interaction and layout;
- `ui/components/` contains reusable folder icons and image thumbnails;
- `ui/quick_look/` is a provider-based preview host;
- `ui/theme.rs` centralizes modal, input, frosted-glass and liquid-glass
  appearances.

## File-browser UI modules

`FileBrowser` coordinates several focused modules instead of owning all
behavior in one render function:

```text
file_browser/
├── mod.rs            view-owned state and top-level composition
├── files.rs          virtualized grid/list rows, hit testing and marquee UI
├── toolbar.rs        navigation, view and sorting controls
├── sidebar.rs        standard locations, Trash and mounted devices
├── context_menu.rs   item/blank-area UIC menus and submenus
├── actions.rs        Linux key bindings and action dispatch
├── file_actions.rs   dialogs and asynchronous filesystem commands
└── inline_rename.rs  persistent UIC TextInput lifecycle for in-place rename
```

Grid and list views use independent `ListState` and scrollbar state. Only
visible rows are laid out. Marquee hit testing consumes those measured bounds,
so it remains compatible with virtualization and scrolling. Grid/list scroll
positions, inline-edit state and marquee geometry are view-local; durable
selection and browser settings remain in `BrowserState`.

Keyboard navigation is resolved against the same presented item order used by
the active view. Grid movement uses measured column count and grouped-row
geometry; list movement stays vertical. The search and inline-rename inputs add
an `editing` key context so plain-letter Vim bindings never consume text input.

## State and operation flows

### Directory browsing

```text
GPUI event
  -> BrowserMessage
  -> BrowserState::update
  -> BrowserEffect::ReadDirectory
  -> BrowserController (background executor)
  -> dyn FileSystem::read_directory
  -> DirectoryLoaded / DirectoryLoadFailed
  -> BrowserState::update
  -> observed GPUI re-render
```

### Filesystem commands

Mutating operations are commands rather than browser-state effects:

```text
UI action or context-menu item
  -> FileOperation
  -> BrowserController::perform (background executor)
  -> dyn FileSystem::perform
  -> FileOperationResult / FileSystemError
  -> refresh BrowserMessage + toast feedback
```

The controller copy buffer stores source paths only for the current process.
Paste performs `CopyInto`; Move uses an explicit destination. Trash and
permanent deletion are separate operations, and permanent deletion is exposed
only while browsing Trash.

### Sidebar discovery

```text
NavigationController
  -> NavigationEffect::Load
  -> dyn NavigationSource
  -> user directories + mounted volumes
  -> NavigationState
  -> sidebar re-render
```

### Configuration persistence

```text
view/sort/hidden-file/sidebar change
  -> AppConfig updates the in-memory BrowserConfig
  -> ordered background configuration writer
  -> rift-config validation + TOML serialization
  -> atomic replacement of config.toml
```

Only durable preferences are saved. Current directory, selection, scroll
positions, inline-edit state and marquee geometry remain session-local. Rapid
preference changes are serialized and coalesced so an older write cannot replace
the latest state.

### Quick Look

Quick Look uses a registry of `QuickLookProvider` implementations. The host
owns focus, Space-to-close behavior, modal sizing and shared liquid-glass
chrome; a provider only decides whether it supports an item and renders the
preview content. The first provider supports raster images and SVG. Additional
document, audio or video providers can be registered without changing the
browser or modal lifecycle.

## Images and caching

`ImageThumbnail` is shared by grid and list layouts. Raster files are decoded
through GPUI's asset system, resized off the UI thread and cached with a key
containing path, modification time and byte length. SVG thumbnails and SVG
Quick Look use `color_svg`; raster Quick Look uses GPUI's image source directly.
Malformed or empty SVG files fall back to a placeholder instead of being
retried on every repaint.

## Dependency policy

GPUI, `gpui_effects`, `gpui_platform` and UIC come from the same remote GPUI
repository and are pinned to one Git revision in the workspace `Cargo.toml`.
Keeping all four packages on the same revision prevents API skew while making
the build reproducible. CPU-heavy SVG dependencies retain optimized development
profiles because Quick Look and thumbnails must remain usable in debug builds.

## Boundary rules

- `rift-core` must not depend on GPUI, UIC or concrete filesystem APIs.
- `rift-fs` must not mutate browser state or format UI strings.
- `rift-platform` owns desktop integration, not file-manager policy.
- `rift-config` defines, validates and atomically stores settings but does not
  initialize logging or UI globals.
- GPUI components must not perform blocking filesystem mutations directly.
- Blocking reads, mutations, image decoding and platform opens must leave the
  GPUI thread.
- Device discovery stays behind `NavigationSource`; Linux mount details do not
  leak into presentation models.
- Persistent browser policy belongs in core state; transient geometry, focus,
  scroll and inline-input state belong in the view.
- Context menus and key bindings must dispatch the same actions so behavior
  does not diverge by input method.
- New preview formats implement `QuickLookProvider`; they do not add file-type
  branches to `FileBrowser`.
- New filesystem operations extend the core command/port contract, receive an
  adapter implementation, and only then become UI actions.

These boundaries leave room for directory watching, tabs, remote providers and
additional previews without rewriting the visual layer.
