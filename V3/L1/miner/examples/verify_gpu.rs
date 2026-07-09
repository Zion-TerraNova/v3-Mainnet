use zion_cosmic_harmony::cosmic_harmony_ekam_deeksha_v2;

fn main() {
    let header_hex = "0300000000007c1b18fb404ae21d3e3deaa40a62bda081797f9dabc24335075084ba57abf1bb1672d1cb4393cda840ff1ef6b57d1ae8ed56016272bde0408dfc38e4e9604435cd69000000007a1f021f";
    let header: Vec<u8> = (0..header_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&header_hex[i..i + 2], 16).unwrap())
        .collect();
    let nonce: u64 = 1930;
    let height: u64 = 6676;
    let hash = cosmic_harmony_ekam_deeksha_v2(&header, nonce, height);
    let hex: String = hash.data.iter().map(|b| format!("{:02x}", b)).collect();
    println!("cpu_hash={}", hex);
    println!("gpu_hash=a964a610bf8584d250982e90ec815dd2e21974c9ccba733f9eb896a7da84b696");
    if hex == "a964a610bf8584d250982e90ec815dd2e21974c9ccba733f9eb896a7da84b696" {
        println!("MATCH: GPU and CPU produce identical hashes");
    } else {
        println!("MISMATCH: GPU hash differs from CPU hash");
    }
}
