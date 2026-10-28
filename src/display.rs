use crate::config::{BorderStyle, ColorMode, Settings, TtyMode};
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::symbols::border;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depth {
    True,
    C256,
    C16,
}

static TTY: AtomicBool = AtomicBool::new(false);
static DEPTH: AtomicU8 = AtomicU8::new(0);
static BORDER: AtomicU8 = AtomicU8::new(0);

pub fn tty() -> bool {
    TTY.load(Ordering::Relaxed)
}

pub fn depth() -> Depth {
    match DEPTH.load(Ordering::Relaxed) {
        1 => Depth::C256,
        2 => Depth::C16,
        _ => Depth::True,
    }
}

fn utf8_locale() -> bool {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
        .is_none_or(|s| s.to_uppercase().replace('-', "").contains("UTF8"))
}

pub fn detect_tty() -> bool {
    std::env::var("TERM").is_ok_and(|t| t == "linux" || t == "vt100" || t == "vt220") || !utf8_locale()
}

pub fn detect_depth() -> Depth {
    let ct = std::env::var("COLORTERM").unwrap_or_default().to_lowercase();
    let term = std::env::var("TERM").unwrap_or_default();
    if cfg!(windows)
        || std::env::var_os("WT_SESSION").is_some()
        || ct.contains("truecolor")
        || ct.contains("24bit")
        || term.contains("kitty")
        || term.contains("direct")
    {
        Depth::True
    } else if term.contains("256") {
        Depth::C256
    } else if term == "linux" || term.starts_with("vt") || term.is_empty() {
        Depth::C16
    } else {
        Depth::C256
    }
}

pub fn apply(s: &Settings) {
    let tty = match s.tty_mode {
        TtyMode::Auto => detect_tty(),
        TtyMode::On => true,
        TtyMode::Off => false,
    };
    let depth = if tty {
        Depth::C16
    } else {
        match s.color_mode {
            ColorMode::Auto => detect_depth(),
            ColorMode::TrueColor => Depth::True,
            ColorMode::C256 => Depth::C256,
            ColorMode::C16 => Depth::C16,
        }
    };
    TTY.store(tty, Ordering::Relaxed);
    DEPTH.store(depth as u8, Ordering::Relaxed);
    BORDER.store(s.border as u8, Ordering::Relaxed);
    crate::theme::set_theme(if tty { crate::theme::ANSI_NAME } else { &s.theme });
}

pub fn border_set() -> border::Set<'static> {
    if tty() {
        return ASCII_BORDER;
    }
    match BORDER.load(Ordering::Relaxed) {
        x if x == BorderStyle::Plain as u8 => border::PLAIN,
        x if x == BorderStyle::Thick as u8 => border::THICK,
        x if x == BorderStyle::Double as u8 => border::DOUBLE,
        _ => border::ROUNDED,
    }
}

pub fn modal_border_set() -> border::Set<'static> {
    if tty() {
        return ASCII_BORDER;
    }
    match BORDER.load(Ordering::Relaxed) {
        x if x == BorderStyle::Double as u8 => border::DOUBLE,
        x if x == BorderStyle::Plain as u8 => border::PLAIN,
        _ => border::THICK,
    }
}

const ASCII_BORDER: border::Set<'static> = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

pub fn enter() -> &'static str {
    if tty() { "Enter" } else { "⏎" }
}

const ANSI16: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (205, 0, 0),
    (0, 205, 0),
    (205, 205, 0),
    (0, 0, 238),
    (205, 0, 205),
    (0, 205, 205),
    (229, 229, 229),
    (127, 127, 127),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (92, 92, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

const NAMED: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

fn idx_to_rgb(i: u8) -> (u8, u8, u8) {
    match i {
        0..=15 => ANSI16[i as usize],
        16..=231 => {
            let i = i - 16;
            let lv = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (lv(i / 36), lv((i / 6) % 6), lv(i % 6))
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            (g, g, g)
        }
    }
}

fn dist(a: (u8, u8, u8), b: (u8, u8, u8)) -> u32 {
    let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2) as u32;
    2 * d(a.0, b.0) + 4 * d(a.1, b.1) + 3 * d(a.2, b.2)
}

fn to_256(r: u8, g: u8, b: u8) -> u8 {
    let q = |v: u8| {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            (v - 35) / 40
        }
    };
    let cube = 16 + 36 * q(r) + 6 * q(g) + q(b);
    let avg = (r as u16 + g as u16 + b as u16) / 3;
    let gray = if avg > 238 { 255 } else { 232 + ((avg.saturating_sub(3)) / 10).min(23) as u8 };
    if dist((r, g, b), idx_to_rgb(cube)) <= dist((r, g, b), idx_to_rgb(gray)) { cube } else { gray }
}

fn to_16(rgb: (u8, u8, u8)) -> Color {
    let i = (0..16).min_by_key(|&i| dist(rgb, ANSI16[i])).unwrap_or(7);
    NAMED[i]
}

