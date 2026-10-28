use crate::event::{HostKeyQuestion, Sid};
use crate::i18n::t;
use crate::sftp::Entry;
use crate::store::{AuthKind, Host};
use crate::theme::th;
use crate::widgets::{TextInput, centered, modal};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConnectMode {
    Tab,
    Window,
    Sftp,
}

pub enum PwPurpose {
    CreateVault,
    Unlock,
    Connect { host_id: String, mode: ConnectMode },
    NewMaster,
}

pub enum InputPurpose {
    Mkdir { sid: Sid },
    Rename { sid: Sid, from: String },
    Transfer { sid: Sid, mv: bool, items: Vec<Entry> },
    Goto { sid: Sid },
    Group { id: Option<String>, parent: String },
    TerminalCmd,
    Editor,
}

pub enum ConfirmAction {
    DeleteNode(String),
    Quit,
    SftpDelete { sid: Sid, items: Vec<Entry> },
    ImportSsh,
}

pub enum Dialog {
    Host(Box<HostForm>),
    Confirm {
        title: String,
        body: Vec<String>,
        action: ConfirmAction,
        danger: bool,
    },
    Password {
        title: String,
        prompt: String,
        input: TextInput,
        confirm: Option<TextInput>,
        second: bool,
        purpose: PwPurpose,
        error: Option<String>,
    },
    Input {
        title: String,
        prompt: String,
        input: TextInput,
        purpose: InputPurpose,
    },
    HostKey(Option<HostKeyQuestion>),
    Help {
        scroll: u16,
    },
}

pub const F_NAME: usize = 0;
pub const F_ADDR: usize = 1;
pub const F_PORT: usize = 2;
pub const F_USER: usize = 3;
pub const F_AUTH: usize = 4;
pub const F_PASS: usize = 5;
pub const F_KEY: usize = 6;
pub const F_KEYPASS: usize = 7;
pub const F_TAGS: usize = 8;
pub const F_CMD: usize = 9;
pub const F_NOTES: usize = 10;
pub const F_MON: usize = 11;

pub enum FieldKind {
    Text,
    Secret,
    Choice,
    Toggle,
}

pub struct Field {
    pub label: &'static str,
    pub kind: FieldKind,
    pub input: TextInput,
    pub choice: usize,
    pub on: bool,
    pub placeholder: String,
}

impl Field {
    fn text(label: &'static str, v: &str, ph: &str) -> Self {
        Self { label, kind: FieldKind::Text, input: TextInput::new(v), choice: 0, on: false, placeholder: ph.into() }
    }
    fn secret(label: &'static str, stored: bool) -> Self {
        let ph = if stored { t("form.secret_kept") } else { t("form.secret_empty") };
        Self {
            label,
            kind: FieldKind::Secret,
            input: TextInput::masked(),
            choice: 0,
            on: false,
            placeholder: ph.into(),
        }
    }
}

pub struct HostForm {
    pub id: Option<String>,
    pub group: String,
    pub fields: Vec<Field>,
    pub focus: usize,
    pub error: Option<String>,
    pub reveal: bool,
}

impl HostForm {
    pub fn new(group: String, h: Option<&Host>) -> Self {
        let d = Host::default();
        let h = h.unwrap_or(&d);
        let auth_idx = AuthKind::ALL.iter().position(|a| *a == h.auth).unwrap_or(0);
        let fields = vec![
            Field::text("form.name", &h.name, "web-01"),
            Field::text("form.address", &h.address, "10.0.0.1 / user@host:port"),
            Field::text("form.port", &h.port.to_string(), "22"),
            Field::text("form.user", &h.user, "root"),
            Field {
                label: "form.auth",
                kind: FieldKind::Choice,
                input: TextInput::default(),
                choice: auth_idx,
                on: false,
                placeholder: String::new(),
            },
            Field::secret("form.password", h.secret.is_some()),
            Field::text("form.key", h.key_path.as_deref().unwrap_or(""), "~/.ssh/id_ed25519"),
            Field::secret("form.keypass", h.key_secret.is_some()),
            Field::text("form.tags", &h.tags.join(", "), "prod, nginx"),
            Field::text("form.cmd", &h.startup_cmd, "tmux new -A -s main"),
            Field::text("form.notes", &h.notes, ""),
            Field {
                label: "form.monitor",
                kind: FieldKind::Toggle,
                input: TextInput::default(),
                choice: 0,
                on: h.monitor,
                placeholder: String::new(),
            },
        ];
        let is_new = h.name.is_empty();
        Self { id: (!is_new).then(|| h.id.clone()), group, fields, focus: 0, error: None, reveal: false }
    }

