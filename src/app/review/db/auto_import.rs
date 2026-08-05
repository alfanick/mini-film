use super::{ReviewDatabase, entities::*, *};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, IntoActiveModel, QueryFilter, Set,
    TransactionTrait,
};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct AutoImportIdentity {
    pub(crate) image_unique_id: Option<String>,
    pub(crate) capture_timestamp: Option<String>,
    pub(crate) capture_subsecond: Option<String>,
    pub(crate) capture_offset: Option<String>,
    pub(crate) camera_serial: Option<String>,
    pub(crate) shutter_count: Option<i64>,
    pub(crate) camera_make: Option<String>,
    pub(crate) camera_model: Option<String>,
    pub(crate) original_raw_filename: Option<String>,
}

impl AutoImportIdentity {
    pub(crate) fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AutoImportMediaKind {
    Raw,
    Jpeg,
}

impl AutoImportMediaKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Jpeg => "jpeg",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutoImportDevice {
    pub(crate) id: i64,
    pub(crate) key: String,
    pub(crate) display_name: String,
    pub(crate) serial: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutoImportStorage {
    pub(crate) id: i64,
    pub(crate) device_id: i64,
    pub(crate) key: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutoImportGroup {
    pub(crate) id: i64,
    pub(crate) device_id: i64,
    pub(crate) destination_stem: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AutoImportAsset {
    pub(crate) id: i64,
    pub(crate) group_id: i64,
    pub(crate) device_id: i64,
    pub(crate) media_kind: AutoImportMediaKind,
    pub(crate) source_filename: String,
    pub(crate) source_modified_ns: i64,
    pub(crate) source_size_bytes: i64,
    pub(crate) destination_filename: String,
    pub(crate) active_filename: String,
    pub(crate) identity: AutoImportIdentity,
}

#[derive(Clone, Debug)]
pub(crate) struct AutoImportRecord {
    pub(crate) device_id: i64,
    pub(crate) storage_id: i64,
    pub(crate) source_stem: String,
    pub(crate) source_stem_key: String,
    pub(crate) source_modified_ns: i64,
    pub(crate) destination_stem: String,
    pub(crate) media_kind: AutoImportMediaKind,
    pub(crate) source_filename: String,
    pub(crate) source_filename_key: String,
    pub(crate) source_size_bytes: i64,
    pub(crate) relative_path: String,
    pub(crate) relative_path_key: String,
    pub(crate) destination_filename: String,
    pub(crate) active_filename: String,
    pub(crate) identity: AutoImportIdentity,
}

#[derive(Clone, Debug)]
pub(crate) struct AutoImportSourceRecord {
    pub(crate) asset_id: i64,
    pub(crate) storage_id: i64,
    pub(crate) relative_path: String,
    pub(crate) relative_path_key: String,
    pub(crate) source_filename: String,
    pub(crate) source_modified_ns: i64,
    pub(crate) source_size_bytes: i64,
}

#[derive(Clone)]
pub(crate) struct AutoImportCatalog {
    database: ReviewDatabase,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl AutoImportCatalog {
    pub(crate) fn open(input_root: &Path, output_root: &Path) -> Result<Self> {
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("mini-film-auto-import-db")
                .enable_all()
                .build()
                .context("building auto-import database runtime")?,
        );
        let (database, stored) =
            runtime.block_on(ReviewDatabase::open_output(input_root, output_root))?;
        if stored.is_none() {
            runtime.block_on(database.replace_store(&ReviewStore::new(Vec::new())))?;
        }
        Ok(Self { database, runtime })
    }

    fn from_database(database: ReviewDatabase, runtime: Arc<tokio::runtime::Runtime>) -> Self {
        Self { database, runtime }
    }

    pub(crate) fn register_device(
        &self,
        key: &str,
        display_name: &str,
        serial: Option<&str>,
    ) -> Result<AutoImportDevice> {
        self.runtime.block_on(
            self.database
                .register_auto_import_device(key, display_name, serial),
        )
    }

    pub(crate) fn register_storage(
        &self,
        device_id: i64,
        key: &str,
        display_name: &str,
    ) -> Result<AutoImportStorage> {
        self.runtime
            .block_on(
                self.database
                    .register_auto_import_storage(device_id, key, display_name),
            )
    }

    pub(crate) fn find_source(
        &self,
        storage_id: i64,
        relative_path_key: &str,
        source_modified_ns: i64,
    ) -> Result<Option<AutoImportAsset>> {
        self.runtime.block_on(self.database.find_auto_import_source(
            storage_id,
            relative_path_key,
            source_modified_ns,
        ))
    }

    pub(crate) fn find_group(
        &self,
        device_id: i64,
        source_stem_key: &str,
        source_modified_ns: i64,
    ) -> Result<Option<AutoImportGroup>> {
        self.runtime.block_on(self.database.find_auto_import_group(
            device_id,
            source_stem_key,
            source_modified_ns,
        ))
    }

    pub(crate) fn find_group_asset(
        &self,
        group_id: i64,
        media_kind: AutoImportMediaKind,
    ) -> Result<Option<AutoImportAsset>> {
        self.runtime.block_on(
            self.database
                .find_auto_import_group_asset(group_id, media_kind),
        )
    }

    pub(crate) fn destination_stem_exists(&self, destination_stem: &str) -> Result<bool> {
        self.runtime.block_on(
            self.database
                .auto_import_destination_stem_exists(destination_stem),
        )
    }

    pub(crate) fn find_filename_candidates(
        &self,
        source_filename_key: &str,
        media_kind: AutoImportMediaKind,
    ) -> Result<Vec<AutoImportAsset>> {
        self.runtime.block_on(
            self.database
                .find_auto_import_filename_candidates(source_filename_key, media_kind),
        )
    }

    pub(crate) fn find_destination_asset(
        &self,
        destination_filename: &str,
    ) -> Result<Option<AutoImportAsset>> {
        self.runtime.block_on(
            self.database
                .find_auto_import_destination_asset(destination_filename),
        )
    }

    pub(crate) fn record_import(&self, record: AutoImportRecord) -> Result<AutoImportAsset> {
        self.runtime
            .block_on(self.database.record_auto_import(record))
    }

    pub(crate) fn record_source(&self, record: AutoImportSourceRecord) -> Result<()> {
        self.runtime
            .block_on(self.database.record_auto_import_source(record))
    }

    pub(crate) fn update_active_filename(
        &self,
        asset_id: i64,
        active_filename: &str,
    ) -> Result<()> {
        self.runtime.block_on(
            self.database
                .update_auto_import_active_filename(asset_id, active_filename),
        )
    }

    pub(crate) fn update_identity(
        &self,
        asset_id: i64,
        identity: &AutoImportIdentity,
    ) -> Result<()> {
        self.runtime.block_on(
            self.database
                .update_auto_import_identity(asset_id, identity),
        )
    }
}

impl ReviewDatabase {
    pub(in crate::app::review) fn auto_import_catalog(
        &self,
        runtime: Arc<tokio::runtime::Runtime>,
    ) -> AutoImportCatalog {
        AutoImportCatalog::from_database(self.clone(), runtime)
    }

    async fn register_auto_import_device(
        &self,
        key: &str,
        display_name: &str,
        serial: Option<&str>,
    ) -> Result<AutoImportDevice> {
        let _write_guard = self.write_lock.lock().await;
        let now = now_text();
        let row = auto_import_devices::Entity::find()
            .filter(auto_import_devices::Column::DeviceKey.eq(key))
            .one(&self.connection)
            .await
            .context("looking up auto-import device")?;
        let row = if let Some(row) = row {
            let mut active = row.into_active_model();
            active.display_name = Set(display_name.to_string());
            active.serial = Set(serial.map(str::to_string));
            active.last_seen_at = Set(now);
            active
                .update(&self.connection)
                .await
                .context("updating auto-import device")?
        } else {
            auto_import_devices::ActiveModel {
                device_id: sea_orm::NotSet,
                device_key: Set(key.to_string()),
                display_name: Set(display_name.to_string()),
                serial: Set(serial.map(str::to_string)),
                first_seen_at: Set(now.clone()),
                last_seen_at: Set(now),
            }
            .insert(&self.connection)
            .await
            .context("creating auto-import device")?
        };
        Ok(device_from_row(row))
    }

    async fn register_auto_import_storage(
        &self,
        device_id: i64,
        key: &str,
        display_name: &str,
    ) -> Result<AutoImportStorage> {
        let _write_guard = self.write_lock.lock().await;
        let row = auto_import_storages::Entity::find()
            .filter(auto_import_storages::Column::DeviceId.eq(device_id))
            .filter(auto_import_storages::Column::StorageKey.eq(key))
            .one(&self.connection)
            .await
            .context("looking up auto-import storage")?;
        let row = if let Some(row) = row {
            let mut active = row.into_active_model();
            active.display_name = Set(display_name.to_string());
            active.last_seen_at = Set(now_text());
            active
                .update(&self.connection)
                .await
                .context("updating auto-import storage")?
        } else {
            auto_import_storages::ActiveModel {
                storage_id: sea_orm::NotSet,
                device_id: Set(device_id),
                storage_key: Set(key.to_string()),
                display_name: Set(display_name.to_string()),
                last_seen_at: Set(now_text()),
            }
            .insert(&self.connection)
            .await
            .context("creating auto-import storage")?
        };
        Ok(AutoImportStorage {
            id: row.storage_id,
            device_id: row.device_id,
            key: row.storage_key,
        })
    }

    async fn find_auto_import_source(
        &self,
        storage_id: i64,
        relative_path_key: &str,
        source_modified_ns: i64,
    ) -> Result<Option<AutoImportAsset>> {
        let source = auto_import_sources::Entity::find()
            .filter(auto_import_sources::Column::StorageId.eq(storage_id))
            .filter(auto_import_sources::Column::RelativePathKey.eq(relative_path_key))
            .filter(auto_import_sources::Column::SourceModifiedNs.eq(source_modified_ns))
            .one(&self.connection)
            .await
            .context("looking up auto-import source")?;
        match source {
            Some(source) => self.auto_import_asset_by_id(source.asset_id).await,
            None => Ok(None),
        }
    }

    async fn find_auto_import_group(
        &self,
        device_id: i64,
        source_stem_key: &str,
        source_modified_ns: i64,
    ) -> Result<Option<AutoImportGroup>> {
        auto_import_groups::Entity::find()
            .filter(auto_import_groups::Column::DeviceId.eq(device_id))
            .filter(auto_import_groups::Column::SourceStemKey.eq(source_stem_key))
            .filter(auto_import_groups::Column::SourceModifiedNs.eq(source_modified_ns))
            .one(&self.connection)
            .await
            .context("looking up auto-import capture group")
            .map(|row| row.map(group_from_row))
    }

    async fn find_auto_import_group_asset(
        &self,
        group_id: i64,
        media_kind: AutoImportMediaKind,
    ) -> Result<Option<AutoImportAsset>> {
        let row = auto_import_assets::Entity::find()
            .filter(auto_import_assets::Column::GroupId.eq(group_id))
            .filter(auto_import_assets::Column::MediaKind.eq(media_kind.as_str()))
            .one(&self.connection)
            .await
            .context("looking up auto-import group asset")?;
        match row {
            Some(row) => self.auto_import_asset_from_row(row).await.map(Some),
            None => Ok(None),
        }
    }

    async fn auto_import_destination_stem_exists(&self, destination_stem: &str) -> Result<bool> {
        auto_import_groups::Entity::find()
            .filter(
                auto_import_groups::Column::DestinationStemKey.eq(destination_stem.to_lowercase()),
            )
            .one(&self.connection)
            .await
            .context("checking auto-import destination stem")
            .map(|row| row.is_some())
    }

    async fn find_auto_import_filename_candidates(
        &self,
        source_filename_key: &str,
        media_kind: AutoImportMediaKind,
    ) -> Result<Vec<AutoImportAsset>> {
        let rows = auto_import_assets::Entity::find()
            .filter(auto_import_assets::Column::SourceFilenameKey.eq(source_filename_key))
            .filter(auto_import_assets::Column::MediaKind.eq(media_kind.as_str()))
            .all(&self.connection)
            .await
            .context("looking up auto-import filename candidates")?;
        let mut assets = Vec::with_capacity(rows.len());
        for row in rows {
            assets.push(self.auto_import_asset_from_row(row).await?);
        }
        Ok(assets)
    }

    async fn find_auto_import_destination_asset(
        &self,
        destination_filename: &str,
    ) -> Result<Option<AutoImportAsset>> {
        let row = auto_import_assets::Entity::find()
            .filter(
                auto_import_assets::Column::DestinationFilenameKey
                    .eq(destination_filename.to_lowercase()),
            )
            .one(&self.connection)
            .await
            .context("looking up auto-import destination asset")?;
        match row {
            Some(row) => self.auto_import_asset_from_row(row).await.map(Some),
            None => Ok(None),
        }
    }

    async fn record_auto_import(&self, record: AutoImportRecord) -> Result<AutoImportAsset> {
        let _write_guard = self.write_lock.lock().await;
        let transaction = self
            .connection
            .begin()
            .await
            .context("starting auto-import record transaction")?;
        let result = record_auto_import_transaction(&transaction, record).await;
        match result {
            Ok(row) => {
                transaction
                    .commit()
                    .await
                    .context("committing auto-import record")?;
                self.auto_import_asset_from_row(row).await
            }
            Err(error) => {
                transaction.rollback().await.ok();
                Err(error)
            }
        }
    }

    async fn record_auto_import_source(&self, record: AutoImportSourceRecord) -> Result<()> {
        let _write_guard = self.write_lock.lock().await;
        upsert_source(&self.connection, &record).await
    }

    async fn update_auto_import_active_filename(
        &self,
        asset_id: i64,
        active_filename: &str,
    ) -> Result<()> {
        let _write_guard = self.write_lock.lock().await;
        let row = auto_import_assets::Entity::find_by_id(asset_id)
            .one(&self.connection)
            .await
            .context("reading auto-import asset")?
            .with_context(|| format!("auto-import asset {asset_id} does not exist"))?;
        let mut active = row.into_active_model();
        active.active_filename = Set(active_filename.to_string());
        active.updated_at = Set(now_text());
        active
            .update(&self.connection)
            .await
            .context("updating auto-import DNG successor")?;
        Ok(())
    }

    async fn update_auto_import_identity(
        &self,
        asset_id: i64,
        identity: &AutoImportIdentity,
    ) -> Result<()> {
        let _write_guard = self.write_lock.lock().await;
        let row = auto_import_assets::Entity::find_by_id(asset_id)
            .one(&self.connection)
            .await
            .context("reading auto-import asset")?
            .with_context(|| format!("auto-import asset {asset_id} does not exist"))?;
        let mut active = row.into_active_model();
        set_identity(&mut active, identity);
        active.updated_at = Set(now_text());
        active
            .update(&self.connection)
            .await
            .context("updating auto-import EXIF identity")?;
        Ok(())
    }

    async fn auto_import_asset_by_id(&self, asset_id: i64) -> Result<Option<AutoImportAsset>> {
        let row = auto_import_assets::Entity::find_by_id(asset_id)
            .one(&self.connection)
            .await
            .context("reading auto-import asset")?;
        match row {
            Some(row) => self.auto_import_asset_from_row(row).await.map(Some),
            None => Ok(None),
        }
    }

    async fn auto_import_asset_from_row(
        &self,
        row: auto_import_assets::Model,
    ) -> Result<AutoImportAsset> {
        let group = auto_import_groups::Entity::find_by_id(row.group_id)
            .one(&self.connection)
            .await
            .context("reading auto-import capture group")?
            .with_context(|| {
                format!(
                    "auto-import asset {} has no capture group {}",
                    row.asset_id, row.group_id
                )
            })?;
        asset_from_rows(row, group)
    }
}

async fn record_auto_import_transaction<C>(
    connection: &C,
    record: AutoImportRecord,
) -> Result<auto_import_assets::Model>
where
    C: ConnectionTrait,
{
    let group = auto_import_groups::Entity::find()
        .filter(auto_import_groups::Column::DeviceId.eq(record.device_id))
        .filter(auto_import_groups::Column::SourceStemKey.eq(&record.source_stem_key))
        .filter(auto_import_groups::Column::SourceModifiedNs.eq(record.source_modified_ns))
        .one(connection)
        .await
        .context("looking up auto-import capture group for record")?;
    let group = if let Some(group) = group {
        group
    } else {
        auto_import_groups::ActiveModel {
            group_id: sea_orm::NotSet,
            device_id: Set(record.device_id),
            source_stem: Set(record.source_stem.clone()),
            source_stem_key: Set(record.source_stem_key.clone()),
            source_modified_ns: Set(record.source_modified_ns),
            destination_stem: Set(record.destination_stem.clone()),
            destination_stem_key: Set(record.destination_stem.to_lowercase()),
            created_at: Set(now_text()),
        }
        .insert(connection)
        .await
        .context("creating auto-import capture group")?
    };
    if !group
        .destination_stem
        .eq_ignore_ascii_case(&record.destination_stem)
    {
        bail!(
            "auto-import capture group destination changed from {:?} to {:?}",
            group.destination_stem,
            record.destination_stem
        );
    }

    let row = auto_import_assets::Entity::find()
        .filter(auto_import_assets::Column::GroupId.eq(group.group_id))
        .filter(auto_import_assets::Column::MediaKind.eq(record.media_kind.as_str()))
        .one(connection)
        .await
        .context("looking up auto-import asset for record")?;
    let row = if let Some(row) = row {
        if !row
            .destination_filename
            .eq_ignore_ascii_case(&record.destination_filename)
        {
            bail!(
                "auto-import capture already has {} asset at {}, cannot record {}",
                record.media_kind.as_str(),
                row.destination_filename,
                record.destination_filename
            );
        }
        let mut active = row.into_active_model();
        active.source_filename = Set(record.source_filename.clone());
        active.source_filename_key = Set(record.source_filename_key.clone());
        active.source_modified_ns = Set(record.source_modified_ns);
        active.source_size_bytes = Set(record.source_size_bytes);
        active.active_filename = Set(record.active_filename.clone());
        set_identity(&mut active, &record.identity);
        active.updated_at = Set(now_text());
        active
            .update(connection)
            .await
            .context("updating auto-import asset")?
    } else {
        let now = now_text();
        let mut active = auto_import_assets::ActiveModel {
            asset_id: sea_orm::NotSet,
            group_id: Set(group.group_id),
            media_kind: Set(record.media_kind.as_str().to_string()),
            source_filename: Set(record.source_filename.clone()),
            source_filename_key: Set(record.source_filename_key.clone()),
            source_modified_ns: Set(record.source_modified_ns),
            source_size_bytes: Set(record.source_size_bytes),
            destination_filename: Set(record.destination_filename.clone()),
            destination_filename_key: Set(record.destination_filename.to_lowercase()),
            active_filename: Set(record.active_filename.clone()),
            image_unique_id: Set(None),
            capture_timestamp: Set(None),
            capture_subsecond: Set(None),
            capture_offset: Set(None),
            camera_serial: Set(None),
            shutter_count: Set(None),
            camera_make: Set(None),
            camera_model: Set(None),
            original_raw_filename: Set(None),
            imported_at: Set(now.clone()),
            updated_at: Set(now),
        };
        set_identity(&mut active, &record.identity);
        active
            .insert(connection)
            .await
            .context("creating auto-import asset")?
    };
    upsert_source(
        connection,
        &AutoImportSourceRecord {
            asset_id: row.asset_id,
            storage_id: record.storage_id,
            relative_path: record.relative_path,
            relative_path_key: record.relative_path_key,
            source_filename: record.source_filename,
            source_modified_ns: record.source_modified_ns,
            source_size_bytes: record.source_size_bytes,
        },
    )
    .await?;
    Ok(row)
}

async fn upsert_source<C>(connection: &C, record: &AutoImportSourceRecord) -> Result<()>
where
    C: ConnectionTrait,
{
    let row = auto_import_sources::Entity::find()
        .filter(auto_import_sources::Column::StorageId.eq(record.storage_id))
        .filter(auto_import_sources::Column::RelativePathKey.eq(&record.relative_path_key))
        .filter(auto_import_sources::Column::SourceModifiedNs.eq(record.source_modified_ns))
        .one(connection)
        .await
        .context("looking up auto-import source record")?;
    if let Some(row) = row {
        let mut active = row.into_active_model();
        active.asset_id = Set(record.asset_id);
        active.relative_path = Set(record.relative_path.clone());
        active.source_filename = Set(record.source_filename.clone());
        active.source_size_bytes = Set(record.source_size_bytes);
        active.last_seen_at = Set(now_text());
        active
            .update(connection)
            .await
            .context("updating auto-import source record")?;
    } else {
        auto_import_sources::ActiveModel {
            source_id: sea_orm::NotSet,
            asset_id: Set(record.asset_id),
            storage_id: Set(record.storage_id),
            relative_path: Set(record.relative_path.clone()),
            relative_path_key: Set(record.relative_path_key.clone()),
            source_filename: Set(record.source_filename.clone()),
            source_modified_ns: Set(record.source_modified_ns),
            source_size_bytes: Set(record.source_size_bytes),
            last_seen_at: Set(now_text()),
        }
        .insert(connection)
        .await
        .context("creating auto-import source record")?;
    }
    Ok(())
}

fn set_identity(active: &mut auto_import_assets::ActiveModel, identity: &AutoImportIdentity) {
    active.image_unique_id = Set(identity.image_unique_id.clone());
    active.capture_timestamp = Set(identity.capture_timestamp.clone());
    active.capture_subsecond = Set(identity.capture_subsecond.clone());
    active.capture_offset = Set(identity.capture_offset.clone());
    active.camera_serial = Set(identity.camera_serial.clone());
    active.shutter_count = Set(identity.shutter_count);
    active.camera_make = Set(identity.camera_make.clone());
    active.camera_model = Set(identity.camera_model.clone());
    active.original_raw_filename = Set(identity.original_raw_filename.clone());
}

fn device_from_row(row: auto_import_devices::Model) -> AutoImportDevice {
    AutoImportDevice {
        id: row.device_id,
        key: row.device_key,
        display_name: row.display_name,
        serial: row.serial,
    }
}

fn group_from_row(row: auto_import_groups::Model) -> AutoImportGroup {
    AutoImportGroup {
        id: row.group_id,
        device_id: row.device_id,
        destination_stem: row.destination_stem,
    }
}

fn asset_from_rows(
    row: auto_import_assets::Model,
    group: auto_import_groups::Model,
) -> Result<AutoImportAsset> {
    let media_kind = match row.media_kind.as_str() {
        "raw" => AutoImportMediaKind::Raw,
        "jpeg" => AutoImportMediaKind::Jpeg,
        value => bail!("invalid auto-import media kind {value:?}"),
    };
    Ok(AutoImportAsset {
        id: row.asset_id,
        group_id: row.group_id,
        device_id: group.device_id,
        media_kind,
        source_filename: row.source_filename,
        source_modified_ns: row.source_modified_ns,
        source_size_bytes: row.source_size_bytes,
        destination_filename: row.destination_filename,
        active_filename: row.active_filename,
        identity: AutoImportIdentity {
            image_unique_id: row.image_unique_id,
            capture_timestamp: row.capture_timestamp,
            capture_subsecond: row.capture_subsecond,
            capture_offset: row.capture_offset,
            camera_serial: row.camera_serial,
            shutter_count: row.shutter_count,
            camera_make: row.camera_make,
            camera_model: row.camera_model,
            original_raw_filename: row.original_raw_filename,
        },
    })
}

fn now_text() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::mpsc::{self, RecvTimeoutError},
        thread,
        time::Duration,
    };

    use super::*;

    #[test]
    fn auto_import_mutations_share_the_review_write_lock() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("input");
        let output = root.path().join("output");
        fs::create_dir_all(&input).unwrap();
        fs::create_dir_all(&output).unwrap();

        let catalog = AutoImportCatalog::open(&input, &output).unwrap();
        let device = catalog
            .register_device("camera-1", "Camera 1", Some("serial-1"))
            .unwrap();
        let primary_storage = catalog
            .register_storage(device.id, "card-1", "Card 1")
            .unwrap();
        let secondary_storage = catalog
            .register_storage(device.id, "card-2", "Card 2")
            .unwrap();
        let asset = catalog
            .record_import(import_record(device.id, primary_storage.id, "frame-1", 1))
            .unwrap();

        assert_catalog_mutation_waits(&catalog, "register device", |catalog| {
            catalog
                .register_device("camera-2", "Camera 2", Some("serial-2"))
                .map(|_| ())
        });
        let device_id = device.id;
        assert_catalog_mutation_waits(&catalog, "register storage", move |catalog| {
            catalog
                .register_storage(device_id, "card-3", "Card 3")
                .map(|_| ())
        });
        let record = import_record(device.id, primary_storage.id, "frame-2", 2);
        assert_catalog_mutation_waits(&catalog, "record import", move |catalog| {
            catalog.record_import(record).map(|_| ())
        });
        let source = AutoImportSourceRecord {
            asset_id: asset.id,
            storage_id: secondary_storage.id,
            relative_path: "DCIM/frame-1.NEF".to_string(),
            relative_path_key: "dcim/frame-1.nef".to_string(),
            source_filename: "frame-1.NEF".to_string(),
            source_modified_ns: 1,
            source_size_bytes: 42,
        };
        assert_catalog_mutation_waits(&catalog, "record source", move |catalog| {
            catalog.record_source(source)
        });
        let asset_id = asset.id;
        assert_catalog_mutation_waits(&catalog, "update active filename", move |catalog| {
            catalog.update_active_filename(asset_id, "frame-1.dng")
        });
        let asset_id = asset.id;
        let identity = AutoImportIdentity {
            image_unique_id: Some("image-1".to_string()),
            ..AutoImportIdentity::default()
        };
        assert_catalog_mutation_waits(&catalog, "update identity", move |catalog| {
            catalog.update_identity(asset_id, &identity)
        });
    }

