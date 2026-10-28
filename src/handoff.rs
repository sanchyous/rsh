use crate::util::{hex_decode, hex_encode};
use crate::vault::Secret;
use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use zeroize::Zeroizing;

pub const ENV_SOCK: &str = "RSH_HANDOFF_SOCK";
pub const ENV_TOKEN: &str = "RSH_HANDOFF_TOKEN";

struct Ticket {
    host_id: String,
    password: Option<Secret>,
    key_pass: Option<Secret>,
    expires: Instant,
}

pub struct Handoff {
    pub path: PathBuf,
    tickets: Arc<Mutex<HashMap<String, Ticket>>>,
}

impl Drop for Handoff {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn enc(s: &Option<Secret>) -> String {
    s.as_ref().map(|s| hex_encode(s.as_bytes())).unwrap_or_else(|| "-".into())
}

fn dec(s: &str) -> Option<Secret> {
    if s == "-" {
        return None;
    }
    hex_decode(s).and_then(|b| String::from_utf8(b).ok()).map(Zeroizing::new)
}

impl Handoff {
    pub fn start() -> Result<Arc<Self>> {
        use std::os::unix::fs::PermissionsExt;
        let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir).join("rsh");
        std::fs::create_dir_all(&base)?;
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))?;
        let path = base.join(format!("handoff-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        let me = Arc::new(Self { path, tickets: Arc::default() });
        let tickets = me.tickets.clone();
        let my_uid = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(&me.path)?.uid()
        };
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                if stream.peer_cred().map(|c| c.uid()).ok() != Some(my_uid) {
                    continue;
                }
                let tickets = tickets.clone();
                tokio::spawn(async move {
                    let _ = serve(stream, tickets).await;
                });
            }
        });
        Ok(me)
    }

    pub fn issue(&self, host_id: &str, password: Option<Secret>, key_pass: Option<Secret>) -> String {
        let token = crate::util::hex_encode(&rand_token());
        let mut t = self.tickets.lock().unwrap();
        t.retain(|_, v| v.expires > Instant::now());
        t.insert(
            token.clone(),
            Ticket { host_id: host_id.into(), password, key_pass, expires: Instant::now() + Duration::from_secs(60) },
        );
        token
    }
}

fn rand_token() -> [u8; 24] {
    use rand::Rng;
    let mut b = [0u8; 24];
    rand::rng().fill_bytes(&mut b);
    b
}

async fn serve(stream: UnixStream, tickets: Arc<Mutex<HashMap<String, Ticket>>>) -> Result<()> {
    let (r, mut w) = stream.into_split();
    let mut line = String::new();
    BufReader::new(r).read_line(&mut line).await?;
    let ticket = tickets.lock().unwrap().remove(line.trim());
    match ticket {
        Some(t) if t.expires > Instant::now() => {
            let msg = Zeroizing::new(format!("OK {} {} {}\n", t.host_id, enc(&t.password), enc(&t.key_pass)));
            w.write_all(msg.as_bytes()).await?;
        }
        _ => w.write_all(b"ERR\n").await?,
    }
    Ok(())
}

pub async fn redeem_from_env() -> Result<Option<(String, Option<Secret>, Option<Secret>)>> {
    let (Ok(sock), Ok(token)) = (std::env::var(ENV_SOCK), std::env::var(ENV_TOKEN)) else {
        return Ok(None);
    };
    unsafe {
        std::env::remove_var(ENV_SOCK);
        std::env::remove_var(ENV_TOKEN);
    }
    let mut s = UnixStream::connect(&sock).await?;
    s.write_all(format!("{token}\n").as_bytes()).await?;
    let mut line = Zeroizing::new(String::new());
    BufReader::new(s).read_line(&mut line).await?;
    let parts: Vec<&str> = line.split_whitespace().collect();
    match parts.as_slice() {
        ["OK", id, p, k] => Ok(Some((id.to_string(), dec(p), dec(k)))),
        ["ERR"] => bail!("hand-off token rejected or expired"),
        _ => Err(anyhow!("bad hand-off reply")),
    }
}
