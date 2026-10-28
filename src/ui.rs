use crate::app::{App, ConsoleState, SETTINGS_LAYOUT, SessState, Session, SettingItem, View, setting_items};
use crate::config::IconSet;
use crate::event::Level;
use crate::i18n::t;
use crate::monitor::HostStatus;
use crate::sftp::{Direction, Panel};
use crate::store::AuthKind;
use crate::theme::{gradient, th};
use crate::util::{self, ellipsize};
use crate::widgets::{bar, centered, chip, gradient_text, latency_color, modal, panel, sparkline, spinner, title};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Wrap};
use std::sync::atomic::Ordering;
use std::time::Instant;
use unicode_width::UnicodeWidthStr;

const LOGO: [&str; 6] = [
    "██████╗ ███████╗██╗  ██╗",
    "██╔══██╗██╔════╝██║  ██║",
    "██████╔╝███████╗███████║",
    "██╔══██╗╚════██║██╔══██║",
    "██║  ██║███████║██║  ██║",
    "╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝",
];

struct Icons {
    set: IconSet,
    open: &'static str,
    closed: &'static str,
    dir: &'static str,
    file: &'static str,
    link: &'static str,
    exec: &'static str,
    up: &'static str,
    local: &'static str,
    remote: &'static str,
    term: &'static str,
}

fn icons(set: IconSet) -> Icons {
    match set {
        IconSet::Unicode => Icons {
            set,
            open: "▾",
            closed: "▸",
            dir: "▰",
            file: "·",
            link: "↪",
            exec: "✱",
            up: "↰",
            local: "⌂",
            remote: "☁",
            term: "❯",
        },
        IconSet::Nerd => Icons {
            set,
            open: "",
            closed: "",
            dir: "",
            file: "",
            link: "",
            exec: "",
            up: "",
            local: "",
            remote: "",
            term: "",
        },
        IconSet::Emoji => Icons {
            set,
            open: "📂",
            closed: "📁",
            dir: "📁",
            file: "📄",
            link: "🔗",
            exec: "🚀",
            up: "⤴",
            local: "💻",
            remote: "🌐",
            term: "🖥",
        },
        IconSet::Ascii => Icons {
            set,
            open: "-",
            closed: "+",
            dir: "/",
            file: " ",
            link: "@",
            exec: "*",
            up: "^",
            local: "L",
            remote: "R",
            term: ">",
        },
    }
}

fn file_icon(set: IconSet, name: &str, fallback: &'static str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    match set {
        IconSet::Nerd => match ext.as_str() {
            "rs" => "",
            "py" => "",
            "js" | "mjs" | "cjs" => "",
            "ts" | "tsx" => "",
            "go" => "",
            "c" | "h" => "",
            "cpp" | "cc" | "hpp" => "",
            "java" | "jar" => "",
            "kt" | "kts" => "",
            "rb" => "",
            "php" => "",
            "lua" => "",
            "sh" | "bash" | "zsh" | "fish" => "",
            "md" | "markdown" => "",
            "json" => "",
            "toml" | "yaml" | "yml" | "ini" | "conf" | "cfg" => "",
            "html" | "htm" => "",
            "css" | "scss" => "",
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" => "",
            "mp3" | "flac" | "ogg" | "wav" | "m4a" | "opus" => "",
            "mp4" | "mkv" | "avi" | "mov" | "webm" => "",
            "zip" | "tar" | "gz" | "xz" | "zst" | "bz2" | "7z" | "rar" | "tgz" => "",
            "pdf" => "",
            "txt" | "log" => "",
            "lock" => "",
            "key" | "pem" | "crt" | "pub" => "",
            "iso" | "img" | "bin" => "",
            "sql" | "db" | "sqlite" => "",
            _ if lower == "dockerfile" || lower.starts_with("docker-compose") => "",
            _ if lower == "makefile" || lower == "cargo.toml" => "",
            _ if lower.starts_with(".git") => "",
            _ => fallback,
        },
        IconSet::Emoji => match ext.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" => "🖼",
            "mp3" | "flac" | "ogg" | "wav" | "m4a" | "opus" => "🎵",
            "mp4" | "mkv" | "avi" | "mov" | "webm" => "🎬",
            "zip" | "tar" | "gz" | "xz" | "zst" | "bz2" | "7z" | "rar" | "tgz" => "📦",
            "pdf" => "📕",
            "md" | "txt" | "log" => "📝",
            "key" | "pem" | "crt" | "pub" => "🔑",
            "sh" | "bash" | "zsh" | "py" | "rs" | "js" | "ts" | "go" | "c" | "cpp" => "📜",
            "toml" | "yaml" | "yml" | "json" | "ini" | "conf" => "⚙",
            _ => fallback,
        },
        _ => fallback,
    }
}

