mod app;
mod config;
mod dialog;
mod display;
mod event;
mod handoff;
mod i18n;
mod monitor;
mod platform;
mod scp;
mod sftp;
mod ssh;
mod store;
mod term;
mod theme;
mod ui;
mod util;
mod vault;
mod widgets;

use anyhow::{Context, Result, anyhow, bail};
use app::{App, Suspend};
use clap::{CommandFactory, Parser, Subcommand};
use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use dialog::ConnectMode;
use event::AppEvent;
use ssh::Creds;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use store::{Host, Store};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(name = "rsh", version, about = "Beautiful SSH / SFTP / SCP commander")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = "Connect to a saved host (name, id, address) or user@host[:port]")]
    Connect {
        host: String,
        #[arg(long, help = "Open straight into the SFTP commander")]
        sftp: bool,
    },
    #[command(alias = "scp", about = "Copy files: rsh cp web-01:/etc/hosts ./  |  rsh cp ./file web-01:/tmp/")]
    Cp {
        src: String,
        dst: String,
        #[arg(long, help = "Use the legacy SCP protocol instead of SFTP")]
        scp: bool,
    },
    #[command(about = "Print the host tree")]
    List,
    #[command(about = "Import hosts from ~/.ssh/config")]
    Import,
    #[command(about = "Print a shell completion script (bash, zsh, fish, elvish, powershell)")]
    Completions { shell: clap_complete::Shell },
    #[command(about = "Print the man page", hide = true)]
    Man,
}

static INPUT_PAUSED: AtomicBool = AtomicBool::new(false);

fn spawn_input(tx: UnboundedSender<AppEvent>) {
    std::thread::spawn(move || {
        loop {
            if INPUT_PAUSED.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(30));
                continue;
            }
            match crossterm::event::poll(Duration::from_millis(50)) {
                Ok(true) => match crossterm::event::read() {
                    Ok(e) => {
                        if tx.send(AppEvent::Input(e)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Ok(false) => {
                    if tx.is_closed() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

enum Start {
    Saved(String, ConnectMode),
    Direct(Box<Host>, Creds, ConnectMode),
}

async fn run_tui(start: Option<Start>) -> Result<()> {
    let (tx, mut rx) = unbounded_channel();
    let mut app = App::new(tx.clone(), start.is_some())?;
    spawn_input(tx.clone());
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableBracketedPaste);
    match start {
        Some(Start::Saved(id, mode)) => app.connect(&id, mode),
        Some(Start::Direct(h, c, mode)) => app.connect_with(*h, c, mode),
        None => {}
    }
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let res: Result<()> = async {
        loop {
            terminal.draw(|f| ui::draw(f, &mut app))?;
            tokio::select! {
                ev = rx.recv() => {
                    let Some(ev) = ev else { break };
                    app.on_event(ev);
                    for _ in 0..1024 {
                        match rx.try_recv() {
                            Ok(ev) => app.on_event(ev),
                            Err(_) => break,
                        }
                    }
                }
                _ = tick.tick() => app.on_tick(),
            }
            if let Some(Suspend { program, file, after }) = app.suspend.take() {
                INPUT_PAUSED.store(true, Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(120)).await;
                let _ = execute!(std::io::stdout(), DisableBracketedPaste);
                ratatui::restore();
                let status = platform::open_with(&program, &file).status();
                terminal = ratatui::init();
                let _ = execute!(std::io::stdout(), EnableBracketedPaste);
                terminal.clear()?;
                INPUT_PAUSED.store(false, Ordering::Relaxed);
                if let Err(e) = status {
                    app.toast(format!("{program}: {e}"), event::Level::Error);
                }
                app.after_suspend(after, file);
            }
            if app.quit {
                break;
            }
        }
        Ok(())
    }
    .await;
    let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    ratatui::restore();
    res
}

fn adhoc(spec: &str) -> Option<Host> {
    let (user, rest) = spec.split_once('@')?;
    let (addr, port) = match rest.rsplit_once(':') {
        Some((a, p)) if p.parse::<u16>().is_ok() => (a, p.parse().unwrap()),
        _ => (rest, 22),
    };
    Some(Host { name: addr.into(), address: addr.into(), port, user: user.into(), monitor: false, ..Host::default() })
}

fn read_secret(prompt: &str) -> Result<Zeroizing<String>> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, read};
    eprint!("{prompt}");
    std::io::stderr().flush()?;
    crossterm::terminal::enable_raw_mode()?;
    let mut s = Zeroizing::new(String::new());
    let res = loop {
        match read() {
            Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Enter => break Ok(()),
                KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => break Err(anyhow!("cancelled")),
                KeyCode::Esc => break Err(anyhow!("cancelled")),
                KeyCode::Backspace => {
                    s.pop();
                }
                KeyCode::Char(c) => s.push(c),
                _ => {}
            },
            Ok(_) => {}
            Err(e) => break Err(e.into()),
        }
    };
    crossterm::terminal::disable_raw_mode()?;
    eprintln!();
    res.map(|_| s)
}

fn cli_creds(h: &Host) -> Result<Creds> {
    let mut creds = Creds::default();
    if h.secret.is_some() || h.key_secret.is_some() {
        let mut v = vault::Vault::load();
        let pw = read_secret(&format!("🔒 {}: ", i18n::t("pw.unlock_prompt")))?;
        v.unlock(&pw)?;
        creds.password = h.secret.as_deref().map(|s| v.open(&h.id, s)).transpose()?;
        creds.key_pass = h.key_secret.as_deref().map(|s| v.open(&format!("{}:key", h.id), s)).transpose()?;
    }
    if h.auth == store::AuthKind::Ask || (h.auth == store::AuthKind::Password && creds.password.is_none()) {
        creds.password = Some(read_secret(&format!("{} {}: ", i18n::t("pw.login_prompt"), h.target()))?);
    }
    Ok(creds)
}

fn cli_events() -> UnboundedSender<AppEvent> {
    let (tx, mut rx) = unbounded_channel::<AppEvent>();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if let AppEvent::HostKey(q) = ev {
                eprintln!(
                    "{}\n  {}:{}  {}\n  SHA256 {}",
                    if q.changed { i18n::t("hk.changed1") } else { i18n::t("hk.new") },
                    q.host,
                    q.port,
                    q.key_type,
                    q.fingerprint
                );
                eprint!("{} [y/N] ", i18n::t("hk.trust"));
                let _ = std::io::stderr().flush();
                let ans = tokio::task::spawn_blocking(|| {
                    let mut s = String::new();
                    std::io::stdin().read_line(&mut s).ok();
                    s
                })
                .await
                .unwrap_or_default();
                let _ = q.reply.send(ans.trim().eq_ignore_ascii_case("y"));
            }
        }
    });
    tx
}

