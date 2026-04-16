use std::cell::RefCell;

thread_local! {
    static ITEMS: RefCell<Vec<(String, String)>> = RefCell::default();
}

// Add a (name, content) pair to the store (called by the sync plugin for each seed file).
#[ic_cdk::update]
fn seed(name: String, content: String) {
    ITEMS.with_borrow_mut(|items| items.push((name, content)));
}

// Return all stored (name, content) pairs.
#[ic_cdk::query]
fn list() -> Vec<(String, String)> {
    ITEMS.with_borrow(|items| items.clone())
}

// Return the number of stored items.
#[ic_cdk::query]
fn count_items() -> u64 {
    ITEMS.with_borrow(|items| items.len() as u64)
}
