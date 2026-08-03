//! `SeaORM` entity for per-image, per-profile diffusion overrides.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "image_profile_diffusion_settings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub image_id: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub profile_index: i64,
    #[sea_orm(column_type = "Text")]
    pub method: String,
    pub softness: i64,
    pub highlight_glow: i64,
    #[sea_orm(
        belongs_to,
        from = "image_id",
        to = "image_id",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    pub images: BelongsTo<super::images::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
