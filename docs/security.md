# Security model

## What is protected

| Asset | Protection |
|---|---|
| Stored passwords and key passphrases | Encrypted at rest under the master password |
| Master password | Never stored. Only an Argon2id-derived key exists, in memory, while the vault is unlocked |
| Server identity | Host key verification against `known_hosts`, trust-on-first-use with a fingerprint prompt |
| Config files | `0600` files in a `0700` directory, written atomically |
| Credentials passed to <kbd>w</kbd> windows | One-time, short-lived token over a private unix socket |

Out of scope: an attacker who already runs code as your user or as root while the vault is unlocked can read process
memory, and nothing in user space can stop that. Lock the vault (<kbd>L</kbd>) or keep the auto-lock short on shared
machines.

## Vault

```
master password
      │  Argon2id  (m = 64 MiB, t = 3, p = 4, 16-byte random salt)
      ▼
256-bit key ──► XChaCha20-Poly1305 ( random 192-bit nonce, AAD = host id )
                       │
                       ▼
               "v1:<nonce hex>:<ciphertext+tag hex>"   → hosts.toml
```

- `vault.toml` holds the KDF parameters, the salt and a *check value*: a known string sealed with the key. Unlocking
  derives the key and tries to open the check value. A wrong password fails AEAD authentication, so the password
  itself is never compared or stored.
- Each secret gets a fresh random nonce. 192-bit nonces make random generation safe for any realistic number of secrets.
- **The associated data is the host id** (`<id>` for passwords, `<id>:key` for key passphrases). Copying a ciphertext
  onto another host entry, or swapping a password with a passphrase, fails authentication instead of silently
  sending the wrong secret to the wrong server.
- Duplicating a host re-seals its secrets under the new id. With a locked vault the copy is made without secrets.
- **Changing the master password** decrypts every secret first, derives a new key with a new salt, re-seals
  everything and saves. If any step fails, the old vault file and key are restored.
- Decrypted values are held in `Zeroizing` buffers and wiped when dropped. Password input fields are zeroized when cleared.
- The key is dropped on <kbd>L</kbd>, on the *Lock vault* setting and after `auto_lock_mins` without input.

## New-window hand-off

Opening a session with <kbd>w</kbd> starts a separate `rsh connect <id>` process in another terminal emulator. That
process needs the decrypted credentials, but asking for the master password again would be annoying, and putting
secrets in `argv` or files would leak them. Instead:

1. The main instance listens on `$XDG_RUNTIME_DIR/rsh/handoff-<pid>.sock` (directory `0700`, socket `0600`)
   and only serves peers whose **uid matches** its own (`SO_PEERCRED`).
2. For each window it issues a random 192-bit token, valid **once** and for **60 seconds**.
3. The socket path and token go to the child through environment variables, never through `argv`, so they don't
   show up in `ps`. The child removes both variables from its own environment before spawning anything.
4. The child sends the token and receives the credentials. The ticket is deleted from memory on first use.

On **Windows** the endpoint is a named pipe with an unguessable name (`\\.\pipe\rsh-handoff-<pid>-<64 random bits>`),
created with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so nobody can squat the name beforehand, and with remote clients rejected.
The default pipe DACL grants write access only to your account, SYSTEM and administrators. A process that cannot
write cannot submit a token, and a token is useless without the 60-second, single-use ticket behind it.

## Host keys

On every connection the server key is checked against RSH's own `known_hosts`, then against `~/.ssh/known_hosts`.

| Situation | `Ask` (default) | `AcceptNew` | `Strict` |
|---|---|---|---|
| Known and matching | connect | connect | connect |
| Unknown | prompt with SHA256 fingerprint | trust and save | refuse |
| **Changed** | red warning prompt | red warning prompt | refuse |

Trusted keys are written only to RSH's file, so `~/.ssh/known_hosts` is never modified. When you accept a changed key,
the stale entry in RSH's file is replaced. A stale entry in `~/.ssh/known_hosts` stays until you fix it yourself.

## Monitoring and sshd penalties

TCP monitoring opens a connection to the SSH port and closes it without authenticating. OpenSSH 9.8+ counts such
connections against the source address (`PerSourcePenalties noauth`, 1 s each by default). With the default 30 s
interval and one host per server, penalties expire long before the next probe. But if you monitor **many entries that
point at the same sshd** (different ports or aliases), or use very short intervals, penalties can add up and sshd will
start refusing your real logins for a while.

To avoid this, RSH stops TCP-probing hosts that have an open session and takes their status from the session's own
keepalive RTT instead. For dense setups, switch *Settings → Monitoring → Method* to **ICMP**, which never touches sshd.

## Reporting

Found a weakness? Please open a private security advisory on the repository instead of a public issue.
