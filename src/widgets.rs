use crate::theme::{gradient, th};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Padding};

#[derive(Clone, Default, Debug)]
pub struct TextInput {
    pub value: String,
    pub cursor: usize,
    pub masked: bool,
}

impl TextInput {
    pub fn new(v: impl Into<String>) -> Self {
        let value: String = v.into();
        let cursor = value.chars().count();
        Self { value, cursor, masked: false }
    }
    pub fn masked() -> Self {
        Self { masked: true, ..Default::default() }
    }
    fn byte_idx(&self, ci: usize) -> usize {
        self.value.char_indices().nth(ci).map(|(i, _)| i).unwrap_or(self.value.len())
    }
    pub fn clear(&mut self) {
        use zeroize::Zeroize;
        self.value.zeroize();
        self.cursor = 0;
    }
    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars().filter(|c| !c.is_control()) {
            let i = self.byte_idx(self.cursor);
            self.value.insert(i, c);
            self.cursor += 1;
        }
    }
    pub fn handle(&mut self, k: KeyEvent) -> bool {
        let n = self.value.chars().count();
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Char('u') if ctrl => {
                let i = self.byte_idx(self.cursor);
                self.value.replace_range(..i, "");
                self.cursor = 0;
            }
            KeyCode::Char('w') if ctrl => {
                let chars: Vec<char> = self.value.chars().collect();
                let mut s = self.cursor;
                while s > 0 && chars[s - 1] == ' ' {
                    s -= 1;
                }
                while s > 0 && chars[s - 1] != ' ' {
                    s -= 1;
                }
                let (a, b) = (self.byte_idx(s), self.byte_idx(self.cursor));
                self.value.replace_range(a..b, "");
                self.cursor = s;
            }
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = n,
            KeyCode::Char(c) if !ctrl => {
                let i = self.byte_idx(self.cursor);
                self.value.insert(i, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let i = self.byte_idx(self.cursor - 1);
                self.value.remove(i);
                self.cursor -= 1;
            }
            KeyCode::Delete if self.cursor < n => {
                let i = self.byte_idx(self.cursor);
                self.value.remove(i);
            }
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(n),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = n,
            _ => return false,
        }
        true
    }
    pub fn spans(&self, width: usize, focused: bool, placeholder: &str) -> Line<'static> {
        let pal = th();
        if self.value.is_empty() && !focused {
            return Line::from(Span::styled(placeholder.to_string(), Style::default().fg(pal.muted).italic()));
        }
        let shown: Vec<char> = if self.masked {
            std::iter::repeat_n('•', self.value.chars().count()).collect()
        } else {
            self.value.chars().collect()
        };
        let width = width.max(2);
        let start = self.cursor.saturating_sub(width - 1);
        let mut spans = Vec::new();
        let before: String = shown[start..self.cursor].iter().collect();
        spans.push(Span::styled(before, Style::default().fg(pal.fg)));
        if focused {
            let c = shown.get(self.cursor).copied().unwrap_or(' ');
            spans.push(Span::styled(c.to_string(), Style::default().fg(pal.bg).bg(pal.accent)));
            let after: String =
                shown.iter().skip(self.cursor + 1).take(width.saturating_sub(self.cursor - start + 1)).collect();
            spans.push(Span::styled(after, Style::default().fg(pal.fg)));
        } else {
            let after: String =
                shown.iter().skip(self.cursor).take(width.saturating_sub(self.cursor - start)).collect();
            spans.push(Span::styled(after, Style::default().fg(pal.fg)));
        }
        if self.value.is_empty() && focused && !placeholder.is_empty() {
            spans.push(Span::styled(format!(" {placeholder}"), Style::default().fg(pal.muted).italic()));
        }
        Line::from(spans)
    }
}

pub fn panel<'a>(title: impl Into<Line<'a>>, focused: bool) -> Block<'a> {
    let pal = th();
    Block::default()
        .borders(Borders::ALL)
        .border_set(crate::display::border_set())
        .border_style(Style::default().fg(if focused { pal.border_hi } else { pal.border }))
        .title(title)
        .style(Style::default().bg(pal.surface).fg(pal.fg))
}

