use std::{
    collections::BTreeMap,
    fs,
    io::{ErrorKind, Write},
    path::Path,
};

use anyhow::{Context, Result, bail};
use toml_edit::{DocumentMut, Item, Table, value};

pub fn save(path: &Path, name: &str, query: &str) -> Result<BTreeMap<String, String>> {
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        bail!("query names must be nonempty and contain no control characters");
    }
    // Follow an existing config symlink rather than replacing the link itself.
    let path = match fs::canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error).context("resolving configuration path"),
    };
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).context("reading configuration to save query"),
    };
    let mut document: DocumentMut = source.parse().map_err(|error: toml_edit::TomlError| {
        anyhow::anyhow!("invalid TOML: {}", error.message())
    })?;
    if !document.contains_key("queries") {
        document["queries"] = Item::Table(Table::new());
    }
    let queries = document["queries"]
        .as_table_like_mut()
        .context("queries must be a TOML table")?;
    let mut replacement = value(query);
    if let Some(previous) = queries.get(name).and_then(Item::as_value) {
        *replacement.as_value_mut().unwrap().decor_mut() = previous.decor().clone();
    }
    queries.insert(name, replacement);
    let saved = queries
        .iter()
        .map(|(name, query)| {
            Ok((
                name.to_owned(),
                query
                    .as_str()
                    .context("saved queries must be strings")?
                    .to_owned(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).context("creating configuration directory")?;
    // Only the on-disk TOML is edited: environment credentials never get serialized.
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).context("creating configuration temporary file")?;
    if let Ok(metadata) = fs::metadata(&path) {
        temporary
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    temporary.write_all(document.to_string().as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(&path)
        .context("replacing configuration file")?;
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_replaces_queries_without_rewriting_unrelated_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let prefix = "# My settings\n[connection]\nnamespace = 'custom' # keep this\n\n";
        fs::write(
            &path,
            format!("{prefix}[queries]\nRunning = 'old' # keep query comment\n"),
        )
        .unwrap();
        save(&path, "Running", "ExecutionStatus = 'Running'").unwrap();
        // A literal name containing TOML punctuation must not create nested tables.
        let saved = save(&path, "team.orders", "WorkflowId = 'quoted \"id\"'").unwrap();
        assert_eq!(saved["team.orders"], "WorkflowId = 'quoted \"id\"'");
        let source = fs::read_to_string(&path).unwrap();
        assert!(source.starts_with(prefix));
        assert!(source.contains("# keep query comment"));
        let config = crate::config::load(Some(path)).unwrap();
        assert_eq!(config.queries, saved);
    }

    #[test]
    fn creates_missing_config_and_does_not_clobber_invalid_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("temporat/config.toml");
        assert_eq!(save(&path, "All", "").unwrap()["All"], "");
        assert!(save(&path, " ", "query").is_err());
        for source in ["[broken", "queries = 42", "[queries]\nInvalid = 42"] {
            fs::write(&path, source).unwrap();
            assert!(save(&path, "All", "").is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), source);
        }
    }

    #[cfg(unix)]
    #[test]
    fn preserves_symlinks_and_existing_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let link = dir.path().join("link.toml");
        fs::write(&path, "").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&path, &link).unwrap();
        save(&link, "All", "").unwrap();
        assert!(link.is_symlink());
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}
