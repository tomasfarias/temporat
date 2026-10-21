use std::{
    env,
    error::Error,
    fs,
    future::Future,
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde_json::{Value, json};
use temporalio_client::{errors::ClientConnectError, tonic::Status};

static LOG: OnceLock<Mutex<Log>> = OnceLock::new();

struct Log {
    file: fs::File,
    secrets: Vec<String>,
}

pub fn init<'a>(api_keys: impl IntoIterator<Item = &'a str>) -> Result<PathBuf> {
    let directory = directory(|key| env::var_os(key).map(PathBuf::from))?;
    fs::create_dir_all(&directory).context("creating debug log directory")?;
    // A separate, owner-only file for each invocation, including concurrent launches.
    let temporary = tempfile::Builder::new()
        .prefix("debug-")
        .suffix(".log")
        .tempfile_in(&directory)
        .context("creating debug log")?;
    let (file, path) = temporary.keep().context("keeping debug log")?;
    LOG.set(Mutex::new(Log::new(file, api_keys)))
        .map_err(|_| anyhow::anyhow!("debug logging already initialized"))?;
    Ok(path)
}

fn directory(get: impl Fn(&str) -> Option<PathBuf>) -> Result<PathBuf> {
    if let Some(base) = get("XDG_STATE_HOME").filter(|path| path.is_absolute()) {
        return Ok(base.join("temporat"));
    }
    let home = get("HOME")
        .filter(|path| path.is_absolute())
        .context("set HOME or XDG_STATE_HOME for --debug logs")?;
    Ok(home.join(".local/state/temporat"))
}

impl Log {
    fn new<'a>(file: fs::File, api_keys: impl IntoIterator<Item = &'a str>) -> Self {
        let mut secrets = Vec::new();
        for key in api_keys.into_iter().filter(|key| !key.is_empty()) {
            // Error messages can contain either literal or Debug-escaped strings.
            let escaped = format!("{key:?}");
            secrets.push(escaped[1..escaped.len() - 1].to_owned());
            secrets.push(key.to_owned());
        }
        secrets.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
        secrets.dedup();
        Self { file, secrets }
    }

    fn write(&mut self, event: &str, mut fields: Value) -> std::io::Result<()> {
        redact(&mut fields, &self.secrets);
        let entry = json!({
            "timestamp_ms": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            "event": event,
            "fields": fields,
        });
        writeln!(self.file, "{entry}")
    }
}

fn redact(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(text) => {
            for secret in secrets {
                *text = text.replace(secret, "[REDACTED]");
            }
        }
        Value::Array(values) => values.iter_mut().for_each(|value| redact(value, secrets)),
        Value::Object(values) => values.values_mut().for_each(|value| redact(value, secrets)),
        _ => {}
    }
}

pub fn record(event: &str, fields: Value) {
    if let Some(log) = LOG.get()
        && let Ok(mut log) = log.lock()
    {
        // Logging must not interrupt requests or write over the terminal UI.
        let _ = log.write(event, fields);
    }
}

fn status(status: &Status) -> Value {
    // Status's Display/Debug representations include metadata; select only these fields.
    json!({ "grpc_code": format!("{:?}", status.code()), "message": status.message() })
}

fn causes(error: &anyhow::Error) -> Vec<Value> {
    let mut result = Vec::new();
    let mut next: Option<&(dyn Error + 'static)> = Some(error.as_ref());
    while let Some(cause) = next {
        next = cause.source();
        let value = if let Some(error) = cause.downcast_ref::<Status>() {
            status(error)
        } else {
            match cause.downcast_ref::<ClientConnectError>() {
                Some(ClientConnectError::SystemInfoCallError(error)) => {
                    // The SDK does not mark this variant's Status as an error source.
                    // Follow it explicitly to retain nested TLS/socket failures.
                    next = error.source();
                    json!({ "operation": "GetSystemInfo", "error": status(error) })
                }
                Some(ClientConnectError::TonicTransportError(_)) => {
                    json!({ "message": "Server connection transport error" })
                }
                _ => json!({ "message": cause.to_string() }),
            }
        };
        result.push(value);
    }
    result
}

pub fn failure(operation: &str, error: &anyhow::Error) {
    if LOG.get().is_some() {
        record(
            "error",
            json!({ "operation": operation, "causes": causes(error) }),
        );
    }
}

pub async fn request<T>(operation: &str, future: impl Future<Output = Result<T>>) -> Result<T> {
    record("request_started", json!({ "operation": operation }));
    let start = Instant::now();
    let result = future.await;
    if let Err(error) = &result {
        failure(operation, error);
    }
    record(
        "request_finished",
        json!({
            "operation": operation, "success": result.is_ok(), "elapsed_ms": start.elapsed().as_millis(),
        }),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use temporalio_client::tonic::Code;

    #[test]
    fn records_full_error_chain_but_redacts_keys_and_omits_grpc_metadata() {
        let key = "private-\"quoted\\key";
        let mut grpc = Status::new(Code::Unauthenticated, format!("rejected {key} ({key:?})"));
        grpc.metadata_mut().insert(
            "authorization",
            "unrelated-metadata-secret".parse().unwrap(),
        );
        let error = anyhow::Error::new(ClientConnectError::SystemInfoCallError(grpc))
            .context("connecting to Temporal");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("debug.log");
        let mut log = Log::new(fs::File::create(&path).unwrap(), Some(key));
        log.write("error", json!({ "causes": causes(&error) }))
            .unwrap();
        log.write("finished", json!({ "success": false })).unwrap();
        let source = fs::read_to_string(path).unwrap();
        assert!(!source.contains("private-"));
        assert!(!source.contains("unrelated-metadata-secret"));
        assert!(!source.contains("authorization"));
        let entries = source
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        let causes = &entries[0]["fields"]["causes"];
        assert_eq!(causes[0]["message"], "connecting to Temporal");
        assert_eq!(causes[1]["error"]["grpc_code"], "Unauthenticated");
        assert!(
            causes[1]["error"]["message"]
                .as_str()
                .unwrap()
                .contains("[REDACTED]")
        );
        let nested = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "socket refused",
        ))
        .context("transport error")
        .context("connecting");
        assert_eq!(super::causes(&nested).len(), 3);
    }

    #[test]
    fn log_directory_uses_absolute_xdg_state_or_home() {
        assert_eq!(
            directory(|key| (key == "XDG_STATE_HOME").then(|| PathBuf::from("/state"))).unwrap(),
            PathBuf::from("/state/temporat")
        );
        assert_eq!(
            directory(|key| Some(PathBuf::from(if key == "HOME" {
                "/home/test"
            } else {
                "relative"
            })))
            .unwrap(),
            PathBuf::from("/home/test/.local/state/temporat")
        );
    }

    #[test]
    fn system_info_status_retains_nested_transport_causes() {
        let mut transport = Status::unknown("transport error");
        transport.set_source(std::sync::Arc::new(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "connection reset by peer",
        )));
        let error = anyhow::Error::new(ClientConnectError::SystemInfoCallError(transport))
            .context("connecting to Temporal");
        let chain = causes(&error);
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[1]["operation"], "GetSystemInfo");
        assert_eq!(chain[1]["error"]["grpc_code"], "Unknown");
        assert_eq!(chain[2]["message"], "connection reset by peer");
    }
}
