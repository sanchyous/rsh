use crate::util::{join_remote, parent_remote};
use anyhow::{Result, anyhow};
use russh_sftp::client::SftpSession;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: u64,
    pub mtime: Option<i64>,
    pub mode: Option<u32>,
}

impl Entry {
    pub fn is_exec(&self) -> bool {
        !self.is_dir && self.mode.is_some_and(|m| m & 0o111 != 0)
    }
    pub fn ext(&self) -> &str {
        if self.is_dir { "" } else { self.name.rsplit_once('.').map(|(_, e)| e).unwrap_or("") }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Local,
    Remote,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Name,
    Ext,
    Size,
    Time,
}

impl SortKey {
    pub fn next(self) -> Self {
        match self {
            Self::Name => Self::Ext,
            Self::Ext => Self::Size,
            Self::Size => Self::Time,
            Self::Time => Self::Name,
        }
    }
    pub fn label(self) -> &'static str {
        use crate::i18n::t;
        match self {
            Self::Name => t("sort.name"),
            Self::Ext => t("sort.ext"),
            Self::Size => t("sort.size"),
            Self::Time => t("sort.time"),
        }
    }
}

pub struct Panel {
    pub side: Side,
    pub path: String,
    pub entries: Vec<Entry>,
    pub view: Vec<usize>,
    pub cursor: usize,
    pub offset: usize,
    pub marked: HashSet<String>,
    pub filter: String,
    pub show_hidden: bool,
    pub sort: SortKey,
    pub reverse: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub history: Vec<String>,
    pub home: Option<String>,
}

impl Panel {
    pub fn new(side: Side, path: String, show_hidden: bool) -> Self {
        Self {
            side,
            path,
            entries: vec![],
            view: vec![],
            cursor: 0,
            offset: 0,
            marked: HashSet::new(),
            filter: String::new(),
            show_hidden,
            sort: SortKey::Name,
            reverse: false,
            loading: false,
            error: None,
            history: vec![],
            home: None,
        }
    }

    pub fn set_entries(&mut self, path: String, mut entries: Vec<Entry>, focus: Option<&str>) {
        let same_dir = path == self.path;
        let keep =
            focus.map(str::to_string).or_else(|| if same_dir { self.current().map(|e| e.name.clone()) } else { None });
        if !same_dir {
            self.marked.clear();
            self.filter.clear();
        }
        if path != "/" {
            entries
                .insert(0, Entry { name: "..".into(), is_dir: true, is_link: false, size: 0, mtime: None, mode: None });
        }
        self.path = path;
        self.entries = entries;
        self.loading = false;
        self.error = None;
        self.rebuild();
        self.cursor = keep.and_then(|n| self.view.iter().position(|&i| self.entries[i].name == n)).unwrap_or(0);
    }

    pub fn rebuild(&mut self) {
        let f = self.filter.to_lowercase();
        let mut v: Vec<usize> = (0..self.entries.len())
            .filter(|&i| {
                let e = &self.entries[i];
                e.name == ".."
                    || ((self.show_hidden || !e.name.starts_with('.'))
                        && (f.is_empty() || e.name.to_lowercase().contains(&f)))
            })
            .collect();
        let es = &self.entries;
        let (sort, rev) = (self.sort, self.reverse);
        v.sort_by(|&a, &b| {
            let (x, y) = (&es[a], &es[b]);
            if x.name == ".." {
                return std::cmp::Ordering::Less;
            }
            if y.name == ".." {
                return std::cmp::Ordering::Greater;
            }
            if x.is_dir != y.is_dir {
                return y.is_dir.cmp(&x.is_dir);
            }
            let o = match sort {
                SortKey::Name => natural_cmp(&x.name, &y.name),
                SortKey::Ext => {
                    x.ext().to_lowercase().cmp(&y.ext().to_lowercase()).then_with(|| natural_cmp(&x.name, &y.name))
                }
                SortKey::Size => y.size.cmp(&x.size).then_with(|| natural_cmp(&x.name, &y.name)),
                SortKey::Time => y.mtime.cmp(&x.mtime).then_with(|| natural_cmp(&x.name, &y.name)),
            };
            if rev { o.reverse() } else { o }
        });
        self.view = v;
        self.cursor = self.cursor.min(self.view.len().saturating_sub(1));
    }