    pub fn auth(&self) -> AuthKind {
        AuthKind::ALL[self.fields[F_AUTH].choice]
    }

    pub fn visible(&self, i: usize) -> bool {
        let a = self.auth();
        match i {
            F_PASS => matches!(a, AuthKind::Password | AuthKind::Auto),
            F_KEY | F_KEYPASS => matches!(a, AuthKind::Key | AuthKind::Auto),
            _ => true,
        }
    }

    pub fn step(&mut self, delta: isize) {
        let n = self.fields.len() as isize;
        let mut i = self.focus as isize;
        loop {
            i = (i + delta).rem_euclid(n);
            if self.visible(i as usize) {
                break;
            }
        }
        self.focus = i as usize;
    }

    pub fn value(&self, i: usize) -> String {
        self.fields[i].input.value.trim().to_string()
    }
}

pub fn render(f: &mut Frame, d: &Dialog, tick: u64) {
    let pal = th();
    let area = f.area();
    match d {
        Dialog::Host(form) => render_host_form(f, form),
        Dialog::Confirm { title, body, danger, .. } => {
            let w = body.iter().map(|l| l.chars().count()).max().unwrap_or(20).max(36) as u16 + 8;
            let r = centered(area, w, body.len() as u16 + 6);
            f.render_widget(Clear, r);
            let acc = if *danger { pal.err } else { pal.accent };
            let mut lines: Vec<Line> = body.iter().map(|l| Line::from(l.as_str())).collect();
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled(" Y/Enter ", Style::default().fg(pal.bg).bg(acc).bold()),
                Span::styled(format!(" {}   ", t("btn.yes")), Style::default().fg(pal.dim)),
                Span::styled(" N/Esc ", Style::default().fg(pal.bg).bg(pal.muted).bold()),
                Span::styled(format!(" {}", t("btn.no")), Style::default().fg(pal.dim)),
            ]));
            f.render_widget(Paragraph::new(lines).block(modal(title, acc)).wrap(Wrap { trim: false }), r);
        }
        Dialog::Password { title, prompt, input, confirm, second, error, .. } => {
            let wdt = 64u16;
            let prompt_lines = (prompt.chars().count() as u16).div_ceil(wdt - 8).max(1);
            let h = 6 + prompt_lines + confirm.is_some() as u16 * 2 + error.is_some() as u16;
            let r = centered(area, wdt, h + 1);
            f.render_widget(Clear, r);
            let iw = r.width.saturating_sub(10) as usize;
            let mut lines = vec![
                Line::styled(prompt.clone(), Style::default().fg(pal.dim)),
                Line::from(
                    [vec![Span::styled(" 🔑 ", Style::default().fg(pal.accent2))], input.spans(iw, !second, "").spans]
                        .concat(),
                ),
            ];
            if let Some(c) = confirm {
                lines.push(Line::styled(t("pw.repeat"), Style::default().fg(pal.dim)));
                lines.push(Line::from(
                    [vec![Span::styled(" 🔑 ", Style::default().fg(pal.accent2))], c.spans(iw, *second, "").spans]
                        .concat(),
                ));
            }
            if let Some(e) = error {
                lines.push(Line::styled(format!("✖ {e}"), Style::default().fg(pal.err).bold()));
            }
            lines.push(Line::raw(""));
            lines.push(Line::styled(t("pw.hint"), Style::default().fg(pal.muted)));
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(modal(title, pal.accent2)), r);
        }
        Dialog::Input { title, prompt, input, .. } => {
            let r = centered(area, 72, 7);
            f.render_widget(Clear, r);
            let iw = r.width.saturating_sub(8) as usize;
            let lines = vec![
                Line::styled(prompt.clone(), Style::default().fg(pal.dim)),
                input.spans(iw, true, ""),
                Line::raw(""),
                Line::styled(t("input.hint"), Style::default().fg(pal.muted)),
            ];
            f.render_widget(Paragraph::new(lines).block(modal(title, pal.accent)), r);
        }
        Dialog::HostKey(q) => {
            let Some(q) = q else { return };
            let acc = if q.changed { pal.err } else { pal.warn };
            let r = centered(area, 78, if q.changed { 15 } else { 12 });
            f.render_widget(Clear, r);
            let mut lines = vec![];
            if q.changed {
                lines.push(Line::styled(t("hk.changed1"), Style::default().fg(pal.err).bold()));
                lines.push(Line::styled(t("hk.changed2"), Style::default().fg(pal.err)));
                lines.push(Line::raw(""));
            } else {
                lines.push(Line::styled(t("hk.new"), Style::default().fg(pal.fg)));
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(vec![
                Span::styled(format!("{:<10}", t("hk.host")), Style::default().fg(pal.dim)),
                Span::raw(format!("{}:{}", q.host, q.port)).bold(),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("{:<10}", t("hk.type")), Style::default().fg(pal.dim)),
                Span::raw(q.key_type.clone()),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("{:<10}", t("hk.fp")), Style::default().fg(pal.dim)),
                Span::styled(q.fingerprint.clone(), Style::default().fg(pal.accent2).bold()),
            ]));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled(" Y ", Style::default().fg(pal.bg).bg(acc).bold()),
                Span::styled(format!(" {}   ", t("hk.trust")), Style::default().fg(pal.dim)),
                Span::styled(" N/Esc ", Style::default().fg(pal.bg).bg(pal.muted).bold()),
                Span::styled(format!(" {}", t("hk.reject")), Style::default().fg(pal.dim)),
            ]));
            let spin = crate::widgets::spinner(tick);
            f.render_widget(
                Paragraph::new(lines)
                    .block(modal(&format!("{spin} {}", t("hk.title")), acc))
                    .wrap(Wrap { trim: false }),
                r,
            );
        }
        Dialog::Help { scroll } => render_help(f, *scroll),
    }
}