fn w(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

fn lr(mut left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let (lw, rw) = (w(&left), w(&right));
    if lw + rw < width {
        left.push(Span::raw(" ".repeat(width - lw - rw)));
    }
    left.extend(right);
    Line::from(left)
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let pal = th();
    f.render_widget(Block::default().style(Style::default().bg(pal.bg).fg(pal.fg)), f.area());
    match app.screen {
        crate::app::Screen::Hosts => draw_hosts(f, app),
        crate::app::Screen::Session => draw_session(f, app),
        crate::app::Screen::Settings => {
            draw_hosts(f, app);
            draw_settings(f, app);
        }
    }
    for d in &app.dialogs {
        crate::dialog::render(f, d, app.tick);
    }
    draw_toasts(f, app);
    crate::display::finalize(f.buffer_mut());
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let pal = th();
    f.render_widget(Block::default().style(Style::default().bg(pal.surface)), area);
    let mut left = vec![Span::styled(" ◆ ", Style::default().fg(pal.accent2))];
    left.extend(gradient_text("RSH", true));
    left.push(Span::styled(format!("  {}", t("app.tagline")), Style::default().fg(pal.muted)));
    let hosts = app.store.all_hosts();
    let online = hosts.iter().filter(|h| app.status.get(&h.id).and_then(|s| s.online) == Some(true)).count();
    let offline = hosts.iter().filter(|h| app.status.get(&h.id).and_then(|s| s.online) == Some(false)).count();
    let mut right = vec![];
    right.push(Span::styled("● ", Style::default().fg(pal.ok)));
    right.push(Span::styled(format!("{online}"), Style::default().fg(pal.fg).bold()));
    if offline > 0 {
        right.push(Span::styled("  ● ", Style::default().fg(pal.err)));
        right.push(Span::styled(format!("{offline}"), Style::default().fg(pal.fg).bold()));
    }
    right.push(Span::styled(format!(" / {}   ", hosts.len()), Style::default().fg(pal.dim)));
    if app.settings.monitor_enabled {
        right.push(Span::styled(format!("⟳ {}s   ", app.settings.monitor_interval_secs), Style::default().fg(pal.dim)));
    }
    if !app.sessions.is_empty() {
        right.push(Span::styled(format!("⧉ {}   ", app.sessions.len()), Style::default().fg(pal.accent)));
    }
    let (icon, label, col) = if !app.vault.exists() {
        ("○", t("vault.none"), pal.muted)
    } else if app.vault.is_unlocked() {
        ("🔓", t("vault.open"), pal.warn)
    } else {
        ("🔒", t("vault.closed"), pal.ok)
    };
    right.push(Span::styled(format!("{icon} {label}   "), Style::default().fg(col)));
    right.push(Span::styled(chrono::Local::now().format("%H:%M:%S ").to_string(), Style::default().fg(pal.fg).bold()));
    f.render_widget(Paragraph::new(lr(left, right, area.width as usize)), area);
}

fn draw_footer(f: &mut Frame, area: Rect, items: &[(&str, &str)]) {
    let pal = th();
    let mut spans = vec![Span::raw(" ")];
    for (k, l) in items {
        let c = chip(k, l);
        if w(&spans) + w(&c) > area.width as usize {
            break;
        }
        spans.extend(c);
    }
    f.render_widget(Paragraph::new(Line::from(spans)).style(Style::default().bg(pal.bg)), area);
}

fn draw_hosts(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let show_filter = app.filter_mode || !app.filter.value.is_empty();
    let [header, _, body, filter, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(5),
        Constraint::Length(show_filter as u16),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(f, app, header);
    let body = Rect { x: body.x + 1, width: body.width.saturating_sub(2), ..body };
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(44), Constraint::Percentage(56)]).spacing(1).areas(body);
    draw_tree(f, app, left);
    let sess_h = if app.sessions.is_empty() { 0 } else { (app.sessions.len() as u16 + 2).min(9) };
    let [details, sessions] = Layout::vertical([Constraint::Min(5), Constraint::Length(sess_h)]).areas(right);
    draw_details(f, app, details);
    if sess_h > 0 {
        draw_session_list(f, app, sessions);
    }
    if show_filter {
        let pal = th();
        let line = Line::from(
            [
                vec![Span::styled(" / ", Style::default().fg(pal.bg).bg(pal.accent2).bold()), Span::raw(" ")],
                app.filter.spans(filter.width as usize - 6, app.filter_mode, t("search.placeholder")).spans,
            ]
            .concat(),
        );
        f.render_widget(Paragraph::new(line), filter);
    }
    let hints: Vec<(&str, &str)> = if app.moving.is_some() {
        vec![("Enter/m", t("hint.move_here")), ("Esc", t("hint.cancel"))]
    } else {
        vec![
            (crate::display::enter(), t("hint.connect")),
            ("w", t("hint.window")),
            ("s", t("hint.sftp")),
            ("a", t("hint.host")),
            ("g", t("hint.group")),
            ("e", t("hint.edit")),
            ("d", t("hint.delete")),
            ("p", t("hint.ping")),
            ("/", t("hint.search")),
            ("F9", t("hint.settings")),
            ("?", t("hint.help")),
            ("q", t("hint.quit")),
        ]
    };
    draw_footer(f, footer, &hints);
}

fn status_dot(st: Option<&HostStatus>, connected: bool) -> (String, Color) {
    let pal = th();
    if connected {
        return ("◉".into(), pal.accent);
    }
    match st.and_then(|s| s.online) {
        Some(true) => ("●".into(), pal.ok),
        Some(false) => ("●".into(), pal.err),
        None => ("○".into(), pal.muted),
    }
}

fn draw_tree(f: &mut Frame, app: &mut App, area: Rect) {
    let pal = th();
    let ic = icons(app.settings.icons);
    let count = app.store.all_hosts().len();
    let block = panel(title("☰", &format!("{} · {count}", t("hosts.title")), true), true);
    let inner = block.inner(area);
    f.render_widget(block, area);
    if app.rows.is_empty() {
        let mut lines = vec![Line::raw("")];
        if app.filter.value.is_empty() {
            lines.push(Line::styled(t("hosts.empty1"), Style::default().fg(pal.dim)));
            lines.push(Line::raw(""));
            for (k, l) in [("a", t("hosts.empty_add")), ("g", t("hosts.empty_group")), ("i", t("hosts.empty_import"))] {
                lines.push(Line::from(chip(k, l)));
            }
        } else {
            lines.push(Line::styled(t("hosts.nomatch"), Style::default().fg(pal.dim)));
        }
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
        return;
    }
    let h = inner.height as usize;
    if app.cursor < app.tree_offset {
        app.tree_offset = app.cursor;
    } else if app.cursor >= app.tree_offset + h {
        app.tree_offset = app.cursor + 1 - h;
    }
    let width = inner.width as usize;
    let mut lines = vec![];
    for (i, r) in app.rows.iter().enumerate().skip(app.tree_offset).take(h) {
        let sel = i == app.cursor;
        let moving = app.moving.as_deref() == Some(r.id.as_str());
        let mut left: Vec<Span> = vec![Span::raw(" ")];
        let guide = Style::default().fg(pal.border);
        for d in 1..r.depth {
            left.push(Span::styled(if r.guides[d] { "   " } else { "│  " }, guide));
        }
        if r.depth > 0 {
            left.push(Span::styled(if r.last { "╰─ " } else { "├─ " }, guide));
        }
        let mut right = vec![];
        if r.is_group {
            let Some(g) = app.store.group(&r.id) else { continue };
            let ids = g.host_ids();
            let on = ids.iter().filter(|id| app.status.get(*id).and_then(|s| s.online) == Some(true)).count();
            let exp = g.expanded || !app.filter.value.is_empty();
            left.push(Span::styled(
                format!("{} ", if exp { ic.open } else { ic.closed }),
                Style::default().fg(pal.accent2),
            ));
            left.push(Span::styled(g.name.clone(), Style::default().fg(pal.accent2).add_modifier(Modifier::BOLD)));
            let col = if ids.is_empty() {
                pal.muted
            } else if on == ids.len() {
                pal.ok
            } else if on == 0 {
                pal.dim
            } else {
                pal.warn
            };
            right.push(Span::styled(format!("{on}/{} ", ids.len()), Style::default().fg(col)));
        } else {
            let Some(hst) = app.store.host(&r.id) else { continue };
            let st = app.status.get(&r.id);
            let connected = app.sessions.iter().any(|s| s.host_id == r.id && matches!(s.state, SessState::Connected));
            let (dot, col) = status_dot(st, connected);
            left.push(Span::styled(format!("{dot} "), Style::default().fg(col)));
            left.push(Span::styled(
                hst.name.clone(),
                Style::default().fg(pal.fg).add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
            ));
            if width > 50 {
                left.push(Span::styled(format!("  {}", hst.address), Style::default().fg(pal.muted)));
            }
            if hst.secret.is_some() || hst.key_secret.is_some() {
                right.push(Span::styled("⚿ ", Style::default().fg(pal.muted)));
            }
            match st.and_then(|s| s.rtt) {
                Some(d) => {
                    let ms = d.as_secs_f64() * 1000.0;
                    right.push(Span::styled(
                        format!("{:>7} ", util::fmt_rtt(d)),
                        Style::default().fg(latency_color(ms)),
                    ));
                }
                None if st.and_then(|s| s.online) == Some(false) => {
                    right.push(Span::styled(format!("{:>7} ", "offline"), Style::default().fg(pal.err)))
                }
                None => right.push(Span::styled(format!("{:>7} ", "—"), Style::default().fg(pal.muted))),
            }
        }
        if moving {
            left.insert(1, Span::styled("✂ ", Style::default().fg(pal.warn).bold()));
        }
        let rw = w(&right);
        let mut acc = 0;
        let mut l2 = vec![];
        for s in left {
            let sw = s.content.width();
            if acc + sw + rw > width {
                let room = width.saturating_sub(acc + rw);
                l2.push(Span::styled(ellipsize(&s.content, room), s.style));
                break;
            }
            acc += sw;
            l2.push(s);
        }
        let mut line = lr(l2, right, width);
        if sel {
            line = line.style(Style::default().bg(pal.sel_bg));
        }
        lines.push(line);
    }
    f.render_widget(Paragraph::new(lines), inner);
    if app.rows.len() > h && h > 2 {
        let pos = (app.cursor * (h - 1)) / app.rows.len().max(1);
        f.render_widget(
            Paragraph::new(Span::styled("┃", Style::default().fg(pal.accent))),
            Rect { x: area.right() - 1, y: inner.y + pos as u16, width: 1, height: 1 },
        );
    }
}

