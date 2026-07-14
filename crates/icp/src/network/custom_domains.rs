use std::collections::BTreeMap;

use candid::Principal;
use snafu::prelude::*;
use url::Url;

use crate::prelude::*;

/// Writes a `custom-domains.txt` file to the given status directory.
///
/// Each line has the format `<friendly_name>.<env_name>.<domain>:<principal>`,
/// where `<friendly_name>` is a canister's friendly-URL subdomain prefix (a bare
/// name like `backend` for an own canister, or a dot-nested form like
/// `backend.openemail` for a dependency canister — see DESIGN §17.2). The file
/// is written fresh each time from the full set of current entries across all
/// environments sharing this network. Entries whose assembled host is not a
/// valid DNS host are skipped (the canister remains reachable by principal).
///
/// `env_entries` maps each environment name to `(friendly_name, principal)`
/// pairs; a de-duplicated shared dependency canister contributes one pair per
/// alias chain that reaches it.
///
/// `extra_entries` are raw `(full_domain, canister_id)` pairs appended after the
/// environment-based entries (e.g. system canisters like Internet Identity).
pub fn write_custom_domains(
    status_dir: &Path,
    domain: &str,
    env_entries: &BTreeMap<String, Vec<(String, Principal)>>,
    extra_entries: &[(String, String)],
) -> Result<(), WriteCustomDomainsError> {
    let file_path = status_dir.join("custom-domains.txt");
    let mut content = String::new();
    for (env_name, entries) in env_entries {
        for (friendly_name, principal) in entries {
            if let Some(host) = friendly_host(friendly_name, env_name, domain) {
                content.push_str(&format!("{host}:{principal}\n"));
            }
        }
    }
    for (full_domain, canister_id) in extra_entries {
        content.push_str(&format!("{full_domain}:{canister_id}\n"));
    }
    crate::fs::write(&file_path, content.as_bytes())?;
    Ok(())
}

/// True if `label` is a valid DNS label: 1–63 characters of ASCII letters,
/// digits, or `-`, not starting or ending with `-`.
fn is_dns_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && label
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

