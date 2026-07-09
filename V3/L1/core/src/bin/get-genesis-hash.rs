fn main() {
    let genesis_hash = zion_core::genesis::genesis_hash();
    println!("NEW_GENESIS_HASH: {}", genesis_hash);
}
