use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "auto_import_sources")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub source_id: i64,
    pub asset_id: i64,
    pub storage_id: i64,
    #[sea_orm(column_type = "Text")]
    pub relative_path: String,
    #[sea_orm(column_type = "Text")]
    pub relative_path_key: String,
    #[sea_orm(column_type = "Text")]
    pub source_filename: String,
    pub source_modified_ns: i64,
    pub source_size_bytes: i64,
    #[sea_orm(column_type = "Text")]
    pub last_seen_at: String,
    #[sea_orm(
        belongs_to,
        from = "asset_id",
        to = "asset_id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    pub asset: BelongsTo<super::auto_import_assets::Entity>,
    #[sea_orm(
        belongs_to,
        from = "storage_id",
        to = "storage_id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    pub storage: BelongsTo<super::auto_import_storages::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
