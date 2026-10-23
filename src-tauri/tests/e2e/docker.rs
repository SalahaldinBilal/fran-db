use std::{
    env,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const WAIT_FOR_DOCKER: Duration = Duration::from_secs(60);

/// With Docker inside WSL, holds a `wsl.exe` open so the distro boots and isn't stopped as idle.
pub struct DockerHost {
    keepalive: Option<Child>,
}

impl Drop for DockerHost {
    fn drop(&mut self) {
        if let Some(mut child) = self.keepalive.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn tcp_address() -> Option<(String, SocketAddr)> {
    let value = env::var("DOCKER_HOST").ok()?;
    let authority = value.strip_prefix("tcp://")?.split('/').next()?.to_string();
    let address = authority.to_socket_addrs().ok()?.next()?;
    Some((value, address))
}

fn reachable(address: &SocketAddr) -> bool {
    TcpStream::connect_timeout(address, Duration::from_secs(1)).is_ok()
}

fn start_wsl_keepalive() -> Result<Child, String> {
    let mut command = Command::new("wsl.exe");
    if let Ok(distro) = env::var("FRAN_E2E_WSL_DISTRO") {
        command.args(["-d", &distro]);
    }
    command
        .args(["--exec", "sleep", "infinity"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Couldn't start wsl.exe to boot the Docker distro: {error}"))
}

pub fn prepare() -> Result<DockerHost, String> {
    let Some((docker_host, address)) = tcp_address() else {
        return Ok(DockerHost { keepalive: None });
    };

    let keepalive = if cfg!(windows) && address.ip().is_loopback() {
        Some(start_wsl_keepalive()?)
    } else {
        None
    };
    let host = DockerHost { keepalive };

    let started = Instant::now();
    while !reachable(&address) {
        if started.elapsed() > WAIT_FOR_DOCKER {
            return Err(format!(
                "Docker isn't reachable at {docker_host} (DOCKER_HOST). Check that dockerd is running and listening on that address."
            ));
        }
        thread::sleep(Duration::from_millis(500));
    }

    Ok(host)
}
