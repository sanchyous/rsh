use std::fs::Metadata;
use std::path::Path;
use std::process::Command;

pub fn file_mode(meta: &Metadata) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode() & 0o7777)
    }
    #[cfg(not(unix))]
    {
        let base = if meta.is_dir() { 0o755 } else { 0o644 };
        Some(if meta.permissions().readonly() { base & 0o555 } else { base })
    }
}

pub fn mtime(meta: &Metadata) -> Option<i64> {
    meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

pub fn set_mode(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode & 0o7777));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}

pub fn private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    set_mode(path, 0o700);
    Ok(())
}

pub fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
}

pub fn default_editor() -> &'static str {
    if cfg!(windows) { "notepad" } else { "nano" }
}

pub fn default_pager() -> &'static str {
    if cfg!(windows) { "more" } else { "less" }
}

pub fn open_with(program: &str, file: &Path) -> Command {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(format!("{program} \"{}\"", file.display()));
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(format!("{program} \"$1\"")).arg("rsh").arg(file);
        c
    }
}

pub fn detach(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

pub fn ping_args(address: &str, timeout: std::time::Duration) -> Vec<String> {
    if cfg!(windows) {
        vec!["-n".into(), "1".into(), "-w".into(), timeout.as_millis().max(1).to_string(), address.into()]
    } else {
        vec!["-c".into(), "1".into(), "-n".into(), "-W".into(), timeout.as_secs().max(1).to_string(), address.into()]
    }
}

pub fn parse_ping_ms(out: &str) -> Option<f64> {
    for (i, _) in out.match_indices(['=', '<']) {
        let rest = &out[i + 1..];
        let num: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',').collect();
        if num.is_empty() {
            continue;
        }
        let tail = rest[num.len()..].trim_start();
        if tail.starts_with("ms") || tail.starts_with("мс") {
            let v: f64 = num.replace(',', ".").parse().ok()?;
            return Some(if out[i..].starts_with('<') { v / 2.0 } else { v });
        }
    }
    None
}

pub fn exe_in_path(bin: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&paths)
        .any(|d| d.join(bin).is_file() || (cfg!(windows) && d.join(format!("{bin}.exe")).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ping_output() {
        assert_eq!(parse_ping_ms("64 bytes from 1.1.1.1: icmp_seq=1 ttl=57 time=12.3 ms"), Some(12.3));
        assert_eq!(parse_ping_ms("Reply from 1.1.1.1: bytes=32 time=14ms TTL=57"), Some(14.0));
        assert_eq!(parse_ping_ms("Reply from 127.0.0.1: bytes=32 time<1ms TTL=128"), Some(0.5));
        assert_eq!(parse_ping_ms("Ответ от 1.1.1.1: число байт=32 время=9мс TTL=57"), Some(9.0));
        assert_eq!(parse_ping_ms("Request timed out."), None);
    }
}