pub fn title<'a>(icon: &str, text: &str, focused: bool) -> Line<'a> {
    let pal = th();
    Line::from(vec![
        Span::raw(" "),
        Span::styled(icon.to_string(), Style::default().fg(if focused { pal.accent2 } else { pal.dim })),
        Span::raw(" "),
        Span::styled(
            text.to_string(),
            Style::default().fg(if focused { pal.fg } else { pal.dim }).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ])
}

pub fn modal<'a>(title_text: &str, accent: Color) -> Block<'a> {
    let pal = th();
    Block::default()
        .borders(Borders::ALL)
        .border_set(crate::display::modal_border_set())
        .border_style(Style::default().fg(accent))
        .title(Line::from(vec![
            Span::styled("▌", Style::default().fg(accent)),
            Span::styled(
                format!(" {title_text} "),
                Style::default().fg(pal.bg).bg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled("▐", Style::default().fg(accent)),
        ]))
        .padding(Padding::new(2, 2, 1, 0))
        .style(Style::default().bg(pal.overlay).fg(pal.fg))
}

pub fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(2));
    let h = h.min(area.height.saturating_sub(2));
    let [r] = Layout::vertical([Constraint::Length(h)]).flex(Flex::Center).areas(area);
    let [r] = Layout::horizontal([Constraint::Length(w)]).flex(Flex::Center).areas(r);
    r
}

pub fn chip(key: &str, label: &str) -> Vec<Span<'static>> {
    let pal = th();
    vec![
        Span::styled(format!(" {key} "), Style::default().fg(pal.bg).bg(pal.accent).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {label}  "), Style::default().fg(pal.dim)),
    ]
}

pub fn gradient_text(s: &str, bold: bool) -> Vec<Span<'static>> {
    let n = s.chars().count().max(2) - 1;
    s.chars()
        .enumerate()
        .map(|(i, c)| {
            let mut st = Style::default().fg(gradient(i as f32 / n as f32));
            if bold {
                st = st.add_modifier(Modifier::BOLD);
            }
            Span::styled(c.to_string(), st)
        })
        .collect()
}

pub fn bar(ratio: f64, width: usize) -> Vec<Span<'static>> {
    const PARTS: [&str; 8] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let pal = th();
    let ratio = ratio.clamp(0.0, 1.0);
    let eighths = (ratio * width as f64 * 8.0).round() as usize;
    let full = eighths / 8;
    let mut out = Vec::new();
    for i in 0..width {
        let col = gradient(i as f32 / width.max(2) as f32);
        let sym = if i < full {
            "█"
        } else if i == full {
            PARTS[eighths % 8]
        } else {
            " "
        };
        out.push(Span::styled(sym, Style::default().fg(col).bg(pal.border)));
    }
    out
}

const SPARK: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub fn sparkline(values: &[u64], width: usize) -> Vec<Span<'static>> {
    let pal = th();
    let vals: Vec<u64> = values.iter().rev().take(width).rev().copied().collect();
    let max = vals.iter().copied().max().unwrap_or(1).max(1);
    vals.iter()
        .map(|&v| {
            if v == 0 {
                Span::styled("·", Style::default().fg(pal.err))
            } else {
                let i = ((v as f64 / max as f64) * 7.0).round() as usize;
                Span::styled(SPARK[i.min(7)].to_string(), Style::default().fg(gradient(i as f32 / 7.0)))
            }
        })
        .collect()
}

pub fn latency_color(rtt_ms: f64) -> Color {
    let pal = th();
    if rtt_ms < 50.0 {
        pal.ok
    } else if rtt_ms < 150.0 {
        pal.warn
    } else {
        pal.err
    }
}

pub const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn spinner(tick: u64) -> &'static str {
    SPINNER[(tick as usize) % SPINNER.len()]
}
