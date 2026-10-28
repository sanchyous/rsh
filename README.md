<div align="center">

<img src="assets/logo.svg" alt="RSH" width="300">

### A terminal SSH commander that looks as good as it works

Host tree · encrypted vault · built-in terminal · Total-Commander-style SFTP · live monitoring

![Rust](https://img.shields.io/badge/rust-2024-f74c00?style=flat-square&logo=rust&logoColor=white)
![ratatui](https://img.shields.io/badge/tui-ratatui-7aa2f7?style=flat-square)
![SSH](https://img.shields.io/badge/ssh-russh-bb9af7?style=flat-square)
[![CI](https://github.com/sanchyous/rsh/actions/workflows/ci.yml/badge.svg)](https://github.com/sanchyous/rsh/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/sanchyous/rsh?style=flat-square&color=7aa2f7)](https://github.com/sanchyous/rsh/releases/latest)
![Linux](https://img.shields.io/badge/linux-x86__64%20%7C%20aarch64-73daca?style=flat-square&logo=linux&logoColor=white)
![Windows](https://img.shields.io/badge/windows-x64%20%7C%20arm64-7dcfff?style=flat-square&logo=windows&logoColor=white)

**English** · [Русский](README.ru.md)

</div>

<br>

<p align="center"><img src="assets/screen-hosts.svg" alt="Host tree" width="100%"></p>

## Why

`~/.ssh/config` remembers where your servers live, but it can't tell you which of them are up, it won't keep your
passwords safe, and copying a file still means a second tool. RSH puts all of that in one keyboard-driven screen:

- **Hosts in a tree.** Groups inside groups, live online status and latency next to each host, fuzzy search.
- **Secrets in a real vault.** Argon2id + XChaCha20-Poly1305 under one master password, and nothing readable ever touches the disk.
- **A terminal that stays out of the way.** Full xterm emulation in tabs. <kbd>F1</kbd> adds a one-line diagnostics bar.
- **Files without leaving the session.** <kbd>F2</kbd> flips the same connection into a dual-panel SFTP commander,
  and <kbd>F3</kbd> drops a shell underneath it.
- **Runs anywhere you type.** Truecolor down to the bare Linux console, 18 themes, Nerd Font icons or pure ASCII.

## Screenshots

<table>
<tr>
<td width="50%"><img src="assets/screen-terminal.svg" alt="Terminal with diagnostics bar"></td>
<td width="50%"><img src="assets/screen-sftp.svg" alt="SFTP commander with console"></td>
</tr>
<tr>
<td align="center"><sub>Tabs, xterm emulation and the <kbd>F1</kbd> diagnostics bar</sub></td>
<td align="center"><sub><kbd>F2</kbd> SFTP commander with a live transfer and the <kbd>F3</kbd> console</sub></td>
</tr>
<tr>
<td colspan="2"><img src="assets/screen-settings.svg" alt="Settings with live theme preview"></td>
</tr>
<tr>
<td colspan="2" align="center"><sub>Settings apply instantly, here in Rosé Pine with a live palette preview</sub></td>
</tr>
</table>

## Install

**Linux: one command, any distro** (detects your package manager and architecture, verifies the checksum):

```bash
curl -fsSL https://github.com/sanchyous/rsh/releases/latest/download/install.sh | sh
```

**Windows: one command in PowerShell** (no admin rights; installs to `%LOCALAPPDATA%\Programs\rsh` and adds it to `PATH`):

```powershell
irm https://github.com/sanchyous/rsh/releases/latest/download/install.ps1 | iex
```

<details>
<summary><b>Or pick your package manually</b></summary>

<br>

**Debian / Ubuntu / Mint / Pop!_OS**
```bash
curl -fLO https://github.com/sanchyous/rsh/releases/latest/download/rsh_amd64.deb   # arm64: rsh_arm64.deb
sudo apt install ./rsh_amd64.deb
```

**Fedora / RHEL / Rocky / Alma**
```bash
sudo dnf install https://github.com/sanchyous/rsh/releases/latest/download/rsh-x86_64.rpm   # or rsh-aarch64.rpm
```

**openSUSE**
```bash
curl -fLO https://github.com/sanchyous/rsh/releases/latest/download/rsh-x86_64.rpm
sudo zypper install --allow-unsigned-rpm ./rsh-x86_64.rpm
```

**Arch / Manjaro / EndeavourOS / CachyOS**
```bash
curl -fLO https://github.com/sanchyous/rsh/releases/latest/download/rsh-x86_64.pkg.tar.zst
sudo pacman -U ./rsh-x86_64.pkg.tar.zst
```
PKGBUILDs for the AUR (`rsh` from source, `rsh-bin` prebuilt) are in [`packaging/aur`](packaging/aur).

**Alpine**
```bash
wget https://github.com/sanchyous/rsh/releases/latest/download/rsh-x86_64.apk
sudo apk add --allow-untrusted ./rsh-x86_64.apk
```

**Any Linux: static binary, no root**
```bash
curl -fsSL https://github.com/sanchyous/rsh/releases/latest/download/install.sh | RSH_INSTALL_METHOD=tarball RSH_PREFIX=~/.local sh
```

**Windows: MSI installer** (system-wide, adds `rsh` to `PATH`, uninstall from *Apps & features*): download `rsh-x86_64.msi` or `rsh-aarch64.msi` from the [latest release](https://github.com/sanchyous/rsh/releases/latest) and run it, or silently:
```powershell
irm https://github.com/sanchyous/rsh/releases/latest/download/rsh-x86_64.msi -OutFile rsh.msi; msiexec /i rsh.msi /qn
```

**Windows: Scoop**
```powershell
scoop install https://raw.githubusercontent.com/sanchyous/rsh/main/packaging/scoop/rsh.json
```

**Windows: portable zip.** `rsh-x86_64-windows.zip` / `rsh-aarch64-windows.zip`: unpack and run `rsh.exe`.

**From source** (Rust 1.88+ and a C compiler; NASM on Windows x64)
```bash
cargo install --locked --git https://github.com/sanchyous/rsh
```

</details>

Linux packages ship the man page (`man rsh`) and bash / zsh / fish completions; Windows builds ship PowerShell completion.

| Variable | Meaning |
|---|---|
| `RSH_VERSION=v0.1.0` | Install a specific release instead of the latest |
| `RSH_INSTALL_METHOD=deb\|rpm\|pacman\|apk\|tarball` | Force the package type |
| `RSH_PREFIX=~/.local` | Where the tarball method installs |

Uninstall. Linux: `curl -fsSL https://github.com/sanchyous/rsh/releases/latest/download/install.sh | sh -s -- --uninstall` · Windows: `$env:RSH_UNINSTALL=1; irm https://github.com/sanchyous/rsh/releases/latest/download/install.ps1 | iex`

## Quick start

```bash
rsh                         # open the TUI
```

1. On first launch RSH asks for a **master password**, which encrypts every secret you store.
   You can skip it and create it later, when you save the first password.
2. Press <kbd>a</kbd> to add a host. The address field understands `user@host:port`.
   Press <kbd>g</kbd> for a group, or <kbd>i</kbd> to import `~/.ssh/config`.
3. <kbd>Enter</kbd> connects in a tab, <kbd>w</kbd> opens the session in a **new terminal window**,
   and <kbd>s</kbd> goes straight to SFTP.

```bash
rsh connect web-01            # straight into a session (name, id, address or user@host:port)
rsh connect web-01 --sftp     # straight into the file manager
rsh cp ./dist.tar web-01:/tmp/        # copy over SFTP, with a progress bar
rsh cp --scp web-01:/etc/nginx/nginx.conf .   # legacy SCP protocol
rsh list                      # print the host tree
rsh import                    # import ~/.ssh/config
```

## Features

<table>
<tr><td valign="top" width="50%">

#### 🗂 Connections
- Groups and subgroups, collapsible, with online counters
- Move nodes between groups (<kbd>m</kbd>), reorder them (<kbd>Shift</kbd>+<kbd>↑↓</kbd>), duplicate hosts (<kbd>c</kbd>)
- Search by name, address, user or tag
- Per-host startup command, tags and notes
- Import from `~/.ssh/config`

#### 🔐 Security
- Master password → **Argon2id** (64 MiB, t=3, p=4) → **XChaCha20-Poly1305**
- The host id is the AEAD associated data, so a sealed secret can't be moved to another host
- The key lives only in memory, auto-locks when idle, and can be rotated with full re-encryption
- Host keys checked against `~/.ssh/known_hosts` plus RSH's own file. Trust-on-first-use dialog with the fingerprint, and a loud warning when a key changes
- Atomic `0600` writes for every config file

#### 🔑 Authentication
- **Auto**: agent → default keys → stored password
- Password with keyboard-interactive fallback
- Private key with an encrypted passphrase
- ssh-agent
- Ask every time (nothing stored)

</td><td valign="top" width="50%">

#### 🖥 Terminal
- vt100 emulation, 256 colours and truecolor, bracketed paste
- Scrollback with <kbd>Shift</kbd>+<kbd>PgUp</kbd>, sessions in tabs (<kbd>Alt</kbd>+<kbd>1..9</kbd>)
- **<kbd>F1</kbd> diagnostics**: SSH RTT with sparkline, ↓/↑ throughput, totals, cipher, kex, host key, auth method, TCP and handshake timings, uptime
- **<kbd>w</kbd> new window**: kitty, alacritty, foot, wezterm, ghostty, konsole, gnome-terminal, xterm or your own template. Credentials are handed over through a one-time token, so you don't type the master password twice

#### 📁 SFTP commander
- Two panels (local ⇄ remote), marks, filter, natural sort by name, type, size or time
- Recursive copy and move with progress, speed, ETA and cancel. Permissions are preserved
- <kbd>F4</kbd> edits a remote file in `$EDITOR` and uploads it back on save
- <kbd>F3</kbd> console under the panels, opened in the current remote directory

#### 📡 Monitoring
- Background reachability checks over TCP (the SSH port itself) or ICMP
- RTT history, availability %, toast notifications when a host goes down or comes back

</td></tr>
</table>

## Make it yours

| Setting  | Options |
|----------|---------|
| Language | English · Русский |
| Theme    | Midnight · Catppuccin Mocha · Catppuccin Latte · Gruvbox · Nord · Matrix · Dracula · Tokyo Night · One Dark · Solarized Dark · Solarized Light · Rosé Pine · Kanagawa · Everforest · Monokai · Cyberpunk · GitHub Light · Terminal (ANSI) |
| Icons    | Unicode · Nerd Font (per-file-type icons) · Emoji · ASCII |
| Borders  | Rounded · Plain · Thick · Double |
| Colours  | Auto · Truecolor · 256 · 16 |
| TTY mode | Auto · On · Off. ASCII only and the terminal's own 16 colours, for the bare Linux console |

Colour reduction and ASCII transliteration run as one pass over the finished frame. Everything, including the output
of remote programs, is guaranteed to fit what your terminal can draw.

## Documentation

| | |
|---|---|
| [Key bindings](docs/keybindings.md) | Every shortcut on every screen |
| [Configuration](docs/configuration.md) | `settings.toml`, `hosts.toml`, file locations, environment variables |
| [Security model](docs/security.md) | Vault format, key derivation, new-window hand-off, host key policy |
| [Architecture](docs/architecture.md) | How the pieces fit together, for contributors |

## Built with

[ratatui](https://ratatui.rs) · [russh](https://github.com/Eugeny/russh) · [russh-sftp](https://github.com/AspectUnk/russh-sftp) ·
[vt100](https://crates.io/crates/vt100) · [RustCrypto](https://github.com/RustCrypto) · [tokio](https://tokio.rs)
