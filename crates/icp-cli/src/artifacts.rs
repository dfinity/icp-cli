// Include the generated artifacts module from the build script
mod embed {
    include!(concat!(env!("OUT_DIR"), "/artifacts.rs"));
}

/// Gets the candid_ui wasm artifact as a byte slice
#[allow(dead_code)]
pub fn get_candid_ui_wasm() -> &'static [u8] {
    embed::candid_ui()
}

#[allow(dead_code)]
pub fn get_proxy_wasm() -> &'static [u8] {
    embed::proxy()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_candid_ui_wasm() {
        let candid_ui_bytes = get_candid_ui_wasm();
        assert!(!candid_ui_bytes.is_empty());
        println!("candid_ui artifact size: {} bytes", candid_ui_bytes.len());
    }

    #[test]
    fn test_get_proxy_wasm() {
        let proxy_bytes = get_proxy_wasm();
        assert!(!proxy_bytes.is_empty());
        println!("proxy artifact size: {} bytes", proxy_bytes.len());
    }
}
