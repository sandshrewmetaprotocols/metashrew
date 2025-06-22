use rocksdb::{DB, Options};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_path = "/home/ubuntu/alkanes-db";
    
    if !Path::new(db_path).exists() {
        println!("Database path does not exist: {}", db_path);
        return Ok(());
    }
    
    let mut opts = Options::default();
    opts.create_if_missing(false);
    
    let db = DB::open(&opts, db_path)?;
    
    // Check indexed height
    let height_key = b"__INTERNAL/height";
    match db.get(height_key)? {
        Some(value) => {
            if value.len() >= 4 {
                let height_bytes: [u8; 4] = value[..4].try_into().unwrap();
                let height = u32::from_le_bytes(height_bytes);
                println!("Indexed height: {}", height);
            } else {
                println!("Invalid height data length: {}", value.len());
            }
        },
        None => println!("No indexed height found"),
    }
    
    // Check some state roots
    for h in 210..=215 {
        let root_key = format!("smt:root:{}", h);
        match db.get(root_key.as_bytes())? {
            Some(root) => {
                println!("State root for height {}: 0x{}", h, hex::encode(&root));
            },
            None => {
                println!("No state root for height {}", h);
            }
        }
    }
    
    // Check some block hashes
    for h in 210..=215 {
        let hash_key = format!("/__INTERNAL/height-to-hash/{}", h);
        match db.get(hash_key.as_bytes())? {
            Some(hash) => {
                println!("Block hash for height {}: 0x{}", h, hex::encode(&hash));
            },
            None => {
                println!("No block hash for height {}", h);
            }
        }
    }
    
    Ok(())
}