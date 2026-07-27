use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "auto_import_devices")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub device_id: i64,
    #[sea_orm(column_type = "Text", unique)]
    pub device_key: String,
    #[sea_orm(column_type = "Text")]
    pub display_name: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub serial: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub first_seen_at: String,
    #[sea_orm(column_type = "Text")]
    pub last_seen_at: String,
    #[sea_orm(has_many)]
    pub storages: HasMany<super::auto_import_storages::Entity>,
    #[sea_orm(has_many)]
    pub groups: HasMany<super::auto_import_groups::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
