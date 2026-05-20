use anyhow::{Context, Result};
use ssh2::Session;
use ssh2_config::{ParseRule, SshConfig};
use std::io::BufReader;
use std::net::TcpStream;
use std::path::PathBuf;

#[derive(Debug)]
pub struct NeedsPassword;

impl std::fmt::Display for NeedsPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no authentication method succeeded")
    }
}
impl std::error::Error for NeedsPassword {}

#[derive(Debug)]
pub struct NeedsKeyPassphrase(pub PathBuf);

impl std::fmt::Display for NeedsKeyPassphrase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "key {} requires a passphrase", self.0.display())
    }
}
impl std::error::Error for NeedsKeyPassphrase {}

#[derive(Debug, Clone)]
pub struct SshTarget {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub identity_file: Option<PathBuf>,
    pub key_passphrase: Option<String>, // for decrypting local private key
    pub password: Option<String>,        // for SSH server password auth
}

impl SshTarget {
    pub fn parse(raw: &str) -> Result<Self> {
        // accepted: host, host:port, user@host, user@host:port
        let (userhost, port_str) = if let Some(idx) = raw.rfind(':') {
            let maybe_port = &raw[idx + 1..];
            if maybe_port.parse::<u16>().is_ok() {
                (&raw[..idx], Some(maybe_port))
            } else {
                (raw, None)
            }
        } else {
            (raw, None)
        };

        let (user, host) = if let Some((u, h)) = userhost.split_once('@') {
            (u.to_string(), h.to_string())
        } else {
            (String::new(), userhost.to_string())
        };

        let port = port_str
            .map(|p| p.parse::<u16>())
            .transpose()?
            .unwrap_or(22);

        let mut target = SshTarget {
            user,
            host,
            port,
            identity_file: None,
            key_passphrase: None,
            password: None,
        };

        target.apply_ssh_config();

        if target.user.is_empty() {
            target.user = std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| whoami());
        }

        Ok(target)
    }

    fn apply_ssh_config(&mut self) {
        let config_path = dirs::home_dir()
            .map(|h| h.join(".ssh").join("config"))
            .filter(|p| p.exists());

        let Some(path) = config_path else { return };
        let Ok(file) = std::fs::File::open(&path) else { return };
        let mut reader = BufReader::new(file);

        let Ok(config) = SshConfig::default().parse(&mut reader, ParseRule::ALLOW_UNKNOWN_FIELDS)
        else {
            return;
        };

        let params = config.query(&self.host);

        if let Some(host) = params.host_name {
            self.host = host;
        }
        if let Some(port) = params.port {
            self.port = port;
        }
        if let Some(user) = params.user {
            if self.user.is_empty() {
                self.user = user;
            }
        }
        if let Some(ids) = params.identity_file {
            for p in &ids {
                let expanded = expand_tilde(p);
                if expanded.exists() {
                    self.identity_file = Some(expanded);
                    break;
                }
            }
        }
    }
}

fn expand_tilde(path: &PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    path.clone()
}

pub struct SshConnection {
    pub session: Session,
    pub home: PathBuf,
}

// LIBSSH2_ERROR_FILE = -16: can't open/decrypt private key (passphrase-protected)
const LIBSSH2_ERROR_FILE: i32 = -16;

enum KeyResult {
    Ok,
    NeedsPassphrase,
    Failed,
}

fn key_file_is_encrypted(path: &PathBuf) -> bool {
    // PEM legacy keys: "ENCRYPTED" in header or DEK-Info line
    // OpenSSH new format: header is always "BEGIN OPENSSH PRIVATE KEY" regardless of encryption;
    // encryption info is only in the binary body. Treat all OpenSSH-format keys as potentially
    // encrypted — empty passphrase works fine for unencrypted keys.
    std::fs::read_to_string(path)
        .map(|s| {
            s.contains("ENCRYPTED")
                || s.contains("DEK-Info:")
                || s.contains("BEGIN OPENSSH PRIVATE KEY")
        })
        .unwrap_or(false)
}

