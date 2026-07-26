mod cache;
mod catalog;
mod entities;
mod migrations;
mod panorama;
mod paths;
mod read;
mod sqlite_compat;
mod write;

use super::{model::*, prelude::*};
#[cfg(test)]
use anyhow::ensure;
use arc_swap::ArcSwapOption;
use catalog::ReviewCatalogLocation;
use migrations::{
    FIRST_SEAORM_SCHEMA_VERSION, LATEST_SCHEMA_VERSION, LEGACY_SCHEMA_VERSION, Migrator,
    PRE_RELEASE_SEAORM_LEDGER, V11_LEDGER, V18_BASELINE_MIGRATION,
};
use paths::ReviewPathRoots;
use sea_orm::{
    DatabaseConnection, EntityTrait, QueryOrder, SqlxSqliteConnector, TransactionTrait,
    sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
#[cfg(test)]
use sea_orm::{IntoActiveModel, PaginatorTrait, Schema};
use sea_orm_migration::{MigratorTrait, SchemaManager};
use tokio::sync::Mutex;

pub(super) const SQLITE_STATE_FILE: &str = "mini-film-review.sqlite";
const LEGACY_JSON_STATE_FILE: &str = "mini-film-review.json";
const V11_BACKUP_FILE: &str = "mini-film-review.sqlite.pre-seaorm-v11";

#[derive(Clone)]
pub(super) struct ReviewDatabase {
    connection: DatabaseConnection,
    path: PathBuf,
    roots: ReviewPathRoots,
    pub(super) write_lock: Arc<Mutex<()>>,
    fatal_error: Arc<ArcSwapOption<String>>,
}

impl ReviewDatabase {
    pub(super) async fn open_output(
        input_root: &Path,
        output_root: &Path,
    ) -> Result<(Self, Option<ReviewStore>)> {
        let location = ReviewCatalogLocation::prepare(input_root, output_root)?;
        let path = location.catalog_path().to_path_buf();
        if !path.exists() {
            for legacy in [
                input_root.join(LEGACY_JSON_STATE_FILE),
                output_root.join(LEGACY_JSON_STATE_FILE),
            ] {
                if legacy.exists() {
                    bail!(legacy_upgrade_error(&legacy));
                }
            }
        }
        let (connection, store, roots) =
            prepare_database(&path, !path.exists(), input_root, output_root).await?;
        if let Err(error) = location.ensure_output_link() {
            connection.close().await.ok();
            return Err(error);
        }
        Ok((
            Self {
                connection,
                path,
                roots,
                write_lock: Arc::new(Mutex::new(())),
                fatal_error: Arc::new(ArcSwapOption::empty()),
            },
            store,
        ))
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn cache_root(&self) -> &Path {
        self.roots.cache_root()
    }

    pub(super) async fn replace_store(&self, store: &ReviewStore) -> Result<()> {
        write::replace_store(&self.connection, store, &self.roots)
            .await
            .with_context(|| format!("saving review state to {}", self.path.display()))
    }

    pub(super) async fn apply_delta(
        &self,
        before: &ReviewStore,
        after: &ReviewStore,
    ) -> Result<()> {
        let result = write::apply_store_delta(&self.connection, before, after, &self.roots)
            .await
            .with_context(|| format!("saving review state to {}", self.path.display()));
        if let Err(error) = &result {
            self.mark_fatal(error);
        }
        result
    }

    pub(super) fn health_error(&self) -> Option<Arc<String>> {
        self.fatal_error.load_full()
    }

    fn mark_fatal(&self, error: &anyhow::Error) {
        if self.fatal_error.load().is_none() {
            self.fatal_error.store(Some(Arc::new(format!("{error:#}"))));
        }
    }
}

pub(super) fn resolve_review_state_for_publish(
    state: &Path,
    input_root: &Path,
    output_root: &Path,
) -> Result<PathBuf> {
    reject_json_state_path(state)?;
    let location = ReviewCatalogLocation::prepare(input_root, output_root)?;
    if !location.catalog_path().is_file() {
        bail!(
            "review database does not exist: {}",
            location.catalog_path().display()
        );
    }
    location.ensure_output_link()?;
    let state = fs::canonicalize(state)
        .with_context(|| format!("canonicalizing review state {}", state.display()))?;
    let catalog = fs::canonicalize(location.catalog_path()).with_context(|| {
        format!(
            "canonicalizing review catalog {}",
            location.catalog_path().display()
        )
    })?;
    if state != catalog {
        bail!(
            "review state {} is not the catalog for input root {}",
            state.display(),
            input_root.display()
        );
    }
    Ok(catalog)
}

pub(super) fn load_store_for_publish(
    state: &Path,
    input_root: &Path,
    output_root: &Path,
) -> Result<Option<ReviewStore>> {
    reject_json_state_path(state)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let (connection, store, _) =
            prepare_database(state, false, input_root, output_root).await?;
        connection
            .close()
            .await
            .context("closing review database")?;
        Ok(store)
    })
}

