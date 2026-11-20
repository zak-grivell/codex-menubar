use crate::token_usage::{TokenTotals, TokenUsage};
use chrono::Utc;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    env,
    io::{self, BufRead, BufReader, Write},
    os::{fd::OwnedFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    time::{Duration, Instant},
};

const QUERY_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    used_percent: f64,
    window_duration_mins: Option<u32>,
    pub resets_at: Option<i64>,
}

impl Window {
    pub fn remaining(&self) -> f64 {
        (100.0 - self.used_percent).clamp(0.0, 100.0)
    }
}

#[derive(Default)]
pub struct Limits {
    pub five_hour: Option<Window>,
    pub weekly: Option<Window>,
}

#[derive(Default, Deserialize)]
struct Bucket {
    primary: Option<Window>,
    secondary: Option<Window>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Response {
    rate_limits: Bucket,
    rate_limits_by_limit_id: Option<std::collections::HashMap<String, Bucket>>,
}

impl Response {
    fn limits(self) -> Limits {
        let bucket = self
            .rate_limits_by_limit_id
            .and_then(|mut buckets| buckets.remove("codex"))
            .unwrap_or(self.rate_limits);
        let mut limits = Limits::default();
        for window in [bucket.primary, bucket.secondary].into_iter().flatten() {
            match window.window_duration_mins {
                Some(300) => limits.five_hour = Some(window),
                Some(10080) => limits.weekly = Some(window),
                _ => {}
            }
        }
        limits
    }
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn(executable: &Path, home: Option<&Path>) -> io::Result<(Server, UnixStream)> {
    let (reader, writer) = UnixStream::pair()?;
    let mut command = Command::new(executable);
    command
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::from(OwnedFd::from(writer)))
        .stderr(Stdio::null());
    if let Some(home) = home {
        command.current_dir(home);
    }
    Ok((Server(command.spawn()?), reader))
}

fn start() -> io::Result<(Server, UnixStream)> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let home = home.as_deref();
    if let Some(executable) = env::var_os("CODEX_BIN") {
        return spawn(Path::new(&executable), home);
    }

    let mut candidates = vec![PathBuf::from("codex")];
    for directory in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/run/current-system/sw/bin",
    ] {
        candidates.push(Path::new(directory).join("codex"));
    }
    if let Some(home) = home {
        candidates.push(home.join(".nix-profile/bin/codex"));
        candidates.push(home.join(".local/bin/codex"));
    }
    for executable in candidates {
        match spawn(&executable, home) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            result => return result,
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "Codex CLI not found",
    ))
}

fn send(input: &mut impl Write, message: Value) -> io::Result<()> {
    serde_json::to_writer(&mut *input, &message)?;
    input.write_all(b"\n")?;
    input.flush()
}

fn receive(
    output: &mut BufReader<UnixStream>,
    id: u64,
    deadline: Instant,
    rejected: &str,
) -> Result<Value, String> {
    let mut line = String::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Codex took too long to respond".into());
        }
        output
            .get_ref()
            .set_read_timeout(Some(remaining))
            .map_err(|error| error.to_string())?;
        line.clear();
        match output.read_line(&mut line) {
            Ok(0) => return Err("Codex app server closed the connection".into()),
            Ok(_) => {}
            Err(error) => {
                return Err(match error.kind() {
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                        "Codex took too long to respond".into()
                    }
                    _ => format!("Couldn’t read from Codex ({error})"),
                });
            }
        }
        let Ok(mut message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if message["id"].as_u64() != Some(id) {
            continue;
        }
        if let Some(result) = message.get_mut("result") {
            return Ok(result.take());
        }
        return Err(rejected.into());
    }
}

pub struct AccountClient {
    input: ChildStdin,
    output: BufReader<UnixStream>,
    _server: Server,
}

impl AccountClient {
    pub fn connect() -> Result<Self, String> {
        let (mut server, output) = start().map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => "Codex CLI not found. Install it or set CODEX_BIN.".into(),
            _ => format!("Couldn’t start Codex ({error})"),
        })?;
        let input = server
            .0
            .stdin
            .take()
            .ok_or("Couldn’t open the Codex app server connection")?;
        let mut client = Self {
            input,
            output: BufReader::new(output),
            _server: server,
        };
        client.send(json!({
            "id": 1,
            "method": "initialize",
            "params": {"clientInfo": {
                "name": "codex_menubar",
                "title": "Codex Menu Bar",
                "version": env!("CARGO_PKG_VERSION")
            }}
        }))?;
        receive(
            &mut client.output,
            1,
            Instant::now() + QUERY_TIMEOUT,
            "Couldn’t initialize the Codex app server",
        )?;
        client.send(json!({"method": "initialized"}))?;
        Ok(client)
    }

    fn send(&mut self, message: Value) -> Result<(), String> {
        send(&mut self.input, message).map_err(|error| format!("Couldn’t write to Codex ({error})"))
    }

    fn request<T: DeserializeOwned>(
        &mut self,
        id: u64,
        method: &str,
        rejected: &str,
    ) -> Result<T, String> {
        self.send(json!({"id": id, "method": method}))?;
        let response = receive(
            &mut self.output,
            id,
            Instant::now() + QUERY_TIMEOUT,
            rejected,
        )?;
        serde_json::from_value(response)
            .map_err(|_| format!("Codex returned an invalid response for {method}"))
    }

    pub fn limits(&mut self) -> Result<Limits, String> {
        self.request::<Response>(
            2,
            "account/rateLimits/read",
            "Codex couldn’t read your limits. Check your Codex CLI sign-in.",
        )
        .map(Response::limits)
    }

    pub fn token_totals(&mut self) -> Result<TokenTotals, String> {
        self.request::<TokenUsage>(
            3,
            "account/usage/read",
            "Codex couldn’t read token activity",
        )?
        .totals(Utc::now().date_naive())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_codex_bucket_and_matches_windows_by_duration() {
        let limits = serde_json::from_value::<Response>(json!({
            "rateLimits": {"primary": {"usedPercent": 77, "windowDurationMins": 300}},
            "rateLimitsByLimitId": {
                "codex": {
                    "primary": {"usedPercent": 20, "windowDurationMins": 10080, "resetsAt": 123},
                    "secondary": {"usedPercent": 10, "windowDurationMins": 300, "resetsAt": 456}
                },
                "codex_other": {"primary": {"usedPercent": 90, "windowDurationMins": 300}}
            }
        }))
        .unwrap()
        .limits();

        let window = limits.five_hour.unwrap();
        assert_eq!(window.used_percent, 10.0);
        assert_eq!(window.resets_at, Some(456));
        assert_eq!(limits.weekly.unwrap().resets_at, Some(123));
    }

    #[test]
    fn supports_legacy_responses_with_missing_windows() {
        let limits = serde_json::from_value::<Response>(json!({
            "rateLimits": {
                "primary": {"usedPercent": 30, "windowDurationMins": 300},
                "secondary": null
            }
        }))
        .unwrap()
        .limits();

        assert_eq!(limits.five_hour.unwrap().used_percent, 30.0);
        assert!(limits.weekly.is_none());
    }
}
