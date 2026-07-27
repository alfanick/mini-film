use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "auto_import_assets")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub asset_id: i64,
    pub group_id: i64,
    #[sea_orm(column_type = "Text")]
    pub media_kind: String,
    #[sea_orm(column_type = "Text")]
    pub source_filename: String,
    #[sea_orm(column_type = "Text")]
    pub source_filename_key: String,
    pub source_modified_ns: i64,
    pub source_size_bytes: i64,
    #[sea_orm(column_type = "Text")]
    pub destination_filename: String,
    #[sea_orm(column_type = "Text")]
    pub destination_filename_key: String,
    #[sea_orm(column_type = "Text")]
    pub active_filename: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub image_unique_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub capture_timestamp: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub capture_subsecond: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub capture_offset: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub camera_serial: Option<String>,
    pub shutter_count: Option<i64>,
    #[sea_orm(column_type = "Text", nullable)]
    pub camera_make: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub camera_model: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub original_raw_filename: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub imported_at: String,
    #[sea_orm(column_type = "Text")]
    pub updated_at: String,
    #[sea_orm(
        belongs_to,
        from = "group_id",
        to = "group_id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    pub group: BelongsTo<super::auto_import_groups::Entity>,
    #[sea_orm(has_many)]
    pub sources: HasMany<super::auto_import_sources::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
