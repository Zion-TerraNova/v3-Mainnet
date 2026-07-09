use zion_free_world::db::{FreeWorldDb, GrantRecord, ProjectRecord};
use zion_free_world::error::FreeWorldResult;

fn in_memory_db() -> FreeWorldResult<FreeWorldDb> {
    FreeWorldDb::open(":memory:")
}

#[test]
fn test_grant_lifecycle() -> FreeWorldResult<()> {
    let db = in_memory_db()?;

    let grant = GrantRecord::new("Clean Water Initiative", "humanitarian", 1_000_000);
    db.insert_grant(&grant)?;

    let grants = db.list_grants(None)?;
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].title, "Clean Water Initiative");
    assert_eq!(grants[0].status, "pending");

    db.update_grant_status(&grant.id, "approved", Some("Approved by DAO vote"))?;

    let approved = db.list_grants(Some("approved"))?;
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].status, "approved");

    let pending = db.list_grants(Some("pending"))?;
    assert!(pending.is_empty());

    Ok(())
}

#[test]
fn test_project_lifecycle() -> FreeWorldResult<()> {
    let db = in_memory_db()?;

    let project = ProjectRecord::new("Solar Village", "energy", 5_000_000);
    db.insert_project(&project)?;

    let projects = db.list_projects(None)?;
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "Solar Village");
    assert_eq!(projects[0].status, "planning");

    Ok(())
}

#[test]
fn test_fund_balance() -> FreeWorldResult<()> {
    let db = in_memory_db()?;

    let balance = db.get_fund_balance()?;
    assert_eq!(balance.total_accumulated, 0);

    let mut updated = balance.clone();
    updated.total_accumulated = 1_000_000_000;
    updated.last_block_height = 100;
    db.update_fund_balance(&updated)?;

    let fetched = db.get_fund_balance()?;
    assert_eq!(fetched.total_accumulated, 1_000_000_000);
    assert_eq!(fetched.last_block_height, 100);

    Ok(())
}
