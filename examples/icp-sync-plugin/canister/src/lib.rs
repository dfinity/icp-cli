use std::cell::RefCell;

thread_local! {
    static CONFIG: RefCell<String> = RefCell::default();
    static FRUITS: RefCell<Vec<(String, String)>> = RefCell::default();
}

// Upload the config value (called once by the sync plugin).
#[ic_cdk::update]
fn set_config(value: String) {
    CONFIG.with_borrow_mut(|c| *c = value);
}

// Register a (name, content) fruit pair (called by the sync plugin for each file).
#[ic_cdk::update]
fn register(name: String, content: String) {
    FRUITS.with_borrow_mut(|f| f.push((name, content)));
}

// Return the stored config and every registered fruit.
#[ic_cdk::query]
fn show() -> (String, Vec<(String, String)>) {
    (
        CONFIG.with_borrow(|c| c.clone()),
        FRUITS.with_borrow(|f| f.clone()),
    )
}
