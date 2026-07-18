use super::{entities::*, sqlite_compat};
use sea_orm::{DbBackend, EntityName, EntityTrait, Schema};
use sea_orm_migration::{MigratorTrait, prelude::*};

pub(super) const LEGACY_SCHEMA_VERSION: i64 = 11;
pub(super) const FIRST_SEAORM_SCHEMA_VERSION: i64 = 12;
pub(super) const LATEST_SCHEMA_VERSION: i64 = 12;
pub(super) const V18_BASELINE_MIGRATION: &str = "m20260718_000001_v18_baseline";
pub(super) const PRE_RELEASE_SEAORM_LEDGER: [&str; 2] = [
    "m20260718_000001_create_review_schema",
    "m20260718_000002_adopt_seaorm",
];

pub(super) const V11_LEDGER: &[(i64, &str)] = &[
    (1, "initial_review_state"),
    (2, "profile_bw_filters"),
    (3, "profile_render_processing_key"),
    (4, "active_d_lighting_and_pp3_adjustments"),
    (5, "source_file_info"),
    (6, "normalized_relational_review_store"),
    (7, "image_shutter_count"),
    (8, "image_shutter_details"),
    (9, "image_auto_iso"),
    (10, "image_white_balance"),
    (11, "image_white_balance_offset"),
];

pub(super) struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(V18Baseline)]
    }
}

struct V18Baseline;

impl MigrationName for V18Baseline {
    fn name(&self) -> &str {
        V18_BASELINE_MIGRATION
    }
}

#[async_trait::async_trait]
impl MigrationTrait for V18Baseline {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let schema = Schema::new(DbBackend::Sqlite);
        create_entity_table(manager, &schema, profiles::Entity).await?;
        create_entity_table(manager, &schema, profile_adjustments::Entity).await?;
        create_entity_table(manager, &schema, profile_sharpening::Entity).await?;
        create_entity_table(manager, &schema, profile_hsl_values::Entity).await?;
        create_entity_table(manager, &schema, profile_tone_curve_points::Entity).await?;
        create_entity_table(manager, &schema, profile_pp3_sections::Entity).await?;
        create_entity_table(manager, &schema, profile_pp3_entries::Entity).await?;
        create_entity_table(manager, &schema, images::Entity).await?;
        create_entity_table(manager, &schema, image_exif_tags::Entity).await?;
        create_entity_table(manager, &schema, tags::Entity).await?;
        create_entity_table(manager, &schema, image_tags::Entity).await?;
        create_entity_table(manager, &schema, image_labels::Entity).await?;
        create_entity_table(manager, &schema, image_publish_profiles::Entity).await?;
        create_entity_table(manager, &schema, image_profile_bw_filters::Entity).await?;
        create_entity_table(manager, &schema, image_profile_renders::Entity).await?;
        create_entity_table(manager, &schema, review_settings::Entity).await?;

        create_indexes(manager).await?;
        manager
            .drop_table(
                Table::drop()
                    .table(legacy_schema_migrations::Entity)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        sqlite_compat::set_user_version(manager.get_connection(), LATEST_SCHEMA_VERSION).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            review_settings::Entity.table_name(),
            image_profile_renders::Entity.table_name(),
            image_profile_bw_filters::Entity.table_name(),
            image_publish_profiles::Entity.table_name(),
            image_labels::Entity.table_name(),
            image_tags::Entity.table_name(),
            tags::Entity.table_name(),
            image_exif_tags::Entity.table_name(),
            images::Entity.table_name(),
            profile_pp3_entries::Entity.table_name(),
            profile_pp3_sections::Entity.table_name(),
            profile_tone_curve_points::Entity.table_name(),
            profile_hsl_values::Entity.table_name(),
            profile_sharpening::Entity.table_name(),
            profile_adjustments::Entity.table_name(),
            profiles::Entity.table_name(),
        ] {
            manager
                .drop_table(
                    Table::drop()
                        .table(Alias::new(table))
                        .if_exists()
                        .to_owned(),
                )
                .await?;
        }
        sqlite_compat::set_user_version(manager.get_connection(), 0).await
    }
}

async fn create_entity_table<E>(
    manager: &SchemaManager<'_>,
    schema: &Schema,
    entity: E,
) -> Result<(), DbErr>
where
    E: EntityTrait,
{
    let table_name = entity.table_name();
    let mut statement = schema.create_table_from_entity(entity);
    statement.if_not_exists();
    match table_name {
        "profile_adjustments" => {
            statement.check(
                Expr::col(profile_adjustments::Column::Scope).is_in(["source", "emulation"]),
            );
        }
        "profile_sharpening" => {
            statement
                .check(Expr::col(profile_sharpening::Column::Scope).is_in(["source", "emulation"]));
        }
        "profile_hsl_values" => {
            statement
                .check(Expr::col(profile_hsl_values::Column::Scope).is_in(["source", "emulation"]))
                .check(Expr::col(profile_hsl_values::Column::Channel).is_in([
                    "hue",
                    "saturation",
                    "luminance",
                ]));
        }
        "profile_tone_curve_points" => {
            statement
                .check(
                    Expr::col(profile_tone_curve_points::Column::Scope)
                        .is_in(["source", "emulation"]),
                )
                .check(
                    Expr::col(profile_tone_curve_points::Column::Channel).is_in([
                        "composite",
                        "red",
                        "green",
                        "blue",
                    ]),
                );
        }
        "image_profile_bw_filters" => {
            statement.check(
                Expr::col(image_profile_bw_filters::Column::BwFilter)
                    .is_in(["none", "yellow", "orange", "red", "green"]),
            );
        }
        "review_settings" => {
            statement.check(Expr::col(review_settings::Column::Id).eq(1));
        }
        _ => {}
    }
    manager.create_table(statement).await
}

async fn create_indexes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let indexes = [
        Index::create()
            .name("idx_profiles_position")
            .table(profiles::Entity)
            .col(profiles::Column::Position)
            .unique()
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_images_position")
            .table(images::Entity)
            .col(images::Column::Position)
            .unique()
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_images_rating_position")
            .table(images::Entity)
            .col(images::Column::Rating)
            .col(images::Column::Position)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_images_updated_at")
            .table(images::Entity)
            .col(images::Column::UpdatedAt)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_image_exif_tags_tag")
            .table(image_exif_tags::Entity)
            .col(image_exif_tags::Column::Tag)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_image_labels_label")
            .table(image_labels::Entity)
            .col(image_labels::Column::Label)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_image_publish_profiles_profile")
            .table(image_publish_profiles::Entity)
            .col(image_publish_profiles::Column::ProfileIndex)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_image_profile_bw_filters_profile")
            .table(image_profile_bw_filters::Entity)
            .col(image_profile_bw_filters::Column::ProfileIndex)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_image_tags_tag_id")
            .table(image_tags::Entity)
            .col(image_tags::Column::TagId)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_image_profile_renders_profile")
            .table(image_profile_renders::Entity)
            .col(image_profile_renders::Column::ProfileIndex)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_image_profile_renders_image_profile")
            .table(image_profile_renders::Entity)
            .col(image_profile_renders::Column::ImageId)
            .col(image_profile_renders::Column::ProfileIndex)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_profile_pp3_sections_section")
            .table(profile_pp3_sections::Entity)
            .col(profile_pp3_sections::Column::Section)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_profile_pp3_entries_key")
            .table(profile_pp3_entries::Entity)
            .col(profile_pp3_entries::Column::Key)
            .if_not_exists()
            .to_owned(),
    ];
    for index in indexes {
        manager.create_index(index).await?;
    }
    Ok(())
}
