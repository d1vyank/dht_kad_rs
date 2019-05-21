use fasthash::{spooky::Hasher128, FastHasher, HasherExt};
use rand::prelude::*;
use std::hash::Hash;

pub fn create_id() -> u128 {
    rand::thread_rng().gen()
}

pub fn common_prefix_length(a: u128, b: u128) -> u32 {
    (a ^ b).leading_zeros()
}

pub fn key_from_bytes(k: &Vec<u8>) -> u128 {
    let mut a: [u8; 16] = Default::default();
    a.copy_from_slice(&k[0..16]);
    u128::from_be_bytes(a)
}

pub fn key_to_bytes(k: u128) -> Vec<u8> {
    k.to_be_bytes().to_vec()
}

pub fn calculate_hash(key: Vec<u8>) -> u128 {
    let mut hasher = Hasher128::new();
    Hash::hash_slice(&key, &mut hasher);
    hasher.finish_ext()
}

#[cfg(test)]
mod tests {
    use crate::keyutil;

    #[test]
    fn test_create() {
        assert_ne!(keyutil::create_id(), 0);
        assert_ne!(keyutil::create_id(), keyutil::create_id());
    }

    #[test]
    fn test_common_prefix_length() {
        let id = keyutil::create_id();
        assert_eq!(keyutil::common_prefix_length(id, id), 128);
    }
}
