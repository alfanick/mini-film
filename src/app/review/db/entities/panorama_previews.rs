use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "panorama_previews")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub panorama_id: i64,
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub matching_mode: String,
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub projection: String,
    #[sea_orm(column_type = "Text")]
    pub status: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub preview_path: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub cache_key: Option<String>,
    pub duration_ms: Option<i64>,
    #[sea_orm(column_type = "Text", nullable)]
    pub error: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub updated_at: String,
    #[sea_orm(
        belongs_to,
        from = "panorama_id",
        to = "panorama_id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    pub project: BelongsTo<super::panorama_projects::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