fn kv(k: &str, v: Vec<Span<'static>>) -> Line<'static> {
    let pal = th();
    let mut s = vec![Span::styled(format!("  {:<14}", k), Style::default().fg(pal.dim))];
    s.extend(v);
    Line::from(s)
}

fn draw_details(f: &mut Frame, app: &App, area: Rect) {
    let pal = th();
    let Some(r) = app.selected().cloned() else {
        let block = panel(title("✦", "RSH", false), false);
        let inner = block.inner(area);
        f.render_widget(block, area);
        let mut lines = vec![Line::raw("")];
        for (i, l) in LOGO.iter().enumerate() {
            lines.push(Line::styled(*l, Style::default().fg(gradient(i as f32 / 5.0)).bold()));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(t("app.tagline"), Style::default().fg(pal.dim)));
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
        return;
    };
    let mut lines: Vec<Line> = vec![];
    let ttl;
    if r.is_group {
        let Some(g) = app.store.group(&r.id) else { return };
        ttl = title(icons(app.settings.icons).open, &g.name, false);
        let ids = g.host_ids();
        let on = ids.iter().filter(|id| app.status.get(*id).and_then(|s| s.online) == Some(true)).count();
        let off = ids.iter().filter(|id| app.status.get(*id).and_then(|s| s.online) == Some(false)).count();
        lines.push(Line::raw(""));
        let path = app.store.path_of(&r.id);
        if !path.is_empty() {
            lines.push(kv(t("det.path"), vec![Span::raw(path.join(" / "))]));
        }
        lines.push(kv(t("det.hosts"), vec![Span::styled(ids.len().to_string(), Style::default().bold())]));
        lines.push(kv(t("det.subgroups"), vec![Span::raw(g.groups.len().to_string())]));
        lines.push(kv(
            t("det.status"),
            vec![
                Span::styled(format!("● {on} online"), Style::default().fg(pal.ok)),
                Span::raw("   "),
                Span::styled(format!("● {off} offline"), Style::default().fg(pal.err)),
                Span::styled(format!("   ○ {}", ids.len() - on - off), Style::default().fg(pal.muted)),
            ],
        ));
        if ids.len() > 1 && area.width > 30 {
            let bw = (area.width as usize).saturating_sub(22).min(40);
            let on_w = if ids.is_empty() { 0 } else { on * bw / ids.len() };
            let off_w = if ids.is_empty() { 0 } else { off * bw / ids.len() };
            lines.push(kv(
                "",
                vec![
                    Span::styled("█".repeat(on_w), Style::default().fg(pal.ok)),
                    Span::styled("█".repeat(off_w), Style::default().fg(pal.err)),
                    Span::styled("█".repeat(bw - on_w - off_w), Style::default().fg(pal.border)),
                ],
            ));
        }
        lines.push(Line::raw(""));
        for id in ids.iter().take(area.height.saturating_sub(12) as usize) {
            if let Some(h) = app.store.host(id) {
                let st = app.status.get(id);
                let (dot, col) = status_dot(st, false);
                lines.push(Line::from(vec![
                    Span::styled(format!("  {dot} "), Style::default().fg(col)),
                    Span::styled(format!("{:<20}", ellipsize(&h.name, 20)), Style::default().fg(pal.fg)),
                    Span::styled(format!("{:<26}", ellipsize(&h.target(), 26)), Style::default().fg(pal.muted)),
                    Span::styled(
                        st.and_then(|s| s.rtt).map(util::fmt_rtt).unwrap_or_default(),
                        Style::default().fg(pal.dim),
                    ),
                ]));
            }
        }
    } else {
        let Some(h) = app.store.host(&r.id) else { return };
        let st = app.status.get(&r.id);
        ttl = title("⬢", &h.name, false);
        lines.push(Line::raw(""));
        let (status_txt, col) = match st.and_then(|s| s.online) {
            Some(true) => (format!("● {}", t("det.online")), pal.ok),
            Some(false) => (format!("● {}", t("det.offline")), pal.err),
            None => (format!("○ {}", t("det.unknown")), pal.muted),
        };
        let mut sv = vec![Span::styled(status_txt, Style::default().fg(col).bold())];
        if let Some(since) = st.and_then(|s| s.since) {
            sv.push(Span::styled(
                format!("  ({} {})", t("det.for"), util::ago(since.elapsed())),
                Style::default().fg(pal.muted),
            ));
        }
        lines.push(kv(t("det.status"), sv));
        lines.push(kv(t("det.address"), vec![Span::styled(h.target(), Style::default().fg(pal.accent).bold())]));
        let path = app.store.path_of(&r.id);
        if !path.is_empty() {
            lines.push(kv(t("det.group"), vec![Span::raw(path.join(" / "))]));
        }
        let mut auth = vec![Span::raw(h.auth.label().to_string())];
        if h.secret.is_some() {
            auth.push(Span::styled(format!("  ⚿ {}", t("det.pw_saved")), Style::default().fg(pal.ok)));
        }
        if matches!(h.auth, AuthKind::Key | AuthKind::Auto)
            && let Some(k) = &h.key_path
        {
            auth.push(Span::styled(
                format!("  {}", ellipsize(k, (area.width as usize).saturating_sub(34))),
                Style::default().fg(pal.muted),
            ));
        }
        lines.push(kv(t("det.auth"), auth));
        let mut ping = vec![];
        match st.and_then(|s| s.rtt) {
            Some(d) => ping.push(Span::styled(
                format!("{:<8}", util::fmt_rtt(d)),
                Style::default().fg(latency_color(d.as_secs_f64() * 1000.0)).bold(),
            )),
            None => ping.push(Span::styled(format!("{:<8}", "—"), Style::default().fg(pal.muted))),
        }
        if let Some(s) = st {
            let vals: Vec<u64> = s.history.iter().copied().collect();
            ping.extend(sparkline(&vals, (area.width as usize).saturating_sub(30).min(40)));
        }
        lines.push(kv(t("det.ping"), ping));
        if let Some(s) = st {
            let mut up = vec![];
            if let Some(p) = s.uptime_pct() {
                up.push(Span::styled(
                    format!("{p:.1}%"),
                    Style::default().fg(if p > 99.0 {
                        pal.ok
                    } else if p > 90.0 {
                        pal.warn
                    } else {
                        pal.err
                    }),
                ));
            }
            if let Some(c) = s.checked {
                up.push(Span::styled(
                    format!("   {} {} {}", t("det.checked"), util::ago(c.elapsed()), t("det.ago")),
                    Style::default().fg(pal.muted),
                ));
            }
            lines.push(kv(t("det.uptime"), up));
        }
        lines.push(kv(
            t("det.monitor"),
            vec![if h.monitor {
                Span::styled(format!("◷ {}", t("on")), Style::default().fg(pal.ok))
            } else {
                Span::styled(t("off"), Style::default().fg(pal.muted))
            }],
        ));
        if h.connect_count > 0 {
            lines.push(kv(
                t("det.sessions"),
                vec![Span::raw(format!("{}× · {}", h.connect_count, util::fmt_mtime(h.last_connected)))],
            ));
        }
        if !h.startup_cmd.is_empty() {
            lines.push(kv(
                t("det.cmd"),
                vec![Span::styled(
                    ellipsize(&h.startup_cmd, (area.width as usize).saturating_sub(20)),
                    Style::default().fg(pal.accent2),
                )],
            ));
        }
        if !h.tags.is_empty() {
            let mut tags = vec![];
            for (i, tg) in h.tags.iter().enumerate() {
                let c = gradient(i as f32 / h.tags.len().max(2) as f32);
                tags.push(Span::styled(format!(" {tg} "), Style::default().fg(pal.bg).bg(c)));
                tags.push(Span::raw(" "));
            }
            lines.push(kv(t("det.tags"), tags));
        }
        if !h.notes.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::styled(format!("  {}", h.notes), Style::default().fg(pal.dim).italic()));
        }
        lines.push(Line::raw(""));
        let mut acts = vec![Span::raw("  ")];
        for (k, l) in [
            (crate::display::enter(), t("hint.connect")),
            ("w", t("hint.window")),
            ("s", "SFTP"),
            ("p", t("hint.ping")),
        ] {
            acts.extend(chip(k, l));
        }
        lines.push(Line::from(acts));
    }
    let block = panel(ttl, false);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
}

