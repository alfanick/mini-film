use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "auto_import_storages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub storage_id: i64,
    pub device_id: i64,
    #[sea_orm(column_type = "Text")]
    pub storage_key: String,
    #[sea_orm(column_type = "Text")]
    pub display_name: String,
    #[sea_orm(column_type = "Text")]
    pub last_seen_at: String,
    #[sea_orm(
        belongs_to,
        from = "device_id",
        to = "device_id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    pub device: BelongsTo<super::auto_import_devices::Entity>,
    #[sea_orm(has_many)]
    pub sources: HasMany<super::auto_import_sources::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