fn reduce(c: Color, d: Depth) -> Color {
    match (c, d) {
        (_, Depth::True) => c,
        (Color::Rgb(r, g, b), Depth::C256) => Color::Indexed(to_256(r, g, b)),
        (Color::Rgb(r, g, b), Depth::C16) => to_16((r, g, b)),
        (Color::Indexed(i), Depth::C16) => {
            if i < 16 {
                NAMED[i as usize]
            } else {
                to_16(idx_to_rgb(i))
            }
        }
        _ => c,
    }
}

fn ascii_for(c: char) -> Option<&'static str> {
    Some(match c {
        '─' | '━' | '═' | '╌' | '┄' | '—' | '–' | '‒' => "-",
        '│' | '┃' | '║' | '▏' | '▕' | '╎' => "|",
        '╭' | '╮' | '╰' | '╯' | '┌' | '┐' | '└' | '┘' | '┏' | '┓' | '┗' | '┛' | '├' | '┤' | '┬' | '┴' | '┼' | '╔'
        | '╗' | '╚' | '╝' | '┣' | '┫' | '╠' | '╣' => "+",
        '▌' | '▎' | '▍' => "[",
        '▐' => "]",
        '█' | '▉' | '▊' | '▋' => "#",
        '░' | '▒' | '▓' => ".",
        '▁' => ".",
        '▂' => ":",
        '▃' => "-",
        '▄' => "=",
        '▅' => "+",
        '▆' => "*",
        '▇' => "#",
        '●' | '•' | '◆' | '✦' | '⬢' | '✱' | '*' => "*",
        '○' | '◌' | '◍' | '◯' => "o",
        '◉' | '◎' | '◷' | '⟳' | '↪' => "@",
        '✖' | '✗' | '⏻' | '✂' => "x",
        '✔' | '✓' => "v",
        '⚠' => "!",
        'ℹ' => "i",
        '▶' | '▸' | '❯' | '›' | '→' | '⇢' | '»' => ">",
        '◀' | '◂' | '‹' | '←' | '«' => "<",
        '▾' | '▼' | '↓' | '⇣' => "v",
        '▴' | '▲' | '↑' | '⇡' | '↰' => "^",
        '⇅' | '⇄' => "~",
        '▰' => "/",
        '·' | '…' => ".",
        '☰' => "=",
        '⧉' => "#",
        '⌂' => "L",
        '☁' => "R",
        '⏱' => "t",
        '⏎' => "<",
        'Σ' => "S",
        '⚿' | '🔑' | '🔐' | '🔒' | '🔓' => "k",
        '⠋' | '⠼' => "|",
        '⠙' | '⠴' => "/",
        '⠹' | '⠦' => "-",
        '⠸' | '⠧' | '⠇' | '⠏' => "\\",
        '×' => "x",
        '⚙' => "*",
        '✎' => "e",
        _ => return None,
    })
}

fn ascii_symbol(sym: &str) -> Option<String> {
    if sym.is_ascii() {
        return None;
    }
    let mut out = String::new();
    for c in sym.chars() {
        if c.is_ascii() || c.is_alphabetic() {
            out.push(c);
        } else if let Some(a) = ascii_for(c) {
            out.push_str(a);
        } else if !c.is_control() && unicode_width::UnicodeWidthChar::width(c).unwrap_or(0) > 0 {
            out.push('?');
        }
    }
    Some(out)
}

pub fn finalize(buf: &mut Buffer) {
    let d = depth();
    let tty = tty();
    if d == Depth::True && !tty {
        return;
    }
    let area = buf.area;
    let mut skip_next = false;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let Some(cell) = buf.cell_mut((x, y)) else { continue };
            if d != Depth::True {
                cell.fg = reduce(cell.fg, d);
                cell.bg = reduce(cell.bg, d);
            }
            if tty {
                if skip_next {
                    skip_next = false;
                    cell.set_symbol(" ");
                    continue;
                }
                let sym = cell.symbol().to_string();
                if let Some(a) = ascii_symbol(&sym) {
                    let wide = unicode_width::UnicodeWidthStr::width(sym.as_str()) > 1;
                    let a = if a.is_empty() { " ".to_string() } else { a.chars().take(1).collect() };
                    cell.set_symbol(&a);
                    skip_next = wide;
                }
            }
        }
        skip_next = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reduction() {
        assert!(matches!(to_256(255, 255, 255), 231 | 255));
        assert_eq!(to_256(0, 0, 0), 16);
        assert_eq!(to_16((250, 10, 10)), Color::LightRed);
        assert_eq!(to_16((5, 5, 5)), Color::Black);
        assert_eq!(ascii_symbol("╭").unwrap(), "+");
        assert_eq!(ascii_symbol("Ж").unwrap(), "Ж");
        assert!(ascii_symbol("a").is_none());
    }
}
