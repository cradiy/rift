# File operations

## Copy, cut and paste

Use `Ctrl+C`, `Ctrl+X` and `Ctrl+V`, or the corresponding context-menu actions.
The file clipboard is shared by Rift tabs and windows in the same process.
It does not yet exchange file-copy/cut data with other applications.

Cut marks items without moving them. Both grid and list views dim their icons
and names while preserving the selection highlight. Pasting a cut selection
moves it; pasting a copied selection copies it. The folder context menu's
`Paste Into Folder` follows the same rules.

Successful moves clear only their own cut entries. Failed, skipped and cancelled
entries remain available for another paste. A new copy or cut replaces the
buffer, and completion of an older task cannot clear that new buffer. Pasting
cut items into their original directory is a no-op. Transfers overlapping a
pending move are rejected, including while it waits for a conflict decision.
Escape does not clear the selection or the clipboard. Cut is unavailable in Trash.

## Transfer progress

Normal small transfers are silent. For a transfer of at least 64 MiB or 128
recursively counted entries that is still running after 500 ms, a compact pie
percentage appears at the right end of the status bar. Click it to open the
details above it. The existing status text makes room automatically.

Visible successful tasks disappear after three seconds, unless their details are
currently open. Closing the details after that removes them immediately; opening
them in the past does not pin the result. Conflicts and failures always show,
regardless of size, and open the details so they cannot go unnoticed.

## Same-name conflicts

A copy or move pauses in the shared transfer panel when an existing destination
needs a decision. The panel shows the incoming and existing paths and types.

- **Keep both** generates an available `copy`-suffixed name, including for moves.
- **Skip** keeps both originals unchanged; skipped items are recorded separately.
- **Replace** prepares a complete new copy on the destination filesystem, then
  swaps it into place atomically on Linux. The displaced item goes to Trash.
  If moving it to Trash fails, Rift attempts to restore the old destination.

Folder replacement replaces the whole item; it does **not** merge directories.
Replacement is disabled for overlapping locations that could destroy the source.
Copying an item into its own directory creates a duplicate without a prompt.

`Apply to all conflicts` is off by default and affects only the remaining conflicts in
that task. A destination changed after prompting invalidates the choice and must
be reviewed again. Retrying a task asks about conflicts again rather than reusing
a destructive choice. Retry includes failures and cancelled unfinished items,
not intentionally skipped items or already completed items.

## Transfer safety and cancellation

Within a filesystem, moving uses a no-replace rename. An `EXDEV` error switches
the operation to copying followed by source removal; no disk-name guessing is
needed. Copying preserves symbolic links without following their targets.

Rift checks cancellation between directory entries and 1 MiB copy chunks.
Completed top-level items remain. A cancelled, unfinished destination created
by the task is cleaned up without removing the source or an existing target.
Cancelling while waiting for a conflict does not touch either item.

Copied files and destination directories are synced before a cross-filesystem
move removes the source. Source metadata is checked for changes during copying
and before removal. Once source removal or a replacement commit begins,
cancellation applies to the next item. Cleanup, rollback or source-removal
failures that require inspection are excluded from automatic retry.

Displaced items are trashed from a temporary staging location. Their Trash
restore location is therefore the staging path, not the original destination;
recover them manually to the desired folder.

These checks are not a full crash-recovery journal or a guarantee against
concurrent writers, failing hardware or unplugged storage. The panel reports
recovery paths when available; check both locations after a recovery failure.
