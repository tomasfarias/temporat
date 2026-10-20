use base64::{Engine, engine::general_purpose::STANDARD};
use temporalio_common::protos::temporal::api::common::v1::{Payload, Payloads};

pub fn payloads(payloads: Option<&Payloads>) -> String {
    match payloads {
        Some(payloads) if !payloads.payloads.is_empty() => payloads
            .payloads
            .iter()
            .enumerate()
            .map(|(index, value)| format!("Payload {}\n{}", index + 1, payload(value)))
            .collect::<Vec<_>>()
            .join("\n\n"),
        _ => "(none)".into(),
    }
}

fn payload(payload: &Payload) -> String {
    let encoding = payload
        .metadata
        .get("encoding")
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .unwrap_or("unknown");
    let data = match encoding {
        "binary/null" => "null".into(),
        "json/plain" | "json/protobuf" => {
            serde_json::from_slice::<serde_json::Value>(&payload.data)
                .map(|value| {
                    serde_json::to_string_pretty(&value).expect("JSON value is serializable")
                })
                .unwrap_or_else(|_| {
                    format!(
                        "Invalid JSON; data (base64): {}",
                        STANDARD.encode(&payload.data)
                    )
                })
        }
        _ => format!(
            "Data (base64, {} bytes): {}",
            payload.data.len(),
            STANDARD.encode(&payload.data)
        ),
    };
    format!("Encoding: {encoding}\n{data}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn formats_json_and_preserves_undecodable_payload_bytes() {
        let mut value = Payload {
            metadata: HashMap::from([("encoding".into(), b"json/plain".to_vec())]),
            data: br#"{"order":42}"#.to_vec(),
            ..Default::default()
        };
        assert!(payload(&value).contains("\"order\": 42"));
        value.data = vec![0xff, 0x00];
        assert!(payload(&value).contains("Invalid JSON; data (base64): /wA="));
        value
            .metadata
            .insert("encoding".into(), b"binary/encrypted".to_vec());
        let text = payload(&value);
        assert!(text.contains("binary/encrypted"));
        assert!(text.contains("/wA="));
        assert_eq!(payloads(None), "(none)");
    }
}
