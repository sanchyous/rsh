use crate::config::{HostKeyPolicy, Settings, known_hosts_path};
use crate::event::{AppEvent, HostKeyQuestion, Pane, Sid};
use crate::store::{AuthKind, Host};
use crate::vault::Secret;
use anyhow::{Context, Result, anyhow, bail};
use russh::ChannelMsg;
use russh::client::{self, KeyboardInteractiveAuthResponse, Msg};
use russh::keys::{HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskCx, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::sync::oneshot;

pub type Handle = client::Handle<Client>;

#[derive(Default)]
pub struct Stats {
    pub rx: AtomicU64,
    pub tx: AtomicU64,
}

struct Counting<S> {
    inner: S,
    stats: Arc<Stats>,
}

impl<S: AsyncRead + Unpin> AsyncRead for Counting<S> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut TaskCx<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let r = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = r {
            self.stats.rx.fetch_add((buf.filled().len() - before) as u64, Ordering::Relaxed);
        }
        r
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for Counting<S> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut TaskCx<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        let r = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = r {
            self.stats.tx.fetch_add(n as u64, Ordering::Relaxed);
        }
        r
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskCx<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskCx<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[derive(Clone, Default, Debug)]
pub struct ConnInfo {
    pub kex: String,
    pub cipher: String,
    pub mac: String,
    pub compression: String,
    pub host_key: String,
    pub fingerprint: String,
    pub banner: Option<String>,
    pub peer: String,
    pub auth_method: String,
    pub tcp_connect: Duration,
    pub handshake: Duration,
}

pub struct Client {
    host: String,
    port: u16,
    policy: HostKeyPolicy,
    events: Option<UnboundedSender<AppEvent>>,
    info: Arc<Mutex<ConnInfo>>,
}

fn drop_known_line(path: &PathBuf, line: usize) {
    if let Ok(txt) = std::fs::read_to_string(path) {
        let kept: Vec<&str> = txt.lines().enumerate().filter(|(i, _)| i + 1 != line).map(|(_, l)| l).collect();
        let _ = crate::util::write_private(path, (kept.join("\n") + "\n").as_bytes());
    }
}

impl client::Handler for Client {
    type Error = anyhow::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let pk: PublicKey = match key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(c) => PublicKey::new(c.public_key().clone(), ""),
        };
        let fingerprint = pk.fingerprint(HashAlg::Sha256).to_string();
        let key_type = pk.algorithm().to_string();
        {
            let mut i = self.info.lock().unwrap();
            i.fingerprint = fingerprint.clone();
            i.host_key = key_type.clone();
        }
        let ours = known_hosts_path();
        let system = dirs::home_dir().map(|h| h.join(".ssh/known_hosts"));
        let mut changed = false;
        let mut our_stale_line = None;
        for (path, is_ours) in [(Some(ours.clone()), true), (system, false)] {
            let Some(path) = path.filter(|p| p.exists()) else { continue };
            match russh::keys::check_known_hosts_path(&self.host, self.port, &pk, &path) {
                Ok(true) => return Ok(true),
                Ok(false) => {}
                Err(russh::keys::Error::KeyChanged { line }) => {
                    changed = true;
                    if is_ours {
                        our_stale_line = Some(line);
                    }
                }
                Err(_) => {}
            }
        }
        let accept = match (self.policy, changed) {
            (HostKeyPolicy::Strict, _) => false,
            (HostKeyPolicy::AcceptNew, false) => true,
            _ => {
                let Some(ev) = &self.events else { return Ok(false) };
                let (tx, rx) = oneshot::channel();
                ev.send(AppEvent::HostKey(HostKeyQuestion {
                    host: self.host.clone(),
                    port: self.port,
                    key_type,
                    fingerprint,
                    changed,
                    reply: tx,
                }))
                .ok();
                rx.await.unwrap_or(false)
            }
        };
        if accept {
            if let Some(line) = our_stale_line {
                drop_known_line(&ours, line);
            }
            if let Err(e) = russh::keys::known_hosts::learn_known_hosts_path(&self.host, self.port, &pk, &ours)
                && let Some(ev) = &self.events
            {
                ev.send(AppEvent::Toast(format!("known_hosts: {e}"), crate::event::Level::Warn)).ok();
            }
        }
        Ok(accept)
    }

    async fn kex_done(
        &mut self,
        _shared_secret: Option<&[u8]>,
        names: &russh::Names,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let mut i = self.info.lock().unwrap();
        i.kex = names.kex.as_ref().to_string();
        i.cipher = names.cipher.as_ref().to_string();
        i.mac = names.client_mac.as_ref().to_string();
        i.compression = format!("{:?}", names.client_compression).to_lowercase();
        Ok(())
    }

    async fn auth_banner(&mut self, banner: &str, _session: &mut client::Session) -> Result<(), Self::Error> {
        self.info.lock().unwrap().banner = Some(banner.trim_end().to_string());
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct Creds {
    pub password: Option<Secret>,
    pub key_pass: Option<Secret>,
}

pub fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(p)
}

