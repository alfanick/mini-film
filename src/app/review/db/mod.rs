mod migrations;
mod read;
mod write;

use super::{model::*, prelude::*, store::*};
use rusqlite::Connection;

pub(super) const SQLITE_STATE_FILE: &str = "mini-film-review.sqlite";
pub(super) const LEGACY_JSON_STATE_FILE: &str = "mini-film-review.json";

#[cfg(test)]
pub(super) const LATEST_SCHEMA_VERSION: i64 = migrations::LATEST_SCHEMA_VERSION;

pub(super) fn review_state_path(output_root: &Path) -> PathBuf {
    output_root.join(SQLITE_STATE_FILE)
}

pub(super) fn legacy_json_state_path(output_root: &Path) -> PathBuf {
    output_root.join(LEGACY_JSON_STATE_FILE)
}

pub(super) fn load_or_migrate_store(output_root: &Path) -> Result<(ReviewStore, PathBuf)> {
    let sqlite_path = review_state_path(output_root);
    let legacy_path = legacy_json_state_path(output_root);
    if sqlite_path.exists() {
        if let Some(store) = load_store(&sqlite_path)? {
            return Ok((store, sqlite_path));
        }
        if legacy_path.exists() {
            let store = migrate_legacy_json(&legacy_path, &sqlite_path)?;
            return Ok((store, sqlite_path));
        }
        return Ok((ReviewStore::new(Vec::new()), sqlite_path));
    }

    if legacy_path.exists() {
        let store = migrate_legacy_json(&legacy_path, &sqlite_path)?;
        return Ok((store, sqlite_path));
    }

    Ok((ReviewStore::new(Vec::new()), sqlite_path))
}

pub(super) fn resolve_review_state_for_publish(
    state: &Path,
    output_root: &Path,
) -> Result<PathBuf> {
    if state.exists() {
        let state = fs::canonicalize(state)
            .with_context(|| format!("canonicalizing review state {}", state.display()))?;
        ensure_path_within(&state, output_root)?;
        return Ok(state);
    }

    if state.file_name().and_then(|name| name.to_str()) == Some(LEGACY_JSON_STATE_FILE) {
        let sqlite_path = state.with_file_name(SQLITE_STATE_FILE);
        if sqlite_path.exists() {
            let sqlite_path = fs::canonicalize(&sqlite_path).with_context(|| {
                format!("canonicalizing review state {}", sqlite_path.display())
            })?;
            ensure_path_within(&sqlite_path, output_root)?;
            return Ok(sqlite_path);
        }
    }

    let state = fs::canonicalize(state)
        .with_context(|| format!("canonicalizing review state {}", state.display()))?;
    ensure_path_within(&state, output_root)?;
    Ok(state)
}

pub(super) fn load_store_for_publish(state: &Path) -> Result<Option<ReviewStore>> {
    if state.file_name().and_then(|name| name.to_str()) == Some(LEGACY_JSON_STATE_FILE) {
        let sqlite_path = state.with_file_name(SQLITE_STATE_FILE);
        if sqlite_path.exists() {
            return load_store(&sqlite_path);
        }
        if state.exists() {
            return migrate_legacy_json(state, &sqlite_path).map(Some);
        }
    }
    load_store(state)
}

pub(super) fn load_store(path: &Path) -> Result<Option<ReviewStore>> {
    if !path.exists() {
        return Ok(None);
    }
    if path.file_name().and_then(|name| name.to_str()) == Some(LEGACY_JSON_STATE_FILE) {
        return load_legacy_json(path).map(Some);
    }

    let connection = open_database(path)?;
    read::load_store_from_connection(&connection)
        .with_context(|| format!("reading review state from {}", path.display()))
}

pub(super) fn save_store(path: &Path, store: &ReviewStore) -> Result<()> {
    let mut connection = open_database(path)?;
    write::replace_store(&mut connection, store)
        .with_context(|| format!("saving review state to {}", path.display()))
}

fn open_database(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut connection =
        Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    configure_connection(&connection)?;
    migrations::run_migrations(&mut connection)?;
    Ok(connection)
}

fn configure_connection(connection: &Connection) -> Result<()> {
    connection
        .busy_timeout(Duration::from_secs(30))
        .context("setting sqlite busy timeout")?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .context("enabling sqlite foreign keys")?;
    Ok(())
}

