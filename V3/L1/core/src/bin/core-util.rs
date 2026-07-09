use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use zion_core::storage::{ChainDb, ChainMeta};

#[derive(Parser)]
#[command(
    name = "core-util",
    about = "ZION core offline chain state utility",
    version
)]
struct Cli {
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Export chain metadata and all blocks as JSON
    ExportState {
        db_path: PathBuf,
        #[arg(long, help = "Output JSON file (default: stdout)")]
        out: Option<PathBuf>,
    },
    /// Verify LMDB integrity and metadata consistency
    VerifyDb { db_path: PathBuf },
    /// Dump blocks to JSON
    DumpBlocks {
        db_path: PathBuf,
        #[arg(long, help = "Maximum blocks to export")]
        limit: Option<u64>,
        #[arg(long, help = "Output JSON file (default: stdout)")]
        out: Option<PathBuf>,
    },
    /// Print current tip height
    TipHeight { db_path: PathBuf },
    /// Get a single block by height or hash
    GetBlock { db_path: PathBuf, id: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Commands::ExportState { db_path, out } => cmd_export_state(db_path, out),
        Commands::VerifyDb { db_path } => cmd_verify_db(db_path),
        Commands::DumpBlocks {
            db_path,
            limit,
            out,
        } => cmd_dump_blocks(db_path, limit, out),
        Commands::TipHeight { db_path } => cmd_tip_height(db_path),
        Commands::GetBlock { db_path, id } => cmd_get_block(db_path, id),
    }
}

fn open_db(path: &Path) -> Result<ChainDb> {
    ChainDb::open(path).with_context(|| format!("Failed to open LMDB at {}", path.display()))
}

fn cmd_export_state(db_path: PathBuf, out: Option<PathBuf>) -> Result<()> {
    let db = open_db(&db_path)?;
    let meta = db.get_meta()?;
    let tip_height = db.tip_height()?;
    let blocks = db.export_blocks(0, tip_height)?;

    let export = serde_json::json!({
        "meta": meta_json(&meta),
        "tip_height": tip_height,
        "blocks_count": blocks.len(),
        "blocks": blocks.iter().map(block_json).collect::<Vec<_>>(),
    });

    let json_str = serde_json::to_string_pretty(&export)?;
    write_output(&json_str, out)?;
    Ok(())
}

fn cmd_verify_db(db_path: PathBuf) -> Result<()> {
    let db = open_db(&db_path)?;
    let mut ok = true;

    print!("Checking meta database... ");
    match db.get_meta() {
        Ok(m) => {
            println!(
                "OK (schema={}, tip_height={}, total_work={})",
                m.schema_version, m.tip_height, m.total_work
            );
        }
        Err(e) => {
            println!("FAIL: {}", e);
            ok = false;
        }
    }

    print!("Checking tip height... ");
    match db.tip_height() {
        Ok(h) => println!("OK (height={})", h),
        Err(e) => {
            println!("FAIL: {}", e);
            ok = false;
        }
    }

    print!("Checking block at height 0 (genesis)... ");
    match db.get_block_by_height(0) {
        Ok(Some(_)) => println!("OK"),
        Ok(None) => {
            println!("MISSING — genesis block not found");
            ok = false;
        }
        Err(e) => {
            println!("FAIL: {}", e);
            ok = false;
        }
    }

    if ok {
        println!("\nVerification PASSED.");
    } else {
        println!("\nVerification FAILED.");
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_dump_blocks(db_path: PathBuf, limit: Option<u64>, out: Option<PathBuf>) -> Result<()> {
    let db = open_db(&db_path)?;
    let tip = db.tip_height()?;
    let end = limit.map(|l| l.min(tip)).unwrap_or(tip);
    let blocks = db.export_blocks(0, end)?;

    let export: Vec<_> = blocks.iter().map(block_json).collect();
    let json_str = serde_json::to_string_pretty(&export)?;
    write_output(&json_str, out)?;
    Ok(())
}

fn cmd_tip_height(db_path: PathBuf) -> Result<()> {
    let db = open_db(&db_path)?;
    let h = db.tip_height()?;
    println!("{}", h);
    Ok(())
}

fn cmd_get_block(db_path: PathBuf, id: String) -> Result<()> {
    let db = open_db(&db_path)?;
    let block = if id.chars().all(|c| c.is_ascii_digit()) {
        let height: u64 = id.parse().context("Invalid height")?;
        db.get_block_by_height(height)?
    } else {
        let hash = hex::decode(&id).context("Invalid hex hash")?;
        if hash.len() != 32 {
            anyhow::bail!("Hash must be 32 bytes (64 hex chars)");
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&hash);
        db.get_block(&arr)?
    };

    match block {
        Some(b) => {
            println!("{}", serde_json::to_string_pretty(&block_json(&b))?);
        }
        None => {
            println!("Block not found: {}", id);
            std::process::exit(1);
        }
    }
    Ok(())
}

// ── JSON helpers ───────────────────────────────────────────────────────

fn meta_json(meta: &ChainMeta) -> serde_json::Value {
    serde_json::json!({
        "schema_version": meta.schema_version,
        "tip_hash": hex::encode(meta.tip_hash),
        "tip_height": meta.tip_height,
        "total_work": meta.total_work.to_string(),
    })
}

fn block_json(block: &zion_core::storage::StoredBlock) -> serde_json::Value {
    serde_json::json!({
        "hash": hex::encode(block.hash),
        "prev_hash": hex::encode(block.prev_hash),
        "height": block.height,
        "timestamp": block.timestamp,
        "difficulty": block.difficulty,
        "nonce": block.nonce,
        "total_work": block.total_work.to_string(),
        "transactions_count": block.transactions.len(),
        "coinbase_amount": block.coinbase_amount,
    })
}

fn write_output(text: &str, out: Option<PathBuf>) -> Result<()> {
    match out {
        Some(path) => {
            std::fs::write(&path, text)
                .with_context(|| format!("Failed to write {}", path.display()))?;
            eprintln!("Wrote {}", path.display());
        }
        None => println!("{}", text),
    }
    Ok(())
}
