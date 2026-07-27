use super::{entities::*, paths, sqlite_compat};
use sea_orm::{
    ActiveModelTrait, DbBackend, EntityName, EntityTrait, IntoActiveModel, QuerySelect, Schema, Set,
};
use sea_orm_migration::{MigratorTrait, prelude::*};

pub(super) const LEGACY_SCHEMA_VERSION: i64 = 11;
pub(super) const FIRST_SEAORM_SCHEMA_VERSION: i64 = 12;
const PANORAMA_SCHEMA_VERSION: i64 = 13;
const REVIEW_SAMPLER_SCHEMA_VERSION: i64 = 14;
const FOCUS_REGIONS_SCHEMA_VERSION: i64 = 15;
const RELATIVE_PATHS_SCHEMA_VERSION: i64 = 16;
const CACHE_ROOT_SCHEMA_VERSION: i64 = 17;
pub(super) const LATEST_SCHEMA_VERSION: i64 = 18;
pub(super) const V18_BASELINE_MIGRATION: &str = "m20260718_000001_v18_baseline";
pub(super) const PANORAMA_PROJECTS_MIGRATION: &str = "m20260719_000002_panorama_projects";
pub(super) const REVIEW_SAMPLER_MIGRATION: &str = "m20260721_000003_review_sampler";
pub(super) const FOCUS_REGIONS_MIGRATION: &str = "m20260726_000004_focus_regions";
pub(super) const RELATIVE_PATHS_MIGRATION: &str = "m20260726_000005_relative_paths";
pub(super) const CACHE_ROOT_MIGRATION: &str = "m20260726_000006_cache_root";
pub(super) const AUTO_IMPORT_MIGRATION: &str = "m20260727_000007_auto_import";
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
        vec![
            Box::new(V18Baseline),
            Box::new(PanoramaProjects),
            Box::new(ReviewSampler),
            Box::new(FocusRegions),
            Box::new(RelativePaths),
            Box::new(CacheRoot),
            Box::new(AutoImport),
        ]
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
        sqlite_compat::set_user_version(manager.get_connection(), FIRST_SEAORM_SCHEMA_VERSION).await
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

struct PanoramaProjects;

impl MigrationName for PanoramaProjects {
    fn name(&self) -> &str {
        PANORAMA_PROJECTS_MIGRATION
    }
}

struct ReviewSampler;

impl MigrationName for ReviewSampler {
    fn name(&self) -> &str {
        REVIEW_SAMPLER_MIGRATION
    }
}

struct FocusRegions;

impl MigrationName for FocusRegions {
    fn name(&self) -> &str {
        FOCUS_REGIONS_MIGRATION
    }
}

struct RelativePaths;

impl MigrationName for RelativePaths {
    fn name(&self) -> &str {
        RELATIVE_PATHS_MIGRATION
    }
}

struct CacheRoot;

impl MigrationName for CacheRoot {
    fn name(&self) -> &str {
        CACHE_ROOT_MIGRATION
    }
}

struct AutoImport;

impl MigrationName for AutoImport {
    fn name(&self) -> &str {
        AUTO_IMPORT_MIGRATION
    }
}

#[async_trait::async_trait]
impl MigrationTrait for ReviewSampler {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let add_profile_columns = !manager
            .has_column(profiles::Entity.table_name(), "identity")
            .await?;
        if add_profile_columns {
            for mut column in [
                ColumnDef::new(profiles::Column::Identity)
                    .text()
                    .not_null()
                    .default("")
                    .to_owned(),
                ColumnDef::new(profiles::Column::SamplerAdded)
                    .big_integer()
                    .not_null()
                    .default(0)
                    .to_owned(),
                ColumnDef::new(profiles::Column::EnabledByDefault)
                    .big_integer()
                    .not_null()
                    .default(1)
                    .to_owned(),
            ] {
                manager
                    .alter_table(
                        Table::alter()
                            .table(profiles::Entity)
                            .add_column(&mut column)
                            .to_owned(),
                    )
                    .await?;
            }
        }
        let add_render_enabled = !manager
            .has_column(image_profile_renders::Entity.table_name(), "enabled")
            .await?;
        if add_render_enabled {
            manager
                .alter_table(
                    Table::alter()
                        .table(image_profile_renders::Entity)
                        .add_column(
                            ColumnDef::new(image_profile_renders::Column::Enabled)
                                .big_integer()
                                .not_null()
                                .default(1),
                        )
                        .to_owned(),
                )
                .await?;
        }

