# Key bindings

Press <kbd>?</kbd> on the host screen to see the most important ones inside the app.

## Everywhere

| Key | Action |
|---|---|
| <kbd>Alt</kbd>+<kbd>1</kbd>…<kbd>9</kbd> | Jump to session tab N |

## Host screen

| Key | Action |
|---|---|
| <kbd>↑</kbd> <kbd>↓</kbd> / <kbd>k</kbd> <kbd>j</kbd> | Move |
| <kbd>PgUp</kbd> <kbd>PgDn</kbd> <kbd>Home</kbd> <kbd>End</kbd> | Jump |
| <kbd>→</kbd> / <kbd>l</kbd> | Expand group |
| <kbd>←</kbd> / <kbd>h</kbd> | Collapse group, or go to the parent group |
| <kbd>Space</kbd> | Toggle group |
| <kbd>Enter</kbd> | Connect in a tab (on a group: toggle it) |
| <kbd>w</kbd> | Connect in a **new terminal window** |
| <kbd>s</kbd> | Connect straight into SFTP |
| <kbd>a</kbd> | New host in the current group |
| <kbd>g</kbd> | New group inside the current group |
| <kbd>e</kbd> / <kbd>F4</kbd> | Edit host, or rename group |
| <kbd>d</kbd> / <kbd>Del</kbd> | Delete (asks first) |
| <kbd>c</kbd> | Duplicate host (secrets are re-encrypted for the copy if the vault is open) |
| <kbd>m</kbd> | Start moving the selected node. Pick a group, then <kbd>m</kbd>/<kbd>Enter</kbd> to drop, <kbd>Esc</kbd> to cancel |
| <kbd>Shift</kbd>+<kbd>↑</kbd> <kbd>↓</kbd> / <kbd>K</kbd> <kbd>J</kbd> | Reorder among siblings |
| <kbd>p</kbd> | Ping the selected host, or every host in the selected group |
| <kbd>P</kbd> | Ping all hosts |
| <kbd>M</kbd> | Toggle background monitoring for the selection |
| <kbd>/</kbd> | Search by name, address, user or tag. <kbd>Enter</kbd> connects to the first match, <kbd>Esc</kbd> clears |
| <kbd>i</kbd> | Import `~/.ssh/config` |
| <kbd>L</kbd> | Lock the vault now |
| <kbd>Tab</kbd> / <kbd>F12</kbd> | Go to open sessions |
| <kbd>F9</kbd> / <kbd>,</kbd> | Settings |
| <kbd>?</kbd> / <kbd>F1</kbd> | Help |
| <kbd>q</kbd> / <kbd>Ctrl</kbd>+<kbd>C</kbd> | Quit (asks if sessions are open) |

## Session: terminal

Every key not listed here goes to the remote shell, including <kbd>Ctrl</kbd>+<kbd>C</kbd>, <kbd>Esc</kbd> and <kbd>F3</kbd>–<kbd>F11</kbd>.

| Key | Action |
|---|---|
| <kbd>F1</kbd> | Toggle the diagnostics bar |
| <kbd>F2</kbd> | Switch to the SFTP commander (opens the SFTP channel on first use) |
| <kbd>F12</kbd> | Back to the host screen (the session keeps running) |
| <kbd>Alt</kbd>+<kbd>←</kbd> <kbd>→</kbd> | Previous / next tab |
| <kbd>Shift</kbd>+<kbd>PgUp</kbd> <kbd>PgDn</kbd> | Scroll back through history. Any other key returns to the live screen |
| <kbd>Esc</kbd> while connecting | Cancel |
| <kbd>Enter</kbd> / <kbd>r</kbd> on a closed session | Reconnect with the same credentials |
| <kbd>Esc</kbd> / <kbd>q</kbd> / <kbd>x</kbd> on a closed session | Close the tab |

Paste (bracketed if the remote side enables it) works through your terminal's normal paste.

## Session: SFTP commander