fn migrate_legacy_json(legacy_path: &Path, sqlite_path: &Path) -> Result<ReviewStore> {
    let store = load_legacy_json(legacy_path)?;
    let temp_path = sqlite_temp_path(sqlite_path);
    if temp_path.exists() {
        fs::remove_file(&temp_path)
            .with_context(|| format!("removing stale {}", temp_path.display()))?;
    }

    {
        let mut connection = open_database(&temp_path)?;
        write::replace_store(&mut connection, &store)?;
        let restored = read::load_store_from_connection(&connection)?
            .ok_or_else(|| anyhow!("migrated review database contains no review state"))?;
        ensure_same_store(&store, &restored)?;
    }

    if let Some(parent) = sqlite_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    if sqlite_path.exists() {
        fs::remove_file(sqlite_path).with_context(|| {
            format!("replacing empty review database {}", sqlite_path.display())
        })?;
    }
    fs::rename(&temp_path, sqlite_path).with_context(|| {
        format!(
            "installing migrated review database {}",
            sqlite_path.display()
        )
    })?;
    let backup = migrated_json_backup_path(legacy_path);
    fs::rename(legacy_path, &backup).with_context(|| {
        format!(
            "renaming migrated review state {} to {}",
            legacy_path.display(),
            backup.display()
        )
    })?;
    Ok(store)
}

fn load_legacy_json(path: &Path) -> Result<ReviewStore> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn sqlite_temp_path(sqlite_path: &Path) -> PathBuf {
    let file_name = sqlite_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(SQLITE_STATE_FILE);
    sqlite_path.with_file_name(format!("{file_name}.tmp"))
}

fn migrated_json_backup_path(legacy_path: &Path) -> PathBuf {
    let timestamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    for suffix in 0.. {
        let candidate = if suffix == 0 {
            legacy_path.with_file_name(format!("{LEGACY_JSON_STATE_FILE}.migrated-{timestamp}"))
        } else {
            legacy_path.with_file_name(format!(
                "{LEGACY_JSON_STATE_FILE}.migrated-{timestamp}-{suffix}"
            ))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn ensure_same_store(expected: &ReviewStore, restored: &ReviewStore) -> Result<()> {
    let expected = canonical_store_value(expected)?;
    let restored = canonical_store_value(restored)?;
    if expected != restored {
        bail!(
            "sqlite migration verification failed: restored review state differs from source state"
        );
    }
    Ok(())
}

fn canonical_store_value(store: &ReviewStore) -> Result<serde_json::Value> {
    serde_json::to_value(store).context("canonicalizing review state")
}

fn ensure_path_within(path: &Path, root: &Path) -> Result<()> {
    if path.starts_with(root) {
        Ok(())
    } else {
        bail!(
            "review state {} is outside output root {}",
            path.display(),
            root.display()
        )
    }
}

#[cfg(test)]
pub(super) fn open_database_at_version_for_test(path: &Path, version: i64) -> Result<Connection> {
    let mut connection = Connection::open(path)?;
    configure_connection(&connection)?;
    migrations::run_migrations_through_for_test(&mut connection, version)?;
    Ok(connection)
}

#[cfg(test)]
pub(super) fn table_count(path: &Path, table: &str) -> Result<u64> {
    if !matches!(
        table,
        "review_settings"
            | "profiles"
            | "profile_adjustments"
            | "profile_sharpening"
            | "profile_hsl_values"
            | "profile_tone_curve_points"
            | "profile_pp3_sections"
            | "profile_pp3_entries"
            | "images"
            | "image_exif_tags"
            | "tags"
            | "image_tags"
            | "image_labels"
            | "image_publish_profiles"
            | "image_profile_bw_filters"
            | "image_profile_renders"
    ) {
        bail!("unsupported table count target {table}");
    }
    let connection = open_database(path)?;
    let mut statement = connection.prepare(&format!("SELECT COUNT(*) FROM {table}"))?;
    let count = statement.query_row([], |row| row.get::<_, i64>(0))?;
    u64::try_from(count).context("sqlite table count does not fit u64")
}

#[cfg(test)]
pub(super) fn table_exists(path: &Path, table: &str) -> Result<bool> {
    let connection = open_database(path)?;
    let count = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count == 1)
}

#[cfg(test)]
pub(super) fn json_storage_columns(path: &Path) -> Result<Vec<(String, String)>> {
    let connection = open_database(path)?;
    let mut tables = connection.prepare(
        "SELECT name FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
         ORDER BY name",
    )?;
    let table_names = tables
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(tables);
    let mut columns = Vec::new();
    for table in table_names {
        let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        columns.extend(
            names
                .into_iter()
                .filter(|column| column.ends_with("_json"))
                .map(|column| (table.clone(), column)),
        );
    }
    Ok(columns)
}