fn draw_session_list(f: &mut Frame, app: &App, area: Rect) {
    let pal = th();
    let block = panel(title("⧉", t("sess.title"), false), false);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines = vec![];
    for (i, s) in app.sessions.iter().enumerate() {
        let (st, col) = sess_state(s);
        let up = s.connected_at.map(|c| util::human_duration(c.elapsed())).unwrap_or_default();
        lines.push(lr(
            vec![
                Span::styled(format!(" Alt+{} ", i + 1), Style::default().fg(pal.muted)),
                Span::styled(format!("{st} "), Style::default().fg(col)),
                Span::styled(s.title.clone(), Style::default().fg(pal.fg).bold()),
                Span::styled(format!("  {}", s.target), Style::default().fg(pal.muted)),
            ],
            vec![
                Span::styled(format!("↓{} ", util::human_rate(s.rate.rx_bps)), Style::default().fg(pal.dim)),
                Span::styled(format!("{up} "), Style::default().fg(pal.dim)),
            ],
            inner.width as usize,
        ));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn sess_state(s: &Session) -> (&'static str, Color) {
    let pal = th();
    match s.state {
        SessState::Connecting(_) => ("◌", pal.warn),
        SessState::Connected => ("●", pal.ok),
        SessState::Closed(_) => ("◍", pal.muted),
        SessState::Failed(_) => ("✖", pal.err),
    }
}

fn draw_tabs(f: &mut Frame, app: &App, area: Rect) {
    let pal = th();
    f.render_widget(Block::default().style(Style::default().bg(pal.surface)), area);
    let mut left = vec![Span::styled(" ◆ ", Style::default().fg(pal.accent2).bg(pal.surface))];
    left.extend(gradient_text("RSH ", true));
    for (i, s) in app.sessions.iter().enumerate() {
        let (st, col) = sess_state(s);
        let active = i == app.active;
        let bg = if active { pal.sel_bg } else { pal.surface };
        left.push(Span::styled(if active { "▎" } else { " " }, Style::default().fg(pal.accent).bg(bg)));
        left.push(Span::styled(format!("{} ", i + 1), Style::default().fg(pal.muted).bg(bg)));
        left.push(Span::styled(format!("{st} "), Style::default().fg(col).bg(bg)));
        let mut name = Style::default().fg(if active { pal.sel_fg } else { pal.dim }).bg(bg);
        if active {
            name = name.add_modifier(Modifier::BOLD);
        }
        left.push(Span::styled(s.title.clone(), name));
        if s.view == View::Sftp {
            left.push(Span::styled(" ⇅", Style::default().fg(pal.accent2).bg(bg)));
        }
        left.push(Span::styled(" ", Style::default().bg(bg)));
    }
    let right = vec![
        Span::styled("F1 ", Style::default().fg(pal.accent)),
        Span::styled(format!("{} ", t("tab.diag")), Style::default().fg(pal.muted)),
        Span::styled("F2 ", Style::default().fg(pal.accent)),
        Span::styled(format!("{} ", t("tab.sftp")), Style::default().fg(pal.muted)),
        Span::styled("F12 ", Style::default().fg(pal.accent)),
        Span::styled(format!("{} ", t("tab.hosts")), Style::default().fg(pal.muted)),
    ];
    let right = if w(&left) + w(&right) < area.width as usize { right } else { vec![] };
    f.render_widget(Paragraph::new(lr(left, right, area.width as usize)), area);
}

fn draw_session(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let Some(s) = app.sessions.get(app.active) else { return };
    let diag = s.show_diag;
    let [tabs, body, diag_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(3), Constraint::Length(diag as u16)]).areas(area);
    draw_tabs(f, app, tabs);
    let tick = app.tick;
    let icon_set = app.settings.icons;
    let Some(s) = app.sessions.get_mut(app.active) else { return };
    if diag {
        draw_diag(f, s, diag_area);
    }
    match s.view {
        View::Term => draw_term_view(f, s, body, tick),
        View::Sftp => draw_sftp(f, s, body, tick, icon_set),
    }
}

fn draw_term_view(f: &mut Frame, s: &mut Session, body: Rect, tick: u64) {
    let pal = th();
    match &s.state {
        SessState::Connecting(stage) => {
            f.render_widget(Block::default().style(Style::default().bg(pal.bg)), body);
            let r = centered(body, 64, 9);
            let elapsed = s.started.elapsed().as_secs_f32();
            let dots: String = (0..20).map(|i| if (i as f32) < (elapsed * 8.0) % 20.0 { '━' } else { '─' }).collect();
            let lines = vec![
                Line::from(vec![
                    Span::styled(format!("{} ", spinner(tick)), Style::default().fg(pal.accent)),
                    Span::styled(format!("{} ", t("conn.connecting")), Style::default().fg(pal.fg)),
                    Span::styled(s.target.clone(), Style::default().fg(pal.accent).bold()),
                ]),
                Line::raw(""),
                Line::styled(format!("  › {stage}"), Style::default().fg(pal.dim)),
                Line::from(gradient_text(&dots, false)),
                Line::raw(""),
                Line::styled(format!("{:.1}s · Esc — {}", elapsed, t("conn.cancel")), Style::default().fg(pal.muted)),
            ];
            f.render_widget(Paragraph::new(lines).alignment(Alignment::Center).block(modal(&s.title, pal.accent)), r);
        }
        SessState::Failed(err) => {
            f.render_widget(Block::default().style(Style::default().bg(pal.bg)), body);
            let r = centered(body, 76, 9 + (err.chars().count() as u16 / 68));
            let lines = vec![
                Line::styled(format!("✖ {}", t("conn.failed")), Style::default().fg(pal.err).bold()),
                Line::raw(""),
                Line::styled(err.clone(), Style::default().fg(pal.fg)),
                Line::raw(""),
                Line::from(
                    [chip("Enter", t("conn.retry")), chip("Esc", t("conn.close")), chip("F12", t("tab.hosts"))]
                        .concat(),
                ),
            ];
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(modal(&s.title, pal.err)), r);
        }
        SessState::Connected | SessState::Closed(_) => {
            if let Some(pos) = s.term.render(body, f.buffer_mut(), true) {
                f.set_cursor_position(pos);
            }
            if let SessState::Closed(code) = s.state {
                let msg = format!(
                    " ⏻ {} {}  ·  Enter — {}  ·  Esc — {} ",
                    t("conn.closed"),
                    code.map(|c| format!("({c})")).unwrap_or_default(),
                    t("conn.retry"),
                    t("conn.close")
                );
                let wdt = (msg.width() as u16 + 2).min(body.width);
                let r = Rect {
                    x: body.x + (body.width - wdt) / 2,
                    y: body.bottom().saturating_sub(2),
                    width: wdt,
                    height: 1,
                };
                f.render_widget(Clear, r);
                f.render_widget(
                    Paragraph::new(msg)
                        .style(Style::default().fg(pal.bg).bg(pal.warn).bold())
                        .alignment(Alignment::Center),
                    r,
                );
            }
        }
    }
}

