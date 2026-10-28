use crate::sftp::Entry;
use crate::ssh::{ConnInfo, Handle, PtyCmd};
use russh_sftp::client::SftpSession;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc::UnboundedSender, oneshot};

pub type Sid = u64;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Main,
    Console,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Info,
    Ok,
    Warn,
    Error,
}

pub struct HostKeyQuestion {
    pub host: String,
    pub port: u16,
    pub key_type: String,
    pub fingerprint: String,
    pub changed: bool,
    pub reply: oneshot::Sender<bool>,
}

pub enum AppEvent {
    Input(crossterm::event::Event),
    Stage { sid: Sid, stage: String },
    Connected { sid: Sid, handle: Arc<Handle>, info: ConnInfo },
    ConnectFailed { sid: Sid, err: String },
    PtyReady { sid: Sid, pane: Pane, tx: UnboundedSender<PtyCmd> },
    PtyData { sid: Sid, pane: Pane, data: Vec<u8> },
    PtyClosed { sid: Sid, pane: Pane, status: Option<u32> },
    HostKey(HostKeyQuestion),
    Rtt { sid: Sid, rtt: Duration },
    Ping { host_id: String, rtt: Option<Duration>, manual: bool },
    SftpReady { sid: Sid, sftp: Arc<SftpSession>, home: String },
    SftpFailed { sid: Sid, err: String },
    RemoteList { sid: Sid, path: String, result: Result<Vec<Entry>, String>, focus: Option<String> },
    OpDone { sid: Sid, result: Result<String, String> },
    TransferDone { sid: Sid, tid: u64, result: Result<(), String> },
    EditReady { sid: Sid, local: std::path::PathBuf, remote: String, edit: bool },
    Toast(String, Level),
}
