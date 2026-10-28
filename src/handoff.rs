use crate::util::{hex_decode, hex_encode};
use crate::vault::Secret;
use anyhow::{Result, anyhow, bail};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use zeroize::Zeroizing;

pub const ENV_SOCK: &str = "RSH_HANDOFF_SOCK";
pub const ENV_TOKEN: &str = "RSH_HANDOFF_TOKEN";

struct Ticket {
    host_id: String,
    password: Option<Secret>,
    key_pass: Option<Secret>,
    expires: Instant,
}

type Tickets = Arc<Mutex<HashMap<String, Ticket>>>;

pub struct Handoff {
    pub address: String,
    tickets: Tickets,
}

impl Drop for Handoff {
    fn drop(&mut self) {
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.address);
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
        let tickets: Tickets = Arc::default();
        let address = listen(tickets.clone())?;
        Ok(Arc::new(Self { address, tickets }))
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

#[cfg(unix)]
fn listen(tickets: Tickets) -> Result<String> {
    use std::os::unix::fs::MetadataExt;
    let base = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir).join("rsh");
    crate::platform::private_dir(&base)?;
    let path = base.join(format!("handoff-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = tokio::net::UnixListener::bind(&path)?;
    crate::platform::set_mode(&path, 0o600);
    let my_uid = std::fs::metadata(&path)?.uid();
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
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(windows)]
fn listen(tickets: Tickets) -> Result<String> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let name = format!(r"\\.\pipe\rsh-handoff-{}-{}", std::process::id(), hex_encode(&rand_token()[..8]));
    let mut server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name)?;
    let pipe = name.clone();
    tokio::spawn(async move {
        loop {
            if server.connect().await.is_err() {
                break;
            }
            let connected = server;
            server = match ServerOptions::new().reject_remote_clients(true).create(&pipe) {
                Ok(s) => s,
                Err(_) => break,
            };
            let tickets = tickets.clone();
            tokio::spawn(async move {
                let _ = serve(connected, tickets).await;
            });
        }
    });
    Ok(name)
}

#[cfg(unix)]
async fn dial(address: &str) -> Result<tokio::net::UnixStream> {
    Ok(tokio::net::UnixStream::connect(address).await?)
}

#[cfg(windows)]
async fn dial(address: &str) -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use tokio::net::windows::named_pipe::ClientOptions;
    const ERROR_PIPE_BUSY: i32 = 231;
    for _ in 0..40 {
        match ClientOptions::new().open(address) {
            Ok(c) => return Ok(c),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => tokio::time::sleep(Duration::from_millis(50)).await,
            Err(e) => return Err(e.into()),
        }
    }
    bail!("hand-off pipe is busy")
}

async fn serve<S: AsyncRead + AsyncWrite + Unpin>(stream: S, tickets: Tickets) -> Result<()> {
    let (r, mut w) = tokio::io::split(stream);
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
    w.flush().await?;
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
    let mut s = dial(&sock).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ticket_roundtrip_is_single_use() {
        let ho = Handoff::start().unwrap();
        let token = ho.issue("h1", Some(Zeroizing::new("pässword".into())), None);
        let ask = |token: String| {
            let address = ho.address.clone();
            async move {
                let mut s = dial(&address).await.unwrap();
                s.write_all(format!("{token}\n").as_bytes()).await.unwrap();
                let mut line = String::new();
                BufReader::new(s).read_line(&mut line).await.unwrap();
                line
            }
        };
        let first = ask(token.clone()).await;
        let parts: Vec<&str> = first.split_whitespace().collect();
        assert_eq!(parts[0], "OK");
        assert_eq!(parts[1], "h1");
        assert_eq!(dec(parts[2]).unwrap().as_str(), "pässword");
        assert!(dec(parts[3]).is_none());
        assert_eq!(ask(token).await.trim(), "ERR");
        assert_eq!(ask("forged".into()).await.trim(), "ERR");
    }
}
