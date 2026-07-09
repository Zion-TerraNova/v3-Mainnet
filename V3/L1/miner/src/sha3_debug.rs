use zion_cosmic_harmony::scratchpad_ekam::memory_hard_transform_ekam_light_v2_sha3;

fn main() {
    let input = [0xAAu8; 64];
    let result = memory_hard_transform_ekam_light_v2_sha3(&input);
    println!("CPU SHA3-512 result: {}", hex::encode(result.data));
}
