use std::cell::RefCell;

use candid::Principal;

thread_local! {
    static UPLOADER: RefCell<Option<Principal>> = RefCell::default();
    static FRUITS: RefCell<Vec<(String, String)>> = RefCell::default();
}

// Set the uploader principal (controller-only). Called once via the proxy canister.
#[ic_cdk::update]
fn set_uploader(uploader: Principal) {
    let caller = ic_cdk::api::msg_caller();
    assert!(
        ic_cdk::api::is_controller(&caller),
        "only a controller can call set_uploader"
    );
    UPLOADER.with_borrow_mut(|u| *u = Some(uploader));
}

// Register a (name, content) fruit pair. Restricted to the stored uploader.
#[ic_cdk::update]
fn register(name: String, content: String) {
    let caller = ic_cdk::api::msg_caller();
    let uploader = UPLOADER.with_borrow(|u| *u);
    assert_eq!(
        Some(caller),
        uploader,
        "only the uploader can call register"
    );
    FRUITS.with_borrow_mut(|f| f.push((name, content)));
}

// Return the stored uploader principal and every registered fruit.
#[ic_cdk::query]
fn show() -> (Option<Principal>, Vec<(String, String)>) {
    (
        UPLOADER.with_borrow(|u| *u),
        FRUITS.with_borrow(|f| f.clone()),
    )
}
