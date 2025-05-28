use crate::managed::pocketic::admin::PocketIcAdminInterface;
use candid::Principal;
use pocket_ic::common::rest::InstanceId;

#[allow(dead_code)]
pub struct PocketIcInstance {
    pub admin: PocketIcAdminInterface,
    pub gateway_port: u16,
    pub instance_id: InstanceId,
    pub effective_canister_id: Principal,
    pub root_key: String,
}
