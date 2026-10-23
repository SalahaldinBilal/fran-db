use std::{future::Future, io, net::SocketAddr, sync::Arc};

use tauri::async_runtime::JoinHandle;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
};

/// A 127.0.0.1 port that relays every accepted connection to a remote stream,
/// for transports a driver can't reach directly (SSH channels, Windows named pipes).
pub struct LocalForward {
    pub port: u16,
    task: JoinHandle<()>,
}

impl Drop for LocalForward {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl LocalForward {
    pub async fn start<Open, Opening, Remote>(open_remote: Open) -> io::Result<Self>
    where
        Open: Fn(SocketAddr) -> Opening + Send + Sync + 'static,
        Opening: Future<Output = io::Result<Remote>> + Send + 'static,
        Remote: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let open_remote = Arc::new(open_remote);

        let task = tauri::async_runtime::spawn(async move {
            while let Ok((mut socket, peer)) = listener.accept().await {
                let open_remote = open_remote.clone();
                tauri::async_runtime::spawn(async move {
                    if let Ok(mut remote) = open_remote(peer).await {
                        let _ = tokio::io::copy_bidirectional(&mut socket, &mut remote).await;
                    }
                });
            }
        });

        Ok(Self { port, task })
    }
}

#[cfg(windows)]
pub async fn open_named_pipe(path: &str) -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use std::time::Duration;
    use tokio::net::windows::named_pipe::ClientOptions;

    const ERROR_PIPE_BUSY: i32 = 231;
    const MAX_BUSY_RETRIES: u32 = 40;

    let mut retries = 0;
    loop {
        match ClientOptions::new().open(path) {
            Ok(client) => return Ok(client),
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) && retries < MAX_BUSY_RETRIES => {
                retries += 1;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return Err(error),
        }
    }
}
