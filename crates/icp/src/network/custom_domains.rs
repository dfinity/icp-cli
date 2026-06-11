use std::collections::BTreeMap;

use candid::Principal;
use snafu::prelude::*;
use url::Url;

use crate::{prelude::*, store_id::IdMapping};

/// Writes a `custom-domains.txt` file to the given status directory.
///
/// Each line has the format `<canister_name>.<env_name>.<domain>:<principal>`.
/// The file is written fresh each time from the full set of current mappings
/// across all environments sharing this network.
///
/// `extra_entries` are raw `(full_domain, canister_id)` pairs appended after the
/// environment-based entries (e.g. system canisters like Internet Identity).
pub fn write_custom_domains(
    status_dir: &Path,
    domain: &str,
    env_mappings: &BTreeMap<String, IdMapping>,
    extra_entries: &[(String, String)],
) -> Result<(), WriteCustomDomainsError> {
    let file_path = status_dir.join("custom-domains.txt");
    let mut content: String = env_mappings
        .iter()
        .flat_map(|(env_name, mappings)| {
            mappings
                .iter()
                .map(move |(name, principal)| format!("{name}.{env_name}.{domain}:{principal}\n"))
        })
        .collect();
    for (full_domain, canister_id) in extra_entries {
        content.push_str(&format!("{full_domain}:{canister_id}\n"));
    }
    crate::fs::write(&file_path, content.as_bytes())?;
    Ok(())
}

/// Returns the custom domain entry for the II frontend canister, if II is enabled.
pub fn ii_custom_domain_entry(ii: bool, domain: &str) -> Option<(String, String)> {
    if ii {
        Some((
            format!("id.ai.{domain}"),
            icp_canister_interfaces::internet_identity::INTERNET_IDENTITY_FRONTEND_CID.to_string(),
        ))
    } else {
        None
    }
}

/// Extracts the domain authority from a gateway URL for use in subdomain-based
/// canister routing.
///
/// Returns `Some(domain)` if the URL has a domain name, or if it's a loopback
/// IP address (in which case `"localhost"` is returned). Returns `None` for
/// other IP addresses where subdomain routing is not available.
pub fn gateway_domain(http_gateway_url: &Url) -> Option<&str> {
    if let Some(domain) = http_gateway_url.domain() {
        Some(domain)
    } else if let Some(host) = http_gateway_url.host_str()
        && (host == "127.0.0.1" || host == "[::1]")
    {
        Some("localhost")
    } else {
        None
    }
}

/// Constructs a gateway URL for accessing a specific canister.
///
/// For managed networks with a status directory (where friendly domains are
/// registered), returns `http://<canister_name>.<env_name>.<domain>:<port>`.
///
/// Otherwise falls back to `http://<principal>.<domain>:<port>`, or
/// `http://<host>:<port>?canisterId=<principal>` if no subdomain is available.
pub fn canister_gateway_url(
    http_gateway_url: &Url,
    canister_id: Principal,
    friendly: Option<(&str, &str)>,
) -> Url {
    let domain = gateway_domain(http_gateway_url);
    let mut url = http_gateway_url.clone();
    match (friendly, domain) {
        (Some((canister_name, env_name)), Some(domain)) => {
            url.set_host(Some(&format!("{canister_name}.{env_name}.{domain}")))
                .expect("friendly domain should be a valid host");
        }
        (None, Some(domain)) => {
            url.set_host(Some(&format!("{canister_id}.{domain}")))
                .expect("principal domain should be a valid host");
        }
        (_, None) => {
            url.set_query(Some(&format!("canisterId={canister_id}")));
        }
    }
    url
}

#[derive(Debug, Snafu)]
pub enum WriteCustomDomainsError {
    #[snafu(transparent)]
    WriteFile { source: crate::fs::IoError },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_custom_domains_produces_correct_file() {
        let dir = camino_tempfile::Utf8TempDir::new().unwrap();
        let mut env_mappings = BTreeMap::new();

        let mut local_mappings = BTreeMap::new();
        local_mappings.insert(
            "backend".to_string(),
            Principal::from_text("bkyz2-fmaaa-aaaaa-qaaaq-cai").unwrap(),
        );
        local_mappings.insert(
            "frontend".to_string(),
            Principal::from_text("bd3sg-teaaa-aaaaa-qaaba-cai").unwrap(),
        );
        env_mappings.insert("local".to_string(), local_mappings);

        let mut staging_mappings = BTreeMap::new();
        staging_mappings.insert(
            "backend".to_string(),
            Principal::from_text("aaaaa-aa").unwrap(),
        );
        env_mappings.insert("staging".to_string(), staging_mappings);

        write_custom_domains(dir.path(), "localhost", &env_mappings, &[]).unwrap();

        let content = std::fs::read_to_string(dir.path().join("custom-domains.txt")).unwrap();
        // BTreeMap is ordered, so local comes before staging
        assert_eq!(
            content,
            "backend.local.localhost:bkyz2-fmaaa-aaaaa-qaaaq-cai\n\
             frontend.local.localhost:bd3sg-teaaa-aaaaa-qaaba-cai\n\
             backend.staging.localhost:aaaaa-aa\n"
        );
    }