fn render_host_form(f: &mut Frame, form: &HostForm) {
    let pal = th();
    let visible: Vec<usize> = (0..form.fields.len()).filter(|&i| form.visible(i)).collect();
    let h = visible.len() as u16 * 2 + 6 + form.error.is_some() as u16;
    let r = centered(f.area(), 76, h);
    f.render_widget(Clear, r);
    let title = if form.id.is_some() { t("form.edit_title") } else { t("form.new_title") };
    let block = modal(title, pal.accent);
    let inner = block.inner(r);
    f.render_widget(block, r);
    let mut rows: Vec<Constraint> = visible.iter().map(|_| Constraint::Length(2)).collect();
    rows.push(Constraint::Min(1));
    let areas = Layout::vertical(rows).split(inner);
    let label_w = 16u16;
    for (slot, &i) in visible.iter().enumerate() {
        let fl = &form.fields[i];
        let focused = form.focus == i;
        let a = areas[slot];
        let [la, va] =
            Layout::horizontal([Constraint::Length(label_w), Constraint::Min(1)]).areas(Rect { height: 1, ..a });
        let marker = if focused { "▶ " } else { "  " };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(marker, Style::default().fg(pal.accent)),
                Span::styled(
                    t(fl.label),
                    Style::default().fg(if focused { pal.fg } else { pal.dim }).add_modifier(if focused {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
            ])),
            la,
        );
        let line = match fl.kind {
            FieldKind::Text => fl.input.spans(va.width as usize - 1, focused, &fl.placeholder),
            FieldKind::Secret => {
                let mut inp = fl.input.clone();
                inp.masked = !form.reveal;
                inp.spans(va.width as usize - 1, focused, &fl.placeholder)
            }
            FieldKind::Choice => {
                let mut spans = vec![Span::styled("‹ ", Style::default().fg(pal.muted))];
                for (k, a) in AuthKind::ALL.iter().enumerate() {
                    let st = if k == fl.choice {
                        Style::default().fg(pal.bg).bg(if focused { pal.accent } else { pal.dim }).bold()
                    } else {
                        Style::default().fg(pal.dim)
                    };
                    spans.push(Span::styled(format!(" {} ", a.label()), st));
                }
                spans.push(Span::styled(" ›", Style::default().fg(pal.muted)));
                Line::from(spans)
            }
            FieldKind::Toggle => Line::from(vec![
                Span::styled(
                    if fl.on { "  ● " } else { "  ○ " },
                    Style::default().fg(if fl.on { pal.ok } else { pal.muted }),
                ),
                Span::styled(
                    if fl.on { t("on") } else { t("off") },
                    Style::default().fg(if focused { pal.fg } else { pal.dim }),
                ),
            ]),
        };
        f.render_widget(Paragraph::new(line), va);
        if matches!(fl.kind, FieldKind::Text | FieldKind::Secret) && a.height > 1 {
            let ul = "─".repeat(va.width as usize);
            f.render_widget(
                Paragraph::new(Span::styled(ul, Style::default().fg(if focused { pal.border_hi } else { pal.border }))),
                Rect { y: a.y + 1, height: 1, ..va },
            );
        }
    }
    let last = areas[visible.len()];
    let mut lines = vec![];
    if let Some(e) = &form.error {
        lines.push(Line::styled(format!("✖ {e}"), Style::default().fg(pal.err).bold()));
    }
    let mut hint = vec![];
    for (k, l) in [
        ("Tab/↓", t("form.next")),
        ("←→", t("form.choose")),
        ("Ctrl+S", t("form.save")),
        ("Ctrl+R", t("form.reveal")),
        ("Esc", t("form.cancel")),
    ] {
        hint.extend(crate::widgets::chip(k, l));
    }
    lines.push(Line::from(hint));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), last);
}