async fn prepare_database(
    path: &Path,
    create_if_missing: bool,
    input_root: &Path,
    output_root: &Path,
) -> Result<(DatabaseConnection, Option<ReviewStore>, ReviewPathRoots)> {
    if create_if_missing {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let connection = connect_database(path, true).await?;
        Migrator::up(&connection, None)
            .await
            .context("creating review database schema")?;
        verify_current_database(&connection)
            .await
            .context("validating new review database")?;
        let roots = prepare_runtime_paths(&connection, input_root, output_root).await?;
        return Ok((connection, None, roots));
    }
    if !path.exists() {
        bail!("review database does not exist: {}", path.display());
    }

    let mut connection = connect_database(path, false).await?;
    let user_version = sqlite_compat::user_version(&connection)
        .await
        .context("reading review database schema version")?;
    match user_version {
        LEGACY_SCHEMA_VERSION => {
            validate_v11_database(&connection).await?;
            connection
                .close()
                .await
                .context("closing v11 review database before backup")?;
            create_v11_backup(path)?;

            connection = connect_database(path, false).await?;
            Migrator::up(&connection, None)
                .await
                .context("adopting v11 review database with SeaORM")?;
            verify_current_database(&connection).await?;
            let roots = prepare_runtime_paths(&connection, input_root, output_root).await?;
            let store = read::load_store(&connection, &roots)
                .await
                .context("reading adopted review database")?;
            Ok((connection, store, roots))
        }
        version if (FIRST_SEAORM_SCHEMA_VERSION..=LATEST_SCHEMA_VERSION).contains(&version) => {
            verify_seaorm_database_before_migration(&connection).await?;
            normalize_pre_release_seaql_ledger(&connection).await?;
            Migrator::up(&connection, None)
                .await
                .context("running review database migrations")?;
            verify_current_database(&connection).await?;
            let roots = prepare_runtime_paths(&connection, input_root, output_root).await?;
            let store = read::load_store(&connection, &roots)
                .await
                .with_context(|| format!("reading review state from {}", path.display()))?;
            Ok((connection, store, roots))
        }
        1..=10 => {
            connection.close().await.ok();
            bail!(legacy_sqlite_upgrade_error(path, user_version));
        }
        version => {
            connection.close().await.ok();
            bail!(
                "review database {} has unsupported schema version {version}; this mini-film release supports only normalized v11 adoption and SeaORM schema v{LATEST_SCHEMA_VERSION}",
                path.display()
            );
        }
    }
}

async fn prepare_runtime_paths(
    connection: &DatabaseConnection,
    input_root: &Path,
    output_root: &Path,
) -> Result<ReviewPathRoots> {
    let cache_root = cache::resolve_cache_root(connection).await?;
    let roots = ReviewPathRoots::new(input_root, output_root, &cache_root)?;
    paths::prepare_relative_path_storage(connection, &roots).await?;
    let moved = cache::migrate_legacy_output_caches(connection, &roots).await?;
    if moved > 0 {
        eprintln!(
            "review cache: migrated {moved} legacy cache entries from {} to {}",
            output_root.display(),
            cache_root.display()
        );
    }
    Ok(roots)
}

