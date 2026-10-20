use std::{collections::HashMap, fs, path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use temporalio_client::grpc::WorkflowService;
use temporalio_client::{
    Client, ClientOptions, ClientTlsOptions, ConnectionOptions, TlsOptions, Url,
};
use temporalio_common::protos::temporal::api::workflowservice::v1::ListWorkflowExecutionsRequest;

use crate::{
    config::{Column, Connection},
    workflow::{self, Page},
};

pub async fn list_page(
    client: &Client,
    columns: &[Column],
    page_size: u16,
    next_page_token: Vec<u8>,
    query: &str,
) -> Result<Page> {
    let request = ListWorkflowExecutionsRequest {
        namespace: client.options().namespace.clone(),
        page_size: i32::from(page_size),
        next_page_token,
        query: query.to_owned(),
    };
    let response = tokio::time::timeout(
        Duration::from_secs(30),
        client
            .clone()
            .list_workflow_executions(temporalio_client::tonic::Request::new(request)),
    )
    .await
    .context("listing workflows timed out")?
    .context("listing workflows")?
    .into_inner();
    Ok(Page {
        rows: response
            .executions
            .into_iter()
            .map(|info| workflow::WorkflowRow::from_info(info, columns))
            .collect(),
        next_page_token: response.next_page_token,
    })
}

pub async fn connect(config: &Connection) -> Result<Client> {
    let options = connection_options(config)?;
    tokio::time::timeout(
        Duration::from_secs(15),
        Client::connect(options, ClientOptions::new(&config.namespace).build()),
    )
    .await
    .context("Temporal connection timed out")?
    .context("connecting to Temporal")
}

fn connection_options(config: &Connection) -> Result<ConnectionOptions> {
    let address = if config.address.contains("://") {
        config.address.clone()
    } else {
        format!("https://{}", config.address)
    };
    let target: Url = address
        .parse()
        .context("invalid connection.address endpoint")?;
    if !matches!(target.scheme(), "http" | "https") || target.host_str().is_none() {
        bail!(
            "connection.address must be a host with an optional port, or an http:// or https:// URL"
        );
    }
    if !target.username().is_empty()
        || target.password().is_some()
        || target.query().is_some()
        || target.fragment().is_some()
        || target.path() != "/"
    {
        bail!("connection.address must contain only a host, optional port, and optional scheme");
    }
    let needs_tls = config.api_key.is_some()
        || config.tls_ca.is_some()
        || config.tls_cert.is_some()
        || config.tls_key.is_some()
        || config.tls_domain.is_some();
    if needs_tls && target.scheme() != "https" {
        bail!("use an https:// connection.address with API keys or TLS settings");
    }
    if let Some(key) = &config.api_key
        && (key.trim().is_empty() || !key.bytes().all(|byte| (0x20..=0x7e).contains(&byte)))
    {
        bail!("connection.api_key must be nonempty printable ASCII");
    }
    let tls = if target.scheme() == "https" {
        // API keys take priority; ignored client certificate files are never opened.
        let client_tls = if config.api_key.is_some() {
            None
        } else {
            match (&config.tls_cert, &config.tls_key) {
                (Some(cert), Some(key)) => Some(
                    ClientTlsOptions::builder()
                        .client_cert(read_pem(cert)?)
                        .client_private_key(read_pem(key)?)
                        .build(),
                ),
                (None, None) => None,
                _ => bail!("set both connection.tls_cert and connection.tls_key"),
            }
        };
        Some(
            TlsOptions::builder()
                .maybe_server_root_ca_cert(config.tls_ca.as_deref().map(read_pem).transpose()?)
                .maybe_domain(config.tls_domain.clone())
                .maybe_client_tls_options(client_tls)
                .build(),
        )
    } else {
        None
    };
    Ok(ConnectionOptions::new(target)
        .identity(format!("temporat/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .maybe_api_key(config.api_key.clone())
        .maybe_tls_options(tls)
        .headers(HashMap::from([(
            "temporal-namespace".into(),
            config.namespace.clone(),
        )]))
        .build())
}

fn read_pem(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).with_context(|| format!("reading PEM file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enables_tls_for_https_and_prefers_api_keys_over_certificates() {
        let config = Connection {
            address: "https://example.com:7233".into(),
            api_key: Some("secret".into()),
            tls_cert: Some("nonexistent.pem".into()),
            tls_key: Some("nonexistent.key".into()),
            ..Default::default()
        };
        let options = connection_options(&config).unwrap();
        assert_eq!(options.api_key.as_deref(), Some("secret"));
        assert!(options.tls_options.unwrap().client_tls_options.is_none());
        assert!(
            connection_options(&Connection::default())
                .unwrap()
                .tls_options
                .is_none()
        );
    }

    #[test]
    fn loads_mtls_files_and_custom_ca() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cert.pem");
        fs::write(&path, b"test PEM bytes").unwrap();
        let config = Connection {
            address: "https://example.com:7233".into(),
            tls_cert: Some(path.clone()),
            tls_key: Some(path.clone()),
            tls_ca: Some(path),
            tls_domain: Some("override.example.com".into()),
            ..Default::default()
        };
        let tls = connection_options(&config).unwrap().tls_options.unwrap();
        assert_eq!(tls.server_root_ca_cert.unwrap(), b"test PEM bytes");
        assert_eq!(tls.domain.as_deref(), Some("override.example.com"));
        assert_eq!(
            tls.client_tls_options.unwrap().client_private_key,
            b"test PEM bytes"
        );
    }

    #[test]
    fn bare_endpoints_enable_tls_with_or_without_api_keys() {
        for address in [
            "namespace.account.tmprl.cloud:7233",
            "localhost:7233",
            "[::1]:7233",
        ] {
            for api_key in [None, Some("secret".into())] {
                let options = connection_options(&Connection {
                    address: address.into(),
                    api_key: api_key.clone(),
                    ..Default::default()
                })
                .unwrap();
                assert!(options.tls_options.is_some());
                assert_eq!(options.api_key, api_key);
            }
        }
    }

    #[test]
    fn rejects_invalid_addresses_and_plaintext_credentials() {
        for address in [
            "",
            "example.com:invalid-port",
            "ftp://example.com",
            "http://example.com/path",
            "http://user:secret@example.com",
            "http://example.com?key=secret",
        ] {
            assert!(
                connection_options(&Connection {
                    address: address.into(),
                    ..Default::default()
                })
                .is_err()
            );
        }
        assert!(
            connection_options(&Connection {
                api_key: Some("secret".into()),
                ..Default::default()
            })
            .is_err()
        );
    }
}
