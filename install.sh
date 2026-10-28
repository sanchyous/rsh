#!/bin/sh
# rsh installer: curl -fsSL https://github.com/sanchyous/rsh/releases/latest/download/install.sh | sh
#
# Environment:
#   RSH_VERSION         release tag to install (default: latest), e.g. v0.1.0
#   RSH_INSTALL_METHOD  auto | deb | rpm | pacman | apk | tarball (default: auto)
#   RSH_PREFIX          install prefix for the tarball method (default: /usr/local, or ~/.local without root)
#
# Flags:
#   --uninstall         remove rsh installed by any method

set -eu

REPO="sanchyous/rsh"
VERSION="${RSH_VERSION:-latest}"
METHOD="${RSH_INSTALL_METHOD:-auto}"
PREFIX="${RSH_PREFIX:-}"

if [ -t 2 ]; then
    B="$(printf '\033[1m')" C="$(printf '\033[36m')" G="$(printf '\033[32m')" R="$(printf '\033[31m')" Y="$(printf '\033[33m')" N="$(printf '\033[0m')"
else
    B="" C="" G="" R="" Y="" N=""
fi

say() { printf '%s\n' "${C}::${N} $*" >&2; }
ok() { printf '%s\n' "${G}✔${N} $*" >&2; }
warn() { printf '%s\n' "${Y}!${N} $*" >&2; }
die() {
    printf '%s\n' "${R}✖${N} $*" >&2
    exit 1
}
has() { command -v "$1" >/dev/null 2>&1; }

if [ "$(id -u)" -eq 0 ]; then
    SUDO=""
elif has sudo; then
    SUDO="sudo"
elif has doas; then
    SUDO="doas"
else
    SUDO="none"
fi

as_root() {
    if [ "$SUDO" = none ]; then
        die "root privileges are required for this method (no sudo/doas found); try RSH_INSTALL_METHOD=tarball"
    fi
    $SUDO "$@"
}

detect_method() {
    if has apt-get || has dpkg; then echo deb
    elif has dnf || has yum || has zypper || has rpm; then echo rpm
    elif has pacman; then echo pacman
    elif has apk; then echo apk
    else echo tarball
    fi
}

uninstall() {
    say "removing rsh"
    if has dpkg && dpkg -s rsh >/dev/null 2>&1; then as_root dpkg -r rsh
    elif has rpm && rpm -q rsh >/dev/null 2>&1; then
        if has dnf; then as_root dnf remove -y rsh
        elif has zypper; then as_root zypper -n remove rsh
        else as_root rpm -e rsh
        fi
    elif has pacman && pacman -Q rsh >/dev/null 2>&1; then as_root pacman -R --noconfirm rsh
    elif has apk && apk info -e rsh >/dev/null 2>&1; then as_root apk del rsh
    fi
    for p in /usr/local "$HOME/.local"; do
        if [ -f "$p/bin/rsh" ]; then
            if [ -w "$p/bin" ]; then rm -f "$p/bin/rsh"; else as_root rm -f "$p/bin/rsh"; fi
            rm -f "$p/share/man/man1/rsh.1.gz" "$p/share/bash-completion/completions/rsh" \
                "$p/share/zsh/site-functions/_rsh" "$p/share/fish/vendor_completions.d/rsh.fish" 2>/dev/null || true
        fi
    done
    ok "rsh removed (your config in ~/.config/rsh was kept)"
    exit 0
}

[ "${1:-}" = "--uninstall" ] && uninstall

case "$(uname -s)" in
    Linux) ;;
    *) die "unsupported OS: $(uname -s) (only Linux builds are published)" ;;
esac

case "$(uname -m)" in
    x86_64 | amd64) ARCH=x86_64 DEB_ARCH=amd64 ;;
    aarch64 | arm64) ARCH=aarch64 DEB_ARCH=arm64 ;;
    *) die "unsupported architecture: $(uname -m) (x86_64 and aarch64 are available)" ;;
esac

case "$VERSION" in
    latest | "") BASE="https://github.com/$REPO/releases/latest/download" ;;
    v*) BASE="https://github.com/$REPO/releases/download/$VERSION" ;;
    *) BASE="https://github.com/$REPO/releases/download/v$VERSION" ;;
esac

