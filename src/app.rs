use crate::config::{self, BorderStyle, ColorMode, HostKeyPolicy, IconSet, PingMethod, Settings, TtyMode};
use crate::dialog::*;
use crate::event::{AppEvent, Level, Pane, Sid};
use crate::handoff::{self, Handoff};
use crate::i18n::{self, Lang, t};
use crate::monitor::{self, HostStatus, MonitorCfg, Target};
use crate::sftp::{self, Direction, Entry, Job, Panel, Progress, Side, Transfer};
use crate::ssh::{self, ConnInfo, Creds, Handle, PtyCmd, Stats};
use crate::store::{AuthKind, Group, Host, Node, ROOT_ID, Row, Store};
use crate::term::TermPane;
use crate::util;
use crate::vault::Vault;
use crate::widgets::TextInput;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use russh_sftp::client::SftpSession;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch;
use zeroize::Zeroizing;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Hosts,
    Session,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Term,
    Sftp,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsoleState {
    Hidden,
    Focused,
    Visible,
}

pub enum SessState {
    Connecting(String),
    Connected,
    Closed(Option<u32>),
    Failed(String),
}

#[derive(Default)]
pub struct Rate {
    last: Option<(Instant, u64, u64)>,
    pub rx_bps: f64,
    pub tx_bps: f64,
    pub rx_hist: VecDeque<u64>,
}

pub struct SftpView {
    pub sftp: Option<Arc<SftpSession>>,
    pub panels: [Panel; 2],
    pub active: usize,
    pub console: Option<TermPane>,
    pub console_state: ConsoleState,
    pub transfers: Vec<Transfer>,
    pub filter_mode: bool,
    pub error: Option<String>,
}

impl SftpView {
    pub fn panel(&mut self) -> &mut Panel {
        &mut self.panels[self.active]
    }
}

pub struct Session {
    pub id: Sid,
    pub host_id: String,
    pub title: String,
    pub target: String,
    pub state: SessState,
    pub handle: Option<Arc<Handle>>,
    pub info: Option<ConnInfo>,
    pub term: TermPane,
    pub view: View,
    pub show_diag: bool,
    pub sftp: Option<SftpView>,
    pub stats: Arc<Stats>,
    pub rate: Rate,
    pub rtt: Option<Duration>,
    pub rtt_hist: VecDeque<u64>,
    pub started: Instant,
    pub connected_at: Option<Instant>,
    pub want_sftp: bool,
    pub creds: Creds,
    pub startup_cmd: String,
}

pub struct Toast {
    pub msg: String,
    pub level: Level,
    pub at: Instant,
}

pub enum Pending {
    Connect { host_id: String, mode: ConnectMode },
    SubmitHostForm,
    NewMaster,
}

pub enum After {
    Cleanup,
    RefreshLocal(Sid),
    Upload { sid: Sid, remote: String, before: Option<std::time::SystemTime> },
}

pub struct Suspend {
    pub program: String,
    pub file: PathBuf,
    pub after: After,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingItem {
    Lang,
    Theme,
    Icons,
    Borders,
    Colors,
    Tty,
    MonitorEnabled,
    MonitorInterval,
    PingMethod,
    PingTimeout,
    Notify,
    HostKeyPolicy,
    Keepalive,
    Scrollback,
    ShowDiag,
    ConfirmQuit,
    AutoLock,
    SftpHidden,
    SftpBuffer,
    TerminalCmd,
    Editor,
    ChangeMaster,
    LockVault,
    ImportSsh,
}

pub const SETTINGS_LAYOUT: &[(&str, &[SettingItem])] = &[
    (
        "set.sec.ui",
        &[
            SettingItem::Lang,
            SettingItem::Theme,
            SettingItem::Icons,
            SettingItem::Borders,
            SettingItem::Colors,
            SettingItem::Tty,
            SettingItem::ShowDiag,
            SettingItem::ConfirmQuit,
        ],
    ),
    (
        "set.sec.monitor",
        &[
            SettingItem::MonitorEnabled,
            SettingItem::MonitorInterval,
            SettingItem::PingMethod,
            SettingItem::PingTimeout,
            SettingItem::Notify,
        ],
    ),
    (
        "set.sec.ssh",
        &[SettingItem::HostKeyPolicy, SettingItem::Keepalive, SettingItem::Scrollback, SettingItem::TerminalCmd],
    ),
    ("set.sec.sftp", &[SettingItem::SftpHidden, SettingItem::SftpBuffer, SettingItem::Editor]),
    (
        "set.sec.security",
        &[SettingItem::AutoLock, SettingItem::ChangeMaster, SettingItem::LockVault, SettingItem::ImportSsh],
    ),
];

pub fn setting_items() -> Vec<SettingItem> {
    SETTINGS_LAYOUT.iter().flat_map(|(_, v)| v.iter().copied()).collect()
}

pub struct App {
    pub settings: Settings,
    pub store: Store,
    pub vault: Vault,
    pub screen: Screen,
    pub rows: Vec<Row>,
    pub cursor: usize,
    pub tree_offset: usize,
    pub filter: TextInput,
    pub filter_mode: bool,
    pub moving: Option<String>,
    pub status: HashMap<String, HostStatus>,
    pub sessions: Vec<Session>,
    pub active: usize,
    next_sid: Sid,
    next_tid: u64,
    pub dialogs: Vec<Dialog>,
    pending: Option<Pending>,
    pub toasts: Vec<Toast>,
    pub tx: UnboundedSender<AppEvent>,
    monitor_tx: watch::Sender<MonitorCfg>,
    handoff: Option<Arc<Handoff>>,
    pub settings_cursor: usize,
    pub quit: bool,
    pub suspend: Option<Suspend>,
    pub single: bool,
    pub tick: u64,
    pub size: (u16, u16),
    last_activity: Instant,
    last_rtt: Instant,
}

impl App {
    pub fn new(tx: UnboundedSender<AppEvent>, single: bool) -> anyhow::Result<Self> {
        let settings = Settings::load();
        i18n::set_lang(settings.lang);
        crate::display::apply(&settings);
        let store = Store::load()?;
        let (monitor_tx, monitor_rx) = watch::channel(MonitorCfg {
            enabled: false,
            interval: Duration::from_secs(60),
            method: PingMethod::Tcp,
            timeout: Duration::from_secs(1),
            targets: vec![],
        });
        if !single {
            monitor::spawn(monitor_rx, tx.clone());
        }
        let size = crossterm::terminal::size().unwrap_or((120, 40));
        let mut app = Self {
            settings,
            store,
            vault: Vault::load(),
            screen: Screen::Hosts,
            rows: vec![],
            cursor: 0,
            tree_offset: 0,
            filter: TextInput::default(),
            filter_mode: false,
            moving: None,
            status: HashMap::new(),
            sessions: vec![],
            active: 0,
            next_sid: 1,
            next_tid: 1,
            dialogs: vec![],
            pending: None,
            toasts: vec![],
            tx,
            monitor_tx,
            handoff: None,
            settings_cursor: 0,
            quit: false,
            suspend: None,
            single,
            tick: 0,
            size,
            last_activity: Instant::now(),
            last_rtt: Instant::now(),
        };
        app.refresh_rows();
        if !single {
            app.push_monitor();
            if !app.vault.exists() {
                app.dialogs.push(Dialog::Password {
                    title: t("pw.create_title").into(),
                    prompt: t("pw.create_prompt").into(),
                    input: TextInput::masked(),
                    confirm: Some(TextInput::masked()),
                    second: false,
                    purpose: PwPurpose::CreateVault,
                    error: None,
                });
            }
        }
        Ok(app)
    }

    pub fn toast(&mut self, msg: impl Into<String>, level: Level) {
        self.toasts.push(Toast { msg: msg.into(), level, at: Instant::now() });
        if self.toasts.len() > 6 {
            self.toasts.remove(0);
        }
    }

    pub fn refresh_rows(&mut self) {
        let cur_id = self.rows.get(self.cursor).map(|r| r.id.clone());
        self.rows = self.store.rows(&self.filter.value);
        if let Some(id) = cur_id {
            self.select_id(&id);
        }
        self.cursor = self.cursor.min(self.rows.len().saturating_sub(1));
    }

    fn select_id(&mut self, id: &str) {
        if let Some(i) = self.rows.iter().position(|r| r.id == id) {
            self.cursor = i;
        }
    }

    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }

    fn save_store(&mut self) {
        if let Err(e) = self.store.save() {
            self.toast(format!("{}: {e}", t("err.save")), Level::Error);
        }
    }

    fn save_settings(&mut self) {
        if let Err(e) = self.settings.save() {
            self.toast(format!("{}: {e}", t("err.save")), Level::Error);
        }
    }

    pub fn push_monitor(&self) {
        let live: Vec<&str> = self
            .sessions
            .iter()
            .filter(|s| matches!(s.state, SessState::Connected))
            .map(|s| s.host_id.as_str())
            .collect();
        let targets = self
            .store
            .all_hosts()
            .iter()
            .filter(|h| h.monitor && !h.address.is_empty() && !live.contains(&h.id.as_str()))
            .map(|h| Target { id: h.id.clone(), address: h.address.clone(), port: h.port })
            .collect();
        let _ = self.monitor_tx.send(MonitorCfg {
            enabled: self.settings.monitor_enabled,
            interval: Duration::from_secs(self.settings.monitor_interval_secs),
            method: self.settings.ping_method,
            timeout: Duration::from_millis(self.settings.ping_timeout_ms),
            targets,
        });
    }

    fn session_idx(&self, sid: Sid) -> Option<usize> {
        self.sessions.iter().position(|s| s.id == sid)
    }

