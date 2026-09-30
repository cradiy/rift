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
| Ctrl + C / Ctrl + X / Ctrl + V | Copy / cut / paste items |
| Ctrl + Shift + N | Create a folder |
| Ctrl + A | Select all |
| Ctrl + H | Show or hide hidden files |
| Ctrl + T | Open a new tab at the current directory |
| Ctrl + W | Close the active tab |
| Ctrl + Tab / Ctrl + Shift + Tab | Select the next / previous tab |
| Ctrl + Page Down / Ctrl + Page Up | Select the next / previous tab |
| Delete | Move to Trash |
| Shift + Delete | Permanently delete an item from Trash |
| F5 or Ctrl + R | Refresh the directory |
| Alt + Up | Open the parent directory |
| Alt + Left / Alt + Right | Move backward / forward through directory history |

Mouse selection supports Shift or Ctrl for toggling multiple items.

Cut items remain in place and their icons and names are dimmed until pasted.
Only successfully moved items leave the cut buffer. Copying or cutting another
selection replaces the buffer; Escape keeps both the selection and buffer.
See [File operations](file-operations.md) for transfer and conflict behavior.

## Vim mode

Enable Vim mode from the toolbar `…` menu or set `browser.vim_mode = true` in
the configuration file.

| Shortcut | Action |
| --- | --- |
| `h` / `j` / `k` / `l` | Move left / down / up / right |
| `H` / `L` | Select the previous / next tab |
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
