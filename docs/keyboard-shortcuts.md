# Keyboard shortcuts

Rift keeps conventional desktop shortcuts available at all times. Vim mode is
optional, disabled by default, and adds plain-letter navigation and commands.
Plain-letter bindings are suspended while searching, renaming, or entering a
file name.

## Standard shortcuts

| Shortcut | Action |
| --- | --- |
| Arrow keys | Move the active selection |
| Shift + Arrow keys | Extend the selection |
| Home / End | Select the first / last item |
| Page Up / Page Down | Move one visible page |
| Enter or Ctrl + O | Open the selected item |
| Space | Quick Look |
| F2 | Rename |
| Alt + Enter | Get Info |
| Ctrl + C / Ctrl + V | Copy / paste items |
| Ctrl + Shift + N | Create a folder |
| Ctrl + A | Select all |
| Ctrl + H | Show or hide hidden files |
| Delete | Move to Trash |
| Shift + Delete | Permanently delete an item from Trash |
| F5 or Ctrl + R | Refresh the directory |
| Alt + Up | Open the parent directory |
| Alt + Left / Alt + Right | Move backward / forward through directory history |

Mouse selection supports Shift or Ctrl for toggling multiple items.

## Vim mode

Enable Vim mode from the toolbar `…` menu or set `browser.vim_mode = true` in
the configuration file.

| Shortcut | Action |
| --- | --- |
| `h` / `j` / `k` / `l` | Move left / down / up / right |
| `o` | Open the selected item |
| `u` or `-` | Open the parent directory |
| `gg` / `G` | Select the first / last item |
| Ctrl + U / Ctrl + D | Move one visible page up / down |
| `a` | Create an item; a trailing `/` creates a folder |
| `d`, then `y` | Confirm moving the selection to Trash |
| `D` | Move the selection to Trash immediately |
| `cf` | Copy the selected file name as text |
| `cc` | Copy the selected full path as text |
| `cd` | Copy the selected parent-directory path as text |

### Which Key

Every Vim command containing two or more keys participates in Which Key. After
the first key, Rift waits briefly before showing the valid continuations. Fast
sequences complete without flashing the panel. Press Escape or an unsupported
continuation to cancel the pending sequence.

Current prefixes are:

- `g`: navigation commands (`gg`)
- `c`: text-copy commands (`cf`, `cc`, `cd`)