        backfill_sampler_profile_state(manager, add_render_enabled).await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_profiles_identity")
                    .table(profiles::Entity)
                    .col(profiles::Column::Identity)
                    .unique()
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_image_profile_renders_enabled")
                    .table(image_profile_renders::Entity)
                    .col(image_profile_renders::Column::ImageId)
                    .col(image_profile_renders::Column::Enabled)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        sqlite_compat::set_user_version(manager.get_connection(), REVIEW_SAMPLER_SCHEMA_VERSION)
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_image_profile_renders_enabled")
                    .table(image_profile_renders::Entity)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_index(
                Index::drop()
                    .name("idx_profiles_identity")
                    .table(profiles::Entity)
                    .to_owned(),
            )
            .await?;
        for column in [
            profiles::Column::EnabledByDefault,
            profiles::Column::SamplerAdded,
            profiles::Column::Identity,
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(profiles::Entity)
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        manager
            .alter_table(
                Table::alter()
                    .table(image_profile_renders::Entity)
                    .drop_column(image_profile_renders::Column::Enabled)
                    .to_owned(),
            )
            .await?;
        sqlite_compat::set_user_version(manager.get_connection(), PANORAMA_SCHEMA_VERSION).await
    }
}

#[async_trait::async_trait]
impl MigrationTrait for FocusRegions {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for column in [
            images::Column::ExifFocusFrameWidth,
            images::Column::ExifFocusFrameHeight,
        ] {
            let column_name = column.to_string();
            if !manager
                .has_column(images::Entity.table_name(), &column_name)
                .await?
            {
                let mut definition = ColumnDef::new(column).big_integer().to_owned();
                manager
                    .alter_table(
                        Table::alter()
                            .table(images::Entity)
                            .add_column(&mut definition)
                            .to_owned(),
                    )
                    .await?;
            }
        }

        let schema = Schema::new(DbBackend::Sqlite);
        create_entity_table(manager, &schema, image_focus_regions::Entity).await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_image_focus_regions_primary")
                    .table(image_focus_regions::Entity)
                    .col(image_focus_regions::Column::ImageId)
                    .col(image_focus_regions::Column::Primary)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;
        sqlite_compat::set_user_version(manager.get_connection(), FOCUS_REGIONS_SCHEMA_VERSION)
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_image_focus_regions_primary")
                    .table(image_focus_regions::Entity)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(image_focus_regions::Entity)
                    .if_exists()
                    .to_owned(),
            )
            .await?;
        for column in [
            images::Column::ExifFocusFrameHeight,
            images::Column::ExifFocusFrameWidth,
        ] {
            manager
                .alter_table(
                    Table::alter()
                        .table(images::Entity)
                        .drop_column(column)
                        .to_owned(),
                )
                .await?;
        }
        sqlite_compat::set_user_version(manager.get_connection(), REVIEW_SAMPLER_SCHEMA_VERSION)
            .await
    }
}

#[async_trait::async_trait]
impl MigrationTrait for RelativePaths {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for column in [
            review_settings::Column::InputRoot,
            review_settings::Column::OutputRoot,
        ] {
            let column_name = column.to_string();
            if !manager
                .has_column(review_settings::Entity.table_name(), &column_name)
                .await?
            {
                manager
                    .alter_table(
                        Table::alter()
                            .table(review_settings::Entity)
                            .add_column(ColumnDef::new(column).text().not_null().default(""))
                            .to_owned(),
                    )
                    .await?;
            }
        }
        sqlite_compat::set_user_version(manager.get_connection(), RELATIVE_PATHS_SCHEMA_VERSION)
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        paths::restore_absolute_path_storage(manager.get_connection())
            .await
            .map_err(|error| DbErr::Custom(format!("{error:#}")))?;
        for column in [
            review_settings::Column::OutputRoot,
            review_settings::Column::InputRoot,
        ] {
            if manager
                .has_column(review_settings::Entity.table_name(), &column.to_string())
                .await?
            {
                manager
                    .alter_table(
                        Table::alter()
                            .table(review_settings::Entity)
                            .drop_column(column)
                            .to_owned(),
                    )
                    .await?;
            }
        }
        sqlite_compat::set_user_version(manager.get_connection(), FOCUS_REGIONS_SCHEMA_VERSION)
            .await
    }
}

