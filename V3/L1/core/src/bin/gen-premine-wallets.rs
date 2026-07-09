fn main() {
    println!("{{");
    println!("  \"premine_wallets\": [");

    for i in 1..=14 {
        let (signing_key, verifying_key) = zion_core::crypto::generate_keypair();
        let address = zion_core::crypto::derive_address(verifying_key.as_bytes());
        let secret_hex = zion_core::crypto::to_hex(signing_key.as_bytes());
        let public_hex = zion_core::crypto::to_hex(verifying_key.as_bytes());

        if i > 1 {
            println!(",");
        }
        print!("    {{");
        print!("\"slot\": {}, ", i);
        print!("\"address\": \"{}\", ", address);
        print!("\"public_key_hex\": \"{}\", ", public_hex);
        print!("\"secret_key_hex\": \"{}\"", secret_hex);
        print!("}}");
    }

    println!();
    println!("  ]");
    println!("}}");
    eprintln!("⚠️  SAVE THIS OUTPUT SECURELY — secret keys shown only once!");
    eprintln!("⚠️  STORE IN ENCRYPTED BACKUP BEFORE PROCEEDING!");
}
