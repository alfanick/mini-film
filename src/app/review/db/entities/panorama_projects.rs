use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "panorama_projects")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub panorama_id: i64,
    #[sea_orm(column_type = "Text")]
    pub name: String,
    #[sea_orm(column_type = "Text")]
    pub status: String,
    #[sea_orm(column_type = "Text")]
    pub matching_mode: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub selected_projection: Option<String>,
    #[sea_orm(column_type = "Text", nullable, unique)]
    pub output_path: Option<String>,
    pub result_image_id: Option<i64>,
    #[sea_orm(column_type = "Text", nullable)]
    pub progress_stage: Option<String>,
    pub progress_completed: i64,
    pub progress_total: i64,
    #[sea_orm(column_type = "Text", nullable)]
    pub error: Option<String>,
    #[sea_orm(column_type = "Text")]
    pub created_at: String,
    #[sea_orm(column_type = "Text")]
    pub updated_at: String,
    #[sea_orm(
        belongs_to,
        from = "result_image_id",
        to = "image_id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    pub result_image: BelongsTo<Option<super::images::Entity>>,
    #[sea_orm(has_many)]
    pub sources: HasMany<super::panorama_project_images::Entity>,
    #[sea_orm(has_many)]
    pub previews: HasMany<super::panorama_previews::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
