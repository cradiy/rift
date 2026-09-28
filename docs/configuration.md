# Configuration

Rift creates a TOML configuration file on first launch. On Linux its default
location is:

```text
~/.config/rift/config.toml
```

Set `RIFT_CONFIG` to load and save another path. Unknown fields and invalid
values are rejected instead of being silently ignored. The complete default
file is available in [`config.toml.example`](../config.toml.example).

## Fonts

| Setting | Type | Default | Description |
| --- | --- | --- | --- |
| `fonts.family` | string or omitted | omitted | UI font family; an explicitly empty value is invalid |

## Logging

| Setting | Type | Default | Description |
| --- | --- | --- | --- |
| `logging.level` | `trace`, `debug`, `info`, `warn`, `error`, `off` | `info` | Base logging level |
| `logging.directory` | path | `~/.local/state/rift/logs` | Log directory; relative paths resolve beside the configuration file |
| `logging.module_filters` | string | see example | Per-module filter directives |
| `logging.max_file_size_bytes` | positive integer | `10485760` | Rotation threshold for one log file |
| `logging.retained_files` | positive integer | `7` | Number of rotated files retained |
| `logging.console` | boolean | `true` | Also write logs to the launching terminal |

`RUST_LOG` can temporarily override the configured logging filters for the
current process.

## Browser

| Setting | Type | Default | Description |
| --- | --- | --- | --- |
| `browser.view_mode` | `grid` or `list` | `grid` | File view layout |
| `browser.sort_field` | `name`, `modified`, `size`, `kind` | `name` | Sort or grouping field |
| `browser.sort_direction` | `ascending` or `descending` | `ascending` | Sort direction |
| `browser.directories_first` | boolean | `true` | Keep directories before files |
| `browser.show_hidden_files` | boolean | `false` | Show Linux dotfiles |
| `browser.sidebar_visible` | boolean | `true` | Restore sidebar visibility on launch |
| `browser.vim_mode` | boolean | `false` | Enable Vim-style keyboard commands |

Browser preferences changed in the UI are saved asynchronously and restored on
the next launch.