#[async_trait::async_trait]
impl MigrationTrait for CacheRoot {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let column = review_settings::Column::CacheRoot;
        if !manager
            .has_column(review_settings::Entity.table_name(), &column.to_string())
            .await?
        {
            manager
                .alter_table(
                    Table::alter()
                        .table(review_settings::Entity)
                        .add_column(ColumnDef::new(column).text().not_null().default(""))
                        .to_owned(),
                )
                .await?;
        }
        sqlite_compat::set_user_version(manager.get_connection(), CACHE_ROOT_SCHEMA_VERSION).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // RelativePaths::down still reads settings through the current entity,
        // so retain this optional forward-compatible column.
        sqlite_compat::set_user_version(manager.get_connection(), RELATIVE_PATHS_SCHEMA_VERSION)
            .await
    }
}

#[async_trait::async_trait]
impl MigrationTrait for AutoImport {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let schema = Schema::new(DbBackend::Sqlite);
        create_entity_table(manager, &schema, auto_import_devices::Entity).await?;
        create_entity_table(manager, &schema, auto_import_storages::Entity).await?;
        create_entity_table(manager, &schema, auto_import_groups::Entity).await?;
        create_entity_table(manager, &schema, auto_import_assets::Entity).await?;
        create_entity_table(manager, &schema, auto_import_sources::Entity).await?;
        create_auto_import_indexes(manager).await?;
        sqlite_compat::set_user_version(manager.get_connection(), LATEST_SCHEMA_VERSION).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            auto_import_sources::Entity.table_name(),
            auto_import_assets::Entity.table_name(),
            auto_import_groups::Entity.table_name(),
            auto_import_storages::Entity.table_name(),
            auto_import_devices::Entity.table_name(),
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
        sqlite_compat::set_user_version(manager.get_connection(), CACHE_ROOT_SCHEMA_VERSION).await
    }
}

