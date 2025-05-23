use crate::pocketic::admin::PocketIcAdminInterface;
use candid::Principal;
use reqwest::Url;

pub struct PocketIcInstance {
    pub admin: PocketIcAdminInterface,
    pub gateway: Url,
    pub instance_id: String,
    pub effective_canister_id: Principal,
}
