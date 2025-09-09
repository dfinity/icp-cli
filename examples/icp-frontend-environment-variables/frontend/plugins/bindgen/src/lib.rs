mod fs;
mod parser;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
    match log::set_logger(&wasm_bindgen_console_logger::DEFAULT_LOGGER) {
        Ok(_) => log::info!("Console logger initialized"),
        Err(e) => log::error!("Failed to set console logger: {}", e),
    }
    log::set_max_level(log::LevelFilter::Trace);
}

#[wasm_bindgen]
pub fn generate(declarations: String) -> Result<String, JsError> {
    let (env, actor, _) =
        parser::check_file(std::path::Path::new(&declarations)).map_err(JsError::from)?;
    let res = candid_parser::bindings::javascript::compile(&env, &actor);
    Ok(res)
}