async fn normalize_pre_release_seaql_ledger(connection: &DatabaseConnection) -> Result<()> {
    let manager = SchemaManager::new(connection);
    if !manager
        .has_table("seaql_migrations")
        .await
        .context("checking pre-release SeaORM migration ledger")?
    {
        return Ok(());
    }
    let rows = sea_orm_migration::seaql_migrations::Entity::find()
        .order_by_asc(sea_orm_migration::seaql_migrations::Column::Version)
        .all(connection)
        .await
        .context("reading pre-release SeaORM migration ledger")?;
    let versions = rows
        .iter()
        .map(|row| row.version.as_str())
        .collect::<Vec<_>>();
    if versions.as_slice() != PRE_RELEASE_SEAORM_LEDGER {
        return Ok(());
    }

    let applied_at = rows
        .iter()
        .map(|row| row.applied_at)
        .max()
        .unwrap_or_default();
    let transaction = connection
        .begin()
        .await
        .context("starting pre-release migration-ledger transaction")?;
    sea_orm_migration::seaql_migrations::Entity::delete_many()
        .exec(&transaction)
        .await
        .context("removing pre-release SeaORM migration entries")?;
    sea_orm_migration::seaql_migrations::Entity::insert(
        sea_orm_migration::seaql_migrations::ActiveModel {
            version: sea_orm::Set(V18_BASELINE_MIGRATION.to_string()),
            applied_at: sea_orm::Set(applied_at),
        },
    )
    .exec(&transaction)
    .await
    .context("recording collapsed v18 baseline migration")?;
    transaction
        .commit()
        .await
        .context("committing pre-release migration-ledger transaction")
}

async fn validate_v11_database(connection: &DatabaseConnection) -> Result<()> {
    let manager = SchemaManager::new(connection);
    for table in required_v11_tables() {
        if !manager
            .has_table(table)
            .await
            .with_context(|| format!("checking v11 table {table}"))?
        {
            bail!("v11 review database is missing required table {table}");
        }
    }
    if manager
        .has_table("seaql_migrations")
        .await
        .context("checking SeaORM migration ledger")?
    {
        bail!("v11 review database has an unexpected SeaORM migration ledger");
    }

    let ledger = entities::legacy_schema_migrations::Entity::find()
        .order_by_asc(entities::legacy_schema_migrations::Column::Version)
        .all(connection)
        .await
        .context("reading v11 migration ledger")?;
    if ledger.len() != V11_LEDGER.len() {
        bail!(
            "v11 review database migration ledger has {} rows, expected {}",
            ledger.len(),
            V11_LEDGER.len()
        );
    }
    for (row, (version, name)) in ledger.iter().zip(V11_LEDGER) {
        if row.version != *version || row.name != *name {
            bail!(
                "v11 review database migration ledger entry {} is {:?}, expected {:?}",
                row.version,
                row.name,
                name
            );
        }
    }
    sqlite_compat::verify_integrity(connection)
        .await
        .context("validating v11 review database")
}

async fn verify_current_database(connection: &DatabaseConnection) -> Result<()> {
    let version = sqlite_compat::user_version(connection)
        .await
        .context("reading SeaORM review database schema version")?;
    if version != LATEST_SCHEMA_VERSION {
        bail!(
            "SeaORM review database has schema version {version}, expected {LATEST_SCHEMA_VERSION}"
        );
    }
    verify_seaorm_database(connection).await
}

async fn verify_seaorm_database(connection: &DatabaseConnection) -> Result<()> {
    verify_seaorm_database_before_migration(connection).await?;
    let manager = SchemaManager::new(connection);
    for table in required_panorama_tables() {
        if !manager
            .has_table(table)
            .await
            .with_context(|| format!("checking review table {table}"))?
        {
            bail!("SeaORM review database is missing required table {table}");
        }
    }
    for table in required_focus_tables() {
        if !manager
            .has_table(table)
            .await
            .with_context(|| format!("checking review table {table}"))?
        {
            bail!("SeaORM review database is missing required table {table}");
        }
    }
    sqlite_compat::verify_integrity(connection)
        .await
        .context("validating SeaORM review database")
}