fn try_key_once(sess: &mut Session, user: &str, priv_key: &PathBuf, passphrase: Option<&str>) -> KeyResult {
    let pub_key = priv_key.with_extension("pub");
    let pub_opt = if pub_key.exists() { Some(pub_key.as_path()) } else { None };

    match sess.userauth_pubkey_file(user, pub_opt, priv_key, passphrase) {
        Ok(_) if sess.authenticated() => KeyResult::Ok,
        Err(e) => {
            // Only suggest passphrase when none was provided and we have evidence the key
            // is encrypted (libssh2 file error OR key file contains encryption markers)
            let libssh2_file_err = e.code() == ssh2::ErrorCode::Session(LIBSSH2_ERROR_FILE);
            if passphrase.is_none() && (libssh2_file_err || key_file_is_encrypted(priv_key)) {
                KeyResult::NeedsPassphrase
            } else {
                KeyResult::Failed
            }
        }
        Ok(_) => KeyResult::Failed,
    }
}

impl SshConnection {
    pub fn connect(target: &SshTarget) -> Result<Self> {
        let addr = format!("{}:{}", target.host, target.port);
        let tcp =
            TcpStream::connect(&addr).with_context(|| format!("TCP connect to {}", addr))?;

        let mut sess = Session::new()?;
        sess.set_tcp_stream(tcp);
        sess.handshake().context("SSH handshake")?;

        let key_pp = target.key_passphrase.as_deref();
        let server_pw = target.password.as_deref();

        // Build ordered list of keys to try: config key first, then defaults
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        let mut keys: Vec<PathBuf> = Vec::new();
        if let Some(ref k) = target.identity_file {
            if k.exists() {
                keys.push(k.clone());
            }
        }
        for name in &["id_ed25519", "id_rsa", "id_ecdsa"] {
            let p = home.join(".ssh").join(name);
            if p.exists() && target.identity_file.as_ref() != Some(&p) {
                keys.push(p);
            }
        }

        // Try each key
        for key_path in &keys {
            if let Some(pp) = key_pp {
                // Passphrase known: try directly. Harmless for unencrypted keys.
                match try_key_once(&mut sess, &target.user, key_path, Some(pp)) {
                    KeyResult::Ok => {
                        let home = get_remote_home(&mut sess)?;
                        return Ok(SshConnection { session: sess, home });
                    }
                    _ => {}
                }
            } else {
                // No passphrase yet: probe without one to detect encrypted keys.
                match try_key_once(&mut sess, &target.user, key_path, None) {
                    KeyResult::Ok => {
                        let home = get_remote_home(&mut sess)?;
                        return Ok(SshConnection { session: sess, home });
                    }
                    KeyResult::NeedsPassphrase => {
                        return Err(anyhow::Error::new(NeedsKeyPassphrase(key_path.clone())));
                    }
                    KeyResult::Failed => {}
                }
            }
        }

        // Try ssh-agent
        if try_agent_auth(&mut sess, &target.user) {
            let home = get_remote_home(&mut sess)?;
            return Ok(SshConnection { session: sess, home });
        }

        // Try server password (never use key_passphrase here — wrong credential type)
        if let Some(pw) = server_pw {
            sess.userauth_password(&target.user, pw)
                .context("password auth")?;
            if sess.authenticated() {
                let home = get_remote_home(&mut sess)?;
                return Ok(SshConnection { session: sess, home });
            }
        }

        Err(anyhow::Error::new(NeedsPassword))
    }
}

fn try_agent_auth(sess: &mut Session, user: &str) -> bool {
    let Ok(mut agent) = sess.agent() else { return false };
    if agent.connect().is_err() {
        return false;
    }
    if agent.list_identities().is_err() {
        return false;
    }
    for identity in agent.identities().unwrap_or_default() {
        if agent.userauth(user, &identity).is_ok() && sess.authenticated() {
            return true;
        }
    }
    false
}

fn whoami() -> String {
    std::process::Command::new("whoami")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "root".to_string())
}

fn get_remote_home(sess: &mut Session) -> Result<PathBuf> {
    let mut channel = sess.channel_session()?;
    channel.exec("echo $HOME")?;
    let mut output = String::new();
    std::io::Read::read_to_string(&mut channel, &mut output)?;
    channel.wait_close()?;
    let home = output.trim();
    if home.is_empty() {
        Ok(PathBuf::from("/home"))
    } else {
        Ok(PathBuf::from(home))
    }
}
