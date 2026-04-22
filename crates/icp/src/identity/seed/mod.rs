use bip32::DerivationPath;
use bip39::{Mnemonic, Seed};

use super::{key::IdentityKey, manifest::IdentityKeyAlgorithm};

mod slip10;

/// Standard ICP derivation path for secp256k1 and p256 (coin type 223).
pub fn default_derivation_path() -> DerivationPath {
    "m/44'/223'/0'/0/0".parse().expect("valid derivation path")
}

/// All-hardened ICP derivation path for ed25519; SLIP-0010 requires all path
/// components to be hardened for ed25519.
pub fn ed25519_derivation_path() -> DerivationPath {
    "m/44'/223'/0'/0'/0'"
        .parse()
        .expect("valid derivation path")
}

/// Derives a key from a BIP-39 mnemonic using SLIP-0010 for the given curve.
///
/// - `Secp256k1`: path `m/44'/223'/0'/0/0`
/// - `Prime256v1`: path `m/44'/223'/0'/0/0`
/// - `Ed25519`: path `m/44'/223'/0'/0'/0'` (all hardened)
pub fn derive_key_from_seed_slip10(
    mnemonic: &Mnemonic,
    algorithm: &IdentityKeyAlgorithm,
) -> IdentityKey {
    let seed = Seed::new(mnemonic, "");
    match algorithm {
        IdentityKeyAlgorithm::Secp256k1 => IdentityKey::Secp256k1(slip10::derive_secp256k1(
            seed.as_bytes(),
            &default_derivation_path(),
        )),
        IdentityKeyAlgorithm::Prime256v1 => IdentityKey::Prime256v1(slip10::derive_p256(
            seed.as_bytes(),
            &default_derivation_path(),
        )),
        IdentityKeyAlgorithm::Ed25519 => IdentityKey::Ed25519(slip10::derive_ed25519(
            seed.as_bytes(),
            &ed25519_derivation_path(),
        )),
    }
}