    #[test]
    fn write_custom_domains_with_extra_entries() {
        let dir = camino_tempfile::Utf8TempDir::new().unwrap();
        let env_mappings = BTreeMap::new();
        let extra = vec![(
            "id.ai.localhost".to_string(),
            "uqzsh-gqaaa-aaaaq-qaada-cai".to_string(),
        )];

        write_custom_domains(dir.path(), "localhost", &env_mappings, &extra).unwrap();

        let content = std::fs::read_to_string(dir.path().join("custom-domains.txt")).unwrap();
        assert_eq!(content, "id.ai.localhost:uqzsh-gqaaa-aaaaq-qaada-cai\n");
    }

    #[test]
    fn ii_custom_domain_entry_returns_entry_when_enabled() {
        let entry = ii_custom_domain_entry(true, "localhost");
        assert_eq!(
            entry,
            Some((
                "id.ai.localhost".to_string(),
                "uqzsh-gqaaa-aaaaq-qaada-cai".to_string()
            ))
        );
    }

    #[test]
    fn ii_custom_domain_entry_returns_none_when_disabled() {
        assert_eq!(ii_custom_domain_entry(false, "localhost"), None);
    }

    #[test]
    fn canister_gateway_url_with_friendly_domain() {
        let base: Url = "http://localhost:8000".parse().unwrap();
        let cid = Principal::from_text("bkyz2-fmaaa-aaaaa-qaaaq-cai").unwrap();
        let url = canister_gateway_url(&base, cid, Some(("backend", "local")));
        assert_eq!(url.as_str(), "http://backend.local.localhost:8000/");
    }

    #[test]
    fn canister_gateway_url_without_friendly_domain() {
        let base: Url = "http://localhost:8000".parse().unwrap();
        let cid = Principal::from_text("bkyz2-fmaaa-aaaaa-qaaaq-cai").unwrap();
        let url = canister_gateway_url(&base, cid, None);
        assert_eq!(
            url.as_str(),
            "http://bkyz2-fmaaa-aaaaa-qaaaq-cai.localhost:8000/"
        );
    }

    #[test]
    fn canister_gateway_url_ip_address_fallback() {
        let base: Url = "http://192.168.1.1:8000".parse().unwrap();
        let cid = Principal::from_text("bkyz2-fmaaa-aaaaa-qaaaq-cai").unwrap();
        let url = canister_gateway_url(&base, cid, None);
        assert_eq!(
            url.as_str(),
            "http://192.168.1.1:8000/?canisterId=bkyz2-fmaaa-aaaaa-qaaaq-cai"
        );
    }

    #[test]
    fn canister_gateway_url_loopback_ip_uses_localhost() {
        let base: Url = "http://127.0.0.1:8000".parse().unwrap();
        let cid = Principal::from_text("bkyz2-fmaaa-aaaaa-qaaaq-cai").unwrap();
        let url = canister_gateway_url(&base, cid, None);
        assert_eq!(
            url.as_str(),
            "http://bkyz2-fmaaa-aaaaa-qaaaq-cai.localhost:8000/"
        );
    }

    #[test]
    fn gateway_domain_extracts_from_domain() {
        let url: Url = "http://example.com:8000".parse().unwrap();
        assert_eq!(gateway_domain(&url), Some("example.com"));
    }

    #[test]
    fn gateway_domain_loopback_ip() {
        let url: Url = "http://127.0.0.1:8000".parse().unwrap();
        assert_eq!(gateway_domain(&url), Some("localhost"));
    }

    #[test]
    fn gateway_domain_non_loopback_ip() {
        let url: Url = "http://192.168.1.1:8000".parse().unwrap();
        assert_eq!(gateway_domain(&url), None);
    }
}
