use std::{
    collections::BTreeMap,
    env, fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub default_profile: String,
    pub profiles: BTreeMap<String, Connection>,
    #[serde(default)]
    pub theme: crate::theme::Theme,
    #[serde(default)]
    pub table: Table,
    #[serde(default)]
    pub keybindings: crate::input::Keybindings,
    #[serde(default)]
    pub queries: BTreeMap<String, String>,
    #[serde(skip)]
    pub path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_profile: "development".into(),
            profiles: BTreeMap::from([(
                "development".into(),
                Connection {
                    tls: false,
                    ..Connection::default()
                },
            )]),
            theme: Default::default(),
            table: Default::default(),
            keybindings: Default::default(),
            queries: Default::default(),
            path: Default::default(),
        }
    }
}

impl Config {
    pub fn connection(&self) -> &Connection {
        &self.profiles[&self.default_profile]
    }

    pub fn select_profile(&mut self, name: String) -> Result<()> {
        if !self.profiles.contains_key(&name) {
            bail!("unknown profile {name:?}");
        }
        self.default_profile = name;
        Ok(())
    }

    pub fn initial_query(&self) -> &str {
        self.table
            .default_query
            .as_ref()
            .and_then(|name| self.queries.get(name))
            .map(String::as_str)
            .unwrap_or_default()
    }
}

// Deliberately no Debug: this structure contains credentials.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Connection {
    pub address: String,
    pub namespace: String,
    #[serde(default = "tls_default")]
    pub tls: bool,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub tls_ca: Option<PathBuf>,
    #[serde(default)]
    pub tls_cert: Option<PathBuf>,
    #[serde(default)]
    pub tls_key: Option<PathBuf>,
    #[serde(default)]
    pub tls_domain: Option<String>,
}

fn tls_default() -> bool {
    true
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            address: "localhost:7233".into(),
            namespace: "default".into(),
            tls: true,
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
    pub overflow: Overflow,
    pub refresh_interval_secs: u32,
    pub columns: Vec<Column>,
    pub page_size: Option<u16>,
    pub default_query: Option<String>,
}

