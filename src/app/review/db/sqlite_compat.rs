use sea_orm::{ConnectionTrait, DbBackend, DbErr, Statement};

pub(super) async fn user_version<C>(connection: &C) -> Result<i64, DbErr>
where
    C: ConnectionTrait,
{
    connection
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA user_version",
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("SQLite returned no user_version row".to_string()))?
        .try_get("", "user_version")
}

pub(super) async fn set_user_version<C>(connection: &C, version: i64) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    connection
        .execute_unprepared(&format!("PRAGMA user_version = {version}"))
        .await?;
    Ok(())
}

pub(super) async fn verify_integrity<C>(connection: &C) -> Result<(), DbErr>
where
    C: ConnectionTrait,
{
    let foreign_keys = connection
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA foreign_key_check",
        ))
        .await?;
    if !foreign_keys.is_empty() {
        return Err(DbErr::Custom(format!(
            "review database failed foreign key validation with {} violation(s)",
            foreign_keys.len()
        )));
    }

    let messages = connection
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "PRAGMA quick_check",
        ))
        .await?
        .into_iter()
        .map(|row| row.try_get::<String>("", "quick_check"))
        .collect::<Result<Vec<_>, _>>()?;
    if messages.as_slice() != ["ok"] {
        return Err(DbErr::Custom(format!(
            "review database failed quick_check: {}",
            messages.join("; ")
        )));
    }
    Ok(())
}
