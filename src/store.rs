use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AuthKind {
    #[default]
    Auto,
    Password,
    Key,
    Agent,
    Ask,
}

impl AuthKind {
    pub const ALL: [AuthKind; 5] = [Self::Auto, Self::Password, Self::Key, Self::Agent, Self::Ask];
    pub fn label(self) -> &'static str {
        use crate::i18n::t;
        match self {
            Self::Auto => t("auth.auto"),
            Self::Password => t("auth.password"),
            Self::Key => t("auth.key"),
            Self::Agent => t("auth.agent"),
            Self::Ask => t("auth.ask"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    pub user: String,
    pub auth: AuthKind,
    pub secret: Option<String>,
    pub key_path: Option<String>,
    pub key_secret: Option<String>,
    pub tags: Vec<String>,
    pub notes: String,
    pub monitor: bool,
    pub startup_cmd: String,
    pub last_connected: Option<i64>,
    pub connect_count: u64,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            id: crate::util::new_id(),
            name: String::new(),
            address: String::new(),
            port: 22,
            user: std::env::var("USER").unwrap_or_else(|_| "root".into()),
            auth: AuthKind::Auto,
            secret: None,
            key_path: None,
            key_secret: None,
            tags: Vec::new(),
            notes: String::new(),
            monitor: true,
            startup_cmd: String::new(),
            last_connected: None,
            connect_count: 0,
        }
    }
}

impl Host {
    pub fn target(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.user, self.address)
        } else {
            format!("{}@{}:{}", self.user, self.address, self.port)
        }
    }

    pub fn matches(&self, q: &str) -> bool {
        let q = q.to_lowercase();
        self.name.to_lowercase().contains(&q)
            || self.address.to_lowercase().contains(&q)
            || self.user.to_lowercase().contains(&q)
            || self.tags.iter().any(|t| t.to_lowercase().contains(&q))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub expanded: bool,
    pub groups: Vec<Group>,
    pub hosts: Vec<Host>,
}

impl Default for Group {
    fn default() -> Self {
        Self { id: crate::util::new_id(), name: String::new(), expanded: true, groups: vec![], hosts: vec![] }
    }
}

pub enum Node {
    Group(Group),
    Host(Host),
}

#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub depth: usize,
    pub is_group: bool,
    pub guides: Vec<bool>,
    pub last: bool,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Store {
    pub root: Group,
}

pub const ROOT_ID: &str = "root";

impl Group {
    fn find_group(&self, id: &str) -> Option<&Group> {
        if self.id == id {
            return Some(self);
        }
        self.groups.iter().find_map(|g| g.find_group(id))
    }
    fn find_group_mut(&mut self, id: &str) -> Option<&mut Group> {
        if self.id == id {
            return Some(self);
        }
        self.groups.iter_mut().find_map(|g| g.find_group_mut(id))
    }
    fn find_host(&self, id: &str) -> Option<&Host> {
        self.hosts.iter().find(|h| h.id == id).or_else(|| self.groups.iter().find_map(|g| g.find_host(id)))
    }
    fn find_host_mut(&mut self, id: &str) -> Option<&mut Host> {
        if let Some(i) = self.hosts.iter().position(|h| h.id == id) {
            return Some(&mut self.hosts[i]);
        }
        self.groups.iter_mut().find_map(|g| g.find_host_mut(id))
    }
    fn parent_of(&self, id: &str) -> Option<&Group> {
        if self.hosts.iter().any(|h| h.id == id) || self.groups.iter().any(|g| g.id == id) {
            return Some(self);
        }
        self.groups.iter().find_map(|g| g.parent_of(id))
    }
    fn remove(&mut self, id: &str) -> Option<Node> {
        if let Some(i) = self.hosts.iter().position(|h| h.id == id) {
            return Some(Node::Host(self.hosts.remove(i)));
        }
        if let Some(i) = self.groups.iter().position(|g| g.id == id) {
            return Some(Node::Group(self.groups.remove(i)));
        }
        self.groups.iter_mut().find_map(|g| g.remove(id))
    }
    fn collect_hosts<'a>(&'a self, out: &mut Vec<&'a Host>) {
        out.extend(self.hosts.iter());
        for g in &self.groups {
            g.collect_hosts(out);
        }
    }
    fn any_match(&self, q: &str) -> bool {
        self.name.to_lowercase().contains(&q.to_lowercase())
            || self.hosts.iter().any(|h| h.matches(q))
            || self.groups.iter().any(|g| g.any_match(q))
    }
    fn contains(&self, id: &str) -> bool {
        self.find_group(id).is_some()
    }
    pub fn host_ids(&self) -> Vec<String> {
        let mut v = Vec::new();
        self.collect_hosts(&mut v);
        v.into_iter().map(|h| h.id.clone()).collect()
    }

    fn flatten(&self, depth: usize, guides: &[bool], q: &str, out: &mut Vec<Row>) {
        let groups: Vec<&Group> = self.groups.iter().filter(|g| q.is_empty() || g.any_match(q)).collect();
        let hosts: Vec<&Host> = if q.is_empty() || self.name.to_lowercase().contains(&q.to_lowercase()) {
            self.hosts.iter().collect()
        } else {
            self.hosts.iter().filter(|h| h.matches(q)).collect()
        };
        let total = groups.len() + hosts.len();
        let mut i = 0;
        for g in groups {
            i += 1;
            let last = i == total;
            out.push(Row { id: g.id.clone(), depth, is_group: true, guides: guides.to_vec(), last });
            if g.expanded || !q.is_empty() {
                let mut ng = guides.to_vec();
                ng.push(last);
                g.flatten(depth + 1, &ng, q, out);
            }
        }
        for h in hosts {
            i += 1;
            out.push(Row { id: h.id.clone(), depth, is_group: false, guides: guides.to_vec(), last: i == total });
        }
    }
}