impl Default for Table {
    fn default() -> Self {
        Self {
            overflow: Overflow::default(),
            refresh_interval_secs: 30,
            columns: vec![
                Column::WorkflowId,
                Column::WorkflowType,
                Column::Status,
                Column::StartTime,
                Column::CloseTime,
            ],
            page_size: None,
            default_query: None,
        }
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Overflow {
    pub scroll_selected: bool,
}

impl Default for Overflow {
    fn default() -> Self {
        Self {
            scroll_selected: true,
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
    let source = read_source(&path, required)?;
    parse(&source, &path).with_context(|| format!("configuration {}", path.display()))
}

fn read_source(path: &Path, required: bool) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(source) => Ok(source),
        Err(error) if error.kind() == ErrorKind::NotFound && !required => {
            create_default(path)?;
            fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
        }
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn create_default(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("creating configuration directory {}", parent.display()))?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("creating configuration temporary file")?;
    temporary.write_all(include_bytes!("../config.example.toml"))?;
    temporary.as_file().sync_all()?;
    // Publish only a complete template, without replacing a concurrent launch's config.
    match temporary.persist_noclobber(path) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error).with_context(|| format!("creating {}", path.display())),
    }
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

fn parse(source: &str, path: &Path) -> Result<Config> {
    // Do not include TOML's source excerpts in errors: they may contain API keys.
    let mut config: Config = toml::from_str(source)
        .map_err(|error: toml::de::Error| anyhow::anyhow!("invalid TOML: {}", error.message()))?;
    config.path = path.to_path_buf();
    config.select_profile(config.default_profile.clone())?;
    for (name, connection) in &mut config.profiles {
        if name.trim().is_empty() || name.chars().any(char::is_control) {
            bail!("profile names must be nonempty and contain no control characters");
        }
        if connection.address.trim().is_empty() || connection.namespace.trim().is_empty() {
            bail!("profile {name:?} requires a nonempty address and namespace");
        }
        if connection
            .api_key
            .as_ref()
            .is_some_and(|key| key.trim().is_empty())
        {
            bail!("profile {name:?}: api_key must not be empty");
        }
        if connection.api_key.is_none()
            && (connection.tls_cert.is_some() != connection.tls_key.is_some())
        {
            bail!("profile {name:?}: set both tls_cert and tls_key");
        }
        if !connection.tls
            && (connection.api_key.is_some()
                || connection.tls_ca.is_some()
                || connection.tls_cert.is_some()
                || connection.tls_key.is_some()
                || connection.tls_domain.is_some())
        {
            bail!("profile {name:?}: API keys and TLS settings require tls = true");
        }
        for file in [
            &mut connection.tls_ca,
            &mut connection.tls_cert,
            &mut connection.tls_key,
        ]
        .into_iter()
        .flatten()
        {
            *file = path.parent().unwrap_or(Path::new(".")).join(&*file);
        }
    }
    if let Some(name) = &config.table.default_query
        && !config.queries.contains_key(name)
    {
        bail!("table.default_query must name a configured query");
    }
    if config
        .queries
        .keys()
        .any(|name| name.trim().is_empty() || name.chars().any(char::is_control))
    {
        bail!("query names must be nonempty and contain no control characters");
    }
    config.keybindings.validate()?;
    if config.table.columns.is_empty() {
        bail!("table.columns must not be empty");
    }
    if config
        .table
        .page_size
        .is_some_and(|size| !(1..=1000).contains(&size))
    {
        bail!("table.page_size must be between 1 and 1000");
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_scrolling_defaults_on_and_can_be_disabled() {
        for (source, enabled) in [
            ("", true),
            ("[overflow]", true),
            ("[overflow]\nscroll_selected = false", false),
        ] {
            let table: Table = toml::from_str(source).unwrap();
            assert_eq!(table.overflow.scroll_selected, enabled);
        }
    }
    fn read(source: &str) -> Result<Config> {
        parse(source, Path::new("/config/temporat/config.toml"))
    }

    const BASE: &str = "default_profile = 'dev'\n[profiles.dev]\naddress = 'localhost:7233'\nnamespace = 'default'\n";

    fn settings(source: &str) -> Result<Config> {
        read(&format!("{BASE}{source}"))
    }

    #[test]
    fn refresh_interval_defaults_to_thirty_seconds_and_accepts_zero_to_start_paused() {
        assert_eq!(settings("").unwrap().table.refresh_interval_secs, 30);
        assert_eq!(
            settings("[table]\nrefresh_interval_secs = 0")
                .unwrap()
                .table
                .refresh_interval_secs,
            0
        );
        assert_eq!(
            settings("[table]\nrefresh_interval_secs = 45")
                .unwrap()
                .table
                .refresh_interval_secs,
            45
        );
        assert!(settings("[table]\nrefresh_interval_secs = -1").is_err());
        assert!(settings("[table]\nrefresh_interval_secs = '30s'").is_err());
    }

    #[test]
    fn example_is_valid_and_profiles_are_required() {
        read(include_str!("../config.example.toml")).unwrap();
        assert!(read("").is_err());
        assert!(read("[connection]\nnamespace = 'default'").is_err());
    }

    #[test]
    fn explicitly_selected_missing_files_are_errors() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing.toml");
        assert!(load(Some(path.clone())).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn first_launch_creates_a_private_template() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("temporat/config.toml");
        let source = read_source(&path, false).unwrap();
        assert_eq!(source, include_str!("../config.example.toml"));
        let config = parse(&source, &path).unwrap();
        assert_eq!(config.connection().address, "localhost:7233");
        assert_eq!(config.connection().namespace, "default");
        assert!(!config.connection().tls);
        assert!(config.connection().api_key.is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
        assert_eq!(read_source(&path, false).unwrap(), source);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn initialization_preserves_existing_files_even_if_invalid() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        for source in [
            "# Custom configuration\n[connection]\nnamespace = 'mine'",
            "[broken",
        ] {
            fs::write(&path, source).unwrap();
            // A file created after the initial read must not be overwritten either.
            create_default(&path).unwrap();
            assert_eq!(read_source(&path, false).unwrap(), source);
        }
    }

    #[test]
    fn profiles_are_explicit_and_startup_selection_is_validated() {
        let mut config = settings("[profiles.cloud]\naddress = 'region.example:7233'\nnamespace = 'team.account'\napi_key = 'secret'").unwrap();
        assert!(config.connection().tls);
        config.select_profile("cloud".into()).unwrap();
        assert_eq!(config.connection().namespace, "team.account");
        assert_eq!(config.connection().api_key.as_deref(), Some("secret"));
        assert!(config.select_profile("missing".into()).is_err());
        assert_eq!(config.default_profile, "cloud");
        for source in [
            "default_profile = 'missing'\n[profiles]",
            "default_profile = 'dev'\n[profiles.dev]\nnamespace = 'default'",
            "default_profile = 'dev'\n[profiles.dev]\naddress = 'localhost:7233'",
        ] {
            assert!(read(source).is_err());
        }
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
        let config = settings("tls_cert = 'client.pem'\ntls_key = '/keys/client.key'").unwrap();
        assert_eq!(
            config.connection().tls_cert.as_ref().unwrap(),
            Path::new("/config/temporat/client.pem")
        );
        assert_eq!(
            config.connection().tls_key.as_ref().unwrap(),
            Path::new("/keys/client.key")
        );
    }

    #[test]
    fn rejects_invalid_settings() {
        for source in [
            "typo = true",
            "[theme]\nname = 'unknown'",
            "[theme]\nbackground = 'unsupported'",
            "[table]\ncolumns = []",
            "[table]\ncolumns = ['unknown']",
            "[table]\npage_size = 0",
            "tls_cert = 'only.pem'",
            "tls = false\napi_key = 'secret'",
            "[profiles.other]\naddress = ''\nnamespace = 'default'",
            "[profiles.other]\naddress = 'localhost:7233'\nnamespace = ''",
        ] {
            assert!(settings(source).is_err(), "accepted {source}");
        }
    }

    #[test]
    fn preserves_column_order() {
        let config = settings("[table]\ncolumns = ['status', 'workflow_id']").unwrap();
        assert_eq!(config.table.columns, [Column::Status, Column::WorkflowId]);
    }

    #[test]
    fn resolves_named_startup_query_and_retains_config_path() {
        let config = settings("[table]\ndefault_query = 'Running'\n[queries]\nRunning = \"ExecutionStatus = 'Running'\"\nAll = ''").unwrap();
        assert_eq!(config.initial_query(), "ExecutionStatus = 'Running'");
        assert_eq!(config.path, Path::new("/config/temporat/config.toml"));
        assert_eq!(settings("").unwrap().initial_query(), "");
        assert!(settings("[table]\ndefault_query = 'Missing'").is_err());
        assert!(settings("[queries]\n' ' = ''").is_err());
    }
}