async fn verify_seaorm_database_before_migration(connection: &DatabaseConnection) -> Result<()> {
    let manager = SchemaManager::new(connection);
    if !manager
        .has_table("seaql_migrations")
        .await
        .context("checking SeaORM migration ledger")?
    {
        bail!("SeaORM review database is missing seaql_migrations");
    }
    if manager
        .has_table("schema_migrations")
        .await
        .context("checking obsolete migration ledger")?
    {
        bail!("SeaORM review database still contains obsolete schema_migrations");
    }
    for table in required_base_review_tables() {
        if !manager
            .has_table(table)
            .await
            .with_context(|| format!("checking review table {table}"))?
        {
            bail!("SeaORM review database is missing required table {table}");
        }
    }
    sqlite_compat::verify_integrity(connection)
        .await
        .context("validating SeaORM review database")
}

async fn connect_database(path: &Path, create_if_missing: bool) -> Result<DatabaseConnection> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(create_if_missing)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(30));
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(30))
        .connect_with(options)
        .await
        .with_context(|| format!("opening {}", path.display()))?;
    Ok(SqlxSqliteConnector::from_sqlx_sqlite_pool(pool))
}

fn create_v11_backup(path: &Path) -> Result<()> {
    let backup = path.with_file_name(V11_BACKUP_FILE);
    if backup.exists() {
        return Ok(());
    }
    let temporary = path.with_file_name(format!("{V11_BACKUP_FILE}.tmp"));
    if temporary.exists() {
        fs::remove_file(&temporary)
            .with_context(|| format!("removing stale {}", temporary.display()))?;
    }
    fs::copy(path, &temporary).with_context(|| {
        format!(
            "copying v11 review database {} to {}",
            path.display(),
            temporary.display()
        )
    })?;
    fs::OpenOptions::new()
        .write(true)
        .open(&temporary)
        .with_context(|| format!("opening {} for sync", temporary.display()))?
        .sync_all()
        .with_context(|| format!("syncing {}", temporary.display()))?;
    if backup.exists() {
        fs::remove_file(&temporary)
            .with_context(|| format!("removing redundant {}", temporary.display()))?;
        return Ok(());
    }
    fs::rename(&temporary, &backup)
        .with_context(|| format!("installing v11 review database backup {}", backup.display()))
}

fn reject_json_state_path(path: &Path) -> Result<()> {
    if path.file_name().and_then(|name| name.to_str()) == Some(LEGACY_JSON_STATE_FILE) {
        bail!(legacy_upgrade_error(path));
    }
    Ok(())
}

fn legacy_upgrade_error(path: &Path) -> String {
    format!(
        "legacy JSON review state is not supported by mini-film 18: {}. Run the final mini-film 17.x release once to migrate it to normalized SQLite v11, then retry with mini-film 18",
        path.display()
    )
}

fn legacy_sqlite_upgrade_error(path: &Path, version: i64) -> String {
    format!(
        "review database {} uses legacy SQLite schema v{version}. Run the final mini-film 17.x release once to migrate it to normalized SQLite v11, then retry with mini-film 18",
        path.display()
    )
}

fn required_v11_tables() -> impl Iterator<Item = &'static str> {
    std::iter::once("schema_migrations").chain(required_base_review_tables())
}

fn required_base_review_tables() -> impl Iterator<Item = &'static str> {
    [
        "review_settings",
        "profiles",
        "profile_adjustments",
        "profile_sharpening",
        "profile_hsl_values",
        "profile_tone_curve_points",
        "profile_pp3_sections",
        "profile_pp3_entries",
        "images",
        "image_exif_tags",
        "tags",
        "image_tags",
        "image_labels",
        "image_publish_profiles",
        "image_profile_bw_filters",
        "image_profile_renders",
    ]
    .into_iter()
}