/// Assemble the friendly host `<friendly_name>.<env_name>.<domain>`, or `None`
/// if any resulting dot-separated label is not a valid DNS label — so callers
/// fall back to a principal-based URL / skip the custom-domains entry rather
/// than emit a malformed host. `friendly_name` may itself be multi-label (e.g.
/// `backend.openemail`); every label is validated.
pub fn friendly_host(friendly_name: &str, env_name: &str, domain: &str) -> Option<String> {
    let host = format!("{friendly_name}.{env_name}.{domain}");
    host.split('.').all(is_dns_label).then_some(host)
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
/// registered), returns `http://<friendly_name>.<env_name>.<domain>:<port>`,
/// where `friendly_name` is the canister's friendly-URL subdomain prefix
/// (bare name, or dot-nested for a dependency canister — DESIGN §17.2).
///
/// Falls back to `http://<principal>.<domain>:<port>` when there is no friendly
/// name or the assembled host is not a valid DNS host (e.g. a raw store key), or
/// to `http://<host>:<port>?canisterId=<principal>` if no subdomain routing is
/// available at all.
pub fn canister_gateway_url(
    http_gateway_url: &Url,
    canister_id: Principal,
    friendly: Option<(&str, &str)>,
) -> Url {
    let domain = gateway_domain(http_gateway_url);
    let mut url = http_gateway_url.clone();
    // A friendly subdomain requires both a routing domain and a friendly name
    // that assembles into a valid host; otherwise fall back below.
    let host = match (friendly, domain) {
        (Some((friendly_name, env_name)), Some(domain)) => {
            friendly_host(friendly_name, env_name, domain)
        }
        _ => None,
    };
    match (host, domain) {
        (Some(host), _) => {
            url.set_host(Some(&host))
                .expect("validated friendly host should be a valid host");
        }
        (None, Some(domain)) => {
            url.set_host(Some(&format!("{canister_id}.{domain}")))
                .expect("principal domain should be a valid host");
        }
        (None, None) => {
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

    fn cid(text: &str) -> Principal {
        Principal::from_text(text).unwrap()
    }

    #[test]
    fn write_custom_domains_produces_correct_file() {
        let dir = camino_tempfile::Utf8TempDir::new().unwrap();
        let mut env_entries: BTreeMap<String, Vec<(String, Principal)>> = BTreeMap::new();

        env_entries.insert(
            "local".to_string(),
            vec![
                ("backend".to_string(), cid("bkyz2-fmaaa-aaaaa-qaaaq-cai")),
                // A dependency canister: dot-nested by its alias chain.
                (
                    "frontend.openemail".to_string(),
                    cid("bd3sg-teaaa-aaaaa-qaaba-cai"),
                ),
            ],
        );
        env_entries.insert(
            "staging".to_string(),
            vec![("backend".to_string(), cid("aaaaa-aa"))],
        );

        write_custom_domains(dir.path(), "localhost", &env_entries, &[]).unwrap();

        let content = std::fs::read_to_string(dir.path().join("custom-domains.txt")).unwrap();
        // BTreeMap is ordered, so local comes before staging.
        assert_eq!(
            content,
            "backend.local.localhost:bkyz2-fmaaa-aaaaa-qaaaq-cai\n\
             frontend.openemail.local.localhost:bd3sg-teaaa-aaaaa-qaaba-cai\n\
             backend.staging.localhost:aaaaa-aa\n"
        );
    }

    #[test]
    fn write_custom_domains_skips_invalid_hosts() {
        let dir = camino_tempfile::Utf8TempDir::new().unwrap();
        let mut env_entries: BTreeMap<String, Vec<(String, Principal)>> = BTreeMap::new();
        env_entries.insert(
            "local".to_string(),
            vec![
                ("ok".to_string(), cid("bkyz2-fmaaa-aaaaa-qaaaq-cai")),
                // A raw store key is not a valid host and must be skipped, not
                // emitted malformed.
                (
                    "vendor/openemail:backend".to_string(),
                    cid("bd3sg-teaaa-aaaaa-qaaba-cai"),
                ),
            ],
        );

        write_custom_domains(dir.path(), "localhost", &env_entries, &[]).unwrap();

        let content = std::fs::read_to_string(dir.path().join("custom-domains.txt")).unwrap();
        assert_eq!(content, "ok.local.localhost:bkyz2-fmaaa-aaaaa-qaaaq-cai\n");
    }

    #[test]
    fn write_custom_domains_with_extra_entries() {
        let dir = camino_tempfile::Utf8TempDir::new().unwrap();
        let env_entries = BTreeMap::new();
        let extra = vec![(
            "id.ai.localhost".to_string(),
            "uqzsh-gqaaa-aaaaq-qaada-cai".to_string(),
        )];

        write_custom_domains(dir.path(), "localhost", &env_entries, &extra).unwrap();

        let content = std::fs::read_to_string(dir.path().join("custom-domains.txt")).unwrap();
        assert_eq!(content, "id.ai.localhost:uqzsh-gqaaa-aaaaq-qaada-cai\n");
    }

    #[test]
    fn friendly_host_validates_labels() {
        assert_eq!(
            friendly_host("backend", "local", "localhost").as_deref(),
            Some("backend.local.localhost")
        );
        assert_eq!(
            friendly_host("backend.openemail", "local", "localhost").as_deref(),
            Some("backend.openemail.local.localhost")
        );
        assert_eq!(
            friendly_host("bar.libfoo.openemail", "local", "localhost").as_deref(),
            Some("bar.libfoo.openemail.local.localhost")
        );
        // Store-key separators are not valid DNS labels.
        assert_eq!(
            friendly_host("vendor/openemail:backend", "local", "localhost"),
            None
        );
        assert_eq!(friendly_host("dep:backend", "local", "localhost"), None);
        // Empty / leading-hyphen labels rejected.
        assert_eq!(friendly_host("", "local", "localhost"), None);
        assert_eq!(friendly_host("-bad", "local", "localhost"), None);
    }

    #[test]
    fn registration_and_url_agree_on_host() {
        // The custom-domains.txt entry and the printed URL must use the same host
        // for a dependency canister, or the printed URL would not route.
        let base: Url = "http://localhost:8000".parse().unwrap();
        let id = cid("bkyz2-fmaaa-aaaaa-qaaaq-cai");
        let url = canister_gateway_url(&base, id, Some(("frontend.openemail", "local")));
        let registered = friendly_host("frontend.openemail", "local", "localhost").unwrap();
        assert_eq!(url.host_str().unwrap(), registered);
        assert_eq!(
            url.as_str(),
            "http://frontend.openemail.local.localhost:8000/"
        );
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
    fn canister_gateway_url_namespaced_name_falls_back_to_principal() {
        let base: Url = "http://localhost:8000".parse().unwrap();
        let cid = Principal::from_text("bkyz2-fmaaa-aaaaa-qaaaq-cai").unwrap();
        // A namespaced dependency canister (e.g. `dep:backend`) is not a valid DNS
        // label, so the friendly host is rejected and we fall back to the principal.
        let url = canister_gateway_url(&base, cid, Some(("dep:backend", "local")));
        assert_eq!(
            url.as_str(),
            "http://bkyz2-fmaaa-aaaaa-qaaaq-cai.localhost:8000/"
        );
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
