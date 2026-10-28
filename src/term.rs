use crate::ssh::PtyCmd;
use crate::theme::th;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use std::num::NonZeroU16;
use tokio::sync::mpsc::UnboundedSender;

pub struct TermPane {
    pub parser: vt100::Parser,
    pub tx: Option<UnboundedSender<PtyCmd>>,
    pub size: (u16, u16),
    pub scroll: usize,
    pub closed: Option<Option<u32>>,
}

fn nz(v: u16) -> NonZeroU16 {
    NonZeroU16::new(v.max(1)).unwrap()
}

impl TermPane {
    pub fn new(rows: u16, cols: u16, scrollback: usize) -> Self {
        Self {
            parser: vt100::Parser::new(nz(rows), nz(cols), scrollback),
            tx: None,
            size: (rows, cols),
            scroll: 0,
            closed: None,
        }
    }

    pub fn feed(&mut self, data: &[u8]) {
        self.parser.process(data);
        if self.scroll > 0 {
            self.parser.screen_mut().set_scrollback(self.scroll);
            self.scroll = self.parser.screen().scrollback();
        }
    }

    pub fn send(&self, bytes: Vec<u8>) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(PtyCmd::Data(bytes));
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == self.size || rows == 0 || cols == 0 {
            return;
        }
        self.size = (rows, cols);
        self.parser.set_size(nz(rows), nz(cols));
        if let Some(tx) = &self.tx {
            let _ = tx.send(PtyCmd::Resize(cols, rows));
        }
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let want = (self.scroll as isize + delta).max(0) as usize;
        self.parser.screen_mut().set_scrollback(want);
        self.scroll = self.parser.screen().scrollback();
    }

    pub fn reset_scroll(&mut self) {
        if self.scroll != 0 {
            self.scroll = 0;
            self.parser.screen_mut().set_scrollback(0);
        }
    }

    pub fn paste(&mut self, text: &str) {
        self.reset_scroll();
        let mut v = Vec::new();
        let bracketed = self.parser.screen().bracketed_paste();
        if bracketed {
            v.extend_from_slice(b"\x1b[200~");
        }
        v.extend_from_slice(text.replace("\r\n", "\r").replace('\n', "\r").as_bytes());
        if bracketed {
            v.extend_from_slice(b"\x1b[201~");
        }
        self.send(v);
    }

    pub fn key(&mut self, k: KeyEvent) {
        if let Some(b) = encode_key(k, self.parser.screen().application_cursor()) {
            self.reset_scroll();
            self.send(b);
        }
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer, focused: bool) -> Option<Position> {
        self.resize(area.height, area.width);
        let screen = self.parser.screen();
        let pal = th();
        for row in 0..area.height {
            for col in 0..area.width {
                let Some(dst) = buf.cell_mut((area.x + col, area.y + row)) else { continue };
                match screen.cell(row, col) {
                    Some(c) => {
                        if c.is_wide_continuation() {
                            continue;
                        }
                        let mut fg = conv(c.fgcolor(), pal.term_fg);
                        let mut bg = conv(c.bgcolor(), pal.term_bg);
                        if c.inverse() {
                            std::mem::swap(&mut fg, &mut bg);
                        }
                        let mut m = Modifier::empty();
                        if c.bold() {
                            m |= Modifier::BOLD;
                        }
                        if c.dim() {
                            m |= Modifier::DIM;
                        }
                        if c.italic() {
                            m |= Modifier::ITALIC;
                        }
                        if c.underline() {
                            m |= Modifier::UNDERLINED;
                        }
                        let s = c.contents();
                        dst.set_symbol(if s.is_empty() { " " } else { s });
                        dst.set_style(Style::default().fg(fg).bg(bg).add_modifier(m));
                    }
                    None => {
                        dst.set_symbol(" ");
                        dst.set_style(Style::default().bg(pal.term_bg));
                    }
                }
            }
        }
        if self.scroll > 0 {
            let label = format!(" ↑ {} ", self.scroll);
            let x = area.right().saturating_sub(label.chars().count() as u16 + 1);
            buf.set_string(x, area.y, label, Style::default().fg(pal.bg).bg(pal.warn).add_modifier(Modifier::BOLD));
        }
        if focused && self.scroll == 0 && !screen.hide_cursor() {
            let (r, c) = screen.cursor_position();
            if r < area.height && c < area.width {
                return Some(Position::new(area.x + c, area.y + r));
            }
        }
        None
    }
}

fn conv(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

pub fn encode_key(k: KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    let alt = k.modifiers.contains(KeyModifiers::ALT);
    let shift = k.modifiers.contains(KeyModifiers::SHIFT);
    let modp = 1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8;
    let csi_mod = |final_: char| -> Vec<u8> {
        if modp > 1 {
            format!("\x1b[1;{modp}{final_}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{final_}").into_bytes()
        } else {
            format!("\x1b[{final_}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modp > 1 { format!("\x1b[{n};{modp}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() }
    };
    let mut out: Vec<u8> = match k.code {
        KeyCode::Char(c) if ctrl => {
            let b = match c.to_ascii_lowercase() {
                c @ 'a'..='z' => c as u8 - b'a' + 1,
                '@' | ' ' | '2' => 0,
                '[' | '3' => 27,
                '\\' | '4' => 28,
                ']' | '5' => 29,
                '^' | '6' => 30,
                '_' | '/' | '7' | '-' => 31,
                '?' | '8' => 127,
                _ => return None,
            };
            vec![b]
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![if ctrl { 0x08 } else { 0x7f }],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => csi_mod('A'),
        KeyCode::Down => csi_mod('B'),
        KeyCode::Right => csi_mod('C'),
        KeyCode::Left => csi_mod('D'),
        KeyCode::Home => csi_mod('H'),
        KeyCode::End => csi_mod('F'),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::F(n @ 1..=4) => {
            let f = b"PQRS"[(n - 1) as usize];
            if modp > 1 { format!("\x1b[1;{modp}{}", f as char).into_bytes() } else { vec![0x1b, b'O', f] }
        }
        KeyCode::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize]),
        _ => return None,
    };
    let is_plain =
        matches!(k.code, KeyCode::Char(_) | KeyCode::Enter | KeyCode::Backspace | KeyCode::Tab | KeyCode::Esc);
    if alt && is_plain {
        out.insert(0, 0x1b);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn k(code: KeyCode, m: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, m)
    }
    #[test]
    fn keys() {
        assert_eq!(encode_key(k(KeyCode::Char('c'), KeyModifiers::CONTROL), false).unwrap(), vec![3]);
        assert_eq!(encode_key(k(KeyCode::Up, KeyModifiers::NONE), false).unwrap(), b"\x1b[A");
        assert_eq!(encode_key(k(KeyCode::Up, KeyModifiers::NONE), true).unwrap(), b"\x1bOA");
        assert_eq!(encode_key(k(KeyCode::Left, KeyModifiers::CONTROL), false).unwrap(), b"\x1b[1;5D");
        assert_eq!(encode_key(k(KeyCode::Char('x'), KeyModifiers::ALT), false).unwrap(), b"\x1bx");
        assert_eq!(encode_key(k(KeyCode::F(5), KeyModifiers::NONE), false).unwrap(), b"\x1b[15~");
    }
}
