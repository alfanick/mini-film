use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "panorama_project_images")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub panorama_id: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub position: i64,
    pub image_id: i64,
    #[sea_orm(
        belongs_to,
        from = "panorama_id",
        to = "panorama_id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    pub project: BelongsTo<super::panorama_projects::Entity>,
    #[sea_orm(
        belongs_to,
        from = "image_id",
        to = "image_id",
        on_update = "NoAction",
        on_delete = "Restrict"
    )]
    pub image: BelongsTo<super::images::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