fetch() {
    if has curl; then curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
    elif has wget; then wget -q -O "$2" "$1"
    else die "curl or wget is required"
    fi
}

[ "$METHOD" = auto ] && METHOD="$(detect_method)"

case "$METHOD" in
    deb) FILE="rsh_${DEB_ARCH}.deb" ;;
    rpm) FILE="rsh-${ARCH}.rpm" ;;
    pacman) FILE="rsh-${ARCH}.pkg.tar.zst" ;;
    apk) FILE="rsh-${ARCH}.apk" ;;
    tarball) FILE="rsh-${ARCH}-linux-musl.tar.gz" ;;
    *) die "unknown RSH_INSTALL_METHOD: $METHOD" ;;
esac

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM

printf '\n%s\n\n' "  ${B}${C}RSH${N} ${B}installer${N}  ·  $ARCH  ·  $METHOD  ·  ${VERSION:-latest}" >&2

say "downloading $FILE"
fetch "$BASE/$FILE" "$TMP/$FILE" || die "download failed: $BASE/$FILE"

if fetch "$BASE/SHA256SUMS" "$TMP/SHA256SUMS" 2>/dev/null; then
    expected="$(grep " $FILE\$" "$TMP/SHA256SUMS" | cut -d' ' -f1)"
    if [ -n "$expected" ]; then
        if has sha256sum; then actual="$(sha256sum "$TMP/$FILE" | cut -d' ' -f1)"
        elif has shasum; then actual="$(shasum -a 256 "$TMP/$FILE" | cut -d' ' -f1)"
        else actual=""
        fi
        if [ -n "$actual" ]; then
            [ "$actual" = "$expected" ] || die "checksum mismatch for $FILE"
            ok "checksum verified"
        fi
    fi
else
    warn "SHA256SUMS not found, skipping verification"
fi

install_tarball() {
    if [ -z "$PREFIX" ]; then
        if [ "$SUDO" = none ]; then PREFIX="$HOME/.local"; else PREFIX="/usr/local"; fi
    fi
    run=""
    mkdir -p "$PREFIX" 2>/dev/null || true
    [ -w "$PREFIX" ] || run="as_root"
    tar -xzf "$TMP/$FILE" -C "$TMP"
    src="$TMP/rsh-${ARCH}-linux-musl"
    $run mkdir -p "$PREFIX/bin" "$PREFIX/share/man/man1" "$PREFIX/share/bash-completion/completions" \
        "$PREFIX/share/zsh/site-functions" "$PREFIX/share/fish/vendor_completions.d"
    $run install -m 0755 "$src/rsh" "$PREFIX/bin/rsh"
    $run install -m 0644 "$src/rsh.1.gz" "$PREFIX/share/man/man1/rsh.1.gz"
    $run install -m 0644 "$src/completions/rsh.bash" "$PREFIX/share/bash-completion/completions/rsh"
    $run install -m 0644 "$src/completions/_rsh" "$PREFIX/share/zsh/site-functions/_rsh"
    $run install -m 0644 "$src/completions/rsh.fish" "$PREFIX/share/fish/vendor_completions.d/rsh.fish"
    case ":$PATH:" in
        *":$PREFIX/bin:"*) ;;
        *) warn "$PREFIX/bin is not in your PATH — add it: export PATH=\"$PREFIX/bin:\$PATH\"" ;;
    esac
}

say "installing"
case "$METHOD" in
    deb)
        if has apt-get; then as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y "$TMP/$FILE"
        else as_root dpkg -i "$TMP/$FILE"
        fi
        ;;
    rpm)
        if has dnf; then as_root dnf install -y "$TMP/$FILE"
        elif has zypper; then as_root zypper -n install --allow-unsigned-rpm "$TMP/$FILE"
        elif has yum; then as_root yum install -y "$TMP/$FILE"
        else as_root rpm -Uvh --replacepkgs "$TMP/$FILE"
        fi
        ;;
    pacman) as_root pacman -U --noconfirm "$TMP/$FILE" ;;
    apk) as_root apk add --allow-untrusted "$TMP/$FILE" ;;
    tarball) install_tarball ;;
esac

if has rsh; then
    ok "installed $(rsh --version)"
else
    ok "installed"
fi
printf '\n  Run %s to start. Docs: https://github.com/%s\n\n' "${B}rsh${N}" "$REPO" >&2