    pub fn current(&self) -> Option<&Entry> {
        self.view.get(self.cursor).map(|&i| &self.entries[i])
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.view.is_empty() {
            return;
        }
        let n = self.view.len() as isize;
        self.cursor = (self.cursor as isize + delta).clamp(0, n - 1) as usize;
    }

    pub fn toggle_mark(&mut self) {
        if let Some(e) = self.current().cloned()
            && e.name != ".."
            && !self.marked.remove(&e.name)
        {
            self.marked.insert(e.name);
        }
        self.move_cursor(1);
    }

    pub fn mark_all(&mut self, on: bool) {
        self.marked.clear();
        if on {
            for &i in &self.view {
                if self.entries[i].name != ".." {
                    self.marked.insert(self.entries[i].name.clone());
                }
            }
        }
    }

    pub fn invert_marks(&mut self) {
        for &i in &self.view {
            let n = &self.entries[i].name;
            if n != ".." && !self.marked.remove(n) {
                self.marked.insert(n.clone());
            }
        }
    }

    pub fn targets(&self) -> Vec<Entry> {
        if !self.marked.is_empty() {
            return self.entries.iter().filter(|e| self.marked.contains(&e.name)).cloned().collect();
        }
        self.current().filter(|e| e.name != "..").cloned().into_iter().collect()
    }

    pub fn join(&self, name: &str) -> String {
        match self.side {
            Side::Remote => join_remote(&self.path, name),
            Side::Local => Path::new(&self.path).join(name).to_string_lossy().into_owned(),
        }
    }

    pub fn parent(&self) -> String {
        match self.side {
            Side::Remote => parent_remote(&self.path),
            Side::Local => {
                Path::new(&self.path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "/".into())
            }
        }
    }

    pub fn dir_name(&self) -> String {
        self.path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string()
    }

    pub fn marked_size(&self) -> u64 {
        self.entries.iter().filter(|e| self.marked.contains(&e.name)).map(|e| e.size).sum()
    }
}

pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek(), bi.peek()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().filter(|c| c.is_ascii_digit()) {
                    na.push(*c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().filter(|c| c.is_ascii_digit()) {
                    nb.push(*c);
                    bi.next();
                }
                let o = na.trim_start_matches('0').len().cmp(&nb.trim_start_matches('0').len()).then(na.cmp(&nb));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = x.to_lowercase().cmp(y.to_lowercase());
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

pub fn list_local(path: &str) -> Result<Vec<Entry>> {
    use std::os::unix::fs::MetadataExt;
    let mut out = Vec::new();
    for de in std::fs::read_dir(path)? {
        let de = de?;
        let lmeta = de.path().symlink_metadata()?;
        let is_link = lmeta.file_type().is_symlink();
        let meta = if is_link { std::fs::metadata(de.path()).unwrap_or(lmeta) } else { lmeta };
        out.push(Entry {
            name: de.file_name().to_string_lossy().into_owned(),
            is_dir: meta.is_dir(),
            is_link,
            size: if meta.is_dir() { 0 } else { meta.len() },
            mtime: Some(meta.mtime()),
            mode: Some(meta.mode() & 0o7777),
        });
    }
    Ok(out)
}

pub async fn list_remote(sftp: &SftpSession, path: &str) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for de in sftp.read_dir(path).await? {
        let name = de.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let m = de.metadata();
        let is_link = m.file_type().is_symlink();
        let mut is_dir = m.file_type().is_dir();
        let mut size = m.size.unwrap_or(0);
        if is_link && let Ok(t) = sftp.metadata(join_remote(path, &name)).await {
            is_dir = t.file_type().is_dir();
            size = t.size.unwrap_or(0);
        }
        out.push(Entry {
            name,
            is_dir,
            is_link,
            size: if is_dir { 0 } else { size },
            mtime: m.mtime.map(|t| t as i64),
            mode: m.permissions.map(|p| p & 0o7777),
        });
    }
    Ok(out)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Default)]