fn split_remote<'a>(spec: &'a str, store: &Store) -> Option<(Host, &'a str)> {
    if spec.starts_with(['/', '.', '~', '\\']) {
        return None;
    }
    let b = spec.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b.len() == 2 || matches!(b[2], b'\\' | b'/')) {
        return None;
    }
    let (h, path) = spec.split_once(':')?;
    let host = store.find_by_name(h).cloned().or_else(|| adhoc(h))?;
    Some((host, if path.is_empty() { "." } else { path }))
}

fn progress_line(name: &str) -> impl Fn(u64, u64) + Send + use<> {
    let name = name.to_string();
    let start = std::time::Instant::now();
    move |done, total| {
        let pct = if total == 0 { 100.0 } else { done as f64 * 100.0 / total as f64 };
        let width = 30;
        let filled = (pct / 100.0 * width as f64) as usize;
        let rate = done as f64 / start.elapsed().as_secs_f64().max(0.001);
        eprint!(
            "\r\x1b[2K  {name}  \x1b[36m{}\x1b[90m{}\x1b[0m {pct:5.1}%  {} / {}  {}",
            "█".repeat(filled),
            "░".repeat(width - filled),
            util::human_bytes(done),
            util::human_bytes(total),
            util::human_rate(rate)
        );
    }
}