    fn assert_catalog_mutation_waits<F>(catalog: &AutoImportCatalog, operation: &str, mutation: F)
    where
        F: FnOnce(&AutoImportCatalog) -> Result<()> + Send + 'static,
    {
        let guard = catalog
            .runtime
            .block_on(catalog.database.write_lock.clone().lock_owned());
        let worker_catalog = catalog.clone();
        let (started_sender, started_receiver) = mpsc::channel();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            started_sender.send(()).unwrap();
            sender.send(mutation(&worker_catalog)).unwrap();
        });

        started_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or_else(|error| panic!("{operation} worker did not start: {error}"));
        let before_release = receiver.recv_timeout(Duration::from_millis(100));
        let blocked = matches!(before_release, Err(RecvTimeoutError::Timeout));
        drop(guard);
        let result = match before_release {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|error| panic!("{operation} did not finish after unlock: {error}")),
            Err(RecvTimeoutError::Disconnected) => {
                panic!("{operation} worker disconnected before returning")
            }
        };
        worker.join().unwrap();

        assert!(blocked, "{operation} bypassed the review write lock");
        result.unwrap_or_else(|error| panic!("{operation} failed after unlock: {error:#}"));
    }

    fn import_record(
        device_id: i64,
        storage_id: i64,
        stem: &str,
        modified_ns: i64,
    ) -> AutoImportRecord {
        let source_filename = format!("{stem}.NEF");
        AutoImportRecord {
            device_id,
            storage_id,
            source_stem: stem.to_string(),
            source_stem_key: stem.to_lowercase(),
            source_modified_ns: modified_ns,
            destination_stem: stem.to_string(),
            media_kind: AutoImportMediaKind::Raw,
            source_filename: source_filename.clone(),
            source_filename_key: source_filename.to_lowercase(),
            source_size_bytes: 42,
            relative_path: format!("DCIM/{source_filename}"),
            relative_path_key: format!("dcim/{}", source_filename.to_lowercase()),
            destination_filename: source_filename.clone(),
            active_filename: source_filename,
            identity: AutoImportIdentity::default(),
        }
    }
}
