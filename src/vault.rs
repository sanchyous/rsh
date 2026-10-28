use crate::util::{hex_decode, hex_encode, write_private};
use anyhow::{Context, Result, anyhow, bail};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use zeroize::Zeroizing;

const CHECK_AAD: &str = "rsh:vault-check";
const CHECK_PLAIN: &str = "rsh-vault-ok";
const PREFIX: &str = "v1";

#[derive(Serialize, Deserialize, Clone)]
struct VaultFile {
    version: u32,
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
    salt: String,
    check: String,
}

pub struct Vault {
    file: Option<VaultFile>,
    key: Option<Zeroizing<[u8; 32]>>,
}

pub type Secret = Zeroizing<String>;

fn path() -> PathBuf {
    crate::config::config_dir().join("vault.toml")
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    rand::rng().fill_bytes(&mut b);
    b
}

fn derive(pw: &str, salt: &[u8], f: &VaultFile) -> Result<Zeroizing<[u8; 32]>> {
    let params =
        argon2::Params::new(f.m_cost, f.t_cost, f.p_cost, Some(32)).map_err(|e| anyhow!("argon2 params: {e}"))?;
    let a2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; 32]);
    a2.hash_password_into(pw.as_bytes(), salt, out.as_mut()).map_err(|e| anyhow!("argon2: {e}"))?;
    Ok(out)
}

fn seal_with(key: &[u8; 32], aad: &str, plain: &[u8]) -> Result<String> {
    let cipher = XChaCha20Poly1305::new(&Key::from(*key));
    let nonce = random::<24>();
    let ct = cipher
        .encrypt(&XNonce::from(nonce), Payload { msg: plain, aad: aad.as_bytes() })
        .map_err(|_| anyhow!("encryption failed"))?;
    Ok(format!("{PREFIX}:{}:{}", hex_encode(&nonce), hex_encode(&ct)))
}

fn open_with(key: &[u8; 32], aad: &str, sealed: &str) -> Result<Zeroizing<Vec<u8>>> {
    let mut it = sealed.splitn(3, ':');
    let (Some(PREFIX), Some(n), Some(c)) = (it.next(), it.next(), it.next()) else {
        bail!("malformed secret");
    };
    let nonce: [u8; 24] = hex_decode(n).and_then(|v| v.try_into().ok()).ok_or_else(|| anyhow!("bad nonce"))?;
    let ct = hex_decode(c).ok_or_else(|| anyhow!("bad ciphertext"))?;
    let cipher = XChaCha20Poly1305::new(&Key::from(*key));
    let pt = cipher
        .decrypt(&XNonce::from(nonce), Payload { msg: &ct, aad: aad.as_bytes() })
        .map_err(|_| anyhow!("decryption failed (wrong key or tampered data)"))?;
    Ok(Zeroizing::new(pt))
}

impl Vault {
    pub fn load() -> Self {
        let file = std::fs::read_to_string(path()).ok().and_then(|s| toml::from_str(&s).ok());
        Self { file, key: None }
    }

    pub fn exists(&self) -> bool {
        self.file.is_some()
    }

    pub fn is_unlocked(&self) -> bool {
        self.key.is_some()
    }

    pub fn create(&mut self, pw: &str) -> Result<()> {
        if pw.chars().count() < 6 {
            bail!(crate::i18n::t("vault.too_short"));
        }
        let salt = random::<16>();
        let mut f = VaultFile {
            version: 1,
            m_cost: 64 * 1024,
            t_cost: 3,
            p_cost: 4,
            salt: hex_encode(&salt),
            check: String::new(),
        };
        let key = derive(pw, &salt, &f)?;
        f.check = seal_with(&key, CHECK_AAD, CHECK_PLAIN.as_bytes())?;
        write_private(&path(), toml::to_string_pretty(&f)?.as_bytes()).context("write vault")?;
        self.file = Some(f);
        self.key = Some(key);
        Ok(())
    }

    pub fn unlock(&mut self, pw: &str) -> Result<()> {
        let f = self.file.as_ref().ok_or_else(|| anyhow!("vault not initialised"))?;
        let salt = hex_decode(&f.salt).ok_or_else(|| anyhow!("bad salt"))?;
        let key = derive(pw, &salt, f)?;
        match open_with(&key, CHECK_AAD, &f.check) {
            Ok(v) if v.as_slice() == CHECK_PLAIN.as_bytes() => {
                self.key = Some(key);
                Ok(())
            }
            _ => bail!(crate::i18n::t("vault.wrong")),
        }
    }

    pub fn lock(&mut self) {
        self.key = None;
    }

    pub fn seal(&self, aad: &str, plain: &str) -> Result<String> {
        let key = self.key.as_ref().ok_or_else(|| anyhow!(crate::i18n::t("vault.locked")))?;
        seal_with(key, aad, plain.as_bytes())
    }

    pub fn open(&self, aad: &str, sealed: &str) -> Result<Secret> {
        let key = self.key.as_ref().ok_or_else(|| anyhow!(crate::i18n::t("vault.locked")))?;
        let pt = open_with(key, aad, sealed)?;
        Ok(Zeroizing::new(String::from_utf8(pt.to_vec()).context("secret is not utf-8")?))
    }

    pub fn change_password(&mut self, new_pw: &str, store: &mut crate::store::Store) -> Result<()> {
        if !self.is_unlocked() {
            bail!(crate::i18n::t("vault.locked"));
        }
        let mut plain: Vec<(String, Option<Secret>, Option<Secret>)> = Vec::new();
        for h in store.all_hosts() {
            let p = h.secret.as_deref().map(|s| self.open(&h.id, s)).transpose()?;
            let k = h.key_secret.as_deref().map(|s| self.open(&format!("{}:key", h.id), s)).transpose()?;
            plain.push((h.id.clone(), p, k));
        }
        let old_file = self.file.clone();
        let old_key = self.key.clone();
        self.create(new_pw)?;
        let res: Result<()> = (|| {
            for (id, p, k) in &plain {
                let sp = p.as_ref().map(|p| self.seal(id, p)).transpose()?;
                let sk = k.as_ref().map(|k| self.seal(&format!("{id}:key"), k)).transpose()?;
                if let Some(h) = store.host_mut(id) {
                    h.secret = sp;
                    h.key_secret = sk;
                }
            }
            store.save()
        })();
        if res.is_err() {
            self.file = old_file;
            self.key = old_key;
            if let Some(f) = &self.file {
                let _ = write_private(&path(), toml::to_string_pretty(f)?.as_bytes());
            }
        }
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seal_open_roundtrip_and_aad_binding() {
        let key = random::<32>();
        let s = seal_with(&key, "host-a", b"hunter2").unwrap();
        assert_eq!(open_with(&key, "host-a", &s).unwrap().as_slice(), b"hunter2");
        assert!(open_with(&key, "host-b", &s).is_err());
        let other = random::<32>();
        assert!(open_with(&other, "host-a", &s).is_err());
    }
}