pub struct Progress {
    pub total: AtomicU64,
    pub done: AtomicU64,
    pub files_total: AtomicU64,
    pub files_done: AtomicU64,
    pub cancel: AtomicBool,
    pub current: Mutex<String>,
}

pub struct Transfer {
    pub id: u64,
    pub dir: Direction,
    pub label: String,
    pub progress: Arc<Progress>,
    pub started: Instant,
    pub finished: Option<(Instant, Result<(), String>)>,
    pub samples: std::collections::VecDeque<(Instant, u64)>,
}

impl Transfer {
    pub fn speed(&self) -> f64 {
        match (self.samples.front(), self.samples.back()) {
            (Some(a), Some(b)) if b.0 > a.0 => (b.1 - a.1) as f64 / (b.0 - a.0).as_secs_f64(),
            _ => 0.0,
        }
    }
    pub fn sample(&mut self) {
        let now = Instant::now();
        self.samples.push_back((now, self.progress.done.load(Ordering::Relaxed)));
        while self.samples.len() > 2 && now.duration_since(self.samples[0].0).as_secs_f64() > 3.0 {
            self.samples.pop_front();
        }
    }
    pub fn ratio(&self) -> f64 {
        let t = self.progress.total.load(Ordering::Relaxed);
        if t == 0 {
            if self.finished.is_some() { 1.0 } else { 0.0 }
        } else {
            self.progress.done.load(Ordering::Relaxed) as f64 / t as f64
        }
    }
}

pub struct Job {
    pub dir: Direction,
    pub items: Vec<(String, String)>,
    pub delete_source: bool,
}

fn cancelled(p: &Progress) -> Result<()> {
    if p.cancel.load(Ordering::Relaxed) { Err(anyhow!(crate::i18n::t("xfer.cancelled"))) } else { Ok(()) }
}

fn local_size(p: &Path, files: &mut u64) -> u64 {
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => {
            std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| local_size(&e.path(), files)).sum()).unwrap_or(0)
        }
        Ok(m) => {
            *files += 1;
            m.len()
        }
        Err(_) => 0,
    }
}

fn remote_size<'a>(s: &'a SftpSession, p: String, files: &'a mut u64) -> futures::future::BoxFuture<'a, u64> {
    Box::pin(async move {
        match s.metadata(p.clone()).await {
            Ok(m) if m.file_type().is_dir() => {
                let mut total = 0;
                if let Ok(rd) = s.read_dir(p.clone()).await {
                    for e in rd {
                        let n = e.file_name();
                        if n != "." && n != ".." {
                            total += remote_size(s, join_remote(&p, &n), files).await;
                        }
                    }
                }
                total
            }
            Ok(m) => {
                *files += 1;
                m.size.unwrap_or(0)
            }
            Err(_) => 0,
        }
    })
}

