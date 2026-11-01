use std::{io, process::Stdio, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    process::{Child, ChildStdout, Command},
};

use crate::error::{AppError, AppResult};

const KUBECTL_TIMEOUT: Duration = Duration::from_secs(15);
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KubeOptions {
    /// Always passed as --context, so the forward never follows kubectl's current context.
    pub context: String,
    pub namespace: String,
    /// What `kubectl port-forward` takes: `pod/name`, `svc/name` or `deploy/name`.
    pub resource: String,
    /// Empty looks for kubectl on PATH.
    pub kubectl_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KubeContexts {
    pub contexts: Vec<String>,
    pub current: Option<String>,
}

/// A `kubectl port-forward` on a 127.0.0.1 port kubectl picked. Dropping it kills kubectl.
pub struct KubeForward {
    port: u16,
    _child: Child,
}

impl KubeForward {
    pub fn local_port(&self) -> u16 {
        self.port
    }

    pub async fn open(
        options: &KubeOptions,
        remote_port: u16,
        timeout: Duration,
    ) -> AppResult<Self> {
        let context = non_empty(&options.context)
            .ok_or_else(|| AppError::general("Choose a Kubernetes context"))?;
        let resource = non_empty(&options.resource)
            .ok_or_else(|| AppError::general("Enter the pod or service to forward to"))?;

        let mut command = kubectl(&options.kubectl_path);
        command.args(["--context", context]);
        if let Some(namespace) = non_empty(&options.namespace) {
            command.args(["--namespace", namespace]);
        }
        command
            .args([
                "port-forward",
                "--address",
                "127.0.0.1",
                resource,
                &format!(":{remote_port}"),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command
            .spawn()
            .map_err(|error| spawn_error(&options.kubectl_path, error))?;
        let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
        let mut stderr = child.stderr.take().expect("stderr is piped");

        let port = match tokio::time::timeout(timeout, read_port(&mut stdout)).await {
            Err(_) => {
                return Err(AppError::general(
                    "Timed out waiting for kubectl port-forward to start",
                ));
            }
            Ok(Err(error)) => {
                return Err(AppError::general(format!(
                    "Couldn't read kubectl's output: {error}"
                )));
            }
            Ok(Ok(Some(port))) => port,
            Ok(Ok(None)) => {
                let mut message = String::new();
                let _ = stderr.read_to_string(&mut message).await;
                return Err(AppError::general(match message.trim() {
                    "" => "kubectl port-forward exited before forwarding the port".to_string(),
                    message => format!("kubectl port-forward failed: {message}"),
                }));
            }
        };

        // kubectl logs every accepted connection, so its pipes must keep draining or it blocks.
        tauri::async_runtime::spawn(async move {
            let _ = tokio::io::copy(&mut stdout, &mut tokio::io::sink()).await;
        });
        tauri::async_runtime::spawn(async move {
            let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        });

        Ok(Self {
            port,
            _child: child,
        })
    }
}

async fn read_port(stdout: &mut BufReader<ChildStdout>) -> io::Result<Option<u16>> {
    let mut line = String::new();
    loop {
        line.clear();
        if stdout.read_line(&mut line).await? == 0 {
            return Ok(None);
        }
        if let Some(port) = forwarded_port(&line) {
            return Ok(Some(port));
        }
    }
}

/// Reads `Forwarding from 127.0.0.1:54321 -> 5432`.
fn forwarded_port(line: &str) -> Option<u16> {
    line.trim()
        .strip_prefix("Forwarding from 127.0.0.1:")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

#[tauri::command]
pub async fn list_kube_contexts(kubectl_path: String) -> AppResult<KubeContexts> {
    let contexts = kubectl_output(
        &kubectl_path,
        &["config", "get-contexts", "--output", "name"],
    )
    .await?
    .lines()
    .filter_map(non_empty)
    .map(str::to_string)
    .collect();
    // kubectl fails when no current context is set, which only means there is none.
    let current = kubectl_output(&kubectl_path, &["config", "current-context"])
        .await
        .ok()
        .and_then(|output| non_empty(&output).map(str::to_string));
    Ok(KubeContexts { contexts, current })
}

async fn kubectl_output(path: &str, args: &[&str]) -> AppResult<String> {
    let mut command = kubectl(path);
    command.args(args).stdin(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(KUBECTL_TIMEOUT, command.output())
        .await
        .map_err(|_| AppError::general("kubectl didn't answer in time"))?
        .map_err(|error| spawn_error(path, error))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::general(format!(
            "kubectl failed: {}",
            message.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(windows)]
fn kubectl(path: &str) -> Command {
    let mut command = Command::new(kubectl_program(path));
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

#[cfg(not(windows))]
fn kubectl(path: &str) -> Command {
    Command::new(kubectl_program(path))
}

/// Apps started from the Finder don't get the shell's PATH, so Homebrew's kubectl is looked for by hand.
fn kubectl_program(path: &str) -> String {
    if let Some(path) = non_empty(path) {
        return path.to_string();
    }
    #[cfg(target_os = "macos")]
    for candidate in ["/opt/homebrew/bin/kubectl", "/usr/local/bin/kubectl"] {
        if std::path::Path::new(candidate).is_file() {
            return candidate.to_string();
        }
    }
    "kubectl".to_string()
}

fn spawn_error(path: &str, error: io::Error) -> AppError {
    if error.kind() == io::ErrorKind::NotFound {
        return AppError::general(match non_empty(path) {
            None => "kubectl wasn't found on PATH. Enter its location in the connection's Kubernetes settings.".to_string(),
            Some(path) => format!("kubectl wasn't found at {path}"),
        });
    }
    AppError::general(format!("Couldn't start kubectl: {error}"))
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::forwarded_port;

    #[test]
    fn reads_the_local_port_from_kubectl_output() {
        assert_eq!(
            forwarded_port("Forwarding from 127.0.0.1:54321 -> 5432\n"),
            Some(54321)
        );
        assert_eq!(forwarded_port("Forwarding from [::1]:54321 -> 5432"), None);
        assert_eq!(forwarded_port("Handling connection for 54321"), None);
    }
}
