use crate::storage::Store;
use anyhow::Result;

pub(crate) async fn cmd_scan(store: &Store, path: String) -> Result<()> {
    match store.scan_codebase(std::path::Path::new(&path)).await {
        Ok((symbols, relations)) => {
            println!("Scanned {} symbols, {} relations", symbols, relations)
        }
        Err(e) => eprintln!("Scan failed: {}", e),
    }
    Ok(())
}

pub(crate) async fn cmd_doctor(store: &Store) -> Result<()> {
    let stats = store.stats().await?;
    println!("Database: {}", stats.db_path);
    Ok(())
}

pub(crate) async fn cmd_init(store: &Store) -> Result<()> {
    println!("Database initialized.");
    Ok(())
}