fn upload<'a>(
    s: &'a SftpSession,
    src: PathBuf,
    dst: String,
    p: &'a Progress,
    buf_size: usize,
) -> futures::future::BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        use std::os::unix::fs::PermissionsExt;
        cancelled(p)?;
        let meta = std::fs::metadata(&src)?;
        if meta.is_dir() {
            if !s.try_exists(dst.clone()).await.unwrap_or(false) {
                s.create_dir(dst.clone()).await?;
            }
            for e in std::fs::read_dir(&src)?.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                upload(s, e.path(), join_remote(&dst, &name), p, buf_size).await?;
            }
            return Ok(());
        }
        *p.current.lock().unwrap() = src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut f = tokio::fs::File::open(&src).await?;
        let mut out = s.create(dst.clone()).await?;
        let mut buf = vec![0u8; buf_size];
        loop {
            cancelled(p)?;
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).await?;
            p.done.fetch_add(n as u64, Ordering::Relaxed);
        }
        out.shutdown().await?;
        let mut attrs = russh_sftp::protocol::FileAttributes::empty();
        attrs.permissions = Some(meta.permissions().mode() & 0o7777);
        let _ = s.set_metadata(dst, attrs).await;
        p.files_done.fetch_add(1, Ordering::Relaxed);
        Ok(())
    })
}

fn download<'a>(
    s: &'a SftpSession,
    src: String,
    dst: PathBuf,
    p: &'a Progress,
    buf_size: usize,
) -> futures::future::BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        use std::os::unix::fs::PermissionsExt;
        cancelled(p)?;
        let meta = s.metadata(src.clone()).await?;
        if meta.file_type().is_dir() {
            std::fs::create_dir_all(&dst)?;
            for e in s.read_dir(src.clone()).await? {
                let n = e.file_name();
                if n == "." || n == ".." {
                    continue;
                }
                download(s, join_remote(&src, &n), dst.join(&n), p, buf_size).await?;
            }
            return Ok(());
        }
        *p.current.lock().unwrap() = src.rsplit('/').next().unwrap_or("").to_string();
        let mut f = s.open(src.clone()).await?;
        let mut out = tokio::fs::File::create(&dst).await?;
        let mut buf = vec![0u8; buf_size];
        loop {
            cancelled(p)?;
            let n = f.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n]).await?;
            p.done.fetch_add(n as u64, Ordering::Relaxed);
        }
        out.flush().await?;
        if let Some(mode) = meta.permissions {
            let _ = std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(mode & 0o7777));
        }
        p.files_done.fetch_add(1, Ordering::Relaxed);
        Ok(())
    })
}

pub async fn run_job(s: Arc<SftpSession>, job: Job, p: Arc<Progress>, buf_kib: usize) -> Result<()> {
    let buf = buf_kib.clamp(16, 4096) * 1024;
    let mut files = 0;
    let mut total = 0;
    for (src, _) in &job.items {
        total += match job.dir {
            Direction::Upload => local_size(Path::new(src), &mut files),
            Direction::Download => remote_size(&s, src.clone(), &mut files).await,
        };
    }
    p.total.store(total, Ordering::Relaxed);
    p.files_total.store(files, Ordering::Relaxed);
    for (src, dst) in &job.items {
        match job.dir {
            Direction::Upload => upload(&s, PathBuf::from(src), dst.clone(), &p, buf).await?,
            Direction::Download => download(&s, src.clone(), PathBuf::from(dst), &p, buf).await?,
        }
        if job.delete_source {
            match job.dir {
                Direction::Upload => {
                    let path = Path::new(src);
                    if path.is_dir() { std::fs::remove_dir_all(path)? } else { std::fs::remove_file(path)? }
                }
                Direction::Download => remove_remote(&s, src.clone()).await?,
            }
        }
    }
    Ok(())
}

pub fn remove_remote(s: &SftpSession, path: String) -> futures::future::BoxFuture<'_, Result<()>> {
    Box::pin(async move {
        let m = s.symlink_metadata(path.clone()).await?;
        if m.file_type().is_dir() {
            for e in s.read_dir(path.clone()).await? {
                let n = e.file_name();
                if n != "." && n != ".." {
                    remove_remote(s, join_remote(&path, &n)).await?;
                }
            }
            s.remove_dir(path).await?;
        } else {
            s.remove_file(path).await?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural() {
        let mut v = vec!["file10", "file2", "File1", "a"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["a", "File1", "file2", "file10"]);
    }
}