async fn cli_copy(src: String, dst: String, use_scp: bool) -> Result<()> {
    let settings = config::Settings::load();
    i18n::set_lang(settings.lang);
    let store = Store::load()?;
    let (remote, local, upload) = match (split_remote(&src, &store), split_remote(&dst, &store)) {
        (Some(r), None) => (r, dst.clone(), false),
        (None, Some(r)) => (r, src.clone(), true),
        (Some(_), Some(_)) => bail!("remote-to-remote copy is not supported"),
        (None, None) => bail!("one side must be host:path"),
    };
    let (host, rpath) = remote;
    let creds = cli_creds(&host)?;
    let stats = std::sync::Arc::new(ssh::Stats::default());
    let (handle, _info) =
        ssh::connect(&host, &creds, &settings, Some(cli_events()), stats, |s| eprintln!("  › {s}")).await?;
    let local_path = ssh::expand_tilde(&local);
    let name = if upload {
        local_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        rpath.rsplit('/').next().unwrap_or("").to_string()
    };
    let t0 = std::time::Instant::now();
    if use_scp {
        if upload {
            scp::upload(&handle, &local_path, rpath, progress_line(&name)).await?;
        } else {
            scp::download(&handle, rpath, &local_path, progress_line(&name)).await?;
        }
    } else {
        let s = std::sync::Arc::new(ssh::open_sftp(&handle).await?);
        let p = std::sync::Arc::new(sftp::Progress::default());
        let (dir, item) = if upload {
            let target = match s.metadata(rpath.to_string()).await {
                Ok(m) if m.file_type().is_dir() => util::join_remote(rpath, &name),
                _ => rpath.to_string(),
            };
            (sftp::Direction::Upload, (local_path.to_string_lossy().into_owned(), target))
        } else {
            let target = if local_path.is_dir() { local_path.join(&name) } else { local_path.clone() };
            (sftp::Direction::Download, (rpath.to_string(), target.to_string_lossy().into_owned()))
        };
        let job = sftp::Job { dir, items: vec![item], delete_source: false };
        let pp = p.clone();
        let show = std::sync::Arc::new(progress_line(&name));
        let show2 = show.clone();
        let ticker = tokio::spawn(async move {
            let show = show2;
            loop {
                show(pp.done.load(Ordering::Relaxed), pp.total.load(Ordering::Relaxed));
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });
        let r = sftp::run_job(s, job, p.clone(), settings.sftp_buffer_kib).await;
        ticker.abort();
        show(p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed));
        r?;
    }
    eprintln!("\n  ✔ {:.1}s", t0.elapsed().as_secs_f64());
    let _ = handle.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    Ok(())
}

fn print_tree(store: &Store) {
    for r in store.rows("") {
        let indent = "  ".repeat(r.depth);
        if r.is_group {
            if let Some(g) = store.group(&r.id) {
                println!("{indent}\x1b[35;1m▾ {}\x1b[0m", g.name);
            }
        } else if let Some(h) = store.host(&r.id) {
            println!("{indent}\x1b[32m●\x1b[0m {:<24} \x1b[90m{}\x1b[0m  \x1b[2m[{}]\x1b[0m", h.name, h.target(), h.id);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        None => run_tui(None).await,
        Some(Cmd::Connect { host, sftp }) => {
            let mode = if sftp { ConnectMode::Sftp } else { ConnectMode::Tab };
            let settings = config::Settings::load();
            i18n::set_lang(settings.lang);
            let store = Store::load()?;
            if let Some((id, password, key_pass)) = handoff::redeem_from_env().await.context("hand-off")? {
                let h = store.host(&id).cloned().ok_or_else(|| anyhow!("unknown host {id}"))?;
                return run_tui(Some(Start::Direct(Box::new(h), Creds { password, key_pass }, mode))).await;
            }
            match store.find_by_name(&host) {
                Some(h) => run_tui(Some(Start::Saved(h.id.clone(), mode))).await,
                None => {
                    let h = adhoc(&host).ok_or_else(|| anyhow!("host «{host}» not found (use user@host[:port])"))?;
                    run_tui(Some(Start::Direct(Box::new(h), Creds::default(), mode))).await
                }
            }
        }
        Some(Cmd::Cp { src, dst, scp }) => cli_copy(src, dst, scp).await,
        Some(Cmd::Completions { shell }) => {
            clap_complete::generate(shell, &mut Cli::command(), "rsh", &mut std::io::stdout());
            Ok(())
        }
        Some(Cmd::Man) => {
            clap_mangen::Man::new(Cli::command()).render(&mut std::io::stdout())?;
            Ok(())
        }
        Some(Cmd::List) => {
            print_tree(&Store::load()?);
            Ok(())
        }
        Some(Cmd::Import) => {
            let mut s = Store::load()?;
            let n = s.import_ssh_config(&dirs::home_dir().unwrap_or_default().join(".ssh/config"))?;
            s.save()?;
            println!("imported {n} hosts");
            Ok(())
        }
    }
}
