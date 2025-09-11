mod fs;
mod parser;

use std::path::PathBuf;

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

#[wasm_bindgen(getter_with_clone)]
pub struct GenerateResult {
    pub declarations_js: String,
    pub declarations_ts: String,
    pub interface_ts: String,
    pub service_ts: String,
}

#[wasm_bindgen]
pub fn generate(declarations: String, service_name: String) -> Result<GenerateResult, JsError> {
    let input_path = PathBuf::from(declarations);
    let (env, actor, prog) = parser::check_file(input_path.as_path()).map_err(JsError::from)?;

    let declarations_js = candid_parser::bindings::javascript::compile(&env, &actor);
    let declarations_ts = candid_parser::bindings::typescript::compile(&env, &actor, &prog);

    let interface_ts = candid_parser::bindings::typescript_native::compile::compile(
        &env,
        &actor,
        &service_name,
        "interface",
        &prog,
    );

    let service_ts = candid_parser::bindings::typescript_native::compile::compile(
        &env,
        &actor,
        &service_name,
        "wrapper",
        &prog,
    );

    Ok(GenerateResult {
        declarations_js,
        declarations_ts,
        interface_ts,
        service_ts,
    })
}
