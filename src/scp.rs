use crate::ssh::Handle;
use crate::util::shell_quote;
use anyhow::{Context, Result, anyhow, bail};
use russh::ChannelMsg;
use russh::client::Msg;
use std::collections::VecDeque;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Reader {
    ch: russh::Channel<Msg>,
    buf: VecDeque<u8>,
    stderr: String,
}

impl Reader {
    async fn fill(&mut self) -> Result<()> {
        loop {
            match self.ch.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    self.buf.extend(data.iter());
                    return Ok(());
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => self.stderr.push_str(&String::from_utf8_lossy(&data)),
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                    bail!(
                        "channel closed{}",
                        if self.stderr.is_empty() { String::new() } else { format!(": {}", self.stderr.trim()) }
                    )
                }
                _ => {}
            }
        }
    }
    async fn byte(&mut self) -> Result<u8> {
        while self.buf.is_empty() {
            self.fill().await?;
        }
        Ok(self.buf.pop_front().unwrap())
    }
    async fn line(&mut self) -> Result<String> {
        let mut v = Vec::new();
        loop {
            let b = self.byte().await?;
            if b == b'\n' {
                return Ok(String::from_utf8_lossy(&v).into_owned());
            }
            v.push(b);
        }
    }
    async fn ack(&mut self) -> Result<()> {
        match self.byte().await? {
            0 => Ok(()),
            1 | 2 => Err(anyhow!("scp: {}", self.line().await.unwrap_or_default())),
            b => bail!("scp: unexpected response byte {b}"),
        }
    }
}

pub async fn upload(h: &Handle, local: &Path, remote: &str, progress: impl Fn(u64, u64)) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(local).with_context(|| local.display().to_string())?;
    if meta.is_dir() {
        bail!("scp: directories are not supported, use sftp mode");
    }
    let ch = h.channel_open_session().await?;
    ch.exec(true, format!("scp -t {}", shell_quote(remote))).await?;
    let mut r = Reader { ch, buf: VecDeque::new(), stderr: String::new() };
    r.ack().await?;
    let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let size = meta.len();
    r.ch.data(format!("C{:04o} {size} {name}\n", meta.permissions().mode() & 0o7777).as_bytes()).await?;
    r.ack().await?;
    let mut f = tokio::fs::File::open(local).await?;
    let mut buf = vec![0u8; 128 * 1024];
    let mut done = 0;
    loop {
        let n = f.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        r.ch.data(&buf[..n]).await?;
        done += n as u64;
        progress(done, size);
    }
    r.ch.data(&[0u8][..]).await?;
    r.ack().await?;
    r.ch.eof().await?;
    Ok(())
}

pub async fn download(h: &Handle, remote: &str, local: &Path, progress: impl Fn(u64, u64)) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let ch = h.channel_open_session().await?;
    ch.exec(true, format!("scp -f {}", shell_quote(remote))).await?;
    let mut r = Reader { ch, buf: VecDeque::new(), stderr: String::new() };
    r.ch.data(&[0u8][..]).await?;
    let header = loop {
        match r.byte().await? {
            b'C' => break r.line().await?,
            b'T' => {
                r.line().await?;
                r.ch.data(&[0u8][..]).await?;
            }
            1 | 2 => bail!("scp: {}", r.line().await.unwrap_or_default()),
            b => bail!("scp: unexpected header byte {b}"),
        }
    };
    let mut parts = header.splitn(3, ' ');
    let mode = u32::from_str_radix(parts.next().unwrap_or("0644"), 8).unwrap_or(0o644);
    let size: u64 = parts.next().and_then(|s| s.parse().ok()).ok_or_else(|| anyhow!("scp: bad header"))?;
    let name = parts.next().unwrap_or("file").to_string();
    let target = if local.is_dir() { local.join(&name) } else { local.to_path_buf() };
    r.ch.data(&[0u8][..]).await?;
    let mut out = tokio::fs::File::create(&target).await?;
    let mut left = size;
    while left > 0 {
        if r.buf.is_empty() {
            r.fill().await?;
        }
        let take = (r.buf.len() as u64).min(left) as usize;
        let chunk: Vec<u8> = r.buf.drain(..take).collect();
        out.write_all(&chunk).await?;
        left -= take as u64;
        progress(size - left, size);
    }
    out.flush().await?;
    r.ack().await?;
    r.ch.data(&[0u8][..]).await?;
    let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o7777));
    r.ch.eof().await?;
    Ok(())
}
