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

    /// Build a minimal valid WASM module with the given custom sections.
    /// Each entry is (section_name, section_data).
    fn build_wasm_with_custom_sections(sections: &[(&str, &[u8])]) -> Vec<u8> {
        let mut wasm = vec![
            0x00, 0x61, 0x73, 0x6d, // magic: \0asm
            0x01, 0x00, 0x00, 0x00, // version: 1
        ];
        for (name, data) in sections {
            let name_bytes = name.as_bytes();
            // Custom section: id=0, then LEB128 length, then name (LEB128 len + bytes), then data
            let section_payload_len =
                leb128_len(name_bytes.len() as u32) + name_bytes.len() + data.len();
            wasm.push(0x00); // custom section id
            write_leb128(&mut wasm, section_payload_len as u32);
            write_leb128(&mut wasm, name_bytes.len() as u32);
            wasm.extend_from_slice(name_bytes);
            wasm.extend_from_slice(data);
        }
        wasm
    }

    fn write_leb128(buf: &mut Vec<u8>, mut value: u32) {
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            buf.push(byte);
            if value == 0 {
                break;
            }
        }
    }

    fn leb128_len(mut value: u32) -> usize {
        let mut len = 0;
        loop {
            value >>= 7;
            len += 1;
            if value == 0 {
                break;
            }
        }
        len
    }

    #[test]
    fn extracts_public_candid_service() {
        let candid = b"service : { greet : (text) -> (text) }";
        let wasm = build_wasm_with_custom_sections(&[("icp:public candid:service", candid)]);
        let result = extract_candid_service(&wasm);
        assert_eq!(
            result.as_deref(),
            Some("service : { greet : (text) -> (text) }")
        );
    }

    #[test]
    fn extracts_private_candid_service() {
        let candid = b"service : { hello : () -> () }";
        let wasm = build_wasm_with_custom_sections(&[("icp:private candid:service", candid)]);
        let result = extract_candid_service(&wasm);
        assert_eq!(result.as_deref(), Some("service : { hello : () -> () }"));
    }

    #[test]
    fn returns_none_when_no_candid_section() {
        let wasm = build_wasm_with_custom_sections(&[("icp:public some_other", b"data")]);
        assert!(extract_candid_service(&wasm).is_none());
    }

    #[test]
    fn returns_none_for_empty_wasm() {
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

        let candid = b"service : { greet : (text) -> (text) }";
        let wasm = build_wasm_with_custom_sections(&[("icp:public candid:service", candid)]);

        let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&wasm).unwrap();
        let compressed = encoder.finish().unwrap();

        let result = extract_candid_service(&compressed);
        assert_eq!(
            result.as_deref(),
            Some("service : { greet : (text) -> (text) }")
        );
    }

    #[test]
    fn skips_unrelated_sections() {
        let candid = b"service : {}";
        let wasm = build_wasm_with_custom_sections(&[
            ("icp:public some_other", b"irrelevant"),
            ("icp:public candid:service", candid),
            ("another_section", b"more data"),
        ]);
        let result = extract_candid_service(&wasm);
        assert_eq!(result.as_deref(), Some("service : {}"));
    }
}
