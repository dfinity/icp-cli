pub(crate) mod canister;
pub(crate) mod icrc;

#[derive(Default)]
pub(crate) struct Initializers {
    pub(crate) canister: canister::Initializers,
    pub(crate) icrc: icrc::Initializers,
}
