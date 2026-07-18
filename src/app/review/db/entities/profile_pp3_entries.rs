//! SeaORM entity for ordered PP3 entries.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "profile_pp3_entries")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub profile_index: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub section_position: i64,
    #[sea_orm(primary_key, auto_increment = false)]
    pub entry_position: i64,
    #[sea_orm(column_type = "Text")]
    pub key: String,
    #[sea_orm(column_type = "Text")]
    pub value: String,
    #[sea_orm(
        belongs_to,
        from = "(profile_index, section_position)",
        to = "(profile_index, section_position)",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    pub profile_pp3_section: BelongsTo<super::profile_pp3_sections::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