fn render_help(f: &mut Frame, scroll: u16) {
    let pal = th();
    let r = centered(f.area(), 92, 40);
    f.render_widget(Clear, r);
    let sections: &[(&str, &[(&str, &str)])] = &[
        (
            "help.hosts",
            &[
                ("Enter", "help.connect"),
                ("w", "help.window"),
                ("s", "help.open_sftp"),
                ("a / g", "help.add"),
                ("e / F4", "help.edit"),
                ("d / Del", "help.delete"),
                ("c", "help.dup"),
                ("m", "help.move"),
                ("Shift+↑↓", "help.reorder"),
                ("p / P", "help.ping"),
                ("M", "help.monitor"),
                ("/", "help.search"),
                ("Space ←→", "help.fold"),
                ("Tab / Alt+1..9", "help.sessions"),
                ("L", "help.lock"),
                ("F9 / ,", "help.settings"),
                ("q", "help.quit"),
            ],
        ),
        (
            "help.session",
            &[
                ("F1", "help.diag"),
                ("F2", "help.toggle_sftp"),
                ("F12", "help.back"),
                ("Alt+←→ / Alt+1..9", "help.switch"),
                ("Shift+PgUp/PgDn", "help.scroll"),
                ("Enter (closed)", "help.reconnect"),
                ("Esc (closed)", "help.close"),
            ],
        ),
        (
            "help.sftp",
            &[
                ("Tab", "help.panel"),
                ("Enter / Backspace", "help.nav"),
                ("Space / Ins", "help.mark"),
                ("* / Ctrl+A", "help.markall"),
                ("F3", "help.console"),
                ("F4", "help.editfile"),
                ("F5", "help.copy"),
                ("F6", "help.mv"),
                ("Shift+F6", "help.rename"),
                ("F7", "help.mkdir"),
                ("F8 / Del", "help.rm"),
                ("F9", "help.sort"),
                ("/", "help.filter"),
                ("Ctrl+H / .", "help.hidden"),
                ("Ctrl+R", "help.refresh"),
                ("Ctrl+G", "help.goto"),
                ("Ctrl+T", "help.cdterm"),
                ("Ctrl+X", "help.cancel"),
                ("F10", "help.back"),
            ],
        ),
    ];
    let mut lines = vec![];
    for (title, keys) in sections {
        lines.push(Line::from(crate::widgets::gradient_text(&format!("── {} ", t(title)), true)));
        for (k, d) in keys.iter() {
            lines.push(Line::from(vec![
                Span::styled(format!("  {k:<22}"), Style::default().fg(pal.accent).bold()),
                Span::styled(t(d), Style::default().fg(pal.fg)),
            ]));
        }
        lines.push(Line::raw(""));
    }
    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)).block(modal(t("help.title"), pal.accent2)), r);
}