fn required_panorama_tables() -> impl Iterator<Item = &'static str> {
    [
        "panorama_projects",
        "panorama_project_images",
        "panorama_previews",
    ]
    .into_iter()
}

fn required_focus_tables() -> impl Iterator<Item = &'static str> {
    ["image_focus_regions"].into_iter()
}

#[cfg(test)]
pub(super) fn load_store(path: &Path) -> Result<Option<ReviewStore>> {
    load_store_with_roots(path, Path::new("/in"), Path::new("/out"))
}

#[cfg(test)]
pub(super) fn load_store_with_roots(
    path: &Path,
    input_root: &Path,
    output_root: &Path,
) -> Result<Option<ReviewStore>> {
    load_store_for_publish(path, input_root, output_root)
}

#[cfg(test)]
pub(super) fn save_store(path: &Path, store: &ReviewStore) -> Result<()> {
    save_store_with_roots(path, store, Path::new("/in"), Path::new("/out"))
}

#[cfg(test)]
pub(super) fn save_store_with_roots(
    path: &Path,
    store: &ReviewStore,
    input_root: &Path,
    output_root: &Path,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let (connection, _, roots) =
            prepare_database(path, !path.exists(), input_root, output_root).await?;
        write::replace_store(&connection, store, &roots).await?;
        connection.close().await?;
        Ok(())
    })
}

#[cfg(test)]
#[derive(Debug)]
pub(super) struct TestDatabaseFacts {
    pub(super) schema_version: i64,
    pub(super) has_legacy_ledger: bool,
    pub(super) has_seaql_ledger: bool,
    pub(super) seaql_migration_count: u64,
    pub(super) seaql_migrations: Vec<String>,
    pub(super) has_review_state: bool,
    pub(super) has_json_storage_columns: bool,
    pub(super) counts: HashMap<&'static str, u64>,
    pub(super) indexes: HashSet<&'static str>,
}

#[cfg(test)]
#[derive(Debug)]
pub(super) struct TestStoredPathFacts {
    pub(super) input_root: String,
    pub(super) output_root: String,
    pub(super) cache_root: String,
    pub(super) source_paths: Vec<String>,
    pub(super) output_paths: Vec<String>,
}

#[cfg(test)]
pub(super) fn stored_path_facts(path: &Path) -> Result<TestStoredPathFacts> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let connection = connect_database(path, false).await?;
        let settings = entities::review_settings::Entity::find_by_id(1)
            .one(&connection)
            .await?
            .context("missing review settings")?;
        let images = entities::images::Entity::find().all(&connection).await?;
        let renders = entities::image_profile_renders::Entity::find()
            .all(&connection)
            .await?;
        let projects = entities::panorama_projects::Entity::find()
            .all(&connection)
            .await?;
        let previews = entities::panorama_previews::Entity::find()
            .all(&connection)
            .await?;
        connection.close().await?;

        let source_paths = images
            .iter()
            .map(|image| image.raw_path.clone())
            .chain(
                images
                    .iter()
                    .filter_map(|image| image.sooc_sidecar_path.clone()),
            )
            .chain(
                projects
                    .into_iter()
                    .filter_map(|project| project.output_path),
            )
            .collect();
        let output_paths = images
            .into_iter()
            .filter_map(|image| image.preview_path)
            .chain(renders.into_iter().filter_map(|render| render.output_path))
            .chain(
                previews
                    .into_iter()
                    .filter_map(|preview| preview.preview_path),
            )
            .collect();
        Ok(TestStoredPathFacts {
            input_root: settings.input_root,
            output_root: settings.output_root,
            cache_root: settings.cache_root,
            source_paths,
            output_paths,
        })
    })
}

