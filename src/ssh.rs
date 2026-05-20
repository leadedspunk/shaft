use anyhow::{Context, Result};

#[derive(Debug)]
pub struct NeedsPassword;

impl std::fmt::Display for NeedsPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "authentication requires a password")
    }
}

impl std::error::Error for NeedsPassword {}
use ssh2::Session;
use ssh2_config::{ParseRule, SshConfig};
use std::io::BufReader;
use std::net::TcpStream;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct SshTarget {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub identity_file: Option<PathBuf>,
    pub password: Option<String>,
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
            // bare hostname or ssh config alias — user resolved below
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
            password: None,
        };

        target.apply_ssh_config();

        // last resort: use local username
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
            // use first identity file that exists after tilde expansion
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

impl SshConnection {
    pub fn connect(target: &SshTarget) -> Result<Self> {
        let addr = format!("{}:{}", target.host, target.port);
        let tcp =
            TcpStream::connect(&addr).with_context(|| format!("TCP connect to {}", addr))?;

        let mut sess = Session::new()?;
        sess.set_tcp_stream(tcp);
        sess.handshake().context("SSH handshake")?;

        let passphrase = target.password.as_deref();

        // 1. pubkey (no passphrase, then with passphrase if provided)
        if try_pubkey_auth(&mut sess, &target.user, &target.identity_file, passphrase) {
            let home = get_remote_home(&mut sess)?;
            return Ok(SshConnection { session: sess, home });
        }

        // 2. ssh-agent
        if try_agent_auth(&mut sess, &target.user) {
            let home = get_remote_home(&mut sess)?;
            return Ok(SshConnection { session: sess, home });
        }

        // 3. login password
        if let Some(pw) = passphrase {
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

fn try_key(sess: &mut Session, user: &str, priv_key: &PathBuf, passphrase: Option<&str>) -> bool {
    let pub_key = priv_key.with_extension("pub");
    let pub_opt = if pub_key.exists() { Some(pub_key.as_path()) } else { None };

    // try without passphrase first (works for unprotected keys and is fast)
    if sess.userauth_pubkey_file(user, pub_opt, priv_key, None).is_ok() && sess.authenticated() {
        return true;
    }
    // try with passphrase if provided
    if let Some(pp) = passphrase {
        if sess.userauth_pubkey_file(user, pub_opt, priv_key, Some(pp)).is_ok()
            && sess.authenticated()
        {
            return true;
        }
    }
    false
}

fn try_pubkey_auth(sess: &mut Session, user: &str, id_file: &Option<PathBuf>, passphrase: Option<&str>) -> bool {
    // explicit identity file from ssh config
    if let Some(ref path) = id_file {
        if path.exists() && try_key(sess, user, path, passphrase) {
            return true;
        }
    }

    // default keys
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    for key_name in &["id_ed25519", "id_rsa", "id_ecdsa"] {
        let priv_key = home.join(".ssh").join(key_name);
        if !priv_key.exists() {
            continue;
        }
        if try_key(sess, user, &priv_key, passphrase) {
            return true;
        }
    }

    false
}

fn try_agent_auth(sess: &mut Session, user: &str) -> bool {
    let Ok(mut agent) = sess.agent() else { return false };
    if agent.connect().is_err() { return false };
    if agent.list_identities().is_err() { return false };
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