async fn backfill_sampler_profile_state(
    manager: &SchemaManager<'_>,
    backfill_render_enabled: bool,
) -> Result<(), DbErr> {
    let connection = manager.get_connection();
    for row in profiles::Entity::find().all(connection).await? {
        if !row.identity.trim().is_empty() {
            continue;
        }
        let mut active = row.clone().into_active_model();
        active.identity = Set(format!(
            "legacy:{}:{}",
            row.profile_index,
            row.selector.trim()
        ));
        active.update(connection).await?;
    }

    if !backfill_render_enabled {
        return Ok(());
    }

    let images = images::Entity::find()
        .select_only()
        .column(images::Column::ImageId)
        .column(images::Column::PublishProfilesDefault)
        .into_tuple::<(i64, i64)>()
        .all(connection)
        .await?;
    let publish_rows = image_publish_profiles::Entity::find()
        .all(connection)
        .await?;
    let selected = publish_rows
        .into_iter()
        .map(|row| (row.image_id, row.profile_index))
        .collect::<std::collections::HashSet<_>>();
    let explicit_images = images
        .into_iter()
        .filter_map(|(image_id, publish_profiles_default)| {
            (publish_profiles_default == 0).then_some(image_id)
        })
        .collect::<std::collections::HashSet<_>>();
    for row in image_profile_renders::Entity::find()
        .all(connection)
        .await?
    {
        if !explicit_images.contains(&row.image_id) {
            continue;
        }
        let mut active = row.clone().into_active_model();
        active.enabled = Set(i64::from(
            selected.contains(&(row.image_id, row.profile_index)),
        ));
        active.update(connection).await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for PanoramaProjects {
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }

    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let schema = Schema::new(DbBackend::Sqlite);
        create_entity_table(manager, &schema, panorama_projects::Entity).await?;
        create_entity_table(manager, &schema, panorama_project_images::Entity).await?;
        create_entity_table(manager, &schema, panorama_previews::Entity).await?;
        create_panorama_indexes(manager).await?;
        sqlite_compat::set_user_version(manager.get_connection(), PANORAMA_SCHEMA_VERSION).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        for table in [
            panorama_previews::Entity.table_name(),
            panorama_project_images::Entity.table_name(),
            panorama_projects::Entity.table_name(),
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
        sqlite_compat::set_user_version(manager.get_connection(), FIRST_SEAORM_SCHEMA_VERSION).await
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
        "image_focus_regions" => {
            statement
                .check(Expr::col(image_focus_regions::Column::X).between(0.0, 1.0))
                .check(Expr::col(image_focus_regions::Column::Y).between(0.0, 1.0))
                .check(Expr::col(image_focus_regions::Column::Width).between(0.0, 1.0))
                .check(Expr::col(image_focus_regions::Column::Height).between(0.0, 1.0))
                .check(Expr::col(image_focus_regions::Column::Primary).is_in([0, 1]));
        }
        "review_settings" => {
            statement.check(Expr::col(review_settings::Column::Id).eq(1));
        }
        "panorama_projects" => {
            statement
                .check(Expr::col(panorama_projects::Column::Status).is_in([
                    "draft",
                    "previewing",
                    "ready",
                    "rendering",
                    "complete",
                    "failed",
                    "interrupted",
                    "cancelled",
                ]))
                .check(Expr::col(panorama_projects::Column::MatchingMode).is_in([
                    "automatic",
                    "sequential",
                    "multi-row",
                    "flat-mosaic",
                ]))
                .check(
                    Expr::col(panorama_projects::Column::SelectedProjection)
                        .is_null()
                        .or(
                            Expr::col(panorama_projects::Column::SelectedProjection).is_in([
                                "rectilinear",
                                "cylindrical",
                                "equirectangular",
                                "panini",
                            ]),
                        ),
                );
        }
        "panorama_previews" => {
            statement
                .check(Expr::col(panorama_previews::Column::MatchingMode).is_in([
                    "automatic",
                    "sequential",
                    "multi-row",
                    "flat-mosaic",
                ]))
                .check(Expr::col(panorama_previews::Column::Projection).is_in([
                    "rectilinear",
                    "cylindrical",
                    "equirectangular",
                    "panini",
                ]))
                .check(Expr::col(panorama_previews::Column::Status).is_in([
                    "queued",
                    "processing",
                    "done",
                    "failed",
                    "cancelled",
                ]));
        }
        "auto_import_assets" => {
            statement
                .check(Expr::col(auto_import_assets::Column::MediaKind).is_in(["raw", "jpeg"]));
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

async fn create_panorama_indexes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let indexes = [
        Index::create()
            .name("idx_panorama_projects_updated_at")
            .table(panorama_projects::Entity)
            .col(panorama_projects::Column::UpdatedAt)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_panorama_projects_result_image")
            .table(panorama_projects::Entity)
            .col(panorama_projects::Column::ResultImageId)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_panorama_project_images_image")
            .table(panorama_project_images::Entity)
            .col(panorama_project_images::Column::ImageId)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_panorama_previews_status")
            .table(panorama_previews::Entity)
            .col(panorama_previews::Column::Status)
            .if_not_exists()
            .to_owned(),
    ];
    for index in indexes {
        manager.create_index(index).await?;
    }
    Ok(())
}

async fn create_auto_import_indexes(manager: &SchemaManager<'_>) -> Result<(), DbErr> {
    let indexes = [
        Index::create()
            .name("idx_auto_import_storages_device_key")
            .table(auto_import_storages::Entity)
            .col(auto_import_storages::Column::DeviceId)
            .col(auto_import_storages::Column::StorageKey)
            .unique()
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_auto_import_groups_source")
            .table(auto_import_groups::Entity)
            .col(auto_import_groups::Column::DeviceId)
            .col(auto_import_groups::Column::SourceStemKey)
            .col(auto_import_groups::Column::SourceModifiedNs)
            .unique()
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_auto_import_groups_destination")
            .table(auto_import_groups::Entity)
            .col(auto_import_groups::Column::DestinationStemKey)
            .unique()
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_auto_import_assets_group_kind")
            .table(auto_import_assets::Entity)
            .col(auto_import_assets::Column::GroupId)
            .col(auto_import_assets::Column::MediaKind)
            .unique()
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_auto_import_assets_source")
            .table(auto_import_assets::Entity)
            .col(auto_import_assets::Column::SourceFilenameKey)
            .col(auto_import_assets::Column::SourceModifiedNs)
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_auto_import_assets_destination")
            .table(auto_import_assets::Entity)
            .col(auto_import_assets::Column::DestinationFilenameKey)
            .unique()
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_auto_import_sources_location")
            .table(auto_import_sources::Entity)
            .col(auto_import_sources::Column::StorageId)
            .col(auto_import_sources::Column::RelativePathKey)
            .col(auto_import_sources::Column::SourceModifiedNs)
            .unique()
            .if_not_exists()
            .to_owned(),
        Index::create()
            .name("idx_auto_import_sources_asset")
            .table(auto_import_sources::Entity)
            .col(auto_import_sources::Column::AssetId)
            .if_not_exists()
            .to_owned(),
    ];
    for index in indexes {
        manager.create_index(index).await?;
    }
    Ok(())
}