    fn sess(&mut self, sid: Sid) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == sid)
    }

    fn ask_unlock(&mut self, then: Pending) {
        self.pending = Some(then);
        let creating = !self.vault.exists();
        self.dialogs.push(Dialog::Password {
            title: if creating { t("pw.create_title") } else { t("pw.unlock_title") }.into(),
            prompt: if creating { t("pw.create_prompt") } else { t("pw.unlock_prompt") }.into(),
            input: TextInput::masked(),
            confirm: creating.then(TextInput::masked),
            second: false,
            purpose: if creating { PwPurpose::CreateVault } else { PwPurpose::Unlock },
            error: None,
        });
    }

    fn run_pending(&mut self) {
        match self.pending.take() {
            Some(Pending::Connect { host_id, mode }) => self.connect(&host_id, mode),
            Some(Pending::SubmitHostForm) => self.submit_host_form(),
            Some(Pending::NewMaster) => self.dialogs.push(Dialog::Password {
                title: t("pw.new_title").into(),
                prompt: t("pw.new_prompt").into(),
                input: TextInput::masked(),
                confirm: Some(TextInput::masked()),
                second: false,
                purpose: PwPurpose::NewMaster,
                error: None,
            }),
            None => {}
        }
    }

    fn term_dims(&self) -> (u16, u16) {
        (self.size.0.max(20), self.size.1.saturating_sub(1).max(5))
    }

    pub fn on_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Input(e) => self.on_input(e),
            AppEvent::Stage { sid, stage } => {
                if let Some(s) = self.sess(sid)
                    && let SessState::Connecting(_) = s.state
                {
                    s.state = SessState::Connecting(stage);
                }
            }
            AppEvent::Connected { sid, handle, info } => {
                let host_id;
                let want_sftp;
                {
                    let Some(s) = self.sess(sid) else { return };
                    s.handle = Some(handle.clone());
                    s.info = Some(info);
                    s.state = SessState::Connected;
                    s.connected_at = Some(Instant::now());
                    host_id = s.host_id.clone();
                    want_sftp = s.want_sftp;
                }
                if let Some(st) = self.status.get_mut(&host_id) {
                    st.online = Some(true);
                }
                self.push_monitor();
                if want_sftp {
                    self.open_sftp(sid);
                }
                let h = handle.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    if let Some(rtt) = ssh::measure_rtt(&h).await {
                        tx.send(AppEvent::Rtt { sid, rtt }).ok();
                    }
                });
            }
            AppEvent::ConnectFailed { sid, err } => {
                if let Some(s) = self.sess(sid) {
                    s.state = SessState::Failed(err.clone());
                    let title = s.title.clone();
                    self.toast(format!("{title}: {err}"), Level::Error);
                }
            }
            AppEvent::PtyReady { sid, pane, tx } => {
                let Some(s) = self.sess(sid) else { return };
                match pane {
                    Pane::Main => {
                        let (rows, cols) = s.term.size;
                        let _ = tx.send(PtyCmd::Resize(cols, rows));
                        if !s.startup_cmd.is_empty() {
                            let _ = tx.send(PtyCmd::Data(format!("{}\r", s.startup_cmd).into_bytes()));
                        }
                        s.term.tx = Some(tx);
                    }
                    Pane::Console => {
                        if let Some(v) = &mut s.sftp
                            && let Some(c) = &mut v.console
                        {
                            let dir = v.panels[1].path.clone();
                            let _ = tx
                                .send(PtyCmd::Data(format!("cd {} && clear\r", util::shell_quote(&dir)).into_bytes()));
                            let (rows, cols) = c.size;
                            let _ = tx.send(PtyCmd::Resize(cols, rows));
                            c.tx = Some(tx);
                        }
                    }
                }
            }
            AppEvent::PtyData { sid, pane, data } => {
                let Some(s) = self.sess(sid) else { return };
                match pane {
                    Pane::Main => s.term.feed(&data),
                    Pane::Console => {
                        if let Some(c) = s.sftp.as_mut().and_then(|v| v.console.as_mut()) {
                            c.feed(&data)
                        }
                    }
                }
            }
            AppEvent::PtyClosed { sid, pane, status } => {
                let single = self.single;
                let Some(s) = self.sess(sid) else { return };
                match pane {
                    Pane::Main => {
                        s.term.tx = None;
                        s.term.closed = Some(status);
                        if matches!(s.state, SessState::Connected) {
                            s.state = SessState::Closed(status);
                        }
                        let quit = single && s.view == View::Term;
                        self.push_monitor();
                        if quit {
                            self.quit = true;
                        }
                    }
                    Pane::Console => {
                        if let Some(v) = &mut s.sftp {
                            v.console = None;
                            v.console_state = ConsoleState::Hidden;
                        }
                    }
                }
            }
            AppEvent::HostKey(q) => {
                if self.single && self.sessions.is_empty() {
                    let _ = q.reply.send(false);
                    return;
                }
                self.dialogs.push(Dialog::HostKey(Some(q)));
            }
            AppEvent::Rtt { sid, rtt } => {
                if let Some(s) = self.sess(sid) {
                    s.rtt = Some(rtt);
                    s.rtt_hist.push_back(rtt.as_micros() as u64);
                    while s.rtt_hist.len() > 40 {
                        s.rtt_hist.pop_front();
                    }
                    let host_id = s.host_id.clone();
                    self.status.entry(host_id).or_default().record(Some(rtt));
                }
            }
            AppEvent::Ping { host_id, rtt, manual } => {
                let flipped = self.status.entry(host_id.clone()).or_default().record(rtt);
                let name = self.store.host(&host_id).map(|h| h.name.clone()).unwrap_or_default();
                if manual {
                    match rtt {
                        Some(d) => self.toast(format!("{name}: {} {}", t("ping.reply"), util::fmt_rtt(d)), Level::Ok),
                        None => self.toast(format!("{name}: {}", t("ping.noreply")), Level::Warn),
                    }
                } else if flipped && self.settings.notify_status_change {
                    match rtt {
                        Some(_) => self.toast(format!("▲ {name} {}", t("ping.up")), Level::Ok),
                        None => self.toast(format!("▼ {name} {}", t("ping.down")), Level::Error),
                    }
                }
            }
            AppEvent::SftpReady { sid, sftp, home } => {
                let Some(s) = self.sess(sid) else { return };
                if let Some(v) = &mut s.sftp {
                    v.sftp = Some(sftp);
                    v.panels[1].path = home.clone();
                    v.panels[1].home = Some(home.clone());
                }
                self.navigate(sid, 1, home, None);
            }
            AppEvent::SftpFailed { sid, err } => {
                if let Some(s) = self.sess(sid)
                    && let Some(v) = &mut s.sftp
                {
                    v.error = Some(err.clone());
                }
                self.toast(format!("SFTP: {err}"), Level::Error);
            }
            AppEvent::RemoteList { sid, path, result, focus } => {
                let Some(s) = self.sess(sid) else { return };
                let Some(v) = &mut s.sftp else { return };
                let p = &mut v.panels[1];
                match result {
                    Ok(entries) => {
                        if p.path != path && !p.path.is_empty() {
                            p.history.push(p.path.clone());
                        }
                        p.set_entries(path, entries, focus.as_deref());
                    }
                    Err(e) => {
                        p.loading = false;
                        p.error = Some(e.clone());
                        self.toast(e, Level::Error);
                    }
                }
            }
            AppEvent::OpDone { sid, result } => {
                match result {
                    Ok(m) => self.toast(m, Level::Ok),
                    Err(e) => self.toast(e, Level::Error),
                }
                self.refresh_panels(sid);
            }
            AppEvent::TransferDone { sid, tid, result } => {
                if let Some(s) = self.sess(sid)
                    && let Some(v) = &mut s.sftp
                    && let Some(tr) = v.transfers.iter_mut().find(|x| x.id == tid)
                {
                    tr.finished = Some((Instant::now(), result.clone()));
                    let label = tr.label.clone();
                    let bytes = tr.progress.done.load(Ordering::Relaxed);
                    let secs = tr.started.elapsed().as_secs_f64().max(0.001);
                    match &result {
                        Ok(()) => self.toast(
                            format!(
                                "{label} — {} ({})",
                                util::human_bytes(bytes),
                                util::human_rate(bytes as f64 / secs)
                            ),
                            Level::Ok,
                        ),
                        Err(e) => self.toast(format!("{label}: {e}"), Level::Error),
                    }
                }
                self.refresh_panels(sid);
            }
            AppEvent::EditReady { sid, local, remote, edit } => {
                let before = std::fs::metadata(&local).and_then(|m| m.modified()).ok();
                let program = if edit {
                    self.settings.editor_cmd()
                } else {
                    std::env::var("PAGER").unwrap_or_else(|_| "less".into())
                };
                let after = if edit { After::Upload { sid, remote, before } } else { After::Cleanup };
                self.suspend = Some(Suspend { program, file: local, after });
            }
            AppEvent::Toast(m, l) => self.toast(m, l),
        }
    }

    pub fn after_suspend(&mut self, after: After, file: PathBuf) {
        let cleanup = |f: &PathBuf| {
            if let Some(dir) = f.parent() {
                let _ = std::fs::remove_dir_all(dir);
            }
        };
        match after {
            After::Cleanup => cleanup(&file),
            After::RefreshLocal(sid) => self.refresh_panels(sid),
            After::Upload { sid, remote, before } => {
                let now = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
                if now == before {
                    cleanup(&file);
                    self.toast(t("edit.unchanged"), Level::Info);
                    return;
                }
                let data = std::fs::read(&file);
                cleanup(&file);
                let Some(sftp) = self.sess(sid).and_then(|s| s.sftp.as_ref()).and_then(|v| v.sftp.clone()) else {
                    return;
                };
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let res = match data {
                        Ok(data) => sftp
                            .write(remote.clone(), &data)
                            .await
                            .map(|_| format!("{} {remote}", t("edit.uploaded")))
                            .map_err(|e| e.to_string()),
                        Err(e) => Err(e.to_string()),
                    };
                    tx.send(AppEvent::OpDone { sid, result: res }).ok();
                });
            }
        }
    }

    pub fn on_tick(&mut self) {
        self.tick += 1;
        let now = Instant::now();
        self.toasts
            .retain(|t| now.duration_since(t.at) < Duration::from_secs(if t.level == Level::Error { 7 } else { 4 }));
        for s in &mut self.sessions {
            let rx = s.stats.rx.load(Ordering::Relaxed);
            let tx = s.stats.tx.load(Ordering::Relaxed);
            match s.rate.last {
                Some((at, prx, ptx)) if now.duration_since(at) >= Duration::from_secs(1) => {
                    let dt = now.duration_since(at).as_secs_f64();
                    s.rate.rx_bps = (rx - prx) as f64 / dt;
                    s.rate.tx_bps = (tx - ptx) as f64 / dt;
                    s.rate.rx_hist.push_back(s.rate.rx_bps as u64 + 1);
                    while s.rate.rx_hist.len() > 30 {
                        s.rate.rx_hist.pop_front();
                    }
                    s.rate.last = Some((now, rx, tx));
                }
                None => s.rate.last = Some((now, rx, tx)),
                _ => {}
            }
            if let Some(v) = &mut s.sftp {
                for tr in &mut v.transfers {
                    if tr.finished.is_none() {
                        tr.sample();
                    }
                }
                v.transfers.retain(|tr| {
                    tr.finished.as_ref().is_none_or(|(at, _)| now.duration_since(*at) < Duration::from_secs(6))
                });
            }
        }
        if now.duration_since(self.last_rtt) >= Duration::from_secs(3) {
            self.last_rtt = now;
            for s in &self.sessions {
                if let (Some(h), SessState::Connected) = (&s.handle, &s.state) {
                    let (h, tx, sid) = (h.clone(), self.tx.clone(), s.id);
                    tokio::spawn(async move {
                        if let Some(rtt) = ssh::measure_rtt(&h).await {
                            tx.send(AppEvent::Rtt { sid, rtt }).ok();
                        }
                    });
                }
            }
        }
        if self.settings.auto_lock_mins > 0
            && self.vault.is_unlocked()
            && now.duration_since(self.last_activity) > Duration::from_secs(self.settings.auto_lock_mins * 60)
        {
            self.vault.lock();
            self.toast(t("vault.autolocked"), Level::Info);
        }
    }

    fn on_input(&mut self, e: Event) {
        match e {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                self.last_activity = Instant::now();
                self.on_key(k);
            }
            Event::Paste(s) => {
                self.last_activity = Instant::now();
                if let Some(d) = self.dialogs.last_mut() {
                    match d {
                        Dialog::Input { input, .. } => input.insert_str(&s),
                        Dialog::Password { input, confirm, second, .. } => {
                            if *second {
                                confirm.as_mut().unwrap().insert_str(&s)
                            } else {
                                input.insert_str(&s)
                            }
                        }
                        Dialog::Host(f) => f.fields[f.focus].input.insert_str(&s),
                        _ => {}
                    }
                    return;
                }
                if self.screen == Screen::Session
                    && let Some(sess) = self.sessions.get_mut(self.active)
                {
                    match (sess.view, sess.sftp.as_mut()) {
                        (View::Sftp, Some(v)) if v.console_state == ConsoleState::Focused => {
                            if let Some(c) = &mut v.console {
                                c.paste(&s)
                            }
                        }
                        (View::Term, _) => sess.term.paste(&s),
                        _ => {}
                    }
                } else if self.filter_mode {
                    self.filter.insert_str(&s);
                    self.refresh_rows();
                }
            }
            Event::Resize(w, h) => self.size = (w, h),
            _ => {}
        }
    }

    fn on_key(&mut self, k: KeyEvent) {
        if !self.dialogs.is_empty() {
            self.dialog_key(k);
            return;
        }
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        if alt && let KeyCode::Char(c @ '1'..='9') = k.code {
            let i = c as usize - '1' as usize;
            if i < self.sessions.len() {
                self.active = i;
                self.screen = Screen::Session;
            }
            return;
        }
        match self.screen {
            Screen::Hosts => self.hosts_key(k),
            Screen::Session => self.session_key(k),
            Screen::Settings => self.settings_key(k),
        }
    }

    fn hosts_key(&mut self, k: KeyEvent) {
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.filter_mode {
            match k.code {
                KeyCode::Esc => {
                    self.filter_mode = false;
                    self.filter.clear();
                    self.refresh_rows();
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Up => {
                    self.filter_mode = false;
                    if k.code == KeyCode::Enter {
                        self.activate(ConnectMode::Tab);
                    }
                }
                _ => {
                    if self.filter.handle(k) {
                        self.cursor = 0;
                        self.refresh_rows();
                        if let Some(i) = self.rows.iter().position(|r| !r.is_group) {
                            self.cursor = i;
                        }
                    }
                }
            }
            return;
        }
        let n = self.rows.len();
        match k.code {
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Char('c') if ctrl => self.request_quit(),
            KeyCode::Esc if self.moving.is_some() => {
                self.moving = None;
                self.toast(t("move.cancel"), Level::Info);
            }
            KeyCode::Esc if !self.filter.value.is_empty() => {
                self.filter.clear();
                self.refresh_rows();
            }
            KeyCode::Up | KeyCode::Char('K') if shift => self.reorder(-1),
            KeyCode::Down | KeyCode::Char('J') if shift => self.reorder(1),
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.cursor = (self.cursor + 1).min(n.saturating_sub(1)),
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(10),
            KeyCode::PageDown => self.cursor = (self.cursor + 10).min(n.saturating_sub(1)),
            KeyCode::Home | KeyCode::Char('g') if k.code == KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = n.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l') => self.set_expanded(Some(true)),
            KeyCode::Left | KeyCode::Char('h') => {
                if let Some(r) = self.selected().cloned() {
                    let expanded = r.is_group && self.store.group(&r.id).is_some_and(|g| g.expanded);
                    if expanded {
                        self.set_expanded(Some(false));
                    } else if let Some(p) = self.store.parent_id(&r.id).filter(|p| p != ROOT_ID) {
                        self.select_id(&p);
                    }
                }
            }
            KeyCode::Char(' ') => self.set_expanded(None),
            KeyCode::Enter => {
                if self.moving.is_some() {
                    self.finish_move();
                } else {
                    self.activate(ConnectMode::Tab)
                }
            }
            KeyCode::Char('w') => self.activate(ConnectMode::Window),
            KeyCode::Char('s') => self.activate(ConnectMode::Sftp),
            KeyCode::Char('/') => self.filter_mode = true,
            KeyCode::Char('a') => self.new_host(),
            KeyCode::Char('g') | KeyCode::Char('G') => self.new_group(),
            KeyCode::Char('e') | KeyCode::F(4) => self.edit_selected(),
            KeyCode::Char('d') | KeyCode::Delete => self.delete_selected(),
            KeyCode::Char('c') => self.duplicate_selected(),
            KeyCode::Char('m') => {
                if self.moving.is_some() {
                    self.finish_move();
                } else if let Some(r) = self.selected() {
                    self.moving = Some(r.id.clone());
                    self.toast(t("move.start"), Level::Info);
                }
            }
            KeyCode::Char('p') => self.ping_selected(),
            KeyCode::Char('P') => self.ping_all(),
            KeyCode::Char('M') => self.toggle_monitor_selected(),
            KeyCode::Char('L') => {
                self.vault.lock();
                self.toast(t("vault.locked_now"), Level::Info);
            }
            KeyCode::Char('?') | KeyCode::F(1) => self.dialogs.push(Dialog::Help { scroll: 0 }),
            KeyCode::F(9) | KeyCode::Char(',') => self.screen = Screen::Settings,
            KeyCode::Tab | KeyCode::F(12) if !self.sessions.is_empty() => {
                self.active = self.active.min(self.sessions.len() - 1);
                self.screen = Screen::Session;
            }
            KeyCode::Char('i') => self.confirm_import(),
            _ => {}
        }
    }

    fn request_quit(&mut self) {
        let live = self.sessions.iter().filter(|s| matches!(s.state, SessState::Connected)).count();
        if self.settings.confirm_quit && live > 0 {
            self.dialogs.push(Dialog::Confirm {
                title: t("quit.title").into(),
                body: vec![format!("{} {live}", t("quit.body"))],
                action: ConfirmAction::Quit,
                danger: true,
            });
        } else {
            self.quit = true;
        }
    }

    fn activate(&mut self, mode: ConnectMode) {
        let Some(r) = self.selected().cloned() else { return };
        if r.is_group {
            if mode == ConnectMode::Tab {
                self.set_expanded(None);
            }
            return;
        }
        self.connect(&r.id, mode);
    }

    fn set_expanded(&mut self, v: Option<bool>) {
        let Some(r) = self.selected().cloned() else { return };
        if !r.is_group {
            return;
        }
        if let Some(g) = self.store.group_mut(&r.id) {
            g.expanded = v.unwrap_or(!g.expanded);
        }
        self.save_store();
        self.refresh_rows();
    }

    fn target_group(&self) -> String {
        match self.selected() {
            Some(r) if r.is_group => r.id.clone(),
            Some(r) => self.store.parent_id(&r.id).unwrap_or_else(|| ROOT_ID.into()),
            None => ROOT_ID.into(),
        }
    }

    fn new_host(&mut self) {
        let g = self.target_group();
        self.dialogs.push(Dialog::Host(Box::new(HostForm::new(g, None))));
    }

    fn new_group(&mut self) {
        let parent = self.target_group();
        self.dialogs.push(Dialog::Input {
            title: t("group.new").into(),
            prompt: t("group.name").into(),
            input: TextInput::default(),
            purpose: InputPurpose::Group { id: None, parent },
        });
    }

    fn edit_selected(&mut self) {
        let Some(r) = self.selected().cloned() else { return };
        if r.is_group {
            let name = self.store.group(&r.id).map(|g| g.name.clone()).unwrap_or_default();
            self.dialogs.push(Dialog::Input {
                title: t("group.rename").into(),
                prompt: t("group.name").into(),
                input: TextInput::new(name),
                purpose: InputPurpose::Group { id: Some(r.id), parent: String::new() },
            });
        } else if let Some(h) = self.store.host(&r.id) {
            let g = self.store.parent_id(&r.id).unwrap_or_else(|| ROOT_ID.into());
            let form = HostForm::new(g, Some(h));
            self.dialogs.push(Dialog::Host(Box::new(form)));
        }
    }

    fn delete_selected(&mut self) {
        let Some(r) = self.selected().cloned() else { return };
        let body = if r.is_group {
            let g = self.store.group(&r.id);
            let n = g.map(|g| g.host_ids().len()).unwrap_or(0);
            vec![
                format!("{} «{}»", t("del.group"), g.map(|g| g.name.as_str()).unwrap_or("")),
                format!("{}: {n}", t("del.group_hosts")),
            ]
        } else {
            let h = self.store.host(&r.id);
            vec![
                format!("{} «{}»", t("del.host"), h.map(|h| h.name.as_str()).unwrap_or("")),
                h.map(|h| h.target()).unwrap_or_default(),
            ]
        };
        self.dialogs.push(Dialog::Confirm {
            title: t("del.title").into(),
            body,
            action: ConfirmAction::DeleteNode(r.id),
            danger: true,
        });
    }

    fn duplicate_selected(&mut self) {
        let Some(r) = self.selected().cloned() else { return };
        if r.is_group {
            return;
        }
        let Some(h) = self.store.host(&r.id).cloned() else { return };
        let mut c = h.clone();
        c.id = util::new_id();
        c.name = format!("{} {}", h.name, t("dup.suffix"));
        c.connect_count = 0;
        c.last_connected = None;
        let reseal = |aad_old: &str, aad_new: &str, s: &Option<String>, v: &Vault| -> Option<String> {
            let s = s.as_ref()?;
            let p = v.open(aad_old, s).ok()?;
            v.seal(aad_new, &p).ok()
        };
        c.secret = reseal(&h.id, &c.id, &h.secret, &self.vault);
        c.key_secret = reseal(&format!("{}:key", h.id), &format!("{}:key", c.id), &h.key_secret, &self.vault);
        if (h.secret.is_some() && c.secret.is_none()) || (h.key_secret.is_some() && c.key_secret.is_none()) {
            self.toast(t("dup.no_secrets"), Level::Warn);
        }
        let pid = self.store.parent_id(&r.id).unwrap_or_else(|| ROOT_ID.into());
        let id = c.id.clone();
        let _ = self.store.insert(&pid, Node::Host(c));
        self.save_store();
        self.refresh_rows();
        self.select_id(&id);
        self.push_monitor();
    }

    fn reorder(&mut self, d: isize) {
        let Some(r) = self.selected().cloned() else { return };
        self.store.reorder(&r.id, d);
        self.save_store();
        self.refresh_rows();
        self.select_id(&r.id);
    }

    fn finish_move(&mut self) {
        let Some(id) = self.moving.take() else { return };
        let target = self.target_group();
        if target == id || self.store.parent_id(&id).as_deref() == Some(target.as_str()) {
            self.toast(t("move.cancel"), Level::Info);
            return;
        }
        let Some(node) = self.store.remove(&id) else { return };
        let old_parent_ok = match self.store.insert(&target, node) {
            Ok(()) => true,
            Err(node) => {
                let _ = self.store.insert(ROOT_ID, node);
                self.toast(t("move.invalid"), Level::Error);
                false
            }
        };
        if let Some(g) = self.store.group_mut(&target) {
            g.expanded = true;
        }
        self.save_store();
        self.refresh_rows();
        self.select_id(&id);
        if old_parent_ok {
            self.toast(t("move.done"), Level::Ok);
        }
    }

    fn selected_hosts(&self) -> Vec<Host> {
        let Some(r) = self.selected() else { return vec![] };
        if r.is_group {
            self.store
                .group(&r.id)
                .map(|g| g.host_ids().iter().filter_map(|id| self.store.host(id).cloned()).collect())
                .unwrap_or_default()
        } else {
            self.store.host(&r.id).cloned().into_iter().collect()
        }
    }

    fn ping_hosts(&mut self, hosts: Vec<Host>) {
        let manual = hosts.len() == 1;
        let timeout = Duration::from_millis(self.settings.ping_timeout_ms);
        for h in &hosts {
            monitor::spawn_probe(
                Target { id: h.id.clone(), address: h.address.clone(), port: h.port },
                self.settings.ping_method,
                timeout,
                manual,
                self.tx.clone(),
            );
        }
        if !manual {
            self.toast(format!("{} {}", t("ping.sent"), hosts.len()), Level::Info);
        }
    }

    fn ping_selected(&mut self) {
        let h = self.selected_hosts();
        self.ping_hosts(h);
    }

    fn ping_all(&mut self) {
        let h: Vec<Host> = self.store.all_hosts().into_iter().cloned().collect();
        self.ping_hosts(h);
    }

    fn toggle_monitor_selected(&mut self) {
        let ids: Vec<String> = self.selected_hosts().iter().map(|h| h.id.clone()).collect();
        if ids.is_empty() {
            return;
        }
        let on = !self.store.host(&ids[0]).is_some_and(|h| h.monitor);
        for id in &ids {
            if let Some(h) = self.store.host_mut(id) {
                h.monitor = on;
            }
        }
        self.save_store();
        self.push_monitor();
        self.toast(if on { t("monitor.on") } else { t("monitor.off") }, Level::Info);
    }

    fn confirm_import(&mut self) {
        self.dialogs.push(Dialog::Confirm {
            title: t("import.title").into(),
            body: vec![t("import.body").into()],
            action: ConfirmAction::ImportSsh,
            danger: false,
        });
    }

    pub fn connect(&mut self, host_id: &str, mode: ConnectMode) {
        let Some(host) = self.store.host(host_id).cloned() else { return };
        let use_pw = host.secret.is_some() && matches!(host.auth, AuthKind::Password | AuthKind::Auto);
        let use_kp = host.key_secret.is_some() && matches!(host.auth, AuthKind::Key | AuthKind::Auto);
        if (use_pw || use_kp) && !self.vault.is_unlocked() {
            self.ask_unlock(Pending::Connect { host_id: host_id.into(), mode });
            return;
        }
        let mut creds = Creds::default();
        if use_pw {
            match self.vault.open(&host.id, host.secret.as_ref().unwrap()) {
                Ok(p) => creds.password = Some(p),
                Err(e) => return self.toast(format!("{}: {e}", t("err.decrypt")), Level::Error),
            }
        }
        if use_kp {
            match self.vault.open(&format!("{}:key", host.id), host.key_secret.as_ref().unwrap()) {
                Ok(p) => creds.key_pass = Some(p),
                Err(e) => return self.toast(format!("{}: {e}", t("err.decrypt")), Level::Error),
            }
        }
        if host.auth == AuthKind::Ask || (host.auth == AuthKind::Password && creds.password.is_none()) {
            self.dialogs.push(Dialog::Password {
                title: format!("{} {}", t("pw.login_title"), host.name),
                prompt: format!("{} {}", t("pw.login_prompt"), host.target()),
                input: TextInput::masked(),
                confirm: None,
                second: false,
                purpose: PwPurpose::Connect { host_id: host.id.clone(), mode },
                error: None,
            });
            return;
        }
        self.connect_with(host, creds, mode);
    }

    pub fn connect_with(&mut self, host: Host, creds: Creds, mode: ConnectMode) {
        if let Some(h) = self.store.host_mut(&host.id) {
            h.last_connected = Some(chrono::Utc::now().timestamp());
            h.connect_count += 1;
        }
        if !self.single {
            self.save_store();
        }
        match mode {
            ConnectMode::Window => {
                if let Err(e) = self.spawn_window(&host, creds) {
                    self.toast(format!("{}: {e:#}", t("err.window")), Level::Error);
                } else {
                    self.toast(format!("{} {}", t("window.opened"), host.name), Level::Ok);
                }
            }
            ConnectMode::Tab | ConnectMode::Sftp => self.open_session(host, creds, mode == ConnectMode::Sftp),
        }
    }

    fn spawn_window(&mut self, host: &Host, creds: Creds) -> anyhow::Result<()> {
        use std::os::unix::process::CommandExt;
        if self.handoff.is_none() {
            self.handoff = Some(Handoff::start()?);
        }
        let ho = self.handoff.as_ref().unwrap();
        let token = ho.issue(&host.id, creds.password.clone(), creds.key_pass.clone());
        let exe = std::env::current_exe()?.to_string_lossy().into_owned();
        let cmd = vec![exe, "connect".into(), host.id.clone()];
        let title = format!("rsh · {}", host.name);
        let (prog, args) = terminal_command(&self.settings.terminal_cmd, &title, &cmd)?;
        std::process::Command::new(prog)
            .args(args)
            .env(handoff::ENV_SOCK, &ho.path)
            .env(handoff::ENV_TOKEN, token)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0)
            .spawn()?;
        Ok(())
    }

    fn open_session(&mut self, host: Host, creds: Creds, sftp: bool) {
        let sid = self.next_sid;
        self.next_sid += 1;
        let (cols, rows) = self.term_dims();
        let stats = Arc::new(Stats::default());
        let sess = Session {
            id: sid,
            host_id: host.id.clone(),
            title: if host.name.is_empty() { host.address.clone() } else { host.name.clone() },
            target: host.target(),
            state: SessState::Connecting(t("stage.start").into()),
            handle: None,
            info: None,
            term: TermPane::new(rows, cols, self.settings.scrollback),
            view: if sftp { View::Sftp } else { View::Term },
            show_diag: self.settings.show_diag_default,
            sftp: None,
            stats: stats.clone(),
            rate: Rate::default(),
            rtt: None,
            rtt_hist: VecDeque::new(),
            started: Instant::now(),
            connected_at: None,
            want_sftp: sftp,
            creds: creds.clone(),
            startup_cmd: host.startup_cmd.clone(),
        };
        self.sessions.push(sess);
        self.active = self.sessions.len() - 1;
        self.screen = Screen::Session;
        if sftp {
            self.ensure_sftp_view(sid);
        }
        self.spawn_connect(sid, host, creds, stats, cols, rows);
    }

    fn spawn_connect(&self, sid: Sid, host: Host, creds: Creds, stats: Arc<Stats>, cols: u16, rows: u16) {
        let tx = self.tx.clone();
        let settings = self.settings.clone();
        tokio::spawn(async move {
            let stx = tx.clone();
            let res = ssh::connect(&host, &creds, &settings, Some(tx.clone()), stats, |s| {
                stx.send(AppEvent::Stage { sid, stage: s.to_string() }).ok();
            })
            .await;
            match res {
                Ok((handle, info)) => {
                    let handle = Arc::new(handle);
                    tx.send(AppEvent::Connected { sid, handle: handle.clone(), info }).ok();
                    match ssh::open_pty(&handle, cols, rows, &settings.term_type).await {
                        Ok(ch) => {
                            let ptx = ssh::spawn_pump(ch, sid, Pane::Main, tx.clone());
                            tx.send(AppEvent::PtyReady { sid, pane: Pane::Main, tx: ptx }).ok();
                        }
                        Err(e) => {
                            tx.send(AppEvent::ConnectFailed { sid, err: format!("PTY: {e:#}") }).ok();
                        }
                    }
                }
                Err(e) => {
                    tx.send(AppEvent::ConnectFailed { sid, err: format!("{e:#}") }).ok();
                }
            }
        });
    }

    fn reconnect(&mut self, idx: usize) {
        let Some(s) = self.sessions.get_mut(idx) else { return };
        let Some(host) = self.store.host(&s.host_id).cloned() else { return };
        let (rows, cols) = s.term.size;
        s.state = SessState::Connecting(t("stage.start").into());
        s.handle = None;
        s.info = None;
        s.term = TermPane::new(rows, cols, self.settings.scrollback);
        s.stats = Arc::new(Stats::default());
        s.rate = Rate::default();
        s.started = Instant::now();
        if let Some(v) = &mut s.sftp {
            v.sftp = None;
            v.console = None;
            v.console_state = ConsoleState::Hidden;
        }
        s.want_sftp = s.sftp.is_some();
        let (sid, creds, stats) = (s.id, s.creds.clone(), s.stats.clone());
        self.spawn_connect(sid, host, creds, stats, cols, rows);
    }

    fn close_session(&mut self, sid: Sid) {
        let Some(i) = self.session_idx(sid) else { return };
        let s = self.sessions.remove(i);
        self.push_monitor();
        if let Some(tx) = &s.term.tx {
            let _ = tx.send(PtyCmd::Close);
        }
        if let Some(h) = s.handle.clone() {
            tokio::spawn(async move {
                let _ = h.disconnect(russh::Disconnect::ByApplication, "bye", "en").await;
            });
        }
        if self.sessions.is_empty() {
            if self.single {
                self.quit = true;
            }
            self.screen = Screen::Hosts;
            self.active = 0;
        } else {
            self.active = self.active.min(self.sessions.len() - 1);
        }
    }

    fn session_key(&mut self, k: KeyEvent) {
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let Some(s) = self.sessions.get_mut(self.active) else {
            self.screen = Screen::Hosts;
            return;
        };
        let sid = s.id;
        match k.code {
            KeyCode::F(12) => {
                self.screen = Screen::Hosts;
                return;
            }
            KeyCode::F(1) => {
                s.show_diag = !s.show_diag;
                return;
            }
            KeyCode::F(2) => {
                if s.view == View::Term {
                    s.view = View::Sftp;
                    self.ensure_sftp_view(sid);
                    let has = self.sess(sid).is_some_and(|s| s.sftp.as_ref().is_some_and(|v| v.sftp.is_some()));
                    if !has {
                        self.open_sftp(sid);
                    }
                } else {
                    s.view = View::Term;
                }
                return;
            }
            KeyCode::Left if alt => {
                self.active = (self.active + self.sessions.len() - 1) % self.sessions.len();
                return;
            }
            KeyCode::Right if alt => {
                self.active = (self.active + 1) % self.sessions.len();
                return;
            }
            _ => {}
        }
        let finished = matches!(s.state, SessState::Closed(_) | SessState::Failed(_));
        if s.view == View::Sftp {
            self.sftp_key(sid, k);
            return;
        }
        if finished {
            match k.code {
                KeyCode::Enter | KeyCode::Char('r') => self.reconnect(self.active),
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('x') => self.close_session(sid),
                _ => {}
            }
            return;
        }
        if matches!(s.state, SessState::Connecting(_)) {
            if k.code == KeyCode::Esc {
                self.close_session(sid);
            }
            return;
        }
        match k.code {
            KeyCode::PageUp if shift => s.term.scroll_by(s.term.size.0 as isize / 2),
            KeyCode::PageDown if shift => s.term.scroll_by(-(s.term.size.0 as isize / 2)),
            _ => s.term.key(k),
        }
    }

    fn ensure_sftp_view(&mut self, sid: Sid) {
        let hidden = self.settings.sftp_show_hidden;
        let Some(s) = self.sess(sid) else { return };
        if s.sftp.is_some() {
            return;
        }
        let local = std::env::current_dir()
            .ok()
            .filter(|p| p != &PathBuf::from("/"))
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("/"))
            .to_string_lossy()
            .into_owned();
        let mut lp = Panel::new(Side::Local, local.clone(), hidden);
        lp.home = dirs::home_dir().map(|h| h.to_string_lossy().into_owned());
        match sftp::list_local(&local) {
            Ok(e) => lp.set_entries(local, e, None),
            Err(e) => lp.error = Some(e.to_string()),
        }
        let mut rp = Panel::new(Side::Remote, String::new(), hidden);
        rp.loading = true;
        s.sftp = Some(SftpView {
            sftp: None,
            panels: [lp, rp],
            active: 1,
            console: None,
            console_state: ConsoleState::Hidden,
            transfers: vec![],
            filter_mode: false,
            error: None,
        });
    }

    fn open_sftp(&mut self, sid: Sid) {
        let Some(s) = self.sess(sid) else { return };
        let Some(h) = s.handle.clone() else {
            s.want_sftp = true;
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match ssh::open_sftp(&h).await {
                Ok(sftp) => {
                    let home = sftp.canonicalize(".").await.unwrap_or_else(|_| "/".into());
                    tx.send(AppEvent::SftpReady { sid, sftp: Arc::new(sftp), home }).ok();
                }
                Err(e) => {
                    tx.send(AppEvent::SftpFailed { sid, err: format!("{e:#}") }).ok();
                }
            }
        });
    }

    fn navigate(&mut self, sid: Sid, idx: usize, path: String, focus: Option<String>) {
        let tx = self.tx.clone();
        let Some(v) = self.sess(sid).and_then(|s| s.sftp.as_mut()) else { return };
        let sftp = v.sftp.clone();
        let p = &mut v.panels[idx];
        match p.side {
            Side::Local => match sftp::list_local(&path) {
                Ok(e) => {
                    if p.path != path {
                        p.history.push(p.path.clone());
                    }
                    p.set_entries(path, e, focus.as_deref())
                }
                Err(e) => {
                    let msg = format!("{path}: {e}");
                    self.toast(msg, Level::Error);
                }
            },
            Side::Remote => {
                let Some(sftp) = sftp else { return };
                p.loading = true;
                tokio::spawn(async move {
                    let result = sftp::list_remote(&sftp, &path).await.map_err(|e| format!("{path}: {e}"));
                    tx.send(AppEvent::RemoteList { sid, path, result, focus }).ok();
                });
            }
        }
    }

    fn refresh_panels(&mut self, sid: Sid) {
        let Some(v) = self.sess(sid).and_then(|s| s.sftp.as_ref()) else { return };
        let (lp, rp) = (v.panels[0].path.clone(), v.panels[1].path.clone());
        self.navigate(sid, 0, lp, None);
        if !rp.is_empty() {
            self.navigate(sid, 1, rp, None);
        }
    }

    fn sftp_key(&mut self, sid: Sid, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let Some(s) = self.sess(sid) else { return };
        let Some(v) = s.sftp.as_mut() else { return };
        match k.code {
            KeyCode::F(3) => {
                v.console_state = match v.console_state {
                    ConsoleState::Hidden => ConsoleState::Focused,
                    ConsoleState::Focused => ConsoleState::Visible,
                    ConsoleState::Visible => ConsoleState::Hidden,
                };
                if v.console_state == ConsoleState::Focused && v.console.is_none() {
                    self.open_console(sid);
                }
                return;
            }
            KeyCode::F(10) => {
                self.screen = Screen::Hosts;
                return;
            }
            _ => {}
        }
        if v.console_state == ConsoleState::Focused {
            if let Some(c) = &mut v.console {
                match k.code {
                    KeyCode::PageUp if shift => c.scroll_by(c.size.0 as isize / 2),
                    KeyCode::PageDown if shift => c.scroll_by(-(c.size.0 as isize / 2)),
                    _ => c.key(k),
                }
            }
            return;
        }
        if v.filter_mode {
            let p = v.panel();
            match k.code {
                KeyCode::Esc => {
                    p.filter.clear();
                    p.rebuild();
                    v.filter_mode = false;
                }
                KeyCode::Enter | KeyCode::Up | KeyCode::Down | KeyCode::Tab => v.filter_mode = false,
                KeyCode::Backspace => {
                    p.filter.pop();
                    p.rebuild();
                }
                KeyCode::Char(c) if !ctrl => {
                    p.filter.push(c);
                    p.cursor = 0;
                    p.rebuild();
                }
                _ => {}
            }
            return;
        }
        let page = (self.size.1 as isize / 2).max(5);
        let v = self.sess(sid).unwrap().sftp.as_mut().unwrap();
        let idx = v.active;
        match k.code {
            KeyCode::Tab | KeyCode::BackTab => v.active = 1 - v.active,
            KeyCode::Up => v.panel().move_cursor(-1),
            KeyCode::Down => v.panel().move_cursor(1),
            KeyCode::PageUp => v.panel().move_cursor(-page),
            KeyCode::PageDown => v.panel().move_cursor(page),
            KeyCode::Home => v.panel().cursor = 0,
            KeyCode::End => {
                let p = v.panel();
                p.cursor = p.view.len().saturating_sub(1)
            }
            KeyCode::Insert | KeyCode::Char(' ') => v.panel().toggle_mark(),
            KeyCode::Char('*') => v.panel().invert_marks(),
            KeyCode::Char('a') if ctrl => {
                let p = v.panel();
                let all = p.marked.len() + 1 < p.view.len();
                p.mark_all(all);
            }
            KeyCode::Char('/') => v.filter_mode = true,
            KeyCode::Char('h') if ctrl => {
                let p = v.panel();
                p.show_hidden = !p.show_hidden;
                p.rebuild();
            }
            KeyCode::Char('.') => {
                let p = v.panel();
                p.show_hidden = !p.show_hidden;
                p.rebuild();
            }
            KeyCode::F(9) => {
                let p = v.panel();
                if shift {
                    p.reverse = !p.reverse;
                } else {
                    p.sort = p.sort.next();
                }
                p.rebuild();
            }
            KeyCode::Char('x') if ctrl => {
                for tr in &v.transfers {
                    tr.progress.cancel.store(true, Ordering::Relaxed);
                }
            }
            KeyCode::Enter => {
                let p = &v.panels[idx];
                let Some(e) = p.current().cloned() else { return };
                if e.is_dir {
                    let (path, focus) =
                        if e.name == ".." { (p.parent(), Some(p.dir_name())) } else { (p.join(&e.name), None) };
                    self.navigate(sid, idx, path, focus);
                } else {
                    self.open_file(sid, idx, e, false);
                }
            }
            KeyCode::Backspace | KeyCode::Left if !shift => {
                let p = &v.panels[idx];
                if p.path != "/" {
                    let (path, focus) = (p.parent(), Some(p.dir_name()));
                    self.navigate(sid, idx, path, focus);
                }
            }
            KeyCode::Char('-') => {
                let p = &mut v.panels[idx];
                if let Some(prev) = p.history.pop() {
                    let len = p.history.len();
                    self.navigate(sid, idx, prev, None);
                    if let Some(v) = self.sess(sid).and_then(|s| s.sftp.as_mut()) {
                        v.panels[idx].history.truncate(len);
                    }
                }
            }
            KeyCode::Char('r') if ctrl => self.refresh_panels(sid),
            KeyCode::Char('g') if ctrl => {
                let cur = v.panels[idx].path.clone();
                self.dialogs.push(Dialog::Input {
                    title: t("sftp.goto").into(),
                    prompt: t("sftp.goto_prompt").into(),
                    input: TextInput::new(cur),
                    purpose: InputPurpose::Goto { sid },
                });
            }
            KeyCode::Char('t') if ctrl => {
                let dir = v.panels[1].path.clone();
                if let Some(c) = &v.console {
                    c.send(format!("cd {}\r", util::shell_quote(&dir)).into_bytes());
                    v.console_state = ConsoleState::Focused;
                } else {
                    v.console_state = ConsoleState::Focused;
                    self.open_console(sid);
                }
            }
            KeyCode::F(4) => {
                if let Some(e) = v.panels[idx].current().cloned().filter(|e| !e.is_dir) {
                    self.open_file(sid, idx, e, true);
                }
            }
            KeyCode::F(5) => self.ask_transfer(sid, false),
            KeyCode::F(6) if shift => self.ask_rename(sid),
            KeyCode::F(6) => self.ask_transfer(sid, true),
            KeyCode::F(7) => self.dialogs.push(Dialog::Input {
                title: t("sftp.mkdir").into(),
                prompt: t("sftp.mkdir_prompt").into(),
                input: TextInput::default(),
                purpose: InputPurpose::Mkdir { sid },
            }),
            KeyCode::F(8) | KeyCode::Delete => {
                let items = v.panels[idx].targets();
                if items.is_empty() {
                    return;
                }
                let side = if v.panels[idx].side == Side::Local { t("sftp.local") } else { t("sftp.remote") };
                let mut body = vec![format!("{} {} ({side}):", t("sftp.delete_q"), items.len())];
                body.extend(items.iter().take(6).map(|e| format!("  {} {}", if e.is_dir { "▸" } else { "·" }, e.name)));
                if items.len() > 6 {
                    body.push(format!("  … +{}", items.len() - 6));
                }
                self.dialogs.push(Dialog::Confirm {
                    title: t("sftp.delete").into(),
                    body,
                    action: ConfirmAction::SftpDelete { sid, items },
                    danger: true,
                });
            }
            _ => {}
        }
    }

    fn open_console(&mut self, sid: Sid) {
        let scrollback = self.settings.scrollback;
        let term_type = self.settings.term_type.clone();
        let Some(s) = self.sess(sid) else { return };
        let Some(h) = s.handle.clone() else { return };
        let Some(v) = &mut s.sftp else { return };
        let cols = s.term.size.1;
        let rows = (s.term.size.0 / 3).max(5);
        v.console = Some(TermPane::new(rows, cols, scrollback));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match ssh::open_pty(&h, cols, rows, &term_type).await {
                Ok(ch) => {
                    let ptx = ssh::spawn_pump(ch, sid, Pane::Console, tx.clone());
                    tx.send(AppEvent::PtyReady { sid, pane: Pane::Console, tx: ptx }).ok();
                }
                Err(e) => {
                    tx.send(AppEvent::Toast(format!("console: {e:#}"), Level::Error)).ok();
                }
            }
        });
    }

    fn open_file(&mut self, sid: Sid, idx: usize, e: Entry, edit: bool) {
        let Some(v) = self.sess(sid).and_then(|s| s.sftp.as_ref()) else { return };
        let p = &v.panels[idx];
        let path = p.join(&e.name);
        match p.side {
            Side::Local => {
                let program = if edit {
                    self.settings.editor_cmd()
                } else {
                    std::env::var("PAGER").unwrap_or_else(|_| "less".into())
                };
                self.suspend = Some(Suspend { program, file: PathBuf::from(path), after: After::RefreshLocal(sid) });
            }
            Side::Remote => {
                let Some(sftp) = v.sftp.clone() else { return };
                if e.size > 64 * 1024 * 1024 {
                    self.toast(t("edit.too_big"), Level::Warn);
                    return;
                }
                let dir = dirs::cache_dir()
                    .unwrap_or_else(std::env::temp_dir)
                    .join("rsh")
                    .join("edit")
                    .join(format!("{sid}-{}", util::new_id()));
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let res: anyhow::Result<PathBuf> = async {
                        let data = sftp.read(path.clone()).await?;
                        std::fs::create_dir_all(&dir)?;
                        std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
                        let local = dir.join(&e.name);
                        std::fs::write(&local, data)?;
                        Ok(local)
                    }
                    .await;
                    match res {
                        Ok(local) => tx.send(AppEvent::EditReady { sid, local, remote: path, edit }).ok(),
                        Err(err) => tx.send(AppEvent::Toast(format!("{}: {err:#}", e.name), Level::Error)).ok(),
                    };
                });
            }
        }
    }

    fn ask_transfer(&mut self, sid: Sid, mv: bool) {
        let Some(v) = self.sess(sid).and_then(|s| s.sftp.as_ref()) else { return };
        if v.sftp.is_none() {
            return;
        }
        let items = v.panels[v.active].targets();
        if items.is_empty() {
            return;
        }
        let dst = v.panels[1 - v.active].path.clone();
        let what = if items.len() == 1 {
            format!("«{}»", items[0].name)
        } else {
            format!("{} {}", items.len(), t("sftp.objects"))
        };
        let arrow = if v.panels[v.active].side == Side::Local { "⇡" } else { "⇣" };
        self.dialogs.push(Dialog::Input {
            title: if mv { t("sftp.move") } else { t("sftp.copy") }.into(),
            prompt: format!("{arrow} {what} {}", t("sftp.to")),
            input: TextInput::new(dst),
            purpose: InputPurpose::Transfer { sid, mv, items },
        });
    }

    fn ask_rename(&mut self, sid: Sid) {
        let Some(v) = self.sess(sid).and_then(|s| s.sftp.as_ref()) else { return };
        let Some(e) = v.panels[v.active].current().filter(|e| e.name != "..").cloned() else { return };
        self.dialogs.push(Dialog::Input {
            title: t("sftp.rename").into(),
            prompt: format!("{} «{}»", t("sftp.rename_prompt"), e.name),
            input: TextInput::new(e.name.clone()),
            purpose: InputPurpose::Rename { sid, from: e.name },
        });
    }

    fn start_transfer(&mut self, sid: Sid, mv: bool, items: Vec<Entry>, dest: String) {
        let buf = self.settings.sftp_buffer_kib;
        let tid = self.next_tid;
        self.next_tid += 1;
        let Some(v) = self.sess(sid).and_then(|s| s.sftp.as_mut()) else { return };
        let Some(sftp) = v.sftp.clone() else { return };
        let src = &v.panels[v.active];
        let dir = if src.side == Side::Local { Direction::Upload } else { Direction::Download };
        let pairs: Vec<(String, String)> = items
            .iter()
            .map(|e| {
                let s = src.join(&e.name);
                let d = match dir {
                    Direction::Upload => util::join_remote(&dest, &e.name),
                    Direction::Download => PathBuf::from(&dest).join(&e.name).to_string_lossy().into_owned(),
                };
                (s, d)
            })
            .collect();
        let label =
            if items.len() == 1 { items[0].name.clone() } else { format!("{} {}", items.len(), t("sftp.objects")) };
        let progress = Arc::new(Progress::default());
        v.transfers.push(Transfer {
            id: tid,
            dir,
            label,
            progress: progress.clone(),
            started: Instant::now(),
            finished: None,
            samples: VecDeque::new(),
        });
        v.panels[v.active].marked.clear();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let job = Job { dir, items: pairs, delete_source: mv };
            let result = sftp::run_job(sftp, job, progress, buf).await.map_err(|e| format!("{e:#}"));
            tx.send(AppEvent::TransferDone { sid, tid, result }).ok();
        });
    }

    fn sftp_delete(&mut self, sid: Sid, items: Vec<Entry>) {
        let Some(v) = self.sess(sid).and_then(|s| s.sftp.as_mut()) else { return };
        let p = &mut v.panels[v.active];
        p.marked.clear();
        let paths: Vec<String> = items.iter().map(|e| p.join(&e.name)).collect();
        match p.side {
            Side::Local => {
                let mut errs = vec![];
                for path in &paths {
                    let pb = PathBuf::from(path);
                    let r = if pb.is_dir() && !pb.is_symlink() {
                        std::fs::remove_dir_all(&pb)
                    } else {
                        std::fs::remove_file(&pb)
                    };
                    if let Err(e) = r {
                        errs.push(format!("{path}: {e}"));
                    }
                }
                let res = if errs.is_empty() {
                    Ok(format!("{} {}", t("sftp.deleted"), paths.len()))
                } else {
                    Err(errs.join("; "))
                };
                self.on_event(AppEvent::OpDone { sid, result: res });
            }
            Side::Remote => {
                let Some(sftp) = v.sftp.clone() else { return };
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let mut errs = vec![];
                    for path in &paths {
                        if let Err(e) = sftp::remove_remote(&sftp, path.clone()).await {
                            errs.push(format!("{path}: {e}"));
                        }
                    }
                    let res = if errs.is_empty() {
                        Ok(format!("{} {}", t("sftp.deleted"), paths.len()))
                    } else {
                        Err(errs.join("; "))
                    };
                    tx.send(AppEvent::OpDone { sid, result: res }).ok();
                });
            }
        }
    }

    fn sftp_simple_op(&mut self, sid: Sid, op: SimpleOp) {
        let Some(v) = self.sess(sid).and_then(|s| s.sftp.as_ref()) else { return };
        let p = &v.panels[v.active];
        match p.side {
            Side::Local => {
                let res = match &op {
                    SimpleOp::Mkdir(n) => {
                        std::fs::create_dir_all(p.join(n)).map(|_| format!("{} {n}", t("sftp.created")))
                    }
                    SimpleOp::Rename(a, b) => std::fs::rename(p.join(a), p.join(b)).map(|_| format!("{a} → {b}")),
                }
                .map_err(|e| e.to_string());
                self.on_event(AppEvent::OpDone { sid, result: res });
            }
            Side::Remote => {
                let Some(sftp) = v.sftp.clone() else { return };
                let base = p.path.clone();
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    let res = match &op {
                        SimpleOp::Mkdir(n) => sftp
                            .create_dir(util::join_remote(&base, n))
                            .await
                            .map(|_| format!("{} {n}", t("sftp.created"))),
                        SimpleOp::Rename(a, b) => sftp
                            .rename(
                                util::join_remote(&base, a),
                                if b.starts_with('/') { b.clone() } else { util::join_remote(&base, b) },
                            )
                            .await
                            .map(|_| format!("{a} → {b}")),
                    }
                    .map_err(|e| e.to_string());
                    tx.send(AppEvent::OpDone { sid, result: res }).ok();
                });
            }
        }
    }

    fn dialog_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let Some(d) = self.dialogs.last_mut() else { return };
        match d {
            Dialog::Help { scroll } => match k.code {
                KeyCode::Down | KeyCode::Char('j') => *scroll += 1,
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::PageDown => *scroll += 10,
                KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
                _ => {
                    self.dialogs.pop();
                }
            },
            Dialog::HostKey(q) => {
                let answer = match k.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => Some(true),
                    KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => Some(false),
                    _ => None,
                };
                if let Some(a) = answer {
                    if let Some(q) = q.take() {
                        let _ = q.reply.send(a);
                    }
                    self.dialogs.pop();
                }
            }
            Dialog::Confirm { .. } => match k.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    if let Some(Dialog::Confirm { action, .. }) = self.dialogs.pop() {
                        self.confirm(action);
                    }
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.dialogs.pop();
                }
                _ => {}
            },
            Dialog::Input { input, .. } => match k.code {
                KeyCode::Esc => {
                    self.dialogs.pop();
                }
                KeyCode::Enter => {
                    if let Some(Dialog::Input { input, purpose, .. }) = self.dialogs.pop() {
                        self.submit_input(purpose, input.value);
                    }
                }
                _ => {
                    input.handle(k);
                }
            },
            Dialog::Password { input, confirm, second, .. } => match k.code {
                KeyCode::Esc => {
                    let d = self.dialogs.pop();
                    if let Some(Dialog::Password { purpose: PwPurpose::CreateVault, .. }) = d
                        && self.pending.is_none()
                    {
                        self.toast(t("vault.later"), Level::Info);
                    }
                    self.pending = None;
                }
                KeyCode::Tab | KeyCode::Down | KeyCode::Up if confirm.is_some() => *second = !*second,
                KeyCode::Enter if confirm.is_some() && !*second => *second = true,
                KeyCode::Enter => self.submit_password(),
                _ => {
                    if *second {
                        confirm.as_mut().unwrap().handle(k)
                    } else {
                        input.handle(k)
                    };
                }
            },
            Dialog::Host(form) => {
                let f = &mut **form;
                match k.code {
                    KeyCode::Esc => {
                        self.dialogs.pop();
                    }
                    KeyCode::Char('s') if ctrl => self.submit_host_form(),
                    KeyCode::Char('r') if ctrl => f.reveal = !f.reveal,
                    KeyCode::Tab | KeyCode::Down => f.step(1),
                    KeyCode::BackTab | KeyCode::Up => f.step(-1),
                    KeyCode::Enter => {
                        let last_visible = (0..f.fields.len()).rev().find(|&i| f.visible(i)).unwrap_or(0);
                        if f.focus == last_visible { self.submit_host_form() } else { f.step(1) }
                    }
                    _ => {
                        let i = f.focus;
                        match f.fields[i].kind {
                            FieldKind::Choice => {
                                let n = AuthKind::ALL.len();
                                match k.code {
                                    KeyCode::Left => f.fields[i].choice = (f.fields[i].choice + n - 1) % n,
                                    KeyCode::Right | KeyCode::Char(' ') => {
                                        f.fields[i].choice = (f.fields[i].choice + 1) % n
                                    }
                                    _ => {}
                                }
                            }
                            FieldKind::Toggle => {
                                if matches!(k.code, KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')) {
                                    f.fields[i].on = !f.fields[i].on;
                                }
                            }
                            _ => {
                                f.fields[i].input.handle(k);
                            }
                        }
                    }
                }
            }
        }
    }

    fn confirm(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::Quit => self.quit = true,
            ConfirmAction::DeleteNode(id) => {
                self.store.remove(&id);
                self.status.remove(&id);
                self.save_store();
                self.refresh_rows();
                self.push_monitor();
                self.toast(t("del.done"), Level::Ok);
            }
            ConfirmAction::SftpDelete { sid, items } => self.sftp_delete(sid, items),
            ConfirmAction::ImportSsh => {
                let path = dirs::home_dir().unwrap_or_default().join(".ssh/config");
                match self.store.import_ssh_config(&path) {
                    Ok(n) => {
                        self.save_store();
                        self.refresh_rows();
                        self.push_monitor();
                        self.toast(format!("{} {n}", t("import.done")), Level::Ok);
                    }
                    Err(e) => self.toast(format!("{}: {e}", path.display()), Level::Error),
                }
            }
        }
    }

    fn submit_input(&mut self, purpose: InputPurpose, value: String) {
        let value = value.trim().to_string();
        match purpose {
            InputPurpose::Group { id, parent } => {
                if value.is_empty() {
                    return;
                }
                match id {
                    Some(id) => {
                        if let Some(g) = self.store.group_mut(&id) {
                            g.name = value;
                        }
                    }
                    None => {
                        let g = Group { name: value, ..Group::default() };
                        let gid = g.id.clone();
                        let _ = self.store.insert(&parent, Node::Group(g));
                        if let Some(p) = self.store.group_mut(&parent) {
                            p.expanded = true;
                        }
                        self.save_store();
                        self.refresh_rows();
                        self.select_id(&gid);
                        return;
                    }
                }
                self.save_store();
                self.refresh_rows();
            }
            InputPurpose::Mkdir { sid } if !value.is_empty() => self.sftp_simple_op(sid, SimpleOp::Mkdir(value)),
            InputPurpose::Rename { sid, from } if !value.is_empty() && value != from => {
                self.sftp_simple_op(sid, SimpleOp::Rename(from, value))
            }
            InputPurpose::Transfer { sid, mv, items } if !value.is_empty() => {
                if mv && items.len() == 1 && !value.contains('/') {
                    self.sftp_simple_op(sid, SimpleOp::Rename(items[0].name.clone(), value));
                } else {
                    self.start_transfer(sid, mv, items, value);
                }
            }
            InputPurpose::Goto { sid } if !value.is_empty() => {
                let Some(v) = self.sess(sid).and_then(|s| s.sftp.as_ref()) else { return };
                let idx = v.active;
                let path = if v.panels[idx].side == Side::Local {
                    ssh::expand_tilde(&value).to_string_lossy().into_owned()
                } else {
                    value
                };
                self.navigate(sid, idx, path, None);
            }
            InputPurpose::TerminalCmd => {
                self.settings.terminal_cmd = if value.is_empty() { "auto".into() } else { value };
                self.save_settings();
            }
            InputPurpose::Editor => {
                self.settings.editor = value;
                self.save_settings();
            }
            _ => {}
        }
    }

    fn submit_password(&mut self) {
        let Some(Dialog::Password { input, confirm, purpose, error, second, .. }) = self.dialogs.last_mut() else {
            return;
        };
        let pw = Zeroizing::new(input.value.clone());
        if let Some(c) = confirm
            && c.value != *pw
        {
            *error = Some(t("pw.mismatch").into());
            c.clear();
            *second = true;
            return;
        }
        let res: Result<(), String> = match purpose {
            PwPurpose::CreateVault => self.vault.create(&pw).map_err(|e| e.to_string()),
            PwPurpose::Unlock => self.vault.unlock(&pw).map_err(|e| e.to_string()),
            PwPurpose::NewMaster => self.vault.change_password(&pw, &mut self.store).map_err(|e| e.to_string()),
            PwPurpose::Connect { .. } => Ok(()),
        };
        if let Err(e) = res {
            if let Some(Dialog::Password { input, error, .. }) = self.dialogs.last_mut() {
                *error = Some(e);
                input.clear();
            }
            return;
        }
        let Some(Dialog::Password { purpose, .. }) = self.dialogs.pop() else { return };
        match purpose {
            PwPurpose::CreateVault => {
                self.toast(t("vault.created"), Level::Ok);
                self.run_pending();
            }
            PwPurpose::Unlock => {
                self.toast(t("vault.unlocked"), Level::Ok);
                self.run_pending();
            }
            PwPurpose::NewMaster => self.toast(t("vault.changed"), Level::Ok),
            PwPurpose::Connect { host_id, mode } => {
                if let Some(host) = self.store.host(&host_id).cloned() {
                    let creds = Creds { password: Some(pw), key_pass: None };
                    self.connect_with(host, creds, mode);
                }
            }
        }
    }

    fn submit_host_form(&mut self) {
        let Some(Dialog::Host(f)) = self.dialogs.last_mut() else { return };
        f.error = None;
        let mut name = f.value(F_NAME);
        let mut addr = f.value(F_ADDR);
        let mut user = f.value(F_USER);
        let mut port_s = f.value(F_PORT);
        if let Some((u, rest)) = addr.clone().split_once('@') {
            user = u.to_string();
            addr = rest.to_string();
        }
        if let Some((h, p)) = addr.clone().rsplit_once(':')
            && !h.contains(':')
            && p.parse::<u16>().is_ok()
        {
            addr = h.to_string();
            port_s = p.to_string();
        }
        if addr.is_empty() {
            f.error = Some(t("form.err_addr").into());
            f.focus = F_ADDR;
            return;
        }
        let Ok(port) = port_s.parse::<u16>() else {
            f.error = Some(t("form.err_port").into());
            f.focus = F_PORT;
            return;
        };
        if name.is_empty() {
            name = addr.clone();
        }
        let auth = f.auth();
        let new_pw = (f.visible(F_PASS) && !f.fields[F_PASS].input.value.is_empty())
            .then(|| Zeroizing::new(f.fields[F_PASS].input.value.clone()));
        let new_kp = (f.visible(F_KEYPASS) && !f.fields[F_KEYPASS].input.value.is_empty())
            .then(|| Zeroizing::new(f.fields[F_KEYPASS].input.value.clone()));
        if (new_pw.is_some() || new_kp.is_some()) && !self.vault.is_unlocked() {
            self.ask_unlock(Pending::SubmitHostForm);
            return;
        }
        let Some(Dialog::Host(f)) = self.dialogs.last() else { return };
        let mut host = f.id.as_ref().and_then(|id| self.store.host(id).cloned()).unwrap_or_default();
        host.name = name;
        host.address = addr;
        host.port = port;
        host.user = if user.is_empty() { "root".into() } else { user };
        host.auth = auth;
        let kp = f.value(F_KEY);
        host.key_path = (!kp.is_empty()).then_some(kp);
        host.tags = f.value(F_TAGS).split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        host.startup_cmd = f.value(F_CMD);
        host.notes = f.value(F_NOTES);
        host.monitor = f.fields[F_MON].on;
        if let Some(pw) = &new_pw {
            match self.vault.seal(&host.id, pw) {
                Ok(s) => host.secret = Some(s),
                Err(e) => return self.toast(e.to_string(), Level::Error),
            }
        }
        if let Some(kp) = &new_kp {
            match self.vault.seal(&format!("{}:key", host.id), kp) {
                Ok(s) => host.key_secret = Some(s),
                Err(e) => return self.toast(e.to_string(), Level::Error),
            }
        }
        if matches!(auth, AuthKind::Agent | AuthKind::Ask) {
            host.secret = None;
        }
        if matches!(auth, AuthKind::Agent | AuthKind::Ask | AuthKind::Password) {
            host.key_secret = None;
        }
        let editing = f.id.is_some();
        let group = f.group.clone();
        let id = host.id.clone();
        if editing {
            if let Some(h) = self.store.host_mut(&id) {
                *h = host;
            }
        } else {
            let _ = self.store.insert(&group, Node::Host(host));
            if let Some(g) = self.store.group_mut(&group) {
                g.expanded = true;
            }
        }
        if let Some(Dialog::Host(f)) = self.dialogs.last_mut() {
            f.fields[F_PASS].input.clear();
            f.fields[F_KEYPASS].input.clear();
        }
        self.dialogs.pop();
        self.save_store();
        self.refresh_rows();
        self.select_id(&id);
        self.push_monitor();
        self.toast(t("form.saved"), Level::Ok);
        if let Some(h) = self.store.host(&id).cloned() {
            self.ping_hosts(vec![h]);
        }
    }

    pub fn setting_value(&self, it: SettingItem) -> String {
        let s = &self.settings;
        let onoff = |b: bool| if b { t("on") } else { t("off") }.to_string();
        let secs =
            |v: u64| if v >= 60 { format!("{} {}", v / 60, t("unit.min")) } else { format!("{v} {}", t("unit.sec")) };
        match it {
            SettingItem::Lang => match s.lang {
                Lang::Ru => "Русский".into(),
                Lang::En => "English".into(),
            },
            SettingItem::Theme => {
                if crate::display::tty() {
                    format!("{} (TTY: ANSI)", s.theme)
                } else {
                    s.theme.clone()
                }
            }
            SettingItem::Icons => match s.icons {
                IconSet::Unicode => "Unicode".into(),
                IconSet::Nerd => "Nerd Font".into(),
                IconSet::Emoji => "Emoji".into(),
                IconSet::Ascii => "ASCII".into(),
            },
            SettingItem::Borders => match s.border {
                BorderStyle::Rounded => t("border.rounded").into(),
                BorderStyle::Plain => t("border.plain").into(),
                BorderStyle::Thick => t("border.thick").into(),
                BorderStyle::Double => t("border.double").into(),
            },
            SettingItem::Colors => {
                let detected = match crate::display::detect_depth() {
                    crate::display::Depth::True => "Truecolor",
                    crate::display::Depth::C256 => "256",
                    crate::display::Depth::C16 => "16",
                };
                match s.color_mode {
                    _ if crate::display::tty() => "16 (TTY)".into(),
                    ColorMode::Auto => format!("{} ({detected})", t("auto")),
                    ColorMode::TrueColor => "Truecolor (24 bit)".into(),
                    ColorMode::C256 => format!("256 {}", t("unit.colors")),
                    ColorMode::C16 => format!("16 {}", t("unit.colors")),
                }
            }
            SettingItem::Tty => match s.tty_mode {
                TtyMode::Auto => {
                    format!("{} ({})", t("auto"), if crate::display::detect_tty() { t("on") } else { t("off") })
                }
                TtyMode::On => t("on").into(),
                TtyMode::Off => t("off").into(),
            },
            SettingItem::MonitorEnabled => onoff(s.monitor_enabled),
            SettingItem::MonitorInterval => secs(s.monitor_interval_secs),
            SettingItem::PingMethod => match s.ping_method {
                PingMethod::Tcp => t("ping.tcp").into(),
                PingMethod::Icmp => t("ping.icmp").into(),
            },
            SettingItem::PingTimeout => format!("{} ms", s.ping_timeout_ms),
            SettingItem::Notify => onoff(s.notify_status_change),
            SettingItem::HostKeyPolicy => match s.host_key_policy {
                HostKeyPolicy::Ask => t("hkp.ask").into(),
                HostKeyPolicy::AcceptNew => t("hkp.accept_new").into(),
                HostKeyPolicy::Strict => t("hkp.strict").into(),
            },
            SettingItem::Keepalive => {
                if s.keepalive_secs == 0 {
                    t("off").into()
                } else {
                    secs(s.keepalive_secs)
                }
            }
            SettingItem::Scrollback => format!("{} {}", s.scrollback, t("unit.lines")),
            SettingItem::ShowDiag => onoff(s.show_diag_default),
            SettingItem::ConfirmQuit => onoff(s.confirm_quit),
            SettingItem::AutoLock => {
                if s.auto_lock_mins == 0 {
                    t("off").into()
                } else {
                    format!("{} {}", s.auto_lock_mins, t("unit.min"))
                }
            }
            SettingItem::SftpHidden => onoff(s.sftp_show_hidden),
            SettingItem::SftpBuffer => format!("{} KiB", s.sftp_buffer_kib),
            SettingItem::TerminalCmd => {
                if s.terminal_cmd == "auto" {
                    format!("auto ({})", detect_terminal().unwrap_or_else(|| "?".into()))
                } else {
                    s.terminal_cmd.clone()
                }
            }
            SettingItem::Editor => {
                if s.editor.is_empty() {
                    format!("$EDITOR ({})", s.editor_cmd())
                } else {
                    s.editor.clone()
                }
            }
            SettingItem::ChangeMaster | SettingItem::LockVault | SettingItem::ImportSsh => {
                crate::display::enter().into()
            }
        }
    }

    fn cycle_setting(&mut self, it: SettingItem, d: isize) {
        fn step<T: PartialEq + Copy>(list: &[T], cur: T, d: isize) -> T {
            let i = list.iter().position(|x| *x == cur).unwrap_or(0) as isize;
            list[(i + d).rem_euclid(list.len() as isize) as usize]
        }
        let s = &mut self.settings;
        match it {
            SettingItem::Lang => {
                s.lang = if s.lang == Lang::Ru { Lang::En } else { Lang::Ru };
                i18n::set_lang(s.lang);
            }
            SettingItem::Theme => {
                let names = crate::theme::theme_names();
                let cur = names.iter().position(|n| n.eq_ignore_ascii_case(&s.theme)).unwrap_or(0) as isize;
                s.theme = names[(cur + d).rem_euclid(names.len() as isize) as usize].into();
            }
            SettingItem::Icons => {
                s.icons = step(&[IconSet::Unicode, IconSet::Nerd, IconSet::Emoji, IconSet::Ascii], s.icons, d)
            }
            SettingItem::Borders => {
                s.border = step(
                    &[BorderStyle::Rounded, BorderStyle::Plain, BorderStyle::Thick, BorderStyle::Double],
                    s.border,
                    d,
                )
            }
            SettingItem::Colors => {
                s.color_mode =
                    step(&[ColorMode::Auto, ColorMode::TrueColor, ColorMode::C256, ColorMode::C16], s.color_mode, d)
            }
            SettingItem::Tty => s.tty_mode = step(&[TtyMode::Auto, TtyMode::On, TtyMode::Off], s.tty_mode, d),
            SettingItem::MonitorEnabled => s.monitor_enabled = !s.monitor_enabled,
            SettingItem::MonitorInterval => {
                s.monitor_interval_secs = step(config::MONITOR_INTERVALS, s.monitor_interval_secs, d)
            }
            SettingItem::PingMethod => s.ping_method = step(&[PingMethod::Tcp, PingMethod::Icmp], s.ping_method, d),
            SettingItem::PingTimeout => s.ping_timeout_ms = step(config::PING_TIMEOUTS, s.ping_timeout_ms, d),
            SettingItem::Notify => s.notify_status_change = !s.notify_status_change,
            SettingItem::HostKeyPolicy => {
                s.host_key_policy =
                    step(&[HostKeyPolicy::Ask, HostKeyPolicy::AcceptNew, HostKeyPolicy::Strict], s.host_key_policy, d)
            }
            SettingItem::Keepalive => s.keepalive_secs = step(config::KEEPALIVES, s.keepalive_secs, d),
            SettingItem::Scrollback => s.scrollback = step(config::SCROLLBACKS, s.scrollback, d),
            SettingItem::ShowDiag => s.show_diag_default = !s.show_diag_default,
            SettingItem::ConfirmQuit => s.confirm_quit = !s.confirm_quit,
            SettingItem::AutoLock => s.auto_lock_mins = step(config::AUTO_LOCKS, s.auto_lock_mins, d),
            SettingItem::SftpHidden => s.sftp_show_hidden = !s.sftp_show_hidden,
            SettingItem::SftpBuffer => s.sftp_buffer_kib = step(config::BUFFERS, s.sftp_buffer_kib, d),
            SettingItem::TerminalCmd => {
                let cur = s.terminal_cmd.clone();
                self.dialogs.push(Dialog::Input {
                    title: t("set.terminal").into(),
                    prompt: t("set.terminal_prompt").into(),
                    input: TextInput::new(cur),
                    purpose: InputPurpose::TerminalCmd,
                });
                return;
            }
            SettingItem::Editor => {
                let cur = s.editor.clone();
                self.dialogs.push(Dialog::Input {
                    title: t("set.editor").into(),
                    prompt: t("set.editor_prompt").into(),
                    input: TextInput::new(cur),
                    purpose: InputPurpose::Editor,
                });
                return;
            }
            SettingItem::ChangeMaster => {
                if !self.vault.exists() {
                    self.ask_unlock(Pending::NewMaster);
                    self.pending = None;
                } else if self.vault.is_unlocked() {
                    self.pending = Some(Pending::NewMaster);
                    self.run_pending();
                } else {
                    self.ask_unlock(Pending::NewMaster);
                }
                return;
            }
            SettingItem::LockVault => {
                self.vault.lock();
                self.toast(t("vault.locked_now"), Level::Info);
                return;
            }
            SettingItem::ImportSsh => {
                self.confirm_import();
                return;
            }
        }
        crate::display::apply(&self.settings);
        self.save_settings();
        self.push_monitor();
    }

    fn settings_key(&mut self, k: KeyEvent) {
        let items = setting_items();
        match k.code {
            KeyCode::Esc | KeyCode::F(9) | KeyCode::Char('q') => self.screen = Screen::Hosts,
            KeyCode::Up | KeyCode::Char('k') => self.settings_cursor = self.settings_cursor.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.settings_cursor = (self.settings_cursor + 1).min(items.len() - 1)
            }
            KeyCode::Left | KeyCode::Char('h') => self.cycle_setting(items[self.settings_cursor], -1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Char(' ') => {
                self.cycle_setting(items[self.settings_cursor], 1)
            }
            _ => {}
        }
    }
}