#[cfg(test)]
pub(super) fn database_facts(path: &Path) -> Result<TestDatabaseFacts> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let connection = connect_database(path, false).await?;
        let manager = SchemaManager::new(&connection);
        let has_review_state = manager.has_table("review_state").await?;
        let has_json_storage_columns = (has_review_state
            && manager.has_column("review_state", "store_json").await?)
            || manager.has_column("profiles", "metadata_json").await?
            || manager.has_column("images", "image_json").await?
            || manager
                .has_column("image_profile_renders", "render_json")
                .await?;
        let mut counts = HashMap::new();
        counts.insert(
            "review_settings",
            entities::review_settings::Entity::find()
                .count(&connection)
                .await?,
        );
        counts.insert(
            "profiles",
            entities::profiles::Entity::find()
                .count(&connection)
                .await?,
        );
        counts.insert(
            "images",
            entities::images::Entity::find().count(&connection).await?,
        );
        counts.insert(
            "tags",
            entities::tags::Entity::find().count(&connection).await?,
        );
        counts.insert(
            "image_tags",
            entities::image_tags::Entity::find()
                .count(&connection)
                .await?,
        );
        counts.insert(
            "image_profile_renders",
            entities::image_profile_renders::Entity::find()
                .count(&connection)
                .await?,
        );
        counts.insert(
            "image_profile_bw_filters",
            entities::image_profile_bw_filters::Entity::find()
                .count(&connection)
                .await?,
        );
        counts.insert(
            "image_focus_regions",
            if manager.has_table("image_focus_regions").await? {
                entities::image_focus_regions::Entity::find()
                    .count(&connection)
                    .await?
            } else {
                0
            },
        );
        counts.insert(
            "profile_pp3_sections",
            entities::profile_pp3_sections::Entity::find()
                .count(&connection)
                .await?,
        );
        counts.insert(
            "profile_pp3_entries",
            entities::profile_pp3_entries::Entity::find()
                .count(&connection)
                .await?,
        );
        let panorama_tables_present = manager.has_table("panorama_projects").await?;
        counts.insert(
            "panorama_projects",
            if panorama_tables_present {
                entities::panorama_projects::Entity::find()
                    .count(&connection)
                    .await?
            } else {
                0
            },
        );
        counts.insert(
            "panorama_project_images",
            if panorama_tables_present {
                entities::panorama_project_images::Entity::find()
                    .count(&connection)
                    .await?
            } else {
                0
            },
        );
        counts.insert(
            "panorama_previews",
            if panorama_tables_present {
                entities::panorama_previews::Entity::find()
                    .count(&connection)
                    .await?
            } else {
                0
            },
        );
        let mut indexes = HashSet::new();
        for (table, index) in expected_indexes() {
            if manager.has_index(table, index).await? {
                indexes.insert(index);
            }
        }
        let has_seaql_ledger = manager.has_table("seaql_migrations").await?;
        let seaql_migrations = if has_seaql_ledger {
            sea_orm_migration::seaql_migrations::Entity::find()
                .order_by_asc(sea_orm_migration::seaql_migrations::Column::Version)
                .all(&connection)
                .await?
                .into_iter()
                .map(|row| row.version)
                .collect()
        } else {
            Vec::new()
        };
        let facts = TestDatabaseFacts {
            schema_version: sqlite_compat::user_version(&connection).await?,
            has_legacy_ledger: manager.has_table("schema_migrations").await?,
            has_seaql_ledger,
            seaql_migration_count: u64::try_from(seaql_migrations.len())
                .expect("migration count fits u64"),
            seaql_migrations,
            has_review_state,
            has_json_storage_columns,
            counts,
            indexes,
        };
        connection.close().await?;
        Ok(facts)
    })
}

