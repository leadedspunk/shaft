use anyhow::{anyhow, Context, Result};

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
        // user@host or user@host:port
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

        let (user, host) = userhost
            .split_once('@')
            .ok_or_else(|| anyhow!("expected user@host, got {}", raw))?;

        let port = port_str
            .map(|p| p.parse::<u16>())
            .transpose()?
            .unwrap_or(22);

        let mut target = SshTarget {
            user: user.to_string(),
            host: host.to_string(),
            port,
            identity_file: None,
            password: None,
        };

        target.apply_ssh_config();
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
            if !ids.is_empty() {
                let p = &ids[0];
                let expanded = if p.starts_with("~") {
                    dirs::home_dir()
                        .map(|h| h.join(p.strip_prefix("~/").unwrap_or(p)))
                        .unwrap_or_else(|| p.clone())
                } else {
                    p.clone()
                };
                self.identity_file = Some(expanded);
            }
        }
    }
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

        // try pubkey auth first
        let authed = try_pubkey_auth(&mut sess, &target.user, &target.identity_file);

        if !authed {
            if let Some(ref pw) = target.password {
                sess.userauth_password(&target.user, pw)
                    .context("password auth")?;
            } else {
                // agent auth fallback
                let mut agent = sess.agent().context("ssh agent")?;
                agent.connect().context("agent connect")?;
                agent.list_identities().context("agent list")?;
                let mut agent_ok = false;
                for identity in agent.identities()? {
                    if agent.userauth(&target.user, &identity).is_ok() {
                        agent_ok = true;
                        break;
                    }
                }
                if !agent_ok {
                    return Err(anyhow::Error::new(NeedsPassword));
                }
            }
        }

        if !sess.authenticated() {
            return Err(anyhow!("SSH authentication failed"));
        }

        // determine remote home
        let home = get_remote_home(&mut sess)?;

        Ok(SshConnection { session: sess, home })
    }
}

fn try_pubkey_auth(sess: &mut Session, user: &str, id_file: &Option<PathBuf>) -> bool {
    // explicit identity file
    if let Some(ref path) = id_file {
        let pubkey = path.with_extension("pub");
        let pub_opt = if pubkey.exists() { Some(pubkey.as_path()) } else { None };
        if sess.userauth_pubkey_file(user, pub_opt, path, None).is_ok()
            && sess.authenticated()
        {
            return true;
        }
    }

    // default keys
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    for key_name in &["id_ed25519", "id_rsa", "id_ecdsa"] {
        let priv_key = home.join(".ssh").join(key_name);
        let pub_key = home.join(".ssh").join(format!("{}.pub", key_name));
        if !priv_key.exists() {
            continue;
        }
        let pub_opt = if pub_key.exists() { Some(pub_key.as_path()) } else { None };
        if sess
            .userauth_pubkey_file(user, pub_opt, &priv_key, None)
            .is_ok()
            && sess.authenticated()
        {
            return true;
        }
    }

    false
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