| Key | Action |
|---|---|
| <kbd>Tab</kbd> | Switch panel |
| <kbd>↑</kbd> <kbd>↓</kbd> <kbd>PgUp</kbd> <kbd>PgDn</kbd> <kbd>Home</kbd> <kbd>End</kbd> | Move |
| <kbd>Enter</kbd> | Enter a directory, or view a file in `$PAGER` |
| <kbd>Backspace</kbd> / <kbd>←</kbd> | Parent directory |
| <kbd>-</kbd> | Back to the previous directory |
| <kbd>Space</kbd> / <kbd>Ins</kbd> | Mark and move down |
| <kbd>*</kbd> | Invert marks |
| <kbd>Ctrl</kbd>+<kbd>A</kbd> | Mark all / none |
| <kbd>/</kbd> | Filter the panel as you type. <kbd>Enter</kbd> keeps it, <kbd>Esc</kbd> clears it |
| <kbd>.</kbd> / <kbd>Ctrl</kbd>+<kbd>H</kbd> | Show or hide dotfiles |
| <kbd>F9</kbd> / <kbd>Shift</kbd>+<kbd>F9</kbd> | Cycle sort key (name → type → size → time) / reverse |
| <kbd>Ctrl</kbd>+<kbd>R</kbd> | Refresh both panels |
| <kbd>Ctrl</kbd>+<kbd>G</kbd> | Go to a path (`~` works on the local side) |
| <kbd>F3</kbd> | Console: show + focus → panels focused → hidden |
| <kbd>Ctrl</kbd>+<kbd>T</kbd> | `cd` the console into the remote panel's directory |
| <kbd>F4</kbd> | Edit the file in `$EDITOR`. Remote files are uploaded back when they change |
| <kbd>F5</kbd> | Copy the marked items (or the one under the cursor) to the other panel. The target path is editable |
| <kbd>F6</kbd> | Move to the other panel. A bare name without `/` renames in place |
| <kbd>Shift</kbd>+<kbd>F6</kbd> | Rename |
| <kbd>F7</kbd> | New directory |
| <kbd>F8</kbd> / <kbd>Del</kbd> | Delete (recursive, asks first) |
| <kbd>Ctrl</kbd>+<kbd>X</kbd> | Cancel running transfers |
| <kbd>F1</kbd> / <kbd>F2</kbd> | Diagnostics bar / back to the terminal |
| <kbd>F10</kbd> / <kbd>F12</kbd> | Back to the host screen |

While the console has focus, keys go to the remote shell. <kbd>F1</kbd>, <kbd>F2</kbd>, <kbd>F3</kbd>, <kbd>F10</kbd>, <kbd>F12</kbd>
and <kbd>Shift</kbd>+<kbd>PgUp</kbd>/<kbd>PgDn</kbd> stay with RSH.

## Dialogs

| Where | Key | Action |
|---|---|---|
| Host form | <kbd>Tab</kbd> / <kbd>↓</kbd>, <kbd>Shift</kbd>+<kbd>Tab</kbd> / <kbd>↑</kbd> | Next / previous field |
| | <kbd>←</kbd> <kbd>→</kbd> / <kbd>Space</kbd> | Change the auth method, toggle monitoring |
| | <kbd>Enter</kbd> | Next field. Saves on the last one |
| | <kbd>Ctrl</kbd>+<kbd>S</kbd> | Save |
| | <kbd>Ctrl</kbd>+<kbd>R</kbd> | Reveal or hide secrets while typing |
| Any text field | <kbd>Ctrl</kbd>+<kbd>A</kbd> / <kbd>Ctrl</kbd>+<kbd>E</kbd> | Start / end |
| | <kbd>Ctrl</kbd>+<kbd>U</kbd> / <kbd>Ctrl</kbd>+<kbd>W</kbd> | Delete to start / previous word |
| Password | <kbd>Tab</kbd> | Switch between password and confirmation |
| Host key | <kbd>y</kbd> / <kbd>n</kbd> | Trust and save / reject |
| Confirm | <kbd>y</kbd> <kbd>Enter</kbd> / <kbd>n</kbd> <kbd>Esc</kbd> | Yes / no |
| Settings | <kbd>↑</kbd> <kbd>↓</kbd>, <kbd>←</kbd> <kbd>→</kbd> <kbd>Enter</kbd> | Choose, change |
| | <kbd>Esc</kbd> / <kbd>F9</kbd> | Close |