fn draw_diag(f: &mut Frame, s: &Session, area: Rect) {
    let pal = th();
    let sep = || Span::styled(" │ ", Style::default().fg(pal.border));
    let mut sp = vec![Span::styled(" ◉ ", Style::default().fg(pal.accent2))];
    match s.rtt {
        Some(d) => {
            sp.push(Span::styled(util::fmt_rtt(d), Style::default().fg(latency_color(d.as_secs_f64() * 1000.0)).bold()))
        }
        None => sp.push(Span::styled("—", Style::default().fg(pal.muted))),
    }
    sp.push(Span::raw(" "));
    let rh: Vec<u64> = s.rtt_hist.iter().copied().collect();
    sp.extend(sparkline(&rh, 10));
    sp.push(sep());
    sp.push(Span::styled("↓ ", Style::default().fg(pal.ok)));
    sp.push(Span::styled(format!("{:<11}", util::human_rate(s.rate.rx_bps)), Style::default().fg(pal.fg)));
    sp.push(Span::styled("↑ ", Style::default().fg(pal.accent)));
    sp.push(Span::styled(format!("{:<11}", util::human_rate(s.rate.tx_bps)), Style::default().fg(pal.fg)));
    let rx: Vec<u64> = s.rate.rx_hist.iter().copied().collect();
    sp.extend(sparkline(&rx, 8));
    sp.push(sep());
    sp.push(Span::styled(
        format!(
            "Σ ↓{} ↑{}",
            util::human_bytes(s.stats.rx.load(Ordering::Relaxed)),
            util::human_bytes(s.stats.tx.load(Ordering::Relaxed))
        ),
        Style::default().fg(pal.dim),
    ));
    if let Some(i) = &s.info {
        sp.push(sep());
        sp.push(Span::styled(format!("🔐 {}", i.cipher), Style::default().fg(pal.accent2)));
        sp.push(sep());
        sp.push(Span::styled(i.kex.clone(), Style::default().fg(pal.dim)));
        sp.push(sep());
        sp.push(Span::styled(i.host_key.clone(), Style::default().fg(pal.dim)));
        sp.push(sep());
        sp.push(Span::styled(format!("auth: {}", i.auth_method), Style::default().fg(pal.dim)));
        sp.push(sep());
        sp.push(Span::styled(
            format!("tcp {} · kex {}", util::fmt_rtt(i.tcp_connect), util::fmt_rtt(i.handshake)),
            Style::default().fg(pal.dim),
        ));
    }
    if let Some(c) = s.connected_at {
        sp.push(sep());
        sp.push(Span::styled(format!("⏱ {}", util::human_duration(c.elapsed())), Style::default().fg(pal.fg)));
    }
    sp.push(sep());
    sp.push(Span::styled(format!("{}×{}", s.term.size.1, s.term.size.0), Style::default().fg(pal.dim)));
    if let Some(i) = &s.info {
        sp.push(sep());
        sp.push(Span::styled(i.peer.clone(), Style::default().fg(pal.dim)));
    }
    f.render_widget(Paragraph::new(Line::from(sp)).style(Style::default().bg(pal.surface)), area);
}

