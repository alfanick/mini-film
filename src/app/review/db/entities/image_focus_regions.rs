//! `SeaORM` entity for normalized camera focus regions.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "image_focus_regions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub image_id: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub position: i64,
    #[sea_orm(column_type = "Double")]
    pub x: f64,
    #[sea_orm(column_type = "Double")]
    pub y: f64,
    #[sea_orm(column_type = "Double")]
    pub width: f64,
    #[sea_orm(column_type = "Double")]
    pub height: f64,
    pub primary: i64,
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