#[cfg(test)]
pub(super) fn assert_domain_constraints(path: &Path) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let connection = connect_database(path, false).await?;

        let mut adjustment = entities::profile_adjustments::Entity::find()
            .one(&connection)
            .await?
            .context("missing profile adjustment fixture")?;
        adjustment.scope = "invalid".to_string();
        ensure!(
            entities::profile_adjustments::Entity::insert(adjustment.into_active_model())
                .exec(&connection)
                .await
                .is_err(),
            "profile adjustment scope constraint accepted invalid data"
        );

        let mut sharpening = entities::profile_sharpening::Entity::find()
            .one(&connection)
            .await?
            .context("missing profile sharpening fixture")?;
        sharpening.scope = "invalid".to_string();
        ensure!(
            entities::profile_sharpening::Entity::insert(sharpening.into_active_model())
                .exec(&connection)
                .await
                .is_err(),
            "profile sharpening scope constraint accepted invalid data"
        );

        let mut hsl = entities::profile_hsl_values::Entity::find()
            .one(&connection)
            .await?
            .context("missing profile HSL fixture")?;
        hsl.channel = "invalid".to_string();
        ensure!(
            entities::profile_hsl_values::Entity::insert(hsl.into_active_model())
                .exec(&connection)
                .await
                .is_err(),
            "profile HSL channel constraint accepted invalid data"
        );

        let mut tone = entities::profile_tone_curve_points::Entity::find()
            .one(&connection)
            .await?
            .context("missing profile tone-curve fixture")?;
        tone.channel = "invalid".to_string();
        ensure!(
            entities::profile_tone_curve_points::Entity::insert(tone.into_active_model())
                .exec(&connection)
                .await
                .is_err(),
            "profile tone-curve channel constraint accepted invalid data"
        );

        let mut filter = entities::image_profile_bw_filters::Entity::find()
            .one(&connection)
            .await?
            .context("missing BW filter fixture")?;
        filter.position = 10_000;
        filter.bw_filter = "invalid".to_string();
        ensure!(
            entities::image_profile_bw_filters::Entity::insert(filter.into_active_model())
                .exec(&connection)
                .await
                .is_err(),
            "BW filter constraint accepted invalid data"
        );

        let mut focus_region = entities::image_focus_regions::Entity::find()
            .one(&connection)
            .await?
            .context("missing focus region fixture")?;
        focus_region.position = 10_000;
        focus_region.x = 2.0;
        ensure!(
            entities::image_focus_regions::Entity::insert(focus_region.into_active_model())
                .exec(&connection)
                .await
                .is_err(),
            "focus region coordinate constraint accepted invalid data"
        );

        let mut settings = entities::review_settings::Entity::find_by_id(1)
            .one(&connection)
            .await?
            .context("missing review settings fixture")?;
        settings.id = 2;
        ensure!(
            entities::review_settings::Entity::insert(settings.into_active_model())
                .exec(&connection)
                .await
                .is_err(),
            "review settings singleton constraint accepted a second row"
        );

        connection.close().await?;
        Ok(())
    })
}

#[cfg(test)]
pub(super) fn make_v11_database(path: &Path, store: &ReviewStore) -> Result<()> {
    save_store(path, store)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let connection = connect_database(path, false).await?;
        let manager = SchemaManager::new(&connection);
        manager
            .drop_table(
                sea_orm_migration::prelude::Table::drop()
                    .table(sea_orm_migration::seaql_migrations::Entity)
                    .to_owned(),
            )
            .await?;
        let schema = Schema::new(sea_orm::DbBackend::Sqlite);
        manager
            .create_table(
                schema.create_table_from_entity(entities::legacy_schema_migrations::Entity),
            )
            .await?;
        for (version, name) in V11_LEDGER {
            entities::legacy_schema_migrations::Entity::insert(
                entities::legacy_schema_migrations::ActiveModel {
                    version: sea_orm::Set(*version),
                    name: sea_orm::Set((*name).to_string()),
                    applied_at: sea_orm::Set("2026-07-18 00:00:00".to_string()),
                },
            )
            .exec(&connection)
            .await?;
        }
        sqlite_compat::set_user_version(&connection, LEGACY_SCHEMA_VERSION).await?;
        sqlite_compat::verify_integrity(&connection).await?;
        connection.close().await?;
        Ok(())
    })
}