impl Store {
    fn path() -> PathBuf {
        crate::config::config_dir().join("hosts.toml")
    }

    pub fn load() -> Result<Self> {
        let p = Self::path();
        let mut s: Store = match std::fs::read_to_string(&p) {
            Ok(txt) => toml::from_str(&txt)?,
            Err(_) => Store::default(),
        };
        s.root.id = ROOT_ID.into();
        s.root.expanded = true;
        Ok(s)
    }

    pub fn save(&self) -> Result<()> {
        let s = toml::to_string_pretty(self)?;
        crate::util::write_private(&Self::path(), s.as_bytes())?;
        Ok(())
    }

    pub fn rows(&self, filter: &str) -> Vec<Row> {
        let mut out = Vec::new();
        self.root.flatten(0, &[], filter, &mut out);
        out
    }

    pub fn group(&self, id: &str) -> Option<&Group> {
        self.root.find_group(id)
    }
    pub fn group_mut(&mut self, id: &str) -> Option<&mut Group> {
        self.root.find_group_mut(id)
    }
    pub fn host(&self, id: &str) -> Option<&Host> {
        self.root.find_host(id)
    }
    pub fn host_mut(&mut self, id: &str) -> Option<&mut Host> {
        self.root.find_host_mut(id)
    }
    pub fn parent_id(&self, id: &str) -> Option<String> {
        self.root.parent_of(id).map(|g| g.id.clone())
    }
    pub fn all_hosts(&self) -> Vec<&Host> {
        let mut v = Vec::new();
        self.root.collect_hosts(&mut v);
        v
    }

    pub fn path_of(&self, id: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = id.to_string();
        while let Some(p) = self.root.parent_of(&cur) {
            if p.id == ROOT_ID {
                break;
            }
            out.push(p.name.clone());
            cur = p.id.clone();
        }
        out.reverse();
        out
    }

    pub fn remove(&mut self, id: &str) -> Option<Node> {
        if id == ROOT_ID { None } else { self.root.remove(id) }
    }

    #[allow(clippy::result_large_err)]
    pub fn insert(&mut self, group_id: &str, node: Node) -> Result<(), Node> {
        if let Node::Group(g) = &node
            && g.contains(group_id)
        {
            return Err(node);
        }
        match self.root.find_group_mut(group_id) {
            Some(target) => {
                match node {
                    Node::Group(g) => target.groups.push(g),
                    Node::Host(h) => target.hosts.push(h),
                }
                Ok(())
            }
            None => Err(node),
        }
    }