enum SimpleOp {
    Mkdir(String),
    Rename(String, String),
}

const TERMINALS: &[&str] =
    &["kitty", "alacritty", "foot", "wezterm", "ghostty", "konsole", "gnome-terminal", "xfce4-terminal", "xterm"];

fn in_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

pub fn detect_terminal() -> Option<String> {
    if let Ok(t) = std::env::var("TERMINAL")
        && in_path(t.split_whitespace().next().unwrap_or(""))
    {
        return Some(t);
    }
    let term_prog = std::env::var("TERM").unwrap_or_default();
    if term_prog.contains("kitty") && in_path("kitty") {
        return Some("kitty".into());
    }
    TERMINALS.iter().find(|t| in_path(t)).map(|s| s.to_string())
}

pub fn terminal_command(setting: &str, title: &str, cmd: &[String]) -> anyhow::Result<(String, Vec<String>)> {
    if setting != "auto" && setting.contains("{cmd}") {
        let mut parts = setting.split_whitespace();
        let prog = parts.next().ok_or_else(|| anyhow::anyhow!("empty terminal command"))?.to_string();
        let mut args = vec![];
        for p in parts {
            match p {
                "{cmd}" => args.extend(cmd.iter().cloned()),
                "{title}" => args.push(title.to_string()),
                _ => args.push(p.to_string()),
            }
        }
        return Ok((prog, args));
    }
    let term = if setting == "auto" {
        detect_terminal().ok_or_else(|| anyhow::anyhow!(t("err.no_terminal")))?
    } else {
        setting.to_string()
    };
    let base = term.split('/').next_back().unwrap_or(&term).to_string();
    let title = title.to_string();
    let mut args: Vec<String> = match base.as_str() {
        "kitty" => vec!["--title".into(), title],
        "alacritty" => vec!["--title".into(), title, "-e".into()],
        "foot" => vec!["--title".into(), title],
        "wezterm" => vec!["start".into(), "--".into()],
        "ghostty" => vec![format!("--title={title}"), "-e".into()],
        "gnome-terminal" => vec!["--title".into(), title, "--".into()],
        "xfce4-terminal" => vec!["--title".into(), title, "-x".into()],
        "xterm" => vec!["-T".into(), title, "-e".into()],
        _ => vec!["-e".into()],
    };
    args.extend(cmd.iter().cloned());
    Ok((term, args))
}
