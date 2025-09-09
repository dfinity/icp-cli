use std::path::Path;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "fs")]
extern "C" {
    // We are defining the function signature for `readFileSync` from the 'fs' module.
    // `#[wasm_bindgen(js_name = "readFileSync")]` maps our Rust function name
    // to the actual JavaScript function name.
    #[wasm_bindgen(js_name = "readFileSync")]
    fn read_file_sync(path: &str, encoding: &str) -> String;
}

pub fn read_file_utf8(path: &Path) -> String {
    read_file_sync(path.to_str().unwrap(), "utf-8")
}