    pub fn reorder(&mut self, id: &str, delta: isize) {
        let Some(pid) = self.parent_id(id) else { return };
        let Some(p) = self.root.find_group_mut(&pid) else { return };
        fn shift<T>(v: &mut [T], i: usize, delta: isize) {
            let j = i as isize + delta;
            if j >= 0 && (j as usize) < v.len() {
                v.swap(i, j as usize);
            }
        }
        if let Some(i) = p.hosts.iter().position(|h| h.id == id) {
            shift(&mut p.hosts, i, delta);
        } else if let Some(i) = p.groups.iter().position(|g| g.id == id) {
            shift(&mut p.groups, i, delta);
        }
    }

    pub fn find_by_name(&self, q: &str) -> Option<&Host> {
        let hosts = self.all_hosts();
        hosts
            .iter()
            .find(|h| h.id == q)
            .or_else(|| hosts.iter().find(|h| h.name == q))
            .or_else(|| hosts.iter().find(|h| h.name.eq_ignore_ascii_case(q)))
            .or_else(|| hosts.iter().find(|h| h.address == q))
            .copied()
    }

    pub fn import_ssh_config(&mut self, path: &std::path::Path) -> Result<usize> {
        let text = std::fs::read_to_string(path)?;
        let mut hosts: Vec<Host> = Vec::new();
        let mut cur: Vec<Host> = Vec::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (k, v) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
                Some((k, v)) => {
                    (k.to_lowercase(), v.trim().trim_start_matches('=').trim().trim_matches('"').to_string())
                }
                None => continue,
            };
            match k.as_str() {
                "host" => {
                    hosts.append(&mut cur);
                    for alias in v.split_whitespace() {
                        if alias.contains(['*', '?', '!']) {
                            continue;
                        }
                        cur.push(Host { name: alias.into(), address: alias.into(), ..Host::default() });
                    }
                }
                "match" => hosts.append(&mut cur),
                "hostname" => cur.iter_mut().for_each(|h| h.address = v.clone()),
                "user" => cur.iter_mut().for_each(|h| h.user = v.clone()),
                "port" => {
                    if let Ok(p) = v.parse() {
                        cur.iter_mut().for_each(|h| h.port = p)
                    }
                }
                "identityfile" => cur.iter_mut().for_each(|h| {
                    h.key_path = Some(v.clone());
                    h.auth = AuthKind::Key;
                }),
                _ => {}
            }
        }
        hosts.append(&mut cur);
        let existing: Vec<(String, u16, String)> =
            self.all_hosts().iter().map(|h| (h.address.clone(), h.port, h.user.clone())).collect();
        hosts.retain(|h| !existing.contains(&(h.address.clone(), h.port, h.user.clone())));
        let n = hosts.len();
        if n > 0 {
            let g = Group { name: "~/.ssh/config".into(), hosts, ..Group::default() };
            self.root.groups.push(g);
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tree_ops() {
        let mut s = Store::default();
        s.root.id = ROOT_ID.into();
        let g = Group { name: "prod".into(), ..Group::default() };
        let gid = g.id.clone();
        s.insert(ROOT_ID, Node::Group(g)).ok().unwrap();
        let h = Host { name: "web".into(), address: "10.0.0.1".into(), ..Host::default() };
        let hid = h.id.clone();
        s.insert(&gid, Node::Host(h)).ok().unwrap();
        assert_eq!(s.rows("").len(), 2);
        assert_eq!(s.path_of(&hid), vec!["prod".to_string()]);
        assert_eq!(s.rows("10.0.0").len(), 2);
        assert_eq!(s.rows("nomatch").len(), 0);
        let sub = Group { name: "sub".into(), ..Group::default() };
        let sub_id = sub.id.clone();
        s.insert(&gid, Node::Group(sub)).ok().unwrap();
        let n = s.remove(&gid).unwrap();
        let n = s.insert(&sub_id, n).unwrap_err();
        assert!(s.insert(ROOT_ID, n).is_ok());
        let toml = toml::to_string_pretty(&s).unwrap();
        let _: Store = toml::from_str(&toml).unwrap();
    }
}
