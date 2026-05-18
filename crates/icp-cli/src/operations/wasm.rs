use std::borrow::Cow;
use std::io::Read;

use wasmparser::{Parser, Payload};

/// Extract the `candid:service` interface from a WASM module's custom sections.
///
/// Canister WASM modules store their Candid interface in a custom section named
/// `icp:public candid:service` or `icp:private candid:service`. This function
/// parses the module (decompressing gzip if needed) and returns the interface
/// text if found.
pub(crate) fn extract_candid_service(wasm: &[u8]) -> Option<String> {
    let wasm = maybe_decompress_gzip(wasm)?;
    for payload in Parser::new(0).parse_all(&wasm) {
        if let Ok(Payload::CustomSection(reader)) = payload {
            let name = reader.name();
            if name == "icp:public candid:service" || name == "icp:private candid:service" {
                return String::from_utf8(reader.data().to_vec()).ok();
            }
        }
    }
    None
}

/// If `data` starts with the gzip magic bytes (`1f 8b`), decompress it.
/// Otherwise return the original slice.
fn maybe_decompress_gzip(data: &[u8]) -> Option<Cow<'_, [u8]>> {
    if data.starts_with(&[0x1f, 0x8b]) {
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(data)
            .read_to_end(&mut decoded)
            .ok()?;
        Some(Cow::Owned(decoded))
    } else {
        Some(Cow::Borrowed(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // (module
    //   (@custom "icp:public some_other" "irrelevant")
    //   (@custom "icp:public candid:service" "service : {}")
    //   (@custom "another_section" "more data")
    // )
    const UNRELATED_SECTIONS: &[u8] = b"\x00\x61\x73\x6d\x01\x00\x00\x00\x00\x20\x15\x69\x63\x70\x3a\x70\x75\x62\x6c\x69\x63\x20\x73\x6f\x6d\x65\x5f\x6f\x74\x68\x65\x72\x69\x72\x72\x65\x6c\x65\x76\x61\x6e\x74\x00\x26\x19\x69\x63\x70\x3a\x70\x75\x62\x6c\x69\x63\x20\x63\x61\x6e\x64\x69\x64\x3a\x73\x65\x72\x76\x69\x63\x65\x73\x65\x72\x76\x69\x63\x65\x20\x3a\x20\x7b\x7d\x00\x19\x0f\x61\x6e\x6f\x74\x68\x65\x72\x5f\x73\x65\x63\x74\x69\x6f\x6e\x6d\x6f\x72\x65\x20\x64\x61\x74\x61";
    // (module (@custom "icp:public some_other" "data") )
    const NO_CANDID_SECTION: &[u8] = b"\x00\x61\x73\x6d\x01\x00\x00\x00\x00\x1a\x15\x69\x63\x70\x3a\x70\x75\x62\x6c\x69\x63\x20\x73\x6f\x6d\x65\x5f\x6f\x74\x68\x65\x72\x64\x61\x74\x61";
    // (module (@custom "icp:private candid:service" "service : { hello : () -> () }") )
    const PRIVATE_SERVICE: &[u8] = b"\x00\x61\x73\x6d\x01\x00\x00\x00\x00\x39\x1a\x69\x63\x70\x3a\x70\x72\x69\x76\x61\x74\x65\x20\x63\x61\x6e\x64\x69\x64\x3a\x73\x65\x72\x76\x69\x63\x65\x73\x65\x72\x76\x69\x63\x65\x20\x3a\x20\x7b\x20\x68\x65\x6c\x6c\x6f\x20\x3a\x20\x28\x29\x20\x2d\x3e\x20\x28\x29\x20\x7d";
    // (module (@custom "icp:public candid:service" "service : { greet : (text) -> (text) }") )
    const PUBLIC_SERVICE: &[u8] = b"\x00\x61\x73\x6d\x01\x00\x00\x00\x00\x40\x19\x69\x63\x70\x3a\x70\x75\x62\x6c\x69\x63\x20\x63\x61\x6e\x64\x69\x64\x3a\x73\x65\x72\x76\x69\x63\x65\x73\x65\x72\x76\x69\x63\x65\x20\x3a\x20\x7b\x20\x67\x72\x65\x65\x74\x20\x3a\x20\x28\x74\x65\x78\x74\x29\x20\x2d\x3e\x20\x28\x74\x65\x78\x74\x29\x20\x7d";

    #[test]
    fn extracts_public_candid_service() {
        assert_eq!(
            extract_candid_service(PUBLIC_SERVICE).as_deref(),
            Some("service : { greet : (text) -> (text) }")
        );
    }

    #[test]
    fn extracts_private_candid_service() {
        assert_eq!(
            extract_candid_service(PRIVATE_SERVICE).as_deref(),
            Some("service : { hello : () -> () }")
        );
    }

    #[test]
    fn returns_none_when_no_candid_section() {
        assert!(extract_candid_service(NO_CANDID_SECTION).is_none());
    }

    #[test]
    fn returns_none_for_empty_input() {
        assert!(extract_candid_service(&[]).is_none());
    }

    #[test]
    fn returns_none_for_invalid_wasm() {
        assert!(extract_candid_service(b"not a wasm module").is_none());
    }

    #[test]
    fn handles_gzip_compressed_wasm() {
        use flate2::write::GzEncoder;
        use std::io::Write;

        let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(PUBLIC_SERVICE).unwrap();
        let compressed = encoder.finish().unwrap();

        assert_eq!(
            extract_candid_service(&compressed).as_deref(),
            Some("service : { greet : (text) -> (text) }")
        );
    }

    #[test]
    fn skips_unrelated_sections() {
        assert_eq!(
            extract_candid_service(UNRELATED_SECTIONS).as_deref(),
            Some("service : {}")
        );
    }
}
