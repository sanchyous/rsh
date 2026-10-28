use std::io::Write;
use std::path::Path;
use std::time::Duration;

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v >= 100.0 {
        format!("{v:.0} {}", UNITS[u])
    } else if v >= 10.0 {
        format!("{v:.1} {}", UNITS[u])
    } else {
        format!("{v:.2} {}", UNITS[u])
    }
}

pub fn human_rate(bps: f64) -> String {
    format!("{}/s", human_bytes(bps.max(0.0) as u64))
}

pub fn human_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 86400 {
        format!("{}d {:02}:{:02}:{:02}", s / 86400, (s / 3600) % 24, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    }
}

pub fn ago(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..=59 => format!("{s}s"),
        60..=3599 => format!("{}m", s / 60),
        3600..=86399 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

pub fn fmt_rtt(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 10.0 { format!("{ms:.1}ms") } else { format!("{ms:.0}ms") }
}

pub fn fmt_mtime(ts: Option<i64>) -> String {
    use chrono::{Local, TimeZone};
    match ts.and_then(|t| Local.timestamp_opt(t, 0).single()) {
        Some(dt) => dt.format("%Y-%m-%d %H:%M").to_string(),
        None => "—".into(),
    }
}

pub fn fmt_perms(mode: Option<u32>, is_dir: bool, is_link: bool) -> String {
    let Some(m) = mode else { return "—".into() };
    let mut s = String::with_capacity(10);
    s.push(if is_link {
        'l'
    } else if is_dir {
        'd'
    } else {
        '-'
    });
    for shift in [6, 3, 0] {
        let b = (m >> shift) & 7;
        s.push(if b & 4 != 0 { 'r' } else { '-' });
        s.push(if b & 2 != 0 { 'w' } else { '-' });
        s.push(if b & 1 != 0 { 'x' } else { '-' });
    }
    s
}

pub fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        crate::platform::private_dir(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = crate::platform::create_private(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn join_remote(dir: &str, name: &str) -> String {
    if dir.ends_with('/') { format!("{dir}{name}") } else { format!("{dir}/{name}") }
}

pub fn parent_remote(path: &str) -> String {
    let p = path.trim_end_matches('/');
    match p.rfind('/') {
        Some(0) | None => "/".into(),
        Some(i) => p[..i].into(),
    }
}

pub fn tilde(path: &str, home: Option<&str>) -> String {
    match home.filter(|h| h.len() > 1) {
        Some(h) if path == h => "~".into(),
        Some(h) if path.starts_with(h) && path[h.len()..].starts_with(['/', '\\']) => format!("~{}", &path[h.len()..]),
        _ => path.to_string(),
    }
}

pub fn shorten_path(path: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if path.width() <= width {
        return path.to_string();
    }
    let sep = if path.contains('/') { '/' } else { '\\' };
    let mut out = String::new();
    for part in path.split(sep).rev().filter(|p| !p.is_empty()) {
        let next = format!("{sep}{part}{out}");
        if next.width() + 1 > width {
            break;
        }
        out = next;
    }
    if out.is_empty() { ellipsize(path.rsplit(sep).next().unwrap_or(path), width) } else { format!("…{out}") }
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..12].to_string()
}

pub fn ellipsize(s: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let total: usize = s.chars().map(|c| c.width().unwrap_or(0)).sum();
    if total <= width {
        return s.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_roundtrip() {
        let v = vec![0u8, 1, 0xab, 0xff];
        assert_eq!(hex_decode(&hex_encode(&v)).unwrap(), v);
    }
    #[test]
    fn path_display() {
        assert_eq!(tilde("/home/u/Проекты", Some("/home/u")), "~/Проекты");
        assert_eq!(tilde("/home/user2", Some("/home/u")), "/home/user2");
        assert_eq!(shorten_path("/very/long/Проекты/rsh", 14), "…/Проекты/rsh");
        assert_eq!(shorten_path(r"C:\Users\u\Documents\work", 16), r"…\Documents\work");
        assert_eq!(tilde(r"C:\Users\u\src", Some(r"C:\Users\u")), r"~\src");
    }
    #[test]
    fn remote_paths() {
        assert_eq!(parent_remote("/a/b"), "/a");
        assert_eq!(parent_remote("/a"), "/");
        assert_eq!(join_remote("/", "x"), "/x");
    }
}
