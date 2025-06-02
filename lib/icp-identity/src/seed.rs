use bip32::{DerivationPath, XPrv};
use bip39::{Mnemonic, Seed};

pub fn default_derivation_path() -> DerivationPath {
    "m/44'/223'/0'/0/0".parse().expect("valid derivation path")
}

pub fn derive_default_key_from_seed(mnemonic: &Mnemonic) -> k256::SecretKey {
    let seed = Seed::new(mnemonic, "");
    let pk = XPrv::derive_from_path(seed.as_bytes(), &default_derivation_path())
        .expect("valid derivation");
    k256::SecretKey::from(pk.private_key())
}