#[cfg(test)]
pub(super) fn make_pre_release_seaorm_database(path: &Path, store: &ReviewStore) -> Result<()> {
    save_store(path, store)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let connection = connect_database(path, false).await?;
        sea_orm_migration::seaql_migrations::Entity::delete_many()
            .exec(&connection)
            .await?;
        for (position, version) in PRE_RELEASE_SEAORM_LEDGER.iter().enumerate() {
            sea_orm_migration::seaql_migrations::Entity::insert(
                sea_orm_migration::seaql_migrations::ActiveModel {
                    version: sea_orm::Set((*version).to_string()),
                    applied_at: sea_orm::Set(i64::try_from(position)? + 1),
                },
            )
            .exec(&connection)
            .await?;
        }
        connection.close().await?;
        Ok(())
    })
}

#[cfg(test)]
pub(super) fn make_schema_v12_database(path: &Path, store: &ReviewStore) -> Result<()> {
    save_store(path, store)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let connection = connect_database(path, false).await?;
        Migrator::down(&connection, Some(5)).await?;
        sqlite_compat::verify_integrity(&connection).await?;
        connection.close().await?;
        Ok(())
    })
}

#[cfg(test)]
pub(super) fn make_schema_v13_database(path: &Path, store: &ReviewStore) -> Result<()> {
    save_store(path, store)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let connection = connect_database(path, false).await?;
        Migrator::down(&connection, Some(4)).await?;
        sqlite_compat::verify_integrity(&connection).await?;
        connection.close().await?;
        Ok(())
    })
}

#[cfg(test)]
pub(super) fn make_schema_v14_database(path: &Path, store: &ReviewStore) -> Result<()> {
    save_store(path, store)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let connection = connect_database(path, false).await?;
        Migrator::down(&connection, Some(3)).await?;
        sqlite_compat::verify_integrity(&connection).await?;
        connection.close().await?;
        Ok(())
    })
}

#[cfg(test)]
pub(super) fn make_schema_v15_database(path: &Path, store: &ReviewStore) -> Result<()> {
    save_store(path, store)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let connection = connect_database(path, false).await?;
        Migrator::down(&connection, Some(2)).await?;
        sqlite_compat::verify_integrity(&connection).await?;
        connection.close().await?;
        Ok(())
    })
}

#[cfg(test)]
pub(super) fn make_legacy_version_database(path: &Path, version: i64) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building review database runtime")?;
    runtime.block_on(async {
        let connection = connect_database(path, true).await?;
        sqlite_compat::set_user_version(&connection, version).await?;
        connection.close().await?;
        Ok(())
    })
}

#[cfg(test)]
fn expected_indexes() -> [(&'static str, &'static str); 20] {
    [
        ("profiles", "idx_profiles_position"),
        ("images", "idx_images_position"),
        ("images", "idx_images_rating_position"),
        ("images", "idx_images_updated_at"),
        ("image_exif_tags", "idx_image_exif_tags_tag"),
        ("image_focus_regions", "idx_image_focus_regions_primary"),
        ("image_labels", "idx_image_labels_label"),
        (
            "image_publish_profiles",
            "idx_image_publish_profiles_profile",
        ),
        (
            "image_profile_bw_filters",
            "idx_image_profile_bw_filters_profile",
        ),
        ("image_tags", "idx_image_tags_tag_id"),
        ("image_profile_renders", "idx_image_profile_renders_profile"),
        ("image_profile_renders", "idx_image_profile_renders_enabled"),
        (
            "image_profile_renders",
            "idx_image_profile_renders_image_profile",
        ),
        ("profile_pp3_sections", "idx_profile_pp3_sections_section"),
        ("profile_pp3_entries", "idx_profile_pp3_entries_key"),
        ("profiles", "idx_profiles_identity"),
        ("panorama_projects", "idx_panorama_projects_updated_at"),
        ("panorama_projects", "idx_panorama_projects_result_image"),
        (
            "panorama_project_images",
            "idx_panorama_project_images_image",
        ),
        ("panorama_previews", "idx_panorama_previews_status"),
    ]
}
