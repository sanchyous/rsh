# Configuration

Everything can be changed from the settings screen (<kbd>F9</kbd>). Changes apply immediately and are saved right
away. This page documents the files behind it.

## Files

| Path | Contents |
|---|---|
| `~/.config/rsh/settings.toml` | Preferences (below) |
| `~/.config/rsh/hosts.toml` | The host tree. Passwords inside are sealed, see [security](security.md) |
| `~/.config/rsh/vault.toml` | KDF parameters, salt and the password check value. No secrets |
| `~/.config/rsh/known_hosts` | Host keys you trusted in RSH (OpenSSH format). `~/.ssh/known_hosts` is read but never written |
| `~/.cache/rsh/edit/` | Short-lived copies of remote files opened with <kbd>F4</kbd> or <kbd>Enter</kbd>. Removed when the editor or pager exits |
| `$XDG_RUNTIME_DIR/rsh/handoff-<pid>.sock` | Hand-off socket for <kbd>w</kbd> windows. Exists only while the main instance runs |

The config directory is created with `0700` permissions, and every file is written atomically with `0600`.

On **Windows** the same files live in `%APPDATA%\rsh\` (the edit cache in `%LOCALAPPDATA%\rsh\edit\`). Your profile
directory is already private to your account, so no extra ACLs are applied. The hand-off endpoint is a named pipe
(`\\.\pipe\rsh-handoff-<pid>-<random>`), and `~/.ssh/known_hosts` means `%USERPROFILE%\.ssh\known_hosts`.

## Environment

| Variable | Effect |
|---|---|
| `RSH_CONFIG_DIR` | Use another config directory, e.g. a separate profile or a test sandbox |
| `TERMINAL` | Preferred emulator for <kbd>w</kbd> when the setting is `auto` |
| `VISUAL`, `EDITOR` | Editor for <kbd>F4</kbd> when the setting is empty (falls back to `nano`, or `notepad` on Windows) |
| `PAGER` | Viewer for <kbd>Enter</kbd> on a file (falls back to `less`, or `more` on Windows) |
| `COLORTERM`, `TERM` | Colour depth detection in `auto` mode. `TERM=linux` turns on TTY mode |
| `LANG`, `LC_ALL`, `LC_CTYPE` | A non-UTF-8 locale turns on TTY mode in `auto` |
| `SSH_AUTH_SOCK` | ssh-agent used by the *Agent* and *Auto* auth methods. On Windows RSH talks to the OpenSSH agent service (`\\.\pipe\openssh-ssh-agent`) and then Pageant |

## `settings.toml`

Missing keys take their defaults, so the file can be as short as you like.

| Key | Default | Values |
|---|---|---|
| `lang` | `"Ru"` | `"Ru"`, `"En"` |
| `theme` | `"Midnight"` | Any theme name from the settings screen, e.g. `"Tokyo Night"`, `"Rosé Pine"`, `"Terminal (ANSI)"` |
| `icons` | `"Unicode"` | `"Unicode"`, `"Nerd"`, `"Emoji"`, `"Ascii"` |
| `border` | `"Rounded"` | `"Rounded"`, `"Plain"`, `"Thick"`, `"Double"` |
| `color_mode` | `"Auto"` | `"Auto"`, `"TrueColor"`, `"C256"`, `"C16"` |
| `tty_mode` | `"Auto"` | `"Auto"`, `"On"`, `"Off"` |
| `monitor_enabled` | `true` | Background reachability checks |
| `monitor_interval_secs` | `30` | Seconds between check rounds |
| `ping_method` | `"Tcp"` | `"Tcp"` connects to the SSH port, `"Icmp"` runs the system `ping` |
| `ping_timeout_ms` | `1500` | Probe timeout |
| `notify_status_change` | `true` | Toast when a monitored host goes down or comes back |
| `terminal_cmd` | `"auto"` | Emulator for <kbd>w</kbd>, see below |
| `scrollback` | `5000` | Lines of history per terminal |
| `keepalive_secs` | `30` | SSH keepalive interval, `0` disables it |
| `host_key_policy` | `"Ask"` | `"Ask"`, `"AcceptNew"`, `"Strict"` (see [security](security.md#host-keys)) |
| `show_diag_default` | `false` | Show the <kbd>F1</kbd> bar in new sessions |
| `confirm_quit` | `true` | Ask before quitting with live sessions |
| `auto_lock_mins` | `15` | Lock the vault after this many idle minutes, `0` = never |
| `sftp_show_hidden` | `false` | Show dotfiles in new SFTP panels |
| `sftp_buffer_kib` | `256` | Transfer chunk size |
| `editor` | `""` | Editor command for <kbd>F4</kbd>, e.g. `"nvim"` or `"code -w"`. Empty uses `$VISUAL`/`$EDITOR` |
| `term_type` | `"xterm-256color"` | `TERM` requested for remote PTYs |

### New-window terminal

With `terminal_cmd = "auto"` RSH uses `$TERMINAL`. If that is unset, it prefers kitty when running inside kitty,
otherwise it takes the first available of kitty, alacritty, foot, wezterm, ghostty, konsole, gnome-terminal,
xfce4-terminal and xterm, with the right arguments for each.

On **Windows**, `auto` opens a new console window with `start`. If Windows Terminal is your default terminal
application, the window opens in Windows Terminal. Setting `terminal_cmd = "wt"` also works, but Windows Terminal may
not pass the hand-off environment to the new tab. In that case the window will ask for the master password.

Any other emulator works through a template. `{cmd}` expands to the RSH command line and `{title}` to the window title:

```toml
terminal_cmd = "tilix --title {title} -e {cmd}"
```

## `hosts.toml`

You normally never edit this file by hand, but it is plain TOML and safe to version or template. Groups nest through
`[[...groups]]` tables, hosts through `[[...hosts]]`:

```toml
[root]
id = "root"
expanded = true

[[root.groups]]
id = "8c2f1e0a9b7d"
name = "production"
expanded = true

[[root.groups.hosts]]
id = "1f9a2b3c4d5e"
name = "web-01"
address = "203.0.113.10"
port = 22
user = "deploy"
auth = "Key"                         # Auto | Password | Key | Agent | Ask
key_path = "~/.ssh/id_ed25519"
key_secret = "v1:…"                  # sealed passphrase, written by RSH
tags = ["nginx", "prod"]
notes = "Frontend"
monitor = true
startup_cmd = "tmux new -A -s main"
```

Ids must be unique. A sealed secret is bound to its host's `id` (see [security](security.md)), so if you change an id
by hand, re-enter that host's password.
