use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "schema_migrations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub version: i64,
    #[sea_orm(column_type = "Text")]
    pub name: String,
    #[sea_orm(column_type = "Text")]
    pub applied_at: String,
}

impl ActiveModelBehavior for ActiveModel {}
