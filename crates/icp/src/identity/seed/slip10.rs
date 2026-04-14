//! SLIP-0010 hierarchical deterministic key derivation.
//!
//! Implements <https://github.com/satoshilabs/slips/blob/master/slip-0010.md>

use bip32::DerivationPath;
use elliptic_curve::{Curve, bigint::Encoding, sec1::ToEncodedPoint};
use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use num_traits::Zero;
use sha2::Sha512;

type HmacSha512 = Hmac<Sha512>;

const SECP256K1_SEED_KEY: &[u8] = b"Bitcoin seed";
const P256_SEED_KEY: &[u8] = b"Nist256p1 seed";
const ED25519_SEED_KEY: &[u8] = b"ed25519 seed";

pub fn derive_secp256k1(seed: &[u8], path: &DerivationPath) -> k256::SecretKey {
    let key_bytes = slip10_derive(
        seed,
        path,
        SECP256K1_SEED_KEY,
        Some(k256::Secp256k1::ORDER.to_be_bytes()),
        k256_compressed_public_key,
    );
    k256::SecretKey::from_slice(&key_bytes)
        .expect("SLIP-0010 secp256k1 derivation produced a valid key")
}

pub fn derive_p256(seed: &[u8], path: &DerivationPath) -> p256::SecretKey {
    let key_bytes = slip10_derive(
        seed,
        path,
        P256_SEED_KEY,
        Some(p256::NistP256::ORDER.to_be_bytes()),
        p256_compressed_public_key,
    );
    p256::SecretKey::from_slice(&key_bytes).expect("SLIP-0010 p256 derivation produced a valid key")
}

/// Panics if any path component is non-hardened; SLIP-0010 forbids it for Ed25519.
pub fn derive_ed25519(seed: &[u8], path: &DerivationPath) -> ic_ed25519::PrivateKey {
    let key_bytes = slip10_derive(seed, path, ED25519_SEED_KEY, None, |_| unreachable!());
    ic_ed25519::PrivateKey::deserialize_raw(&key_bytes)
        .expect("SLIP-0010 ed25519 derivation produced a valid key")
}

fn hmac_sha512_split(key: &[u8], data: &[u8]) -> ([u8; 32], [u8; 32]) {
    let mut mac = HmacSha512::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    let out = mac.finalize().into_bytes();
    (out[..32].try_into().unwrap(), out[32..].try_into().unwrap())
}

/// Generic SLIP-0010 derivation.
///
/// `order` is `Some(n)` for EC curves (secp256k1, P-256), where child keys are
/// derived via modular scalar addition. Pass `None` for Ed25519, which uses the
/// left 32 HMAC bytes directly and requires all path components to be hardened.
///
/// `pub_key` computes the compressed public key for non-hardened child steps;
/// it is never called when `order` is `None`.
fn slip10_derive(
    seed: &[u8],
    path: &DerivationPath,
    curve_key: &[u8],
    order: Option<[u8; 32]>,
    pub_key: impl Fn(&[u8; 32]) -> [u8; 33],
) -> [u8; 32] {
    let (mut key, mut chain_code) = hmac_sha512_split(curve_key, seed);

    for child in path.iter() {
        assert!(
            order.is_some() || child.is_hardened(),
            "SLIP-0010 {}: all path components must be hardened, but {child} is non-hardened",
            std::str::from_utf8(curve_key).unwrap_or("?"),
        );

        let raw_index = u32::from(child);
        let mut data = Vec::with_capacity(37);
        if child.is_hardened() {
            data.push(0x00);
            data.extend_from_slice(&key);
        } else {
            data.extend_from_slice(&pub_key(&key));
        }
        data.extend_from_slice(&raw_index.to_be_bytes());

        let (il, ir) = hmac_sha512_split(&chain_code, &data);

        if let Some(order_bytes) = order {
            // EC: child key = (IL + parent_key) mod n
            let order = BigUint::from_bytes_be(&order_bytes);
            let il_big = BigUint::from_bytes_be(&il);
            assert!(
                il_big < order,
                "SLIP-0010: IL >= order at index {child} (astronomically unlikely)"
            );
            let child_big = (il_big + BigUint::from_bytes_be(&key)) % &order;
            assert!(
                !child_big.is_zero(),
                "SLIP-0010: child key is zero at index {child} (astronomically unlikely)"
            );
            let child_bytes = child_big.to_bytes_be();
            key = [0u8; 32];
            key[32 - child_bytes.len()..].copy_from_slice(&child_bytes);
            chain_code = ir;
        } else {
            // Ed25519: child key is the left 32 bytes directly; no modular arithmetic.
            (key, chain_code) = (il, ir);
        }
    }

    key
}

/// Returns the compressed SEC1 public key (33 bytes) for a secp256k1 private key scalar.
fn k256_compressed_public_key(key_bytes: &[u8; 32]) -> [u8; 33] {
    let secret = k256::SecretKey::from_slice(key_bytes).expect("valid k256 secret key");
    secret
        .public_key()
        .to_encoded_point(true)
        .as_bytes()
        .try_into()
        .expect("compressed k256 point is 33 bytes")
}

/// Returns the compressed SEC1 public key (33 bytes) for a p256 private key scalar.
fn p256_compressed_public_key(key_bytes: &[u8; 32]) -> [u8; 33] {
    let secret = p256::SecretKey::from_slice(key_bytes).expect("valid p256 secret key");
    secret
        .public_key()
        .to_encoded_point(true)
        .as_bytes()
        .try_into()
        .expect("compressed p256 point is 33 bytes")
}
