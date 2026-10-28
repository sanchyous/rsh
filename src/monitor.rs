use crate::config::PingMethod;
use crate::event::AppEvent;
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::watch;

#[derive(Clone, Debug)]
pub struct Target {
    pub id: String,
    pub address: String,
    pub port: u16,
}

#[derive(Clone, Debug)]
pub struct MonitorCfg {
    pub enabled: bool,
    pub interval: Duration,
    pub method: PingMethod,
    pub timeout: Duration,
    pub targets: Vec<Target>,
}

pub const HISTORY: usize = 60;

#[derive(Default, Clone)]
pub struct HostStatus {
    pub online: Option<bool>,
    pub rtt: Option<Duration>,
    pub checked: Option<Instant>,
    pub since: Option<Instant>,
    pub history: VecDeque<u64>,
    pub ok_count: u64,
    pub fail_count: u64,
}

impl HostStatus {
    pub fn record(&mut self, rtt: Option<Duration>) -> bool {
        let now_online = rtt.is_some();
        let flipped = self.online.is_some_and(|o| o != now_online);
        if self.online != Some(now_online) {
            self.since = Some(Instant::now());
        }
        self.online = Some(now_online);
        self.rtt = rtt;
        self.checked = Some(Instant::now());
        if now_online {
            self.ok_count += 1
        } else {
            self.fail_count += 1
        }
        self.history.push_back(rtt.map(|d| d.as_micros() as u64).unwrap_or(0));
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
        flipped
    }
    pub fn uptime_pct(&self) -> Option<f64> {
        let n = self.ok_count + self.fail_count;
        (n > 0).then(|| self.ok_count as f64 * 100.0 / n as f64)
    }
}

pub async fn probe(address: &str, port: u16, method: PingMethod, timeout: Duration) -> Option<Duration> {
    match method {
        PingMethod::Tcp => {
            let addr =
                tokio::time::timeout(timeout, tokio::net::lookup_host((address, port))).await.ok()?.ok()?.next()?;
            let t = Instant::now();
            tokio::time::timeout(timeout, tokio::net::TcpStream::connect(addr)).await.ok()?.ok()?;
            Some(t.elapsed())
        }
        PingMethod::Icmp => {
            let secs = timeout.as_secs().max(1).to_string();
            let out = tokio::process::Command::new("ping")
                .args(["-c", "1", "-n", "-W", &secs, address])
                .output()
                .await
                .ok()?;
            if !out.status.success() {
                return None;
            }
            let s = String::from_utf8_lossy(&out.stdout);
            let ms: f64 = s.split("time=").nth(1)?.split_whitespace().next()?.parse().ok()?;
            Some(Duration::from_secs_f64(ms / 1000.0))
        }
    }
}

pub fn spawn_probe(t: Target, method: PingMethod, timeout: Duration, manual: bool, ev: UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        let rtt = probe(&t.address, t.port, method, timeout).await;
        ev.send(AppEvent::Ping { host_id: t.id, rtt, manual }).ok();
    });
}

pub fn spawn(mut cfg: watch::Receiver<MonitorCfg>, ev: UnboundedSender<AppEvent>) {
    tokio::spawn(async move {
        loop {
            let c = cfg.borrow_and_update().clone();
            if c.enabled {
                for t in c.targets.iter().cloned() {
                    spawn_probe(t, c.method, c.timeout, false, ev.clone());
                }
            }
            let wait = if c.enabled { c.interval } else { Duration::from_secs(3600) };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                r = cfg.changed() => if r.is_err() { break },
            }
            if ev.is_closed() {
                break;
            }
        }
    });
}
