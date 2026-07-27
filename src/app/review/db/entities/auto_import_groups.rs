use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "auto_import_groups")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub group_id: i64,
    pub device_id: i64,
    #[sea_orm(column_type = "Text")]
    pub source_stem: String,
    #[sea_orm(column_type = "Text")]
    pub source_stem_key: String,
    pub source_modified_ns: i64,
    #[sea_orm(column_type = "Text")]
    pub destination_stem: String,
    #[sea_orm(column_type = "Text")]
    pub destination_stem_key: String,
    #[sea_orm(column_type = "Text")]
    pub created_at: String,
    #[sea_orm(
        belongs_to,
        from = "device_id",
        to = "device_id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    pub device: BelongsTo<super::auto_import_devices::Entity>,
    #[sea_orm(has_many)]
    pub assets: HasMany<super::auto_import_assets::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
