use tokio::net::TcpStream;
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use log;
use russh::*;
use russh::keys::*;
use tokio::sync::Mutex;

pub struct SSHTunnel {
    pub host: String,
    pub user: String,
    private_key_path: Option<String>,
    password: Option<String>,
    port: Option<u16>,
    running: Arc<AtomicBool>,
    session: Arc<Mutex<Option<client::Handle<Client>>>>,
}

struct Client;

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

impl SSHTunnel {
    pub fn new(host: String, user: String, port: Option<u16>) -> Self {
        Self {
            host,
            user,
            private_key_path: None,
            password: None,
            running: Arc::new(AtomicBool::new(true)),
            port: Some(port.unwrap_or(22)),
            session: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_private_key_path(mut self, private_key_path: String) -> Self {
        self.private_key_path = Some(private_key_path);
        self
    }

    pub fn with_password(mut self, password: String) -> Self {
        self.password = Some(password);
        self
    }

    pub fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.running.store(false, Ordering::SeqCst);
        log::info!("SSH tunnel stop signal sent");
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub async fn forward_port(
        &self,
        local_port: u16,
        remote_port: u16,
        remote_host: String,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let key_pair = load_secret_key(self.private_key_path.as_ref().unwrap(), None)?;
        let config = client::Config::default();
        let config = Arc::new(config);

        let mut session = client::connect(config, (self.host.as_str(), self.port.unwrap()), Client).await?;

        let auth_res = session
            .authenticate_publickey(
                &self.user,
                PrivateKeyWithHashAlg::new(
                    Arc::new(key_pair),
                    session.best_supported_rsa_hash().await?.flatten(),
                ),
            )
            .await?;

        if !auth_res.success() {
            return Err("Authentication failed".into());
        }

        *self.session.lock().await = Some(session);

        let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", local_port)).await?;
        log::info!("Listening on localhost:{}", local_port);

        let session = self.session.clone();
        let running = self.running.clone();

        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                match listener.accept().await {
                    Ok((local_stream, addr)) => {
                        log::info!("Accepting connection from {}", addr);
                        let session = session.clone();
                        let remote_host = remote_host.clone();
                        let running = running.clone();

                        tokio::spawn(async move {
                            if let Err(e) = forward_connection(
                                session,
                                local_stream,
                                &remote_host,
                                remote_port,
                                local_port,
                                running,
                            ).await {
                                log::error!("Forwarding error: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        if running.load(Ordering::SeqCst) {
                            log::error!("Error accepting connection: {}", e);
                        }
                    }
                }
            }
            log::info!("SSH tunnel listener stopped");
        });

        Ok(())
    }
}

async fn forward_connection(
    session: Arc<Mutex<Option<client::Handle<Client>>>>,
    mut local_stream: TcpStream,
    remote_host: &str,
    remote_port: u16,
    local_port: u16,
    running: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let session = session.lock().await;
    if let Some(session) = &*session {
        let mut channel = session
            .channel_open_direct_tcpip(
                remote_host.to_string(),
                remote_port.into(),
                "127.0.0.1".to_string(),
                local_port.into(),
            )
            .await?;

        let mut stream_closed = false;
        let mut buf = vec![0; 65536];

        while running.load(Ordering::SeqCst) {
            tokio::select! {
                r = local_stream.read(&mut buf), if !stream_closed => {
                    match r {
                        Ok(0) => {
                            stream_closed = true;
                            channel.eof().await?;
                        },
                        Ok(n) => channel.data(&buf[..n]).await?,
                        Err(_) => break,
                    };
                },
                Some(msg) = channel.wait() => {
                    match msg {
                        ChannelMsg::Data { ref data } => {
                            local_stream.write_all(data).await?;
                        }
                        ChannelMsg::Eof => {
                            if !stream_closed {
                                channel.eof().await?;
                            }
                            break;
                        }
                        _ => {}
                    }
                },
            }
        }
    }
    Ok(())
}
