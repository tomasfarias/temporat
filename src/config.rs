use std::{
    env, fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub connection: Connection,
    pub table: Table,
}

// Deliberately no Debug: this structure contains credentials.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Connection {
    pub address: String,
    pub namespace: String,
    pub api_key: Option<String>,
    pub tls_ca: Option<PathBuf>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub tls_domain: Option<String>,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            address: "http://localhost:7233".into(),
            namespace: "default".into(),
            api_key: None,
            tls_ca: None,
            tls_cert: None,
            tls_key: None,
            tls_domain: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Table {
    pub columns: Vec<Column>,
    pub page_size: u16,
}

impl Default for Table {
    fn default() -> Self {
        Self {
            columns: vec![
                Column::WorkflowId,
                Column::WorkflowType,
                Column::Status,
                Column::StartTime,
            ],
            page_size: 100,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Column {
    WorkflowId,
    RunId,
    WorkflowType,
    Status,
    StartTime,
    CloseTime,
    TaskQueue,
    HistoryLength,
}

pub fn load(explicit: Option<PathBuf>) -> Result<Config> {
    let explicit = explicit.or_else(|| env::var_os("TEMPORAT_CONFIG").map(PathBuf::from));
    let required = explicit.is_some();
    let path = explicit
        .map(Ok)
        .unwrap_or_else(|| default_path(|key| env::var_os(key).map(PathBuf::from)))?;
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound && !required => String::new(),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    parse(&source, &path, |key| env::var(key).ok())
        .with_context(|| format!("configuration {}", path.display()))
}

fn default_path(get: impl Fn(&str) -> Option<PathBuf>) -> Result<PathBuf> {
    if let Some(base) = get("XDG_CONFIG_HOME").filter(|path| path.is_absolute()) {
        return Ok(base.join("temporat/config.toml"));
    }
    let home = get("HOME")
        .filter(|path| path.is_absolute())
        .context("set HOME, XDG_CONFIG_HOME, or --config")?;
    Ok(home.join(".config/temporat/config.toml"))
}

fn parse(source: &str, path: &Path, get: impl Fn(&str) -> Option<String>) -> Result<Config> {
    // Do not include TOML's source excerpts in errors: they may contain API keys.
    let mut document: toml::Table = toml::from_str(source)
        .map_err(|error: toml::de::Error| anyhow::anyhow!("invalid TOML: {}", error.message()))?;
    let connection = document
        .entry("connection")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let connection = connection
        .as_table_mut()
        .context("connection must be a TOML table")?;
    for (field, variable) in [
        ("address", "TEMPORAL_ADDRESS"),
        ("namespace", "TEMPORAL_NAMESPACE"),
        ("api_key", "TEMPORAL_API_KEY"),
        ("tls_ca", "TEMPORAL_TLS_CA"),
        ("tls_cert", "TEMPORAL_TLS_CERT"),
        ("tls_key", "TEMPORAL_TLS_KEY"),
        ("tls_domain", "TEMPORAL_TLS_DOMAIN"),
    ] {
        if let Some(value) = connection.get_mut(field) {
            if matches!(field, "tls_ca" | "tls_cert" | "tls_key") {
                let relative = Path::new(
                    value
                        .as_str()
                        .context("certificate paths must be strings")?,
                );
                *value = toml::Value::String(
                    path.parent()
                        .unwrap_or(Path::new("."))
                        .join(relative)
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        } else if let Some(value) = get(variable) {
            connection.insert(field.into(), toml::Value::String(value));
        }
    }
    let config: Config = document.try_into().map_err(|error: toml::de::Error| {
        anyhow::anyhow!("invalid settings: {}", error.message())
    })?;
    if config.connection.namespace.trim().is_empty() {
        bail!("connection.namespace must not be empty");
    }
    if config
        .connection
        .api_key
        .as_ref()
        .is_some_and(|key| key.trim().is_empty())
    {
        bail!("connection.api_key must not be empty");
    }
    if config.connection.api_key.is_none()
        && (config.connection.tls_cert.is_some() != config.connection.tls_key.is_some())
    {
        bail!("set both connection.tls_cert and connection.tls_key");
    }
    if config.table.columns.is_empty() {
        bail!("table.columns must not be empty");
    }
    if !(1..=1000).contains(&config.table.page_size) {
        bail!("table.page_size must be between 1 and 1000");
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(source: &str) -> Result<Config> {
        parse(source, Path::new("/config/temporat/config.toml"), |_| None)
    }

    #[test]
    fn example_and_empty_config_are_valid() {
        read(include_str!("../config.example.toml")).unwrap();
        assert_eq!(read("").unwrap().connection.namespace, "default");
    }

    #[test]
    fn explicitly_selected_missing_files_are_errors() {
        let directory = tempfile::tempdir().unwrap();
        assert!(load(Some(directory.path().join("missing.toml"))).is_err());
    }

    #[test]
    fn file_overrides_environment_per_field() {
        let config = parse(
            "[connection]\nnamespace = 'file'",
            Path::new("config.toml"),
            |key| match key {
                "TEMPORAL_NAMESPACE" => Some("env".into()),
                "TEMPORAL_API_KEY" => Some("secret".into()),
                _ => None,
            },
        )
        .unwrap();
        assert_eq!(config.connection.namespace, "file");
        assert_eq!(config.connection.api_key.as_deref(), Some("secret"));
    }

    #[test]
    fn xdg_requires_absolute_paths_and_falls_back_to_home() {
        let path = |xdg: &str| {
            default_path(|key| {
                Some(PathBuf::from(if key == "XDG_CONFIG_HOME" {
                    xdg
                } else {
                    "/home/test"
                }))
            })
            .unwrap()
        };
        assert_eq!(path("/config"), Path::new("/config/temporat/config.toml"));
        assert_eq!(
            path("relative"),
            Path::new("/home/test/.config/temporat/config.toml")
        );
    }

    #[test]
    fn file_certificate_paths_are_relative_to_config() {
        let config =
            read("[connection]\ntls_cert = 'client.pem'\ntls_key = '/keys/client.key'").unwrap();
        assert_eq!(
            config.connection.tls_cert.unwrap(),
            Path::new("/config/temporat/client.pem")
        );
        assert_eq!(
            config.connection.tls_key.unwrap(),
            Path::new("/keys/client.key")
        );
    }

    #[test]
    fn rejects_invalid_settings() {
        for source in [
            "typo = true",
            "[table]\ncolumns = []",
            "[table]\ncolumns = ['unknown']",
            "[table]\npage_size = 0",
            "[connection]\ntls_cert = 'only.pem'",
            "[connection]\nnamespace = ''",
        ] {
            assert!(read(source).is_err(), "accepted {source}");
        }
    }

    #[test]
    fn preserves_column_order() {
        let config = read("[table]\ncolumns = ['status', 'workflow_id']").unwrap();
        assert_eq!(config.table.columns, [Column::Status, Column::WorkflowId]);
    }
}