fn draw_sftp(f: &mut Frame, s: &mut Session, body: Rect, tick: u64, icon_set: IconSet) {
    let pal = th();
    let host_title = s.title.clone();
    let connecting = matches!(s.state, SessState::Connecting(_));
    let Some(v) = s.sftp.as_mut() else { return };
    let active_tr: Vec<usize> = (0..v.transfers.len()).collect();
    let tr_h = active_tr.len().min(4) as u16;
    let console_h = match v.console_state {
        ConsoleState::Hidden => 0,
        _ => (body.height * 2 / 5).max(6),
    };
    let [panels, console, transfers, fkeys] = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(console_h),
        Constraint::Length(tr_h),
        Constraint::Length(1),
    ])
    .areas(body);
    let [lp, rp] = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(panels);
    let ic = icons(icon_set);
    let panels_focused = v.console_state != ConsoleState::Focused;
    let ready = v.sftp.is_some();
    let err = v.error.clone();
    let filter_mode = v.filter_mode;
    let active = v.active;
    for (i, a) in [(0usize, lp), (1, rp)] {
        let label =
            if i == 0 { format!("{} {}", ic.local, t("sftp.local")) } else { format!("{} {host_title}", ic.remote) };
        let focused = panels_focused && active == i;
        draw_panel(
            f,
            &mut v.panels[i],
            a,
            focused,
            &label,
            &ic,
            tick,
            i == 1 && !ready,
            if i == 1 { err.as_deref() } else { None },
            focused && filter_mode,
            connecting,
        );
    }
    if console_h > 0 {
        let focused = v.console_state == ConsoleState::Focused;
        let block = panel(title(ic.term, &format!("{} · {host_title}", t("sftp.console")), focused), focused);
        let inner = block.inner(console);
        f.render_widget(block, console);
        match &mut v.console {
            Some(c) => {
                if let Some(pos) = c.render(inner, f.buffer_mut(), focused)
                    && focused
                {
                    f.set_cursor_position(pos);
                }
            }
            None => f.render_widget(
                Paragraph::new(format!("{} {}", spinner(tick), t("sftp.console_opening")))
                    .style(Style::default().fg(pal.dim)),
                inner,
            ),
        }
    }
    if tr_h > 0 {
        let mut lines = vec![];
        for tr in v.transfers.iter().rev().take(4) {
            let p = &tr.progress;
            let (arrow, col) = match tr.dir {
                Direction::Upload => ("⇡", pal.accent),
                Direction::Download => ("⇣", pal.ok),
            };
            let total = p.total.load(Ordering::Relaxed);
            let done = p.done.load(Ordering::Relaxed);
            let speed = tr.speed();
            let mut sp = vec![Span::styled(format!(" {arrow} "), Style::default().fg(col).bold())];
            sp.push(Span::styled(format!("{:<22}", ellipsize(&tr.label, 22)), Style::default().fg(pal.fg)));
            let bw = (transfers.width as usize).saturating_sub(80).clamp(10, 50);
            sp.extend(bar(tr.ratio(), bw));
            sp.push(Span::styled(format!(" {:>5.1}% ", tr.ratio() * 100.0), Style::default().fg(pal.fg).bold()));
            match &tr.finished {
                Some((_, Ok(()))) => {
                    sp.push(Span::styled(format!("✔ {}", util::human_bytes(done)), Style::default().fg(pal.ok)))
                }
                Some((_, Err(e))) => {
                    sp.push(Span::styled(format!("✖ {}", ellipsize(e, 40)), Style::default().fg(pal.err)))
                }
                None => {
                    sp.push(Span::styled(format!("{:>12}", util::human_rate(speed)), Style::default().fg(pal.accent2)));
                    let eta = if speed > 1.0 && total > done {
                        util::human_duration(std::time::Duration::from_secs_f64((total - done) as f64 / speed))
                    } else {
                        "--:--:--".into()
                    };
                    sp.push(Span::styled(format!("  ETA {eta}"), Style::default().fg(pal.dim)));
                    sp.push(Span::styled(
                        format!(
                            "  {}/{} · {}",
                            p.files_done.load(Ordering::Relaxed),
                            p.files_total.load(Ordering::Relaxed),
                            ellipsize(&p.current.lock().unwrap(), 24)
                        ),
                        Style::default().fg(pal.muted),
                    ));
                }
            }
            lines.push(Line::from(sp));
        }
        f.render_widget(Paragraph::new(lines).style(Style::default().bg(pal.surface)), transfers);
    }
    let console_label = match v.console_state {
        ConsoleState::Hidden => t("fk.console"),
        ConsoleState::Focused => t("fk.panels"),
        ConsoleState::Visible => t("fk.hide"),
    };
    let keys: [(&str, &str); 10] = [
        ("1", t("fk.diag")),
        ("2", t("fk.term")),
        ("3", console_label),
        ("4", t("fk.edit")),
        ("5", t("fk.copy")),
        ("6", t("fk.move")),
        ("7", t("fk.mkdir")),
        ("8", t("fk.delete")),
        ("9", t("fk.sort")),
        ("10", t("fk.hosts")),
    ];
    let cell = (fkeys.width as usize / keys.len()).max(6);
    let mut sp = vec![];
    for (n, l) in keys {
        sp.push(Span::styled(n.to_string(), Style::default().fg(pal.fg).bg(pal.bg).bold()));
        let lw = cell.saturating_sub(n.len());
        sp.push(Span::styled(format!("{:<lw$}", ellipsize(l, lw)), Style::default().fg(pal.bg).bg(pal.accent)));
    }
    f.render_widget(Paragraph::new(Line::from(sp)).style(Style::default().bg(pal.bg)), fkeys);
}

