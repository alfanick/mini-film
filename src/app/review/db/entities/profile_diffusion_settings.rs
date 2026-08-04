//! `SeaORM` entity for persisted profile-wide diffusion defaults.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "profile_diffusion_settings")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub profile_index: i64,
    #[sea_orm(column_type = "Text")]
    pub method: String,
    pub softness: i64,
    pub highlight_glow: i64,
    #[sea_orm(default_value = 100)]
    pub softness_radius_percent: i64,
    #[sea_orm(default_value = 100)]
    pub glow_radius_percent: i64,
    #[sea_orm(default_value = 100)]
    pub intensity_percent: i64,
    #[sea_orm(default_value = 50)]
    pub highlight_reach: i64,
    #[sea_orm(
        belongs_to,
        from = "profile_index",
        to = "profile_index",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    pub profiles: BelongsTo<super::profiles::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