async fn rsa_hash(h: &Handle) -> Option<HashAlg> {
    tokio::time::timeout(Duration::from_secs(3), h.best_supported_rsa_hash())
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten()
        .flatten()
}

async fn try_password(h: &mut Handle, user: &str, pw: &str) -> Result<bool> {
    if h.authenticate_password(user, pw).await?.success() {
        return Ok(true);
    }
    let mut resp = h.authenticate_keyboard_interactive_start(user, None).await?;
    for _ in 0..4 {
        match resp {
            KeyboardInteractiveAuthResponse::Success => return Ok(true),
            KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            KeyboardInteractiveAuthResponse::InfoRequest { prompts, .. } => {
                let answers = prompts.iter().map(|p| if p.echo { String::new() } else { pw.to_string() }).collect();
                resp = h.authenticate_keyboard_interactive_respond(answers).await?;
            }
        }
    }
    Ok(false)
}

async fn try_key(h: &mut Handle, user: &str, path: &PathBuf, pass: Option<&str>) -> Result<bool> {
    let key = russh::keys::load_secret_key(path, pass).with_context(|| format!("{}", path.display()))?;
    let hash = rsa_hash(h).await;
    Ok(h.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash)).await?.success())
}

async fn try_agent(h: &mut Handle, user: &str) -> Result<bool> {
    let mut agent = russh::keys::agent::client::AgentClient::connect_env().await?;
    let ids = agent.request_identities().await?;
    let hash = rsa_hash(h).await;
    for id in ids {
        let pk = id.public_key().into_owned();
        if let Ok(r) = h.authenticate_publickey_with(user, pk, hash, &mut agent).await
            && r.success()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn authenticate(h: &mut Handle, host: &Host, creds: &Creds) -> Result<String> {
    let user = host.user.as_str();
    let pw = creds.password.as_deref().map(|s| s.as_str());
    let kp = creds.key_pass.as_deref().map(|s| s.as_str());
    match host.auth {
        AuthKind::Password | AuthKind::Ask => {
            let pw = pw.ok_or_else(|| anyhow!(crate::i18n::t("err.no_password")))?;
            if try_password(h, user, pw).await? {
                return Ok("password".into());
            }
        }
        AuthKind::Key => {
            let path = host.key_path.as_deref().ok_or_else(|| anyhow!(crate::i18n::t("err.no_key")))?;
            if try_key(h, user, &expand_tilde(path), kp).await? {
                return Ok(format!("publickey ({path})"));
            }
        }
        AuthKind::Agent => {
            if try_agent(h, user).await? {
                return Ok("agent".into());
            }
        }
        AuthKind::Auto => {
            if try_agent(h, user).await.unwrap_or(false) {
                return Ok("agent".into());
            }
            let mut keys: Vec<PathBuf> = host.key_path.iter().map(|p| expand_tilde(p)).collect();
            if let Some(home) = dirs::home_dir() {
                for n in ["id_ed25519", "id_ecdsa", "id_rsa"] {
                    keys.push(home.join(".ssh").join(n));
                }
            }
            for k in keys.iter().filter(|k| k.exists()) {
                if try_key(h, user, k, kp).await.unwrap_or(false) {
                    return Ok(format!("publickey ({})", k.display()));
                }
            }
            if let Some(pw) = pw
                && try_password(h, user, pw).await?
            {
                return Ok("password".into());
            }
        }
    }
    bail!(crate::i18n::t("err.auth_failed"))
}

pub async fn connect(
    host: &Host,
    creds: &Creds,
    settings: &Settings,
    events: Option<UnboundedSender<AppEvent>>,
    stats: Arc<Stats>,
    stage: impl Fn(&str),
) -> Result<(Handle, ConnInfo)> {
    let config = client::Config {
        keepalive_interval: (settings.keepalive_secs > 0).then(|| Duration::from_secs(settings.keepalive_secs)),
        keepalive_max: 4,
        nodelay: true,
        window_size: 8 * 1024 * 1024,
        ..Default::default()
    };
    stage(crate::i18n::t("stage.tcp"));
    let t0 = std::time::Instant::now();
    let tcp = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::net::TcpStream::connect((host.address.as_str(), host.port)),
    )
    .await
    .map_err(|_| anyhow!(crate::i18n::t("err.timeout")))??;
    let tcp_connect = t0.elapsed();
    let peer = tcp.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    tcp.set_nodelay(true).ok();
    let info = Arc::new(Mutex::new(ConnInfo { peer, tcp_connect, ..Default::default() }));
    let handler = Client {
        host: host.address.clone(),
        port: host.port,
        policy: settings.host_key_policy,
        events,
        info: info.clone(),
    };
    stage(crate::i18n::t("stage.handshake"));
    let t1 = std::time::Instant::now();
    let mut handle =
        client::connect_stream(Arc::new(config), Counting { inner: tcp, stats }, handler).await.map_err(|e| {
            let fp = info.lock().unwrap().fingerprint.clone();
            if e.to_string().contains("UnknownKey") || e.to_string().contains("nknown") {
                anyhow!("{} ({fp})", crate::i18n::t("err.hostkey_rejected"))
            } else {
                e
            }
        })?;
    let handshake = t1.elapsed();
    stage(crate::i18n::t("stage.auth"));
    let method = authenticate(&mut handle, host, creds).await?;
    let mut info = info.lock().unwrap().clone();
    info.auth_method = method;
    info.handshake = handshake;
    Ok((handle, info))
}

pub enum PtyCmd {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
}

pub async fn open_pty(handle: &Handle, cols: u16, rows: u16, term: &str) -> Result<russh::Channel<Msg>> {
    let ch = handle.channel_open_session().await?;
    ch.request_pty(false, term, cols as u32, rows as u32, 0, 0, &[]).await?;
    ch.set_env(false, "COLORTERM", "truecolor").await.ok();
    ch.request_shell(false).await?;
    Ok(ch)
}

pub fn spawn_pump(
    ch: russh::Channel<Msg>,
    sid: Sid,
    pane: Pane,
    ev: UnboundedSender<AppEvent>,
) -> UnboundedSender<PtyCmd> {
    let (tx, mut rx) = unbounded_channel::<PtyCmd>();
    let (mut rd, wr) = ch.split();
    tokio::spawn(async move {
        let mut status = None;
        loop {
            tokio::select! {
                m = rd.wait() => match m {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        if ev.send(AppEvent::PtyData { sid, pane, data: data.to_vec() }).is_err() { break }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => status = Some(exit_status),
                    Some(ChannelMsg::Close) | None => break,
                    _ => {}
                },
                c = rx.recv() => match c {
                    Some(PtyCmd::Data(b)) => { if wr.data(&b[..]).await.is_err() { break } }
                    Some(PtyCmd::Resize(c, r)) => { let _ = wr.window_change(c as u32, r as u32, 0, 0).await; }
                    Some(PtyCmd::Close) | None => { let _ = wr.close().await; break }
                },
            }
        }
        ev.send(AppEvent::PtyClosed { sid, pane, status }).ok();
    });
    tx
}

pub async fn open_sftp(handle: &Handle) -> Result<russh_sftp::client::SftpSession> {
    let ch = handle.channel_open_session().await?;
    ch.request_subsystem(true, "sftp").await?;
    let s = russh_sftp::client::SftpSession::new(ch.into_stream()).await?;
    Ok(s)
}

pub async fn measure_rtt(handle: &Handle) -> Option<Duration> {
    let t = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(5), handle.send_ping()).await.ok()?.ok()?;
    Some(t.elapsed())
}