#[allow(clippy::too_many_arguments)]
fn draw_panel(
    f: &mut Frame,
    p: &mut Panel,
    area: Rect,
    focused: bool,
    label: &str,
    ic: &Icons,
    tick: u64,
    waiting: bool,
    err: Option<&str>,
    filter_mode: bool,
    connecting: bool,
) {
    let pal = th();
    let path_room = (area.width as usize).saturating_sub(label.width() + 8);
    let mut ttl = vec![
        Span::raw(" "),
        Span::styled(label.to_string(), Style::default().fg(if focused { pal.accent2 } else { pal.dim }).bold()),
        Span::styled(" : ", Style::default().fg(pal.muted)),
    ];
    let path = if p.path.is_empty() { "…".to_string() } else { util::tilde(&p.path, p.home.as_deref()) };
    let shown = util::shorten_path(&path, path_room);
    ttl.push(Span::styled(shown, Style::default().fg(if focused { pal.fg } else { pal.dim })));
    ttl.push(Span::raw(" "));
    let files = p.view.iter().filter(|&&i| !p.entries[i].is_dir).count();
    let dirs = p.view.iter().filter(|&&i| p.entries[i].is_dir && p.entries[i].name != "..").count();
    let mut bottom = vec![Span::styled(format!(" {}/{} ", p.cursor + 1, p.view.len()), Style::default().fg(pal.dim))];
    if !p.marked.is_empty() {
        bottom.push(Span::styled(
            format!("✓ {} · {} ", p.marked.len(), util::human_bytes(p.marked_size())),
            Style::default().fg(pal.warn).bold(),
        ));
    }
    bottom.push(Span::styled(format!("· {dirs} ▰ {files} · "), Style::default().fg(pal.muted)));
    bottom.push(Span::styled(
        format!("{}{} ", p.sort.label(), if p.reverse { "↑" } else { "↓" }),
        Style::default().fg(pal.muted),
    ));
    if !p.filter.is_empty() || filter_mode {
        bottom.push(Span::styled(
            format!(" /{}{} ", p.filter, if filter_mode { "▏" } else { "" }),
            Style::default().fg(pal.bg).bg(pal.accent2),
        ));
    }
    let block = panel(Line::from(ttl), focused).title_bottom(Line::from(bottom));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if waiting {
        let msg = match err {
            Some(e) => Line::styled(format!("✖ {e}"), Style::default().fg(pal.err)),
            None if connecting => {
                Line::styled(format!("{} {}", spinner(tick), t("conn.connecting")), Style::default().fg(pal.dim))
            }
            None => Line::styled(format!("{} {}", spinner(tick), t("sftp.opening")), Style::default().fg(pal.dim)),
        };
        f.render_widget(
            Paragraph::new(vec![Line::raw(""), msg]).alignment(Alignment::Center).wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }
    let wdt = inner.width as usize;
    let show_time = wdt >= 46;
    let show_perm = wdt >= 66;
    let size_w = 9;
    let time_w = if show_time { 17 } else { 0 };
    let perm_w = if show_perm { 11 } else { 0 };
    let name_w = wdt.saturating_sub(size_w + time_w + perm_w + 1);
    let hdr = Style::default().fg(pal.muted).bold();
    let mut hs = vec![
        Span::styled(format!(" {:<w$}", t("col.name"), w = name_w.saturating_sub(1)), hdr),
        Span::styled(format!("{:>size_w$}", t("col.size")), hdr),
    ];
    if show_time {
        hs.push(Span::styled(format!(" {:<16}", t("col.mtime")), hdr));
    }
    if show_perm {
        hs.push(Span::styled(format!(" {:<10}", t("col.perm")), hdr));
    }
    let [hdr_a, list_a] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    f.render_widget(Paragraph::new(Line::from(hs)).style(Style::default().bg(pal.overlay)), hdr_a);
    let h = list_a.height as usize;
    if p.cursor < p.offset {
        p.offset = p.cursor;
    } else if p.cursor >= p.offset + h {
        p.offset = p.cursor + 1 - h;
    }
    let mut lines = vec![];
    if p.loading && p.entries.is_empty() {
        lines.push(Line::styled(format!(" {} {}", spinner(tick), t("sftp.loading")), Style::default().fg(pal.dim)));
    }
    for (row, &ei) in p.view.iter().enumerate().skip(p.offset).take(h) {
        let e = &p.entries[ei];
        let cur = row == p.cursor;
        let marked = p.marked.contains(&e.name);
        let (icon, mut style) = if e.name == ".." {
            (ic.up, Style::default().fg(pal.accent))
        } else if e.is_dir {
            (if e.is_link { ic.link } else { ic.dir }, Style::default().fg(pal.accent).bold())
        } else if e.is_link {
            (ic.link, Style::default().fg(pal.accent2))
        } else if e.is_exec() {
            (ic.exec, Style::default().fg(pal.ok))
        } else {
            (file_icon(ic.set, &e.name, ic.file), Style::default().fg(pal.fg))
        };
        if e.name.starts_with('.') && e.name != ".." {
            style = style.fg(pal.dim);
        }
        if marked {
            style = Style::default().fg(pal.warn).bold();
        }
        let size = if e.name == ".." {
            t("col.up").to_string()
        } else if e.is_dir {
            t("col.dir").to_string()
        } else {
            util::human_bytes(e.size)
        };
        let mark = if marked { "✓" } else { " " };
        let name = ellipsize(&e.name, name_w.saturating_sub(4));
        let mut sp = vec![
            Span::styled(mark, Style::default().fg(pal.warn).bold()),
            Span::styled(format!("{icon} "), style),
            Span::styled(name.clone(), style),
        ];
        let used = 1 + icon.width() + 1 + name.width();
        sp.push(Span::raw(" ".repeat(name_w.saturating_sub(used))));
        sp.push(Span::styled(
            format!("{size:>size_w$}"),
            Style::default().fg(if e.is_dir { pal.muted } else { pal.dim }),
        ));
        if show_time {
            sp.push(Span::styled(format!(" {:<16}", util::fmt_mtime(e.mtime)), Style::default().fg(pal.muted)));
        }
        if show_perm {
            sp.push(Span::styled(
                format!(" {:<10}", util::fmt_perms(e.mode, e.is_dir, e.is_link)),
                Style::default().fg(pal.muted),
            ));
        }
        let mut line = Line::from(sp);
        if cur {
            line = line.style(Style::default().bg(if focused { pal.sel_bg } else { pal.overlay }));
        }
        lines.push(line);
    }
    f.render_widget(Paragraph::new(lines), list_a);
}

fn item_keys(it: SettingItem) -> (&'static str, &'static str) {
    use SettingItem::*;
    match it {
        Lang => ("set.lang", "set.lang.d"),
        Theme => ("set.theme", "set.theme.d"),
        Icons => ("set.icons", "set.icons.d"),
        Borders => ("set.borders", "set.borders.d"),
        Colors => ("set.colors", "set.colors.d"),
        Tty => ("set.tty", "set.tty.d"),
        MonitorEnabled => ("set.mon", "set.mon.d"),
        MonitorInterval => ("set.mon_int", "set.mon_int.d"),
        PingMethod => ("set.ping", "set.ping.d"),
        PingTimeout => ("set.ping_to", "set.ping_to.d"),
        Notify => ("set.notify", "set.notify.d"),
        HostKeyPolicy => ("set.hkp", "set.hkp.d"),
        Keepalive => ("set.keepalive", "set.keepalive.d"),
        Scrollback => ("set.scrollback", "set.scrollback.d"),
        ShowDiag => ("set.diag", "set.diag.d"),
        ConfirmQuit => ("set.confirm_quit", "set.confirm_quit.d"),
        AutoLock => ("set.autolock", "set.autolock.d"),
        SftpHidden => ("set.hidden", "set.hidden.d"),
        SftpBuffer => ("set.buffer", "set.buffer.d"),
        TerminalCmd => ("set.terminal", "set.terminal.d"),
        Editor => ("set.editor", "set.editor.d"),
        ChangeMaster => ("set.master", "set.master.d"),
        LockVault => ("set.lock", "set.lock.d"),
        ImportSsh => ("set.import", "set.import.d"),
    }
}

fn draw_settings(f: &mut Frame, app: &App) {
    let pal = th();
    let items = setting_items();
    let r = centered(f.area(), 86, (items.len() + SETTINGS_LAYOUT.len() * 2 + 8) as u16);
    f.render_widget(Clear, r);
    let block = modal(&format!("⚙ {}", t("set.title")), pal.accent);
    let inner = block.inner(r);
    f.render_widget(block, r);
    let [list, desc] = Layout::vertical([Constraint::Min(3), Constraint::Length(4)]).areas(inner);
    let mut lines = vec![];
    let mut idx = 0;
    let wdt = list.width as usize;
    let mut cursor_line = 0;
    for (sec, its) in SETTINGS_LAYOUT {
        lines.push(Line::from(gradient_text(&format!("━━ {} ", t(sec)), true)));
        for it in its.iter() {
            let sel = idx == app.settings_cursor;
            if sel {
                cursor_line = lines.len();
            }
            let (lk, _) = item_keys(*it);
            let val = app.setting_value(*it);
            let left = vec![
                Span::styled(if sel { " ▶ " } else { "   " }, Style::default().fg(pal.accent)),
                Span::styled(
                    t(lk).to_string(),
                    Style::default().fg(if sel { pal.fg } else { pal.dim }).add_modifier(if sel {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
            ];
            let right = vec![
                Span::styled("‹ ", Style::default().fg(if sel { pal.accent } else { pal.border })),
                Span::styled(ellipsize(&val, 40), Style::default().fg(if sel { pal.accent2 } else { pal.fg }).bold()),
                Span::styled(" › ", Style::default().fg(if sel { pal.accent } else { pal.border })),
            ];
            let mut line = lr(left, right, wdt);
            if sel {
                line = line.style(Style::default().bg(pal.sel_bg));
            }
            lines.push(line);
            idx += 1;
        }
        lines.push(Line::raw(""));
    }
    let scroll = cursor_line.saturating_sub(list.height as usize - 2) as u16;
    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), list);
    let (_, dk) = item_keys(items[app.settings_cursor]);
    let info = if items[app.settings_cursor] == SettingItem::Theme {
        let mut sp = vec![Span::styled(" ", Style::default())];
        for (c, name) in [
            (pal.accent, "accent"),
            (pal.accent2, "accent2"),
            (pal.ok, "ok"),
            (pal.warn, "warn"),
            (pal.err, "err"),
            (pal.sel_bg, "sel"),
            (pal.surface, "surface"),
        ] {
            sp.push(Span::styled("███", Style::default().fg(c)));
            sp.push(Span::styled(format!(" {name}  "), Style::default().fg(pal.muted)));
        }
        Line::from(sp)
    } else {
        Line::styled(format!(" ℹ {}", t(dk)), Style::default().fg(pal.dim).italic())
    };
    let d = vec![
        Line::styled("─".repeat(wdt), Style::default().fg(pal.border)),
        info,
        Line::from([chip("↑↓", t("set.nav")), chip("←→ ⏎", t("set.change")), chip("Esc", t("set.back"))].concat()),
    ];
    f.render_widget(Paragraph::new(d).wrap(Wrap { trim: false }), desc);
}

fn draw_toasts(f: &mut Frame, app: &App) {
    let pal = th();
    let area = f.area();
    let now = Instant::now();
    for (i, toast) in app.toasts.iter().rev().take(5).enumerate() {
        let (icon, col) = match toast.level {
            Level::Info => ("ℹ", pal.accent),
            Level::Ok => ("✔", pal.ok),
            Level::Warn => ("⚠", pal.warn),
            Level::Error => ("✖", pal.err),
        };
        let maxw = (area.width as usize).saturating_sub(4).min(70);
        let msg = ellipsize(&toast.msg, maxw.saturating_sub(5));
        let wdt = (msg.width() + 5) as u16;
        let age = now.duration_since(toast.at).as_millis() as u16;
        let slide = if age < 150 { (150 - age) / 15 } else { 0 };
        let x = area.right().saturating_sub(wdt + 1).saturating_add(slide).min(area.right().saturating_sub(1));
        let r = Rect { x, y: area.y + 1 + i as u16, width: wdt.min(area.right() - x), height: 1 };
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("▌", Style::default().fg(col).bg(pal.overlay)),
                Span::styled(format!("{icon} "), Style::default().fg(col).bg(pal.overlay).bold()),
                Span::styled(format!("{msg} "), Style::default().fg(pal.fg).bg(pal.overlay)),
            ])),
            r,
        );
    }
}
