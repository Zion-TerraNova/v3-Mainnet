use sha3::{Digest, Sha3_512};

use crate::algorithms_opt::Hash64;

#[inline]
pub fn sha3_512_bytes(input: &[u8]) -> Hash64 {
    sha3_512_chunks([input])
}

#[inline]
pub fn sha3_512_chunks<const N: usize>(chunks: [&[u8]; N]) -> Hash64 {
    let mut hasher = Sha3_512::new();
    for chunk in chunks {
        hasher.update(chunk);
    }

    let result = hasher.finalize();
    let mut hash = Hash64::new();
    hash.data.copy_from_slice(&result);
    hash
}

#[inline(always)]
pub fn sha3_512_64_8(input_a: &[u8; 64], input_b: &[u8; 8]) -> Hash64 {
    let mut hasher = Sha3_512::new();
    hasher.update(input_a);
    hasher.update(input_b);

    let result = hasher.finalize();
    let mut hash = Hash64::new();
    hash.data.copy_from_slice(&result);
    hash
}

#[inline(always)]
pub fn sha3_512_64_64_64_8_8(
    input_a: &[u8; 64],
    input_b: &[u8; 64],
    input_c: &[u8; 64],
    input_d: &[u8; 8],
    input_e: &[u8; 8],
) -> Hash64 {
    let mut hasher = Sha3_512::new();
    hasher.update(input_a);
    hasher.update(input_b);
    hasher.update(input_c);
    hasher.update(input_d);
    hasher.update(input_e);

    let result = hasher.finalize();
    let mut hash = Hash64::new();
    hash.data.copy_from_slice(&result);
    hash
}

#[inline(always)]
pub fn sha3_512_64_64_64(input_a: &[u8; 64], input_b: &[u8; 64], input_c: &[u8; 64]) -> Hash64 {
    let mut hasher = Sha3_512::new();
    hasher.update(input_a);
    hasher.update(input_b);
    hasher.update(input_c);

    let result = hasher.finalize();
    let mut hash = Hash64::new();
    hash.data.copy_from_slice(&result);
    hash
}

#[inline(always)]
pub fn sha3_512_64_64_8_64_8_8(
    input_a: &[u8; 64],
    input_b: &[u8; 64],
    input_c: &[u8; 8],
    input_d: &[u8; 64],
    input_e: &[u8; 8],
    input_f: &[u8; 8],
) -> Hash64 {
    let mut hasher = Sha3_512::new();
    hasher.update(input_a);
    hasher.update(input_b);
    hasher.update(input_c);
    hasher.update(input_d);
    hasher.update(input_e);
    hasher.update(input_f);

    let result = hasher.finalize();
    let mut hash = Hash64::new();
    hash.data.copy_from_slice(&result);
    hash
}

#[inline]
pub fn sha3_512_into(input: &[u8], output: &mut Hash64) {
    *output = sha3_512_bytes(input);
}
