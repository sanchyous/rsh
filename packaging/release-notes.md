## Install

```bash
curl -fsSL https://github.com/sanchyous/rsh/releases/latest/download/install.sh | sh
```

The script detects your distro and architecture, verifies the SHA-256 checksum and installs the matching package
(apt, dnf, yum, zypper, pacman, apk), or a static binary everywhere else.

| Distro | Package |
|---|---|
| Debian, Ubuntu, Mint, Pop!_OS | `rsh_amd64.deb` · `rsh_arm64.deb` |
| Fedora, RHEL, Rocky, Alma, openSUSE | `rsh-x86_64.rpm` · `rsh-aarch64.rpm` |
| Arch, Manjaro, EndeavourOS, CachyOS | `rsh-x86_64.pkg.tar.zst` · `rsh-aarch64.pkg.tar.zst` |
| Alpine, postmarketOS | `rsh-x86_64.apk` · `rsh-aarch64.apk` |
| Any Linux (static musl) | `rsh-x86_64-linux-musl.tar.gz` · `rsh-aarch64-linux-musl.tar.gz` |

Every package ships the binary, the man page, bash/zsh/fish completions and the docs.
