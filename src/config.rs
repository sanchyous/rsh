use crate::i18n::Lang;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PingMethod {
    #[default]
    Tcp,
    Icmp,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HostKeyPolicy {
    #[default]
    Ask,
    AcceptNew,
    Strict,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum IconSet {
    #[default]
    Unicode,
    Nerd,
    Emoji,
    Ascii,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ColorMode {
    #[default]
    Auto,
    TrueColor,
    C256,
    C16,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TtyMode {
    #[default]
    Auto,
    On,
    Off,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BorderStyle {
    #[default]
    Rounded,
    Plain,
    Thick,
    Double,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub lang: Lang,
    pub theme: String,
    pub icons: IconSet,
    pub border: BorderStyle,
    pub color_mode: ColorMode,
    pub tty_mode: TtyMode,
    pub monitor_enabled: bool,
    pub monitor_interval_secs: u64,
    pub ping_method: PingMethod,
    pub ping_timeout_ms: u64,
    pub notify_status_change: bool,
    pub terminal_cmd: String,
    pub scrollback: usize,
    pub keepalive_secs: u64,
    pub host_key_policy: HostKeyPolicy,
    pub show_diag_default: bool,
    pub confirm_quit: bool,
    pub auto_lock_mins: u64,
    pub sftp_show_hidden: bool,
    pub sftp_buffer_kib: usize,
    pub editor: String,
    pub term_type: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            lang: Lang::Ru,
            theme: "Midnight".into(),
            icons: IconSet::Unicode,
            border: BorderStyle::Rounded,
            color_mode: ColorMode::Auto,
            tty_mode: TtyMode::Auto,
            monitor_enabled: true,
            monitor_interval_secs: 30,
            ping_method: PingMethod::Tcp,
            ping_timeout_ms: 1500,
            notify_status_change: true,
            terminal_cmd: "auto".into(),
            scrollback: 5000,
            keepalive_secs: 30,
            host_key_policy: HostKeyPolicy::Ask,
            show_diag_default: false,
            confirm_quit: true,
            auto_lock_mins: 15,
            sftp_show_hidden: false,
            sftp_buffer_kib: 256,
            editor: String::new(),
            term_type: "xterm-256color".into(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Ok(p) = std::env::var("RSH_CONFIG_DIR") {
        return PathBuf::from(p);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("rsh")
}

pub fn known_hosts_path() -> PathBuf {
    config_dir().join("known_hosts")
}

impl Settings {
    fn path() -> PathBuf {
        config_dir().join("settings.toml")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let s = toml::to_string_pretty(self)?;
        crate::util::write_private(&Self::path(), s.as_bytes())?;
        Ok(())
    }

    pub fn editor_cmd(&self) -> String {
        if !self.editor.trim().is_empty() {
            return self.editor.clone();
        }
        std::env::var("VISUAL").or_else(|_| std::env::var("EDITOR")).unwrap_or_else(|_| "nano".into())
    }
}

pub const MONITOR_INTERVALS: &[u64] = &[5, 10, 15, 30, 60, 120, 300, 600, 1800];
pub const PING_TIMEOUTS: &[u64] = &[300, 500, 1000, 1500, 2000, 3000, 5000];
pub const KEEPALIVES: &[u64] = &[0, 10, 15, 30, 60, 120];
pub const AUTO_LOCKS: &[u64] = &[0, 1, 5, 10, 15, 30, 60, 240];
pub const SCROLLBACKS: &[usize] = &[1000, 2000, 5000, 10000, 50000];
pub const BUFFERS: &[usize] = &[32, 64, 128, 256, 512, 1024];
