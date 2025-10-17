pub(crate) mod canister;

#[derive(Default)]
pub(crate) struct Initializers {
    pub(crate) canister: canister::Initializers,
}
