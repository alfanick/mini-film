use super::*;
use rusqlite::{OptionalExtension, Transaction, params};

pub(super) const LATEST_SCHEMA_VERSION: i64 = 6;

enum MigrationAction {
    Sql(&'static str),
    Rust(fn(&Transaction<'_>) -> Result<()>),
}

struct ReviewMigration {
    version: i64,
    name: &'static str,
    action: MigrationAction,
}

const MIGRATIONS: &[ReviewMigration] = &[
    ReviewMigration {
        version: 1,
        name: "initial_review_state",
        action: MigrationAction::Sql(INITIAL_SCHEMA),
    },
    ReviewMigration {
        version: 2,
        name: "profile_bw_filters",
        action: MigrationAction::Sql(PROFILE_BW_FILTERS_SCHEMA),
    },
    ReviewMigration {
        version: 3,
        name: "profile_render_processing_key",
        action: MigrationAction::Sql(PROFILE_RENDER_PROCESSING_KEY_SCHEMA),
    },
    ReviewMigration {
        version: 4,
        name: "active_d_lighting_and_pp3_adjustments",
        action: MigrationAction::Sql(ACTIVE_D_LIGHTING_AND_PP3_ADJUSTMENTS_SCHEMA),
    },
    ReviewMigration {
        version: 5,
        name: "source_file_info",
        action: MigrationAction::Sql(SOURCE_FILE_INFO_SCHEMA),
    },
    ReviewMigration {
        version: 6,
        name: "normalized_relational_review_store",
        action: MigrationAction::Rust(migrate_normalized_relational_review_store),
    },
];

const INITIAL_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS review_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    next_id INTEGER NOT NULL,
    current_image_id INTEGER,
    min_rating INTEGER NOT NULL,
    exif_schema_version INTEGER NOT NULL,
    store_json TEXT NOT NULL,
    store_sha1 TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS profiles (
    profile_index INTEGER PRIMARY KEY,
    position INTEGER NOT NULL,
    selector TEXT NOT NULL,
    stem TEXT NOT NULL,
    retouch_exposure REAL NOT NULL,
    retouch_highlights REAL NOT NULL,
    retouch_shadows REAL NOT NULL,
    retouch_whites REAL NOT NULL,
    retouch_blacks REAL NOT NULL,
    retouch_temperature REAL NOT NULL,
    retouch_offset REAL NOT NULL,
    retouch_clarity REAL NOT NULL,
    metadata_present INTEGER NOT NULL,
    profile_name TEXT,
    profile_uuid TEXT,
    look_name TEXT,
    look_uuid TEXT,
    source_profile_name TEXT,
    source_profile_uuid TEXT,
    has_camera_raw_settings INTEGER NOT NULL,
    grain_amount INTEGER,
    grain_size INTEGER,
    grain_frequency INTEGER,
    has_hald INTEGER NOT NULL,
    has_pp3 INTEGER NOT NULL,
    pp3_name TEXT,
    metadata_json TEXT
);

CREATE TABLE IF NOT EXISTS profile_adjustments (
    profile_index INTEGER NOT NULL REFERENCES profiles(profile_index) ON DELETE CASCADE,
    scope TEXT NOT NULL CHECK (scope IN ('source', 'emulation')),
    exposure REAL NOT NULL,
    contrast REAL NOT NULL,
    highlights REAL NOT NULL,
    shadows REAL NOT NULL,
    whites REAL NOT NULL,
    blacks REAL NOT NULL,
    saturation REAL NOT NULL,
    vibrance REAL NOT NULL,
    clarity REAL NOT NULL,
    parametric_shadows REAL NOT NULL,
    parametric_darks REAL NOT NULL,
    parametric_lights REAL NOT NULL,
    parametric_highlights REAL NOT NULL,
    parametric_shadow_split REAL NOT NULL,
    parametric_midtone_split REAL NOT NULL,
    parametric_highlight_split REAL NOT NULL,
    calibration_red_hue REAL NOT NULL,
    calibration_red_saturation REAL NOT NULL,
    calibration_green_hue REAL NOT NULL,
    calibration_green_saturation REAL NOT NULL,
    calibration_blue_hue REAL NOT NULL,
    calibration_blue_saturation REAL NOT NULL,
    PRIMARY KEY (profile_index, scope)
);

CREATE TABLE IF NOT EXISTS profile_sharpening (
    profile_index INTEGER NOT NULL REFERENCES profiles(profile_index) ON DELETE CASCADE,
    scope TEXT NOT NULL CHECK (scope IN ('source', 'emulation')),
    present INTEGER NOT NULL,
    amount REAL NOT NULL,
    radius REAL NOT NULL,
    detail REAL NOT NULL,
    masking REAL NOT NULL,
    PRIMARY KEY (profile_index, scope)
);

CREATE TABLE IF NOT EXISTS profile_hsl_values (
    profile_index INTEGER NOT NULL REFERENCES profiles(profile_index) ON DELETE CASCADE,
    scope TEXT NOT NULL CHECK (scope IN ('source', 'emulation')),
    channel TEXT NOT NULL CHECK (channel IN ('hue', 'saturation', 'luminance')),
    value_index INTEGER NOT NULL,
    value REAL NOT NULL,
    PRIMARY KEY (profile_index, scope, channel, value_index)
);

CREATE TABLE IF NOT EXISTS profile_tone_curve_points (
    profile_index INTEGER NOT NULL REFERENCES profiles(profile_index) ON DELETE CASCADE,
    scope TEXT NOT NULL CHECK (scope IN ('source', 'emulation')),
    channel TEXT NOT NULL CHECK (channel IN ('composite', 'red', 'green', 'blue')),
    point_index INTEGER NOT NULL,
    x REAL NOT NULL,
    y REAL NOT NULL,
    PRIMARY KEY (profile_index, scope, channel, point_index)
);

CREATE TABLE IF NOT EXISTS images (
    image_id INTEGER PRIMARY KEY,
    position INTEGER NOT NULL,
    raw_path TEXT NOT NULL UNIQUE,
    sooc_sidecar_path TEXT,
    relative_path TEXT NOT NULL,
    file_name TEXT NOT NULL,
    exif_capture_timestamp INTEGER,
    exif_rating INTEGER,
    exif_focal_length TEXT,
    exif_aperture TEXT,
    exif_shutter_speed TEXT,
    exif_iso TEXT,
    exif_camera_model TEXT,
    exif_lens_model TEXT,
    exif_shooting_mode TEXT,
    exif_exposure_compensation TEXT,
    exif_flash TEXT,
    exif_note TEXT,
    preview_status TEXT NOT NULL,
    preview_path TEXT,
    preview_error TEXT,
    preview_duration_ms INTEGER,
    preview_render_key TEXT,
    preview_updated_at TEXT NOT NULL,
    selected_profile_index INTEGER NOT NULL,
    rating INTEGER NOT NULL,
    label TEXT NOT NULL,
    notes TEXT NOT NULL,
    rating_source TEXT NOT NULL,
    tags_source TEXT NOT NULL,
    notes_source TEXT NOT NULL,
    codex_status TEXT NOT NULL,
    codex_flags_tags INTEGER NOT NULL,
    codex_flags_note INTEGER NOT NULL,
    codex_flags_rating INTEGER NOT NULL,
    codex_model TEXT NOT NULL,
    codex_analysis_key TEXT,
    codex_error TEXT,
    codex_updated_at TEXT NOT NULL,
    retouch_exposure REAL NOT NULL,
    retouch_highlights REAL NOT NULL,
    retouch_shadows REAL NOT NULL,
    retouch_whites REAL NOT NULL,
    retouch_blacks REAL NOT NULL,
    retouch_temperature REAL NOT NULL,
    retouch_offset REAL NOT NULL,
    retouch_clarity REAL NOT NULL,
    retouch_crop_x REAL,
    retouch_crop_y REAL,
    retouch_crop_width REAL,
    retouch_crop_height REAL,
    retouch_rotation_degrees REAL NOT NULL,
    publish_profiles_default INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    image_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS image_exif_tags (
    image_id INTEGER NOT NULL REFERENCES images(image_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY (image_id, position)
);

CREATE TABLE IF NOT EXISTS image_tags (
    image_id INTEGER NOT NULL REFERENCES images(image_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY (image_id, position)
);

CREATE TABLE IF NOT EXISTS image_labels (
    image_id INTEGER NOT NULL REFERENCES images(image_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    label TEXT NOT NULL,
    PRIMARY KEY (image_id, position)
);

CREATE TABLE IF NOT EXISTS image_publish_profiles (
    image_id INTEGER NOT NULL REFERENCES images(image_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    profile_index INTEGER NOT NULL,
    PRIMARY KEY (image_id, position)
);

CREATE TABLE IF NOT EXISTS image_profile_renders (
    image_id INTEGER NOT NULL REFERENCES images(image_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    profile_index INTEGER NOT NULL,
    profile_stem TEXT NOT NULL,
    display_name TEXT,
    status TEXT NOT NULL,
    output_path TEXT,
    error TEXT,
    duration_ms INTEGER,
    render_key TEXT,
    width INTEGER,
    height INTEGER,
    updated_at TEXT NOT NULL,
    render_json TEXT NOT NULL,
    PRIMARY KEY (image_id, position)
);

CREATE INDEX IF NOT EXISTS idx_profiles_position ON profiles(position);
CREATE INDEX IF NOT EXISTS idx_images_position ON images(position);
CREATE INDEX IF NOT EXISTS idx_images_rating ON images(rating);
CREATE INDEX IF NOT EXISTS idx_images_updated_at ON images(updated_at);
CREATE INDEX IF NOT EXISTS idx_image_tags_tag ON image_tags(tag);
CREATE INDEX IF NOT EXISTS idx_image_exif_tags_tag ON image_exif_tags(tag);
CREATE INDEX IF NOT EXISTS idx_image_labels_label ON image_labels(label);
CREATE INDEX IF NOT EXISTS idx_image_publish_profiles_profile ON image_publish_profiles(profile_index);
CREATE INDEX IF NOT EXISTS idx_image_profile_renders_profile ON image_profile_renders(profile_index);
"#;

const PROFILE_BW_FILTERS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS image_profile_bw_filters (
    image_id INTEGER NOT NULL REFERENCES images(image_id) ON DELETE CASCADE,
    profile_index INTEGER NOT NULL,
    bw_filter TEXT NOT NULL CHECK (bw_filter IN ('yellow', 'orange', 'red', 'green')),
    PRIMARY KEY (image_id, profile_index)
);

CREATE INDEX IF NOT EXISTS idx_image_profile_bw_filters_profile
    ON image_profile_bw_filters(profile_index);
"#;

const PROFILE_RENDER_PROCESSING_KEY_SCHEMA: &str = r#"
ALTER TABLE image_profile_renders ADD COLUMN processing_key TEXT;
"#;

const ACTIVE_D_LIGHTING_AND_PP3_ADJUSTMENTS_SCHEMA: &str = r#"
ALTER TABLE images ADD COLUMN exif_active_d_lighting TEXT;

CREATE TABLE IF NOT EXISTS profile_pp3_adjustments (
    profile_index INTEGER NOT NULL REFERENCES profiles(profile_index) ON DELETE CASCADE,
    section_position INTEGER NOT NULL,
    entry_position INTEGER NOT NULL,
    source TEXT NOT NULL,
    section TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (profile_index, section_position, entry_position)
);

CREATE INDEX IF NOT EXISTS idx_profile_pp3_adjustments_section
    ON profile_pp3_adjustments(section);
"#;

const SOURCE_FILE_INFO_SCHEMA: &str = r#"
ALTER TABLE images ADD COLUMN source_file_size_bytes INTEGER;
ALTER TABLE images ADD COLUMN source_width INTEGER;
ALTER TABLE images ADD COLUMN source_height INTEGER;
"#;

const NORMALIZED_RELATIONAL_SCHEMA: &str = r#"
CREATE TABLE review_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    next_id INTEGER NOT NULL,
    current_image_id INTEGER REFERENCES images(image_id) ON DELETE SET NULL,
    min_rating INTEGER NOT NULL,
    exif_schema_version INTEGER NOT NULL
);

DROP TABLE profile_pp3_adjustments;
CREATE TABLE profile_pp3_sections (
    profile_index INTEGER NOT NULL REFERENCES profiles(profile_index) ON DELETE CASCADE,
    section_position INTEGER NOT NULL,
    source TEXT NOT NULL,
    section TEXT NOT NULL,
    PRIMARY KEY (profile_index, section_position)
);
CREATE TABLE profile_pp3_entries (
    profile_index INTEGER NOT NULL,
    section_position INTEGER NOT NULL,
    entry_position INTEGER NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (profile_index, section_position, entry_position),
    FOREIGN KEY (profile_index, section_position)
        REFERENCES profile_pp3_sections(profile_index, section_position)
        ON DELETE CASCADE
);

DROP TABLE image_profile_bw_filters;
CREATE TABLE image_profile_bw_filters (
    image_id INTEGER NOT NULL REFERENCES images(image_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    profile_index INTEGER NOT NULL,
    bw_filter TEXT NOT NULL CHECK (bw_filter IN ('none', 'yellow', 'orange', 'red', 'green')),
    PRIMARY KEY (image_id, position)
);

DROP TABLE image_tags;
CREATE TABLE tags (
    tag_id INTEGER PRIMARY KEY,
    tag TEXT NOT NULL UNIQUE
);
CREATE TABLE image_tags (
    image_id INTEGER NOT NULL REFERENCES images(image_id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tags(tag_id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    PRIMARY KEY (image_id, position)
);

ALTER TABLE profiles DROP COLUMN metadata_json;
ALTER TABLE images DROP COLUMN image_json;
ALTER TABLE image_profile_renders DROP COLUMN render_json;

DROP INDEX idx_profiles_position;
CREATE UNIQUE INDEX idx_profiles_position ON profiles(position);
DROP INDEX idx_images_position;
CREATE UNIQUE INDEX idx_images_position ON images(position);
DROP INDEX idx_images_rating;
CREATE INDEX idx_images_rating_position ON images(rating, position);
CREATE INDEX idx_profile_pp3_sections_section ON profile_pp3_sections(section);
CREATE INDEX idx_profile_pp3_entries_key ON profile_pp3_entries(key);
CREATE INDEX idx_image_profile_bw_filters_profile
    ON image_profile_bw_filters(profile_index);
CREATE INDEX idx_image_tags_tag_id ON image_tags(tag_id);
CREATE INDEX idx_image_profile_renders_image_profile
    ON image_profile_renders(image_id, profile_index);
"#;

pub(super) fn run_migrations(connection: &mut rusqlite::Connection) -> Result<()> {
    run_migrations_through(connection, LATEST_SCHEMA_VERSION)
}

#[cfg(test)]
pub(super) fn run_migrations_through_for_test(
    connection: &mut rusqlite::Connection,
    target_version: i64,
) -> Result<()> {
    run_migrations_through(connection, target_version)
}

fn run_migrations_through(
    connection: &mut rusqlite::Connection,
    target_version: i64,
) -> Result<()> {
    if !(0..=LATEST_SCHEMA_VERSION).contains(&target_version) {
        bail!("unsupported review database target schema version {target_version}");
    }
    let user_version = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .context("reading sqlite user_version")?;
    if user_version > LATEST_SCHEMA_VERSION {
        bail!(
            "review database schema version {user_version} is newer than supported version {LATEST_SCHEMA_VERSION}"
        );
    }

    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );",
        )
        .context("creating sqlite migration ledger")?;
    validate_migration_ledger(connection, user_version)?;

    for migration in MIGRATIONS
        .iter()
        .filter(|migration| migration.version <= target_version)
    {
        let applied = connection
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE version = ?1",
                params![migration.version],
                |_| Ok(()),
            )
            .optional()
            .context("checking sqlite migration ledger")?
            .is_some();
        if applied {
            continue;
        }
        let tx = connection
            .transaction()
            .context("starting sqlite migration")?;
        match migration.action {
            MigrationAction::Sql(sql) => tx
                .execute_batch(sql)
                .with_context(|| format!("applying sqlite migration {}", migration.name))?,
            MigrationAction::Rust(apply) => apply(&tx)
                .with_context(|| format!("applying sqlite migration {}", migration.name))?,
        }
        tx.execute(
            "INSERT INTO schema_migrations(version, name) VALUES (?1, ?2)",
            params![migration.version, migration.name],
        )
        .context("recording sqlite migration")?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", migration.version))
            .context("setting sqlite user_version")?;
        tx.commit().context("committing sqlite migration")?;
    }
    Ok(())
}

fn validate_migration_ledger(connection: &rusqlite::Connection, user_version: i64) -> Result<()> {
    let mut statement = connection
        .prepare("SELECT version, name FROM schema_migrations ORDER BY version")
        .context("reading sqlite migration ledger")?;
    let mut rows = statement.query([]).context("querying migration ledger")?;
    let mut applied_count = 0_i64;
    while let Some(row) = rows.next().context("reading migration ledger row")? {
        let version = row.get::<_, i64>(0)?;
        let name = row.get::<_, String>(1)?;
        let expected_version = applied_count + 1;
        if version != expected_version {
            bail!(
                "review database migration ledger is not contiguous: expected version {expected_version}, found {version}"
            );
        }
        let migration = MIGRATIONS
            .iter()
            .find(|migration| migration.version == version)
            .ok_or_else(|| {
                anyhow!("review database migration {version} is newer than this mini-film binary")
            })?;
        if name != migration.name {
            bail!(
                "review database migration {version} is named {name:?}, expected {:?}",
                migration.name
            );
        }
        applied_count += 1;
    }
    if user_version != applied_count {
        bail!(
            "review database user_version {user_version} does not match migration ledger version {applied_count}"
        );
    }
    Ok(())
}

fn migrate_normalized_relational_review_store(tx: &Transaction<'_>) -> Result<()> {
    let store = tx
        .query_row(
            "SELECT store_json FROM review_state WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .context("reading legacy sqlite review snapshot")?
        .map(|store_json| {
            serde_json::from_str::<ReviewStore>(&store_json)
                .context("parsing legacy sqlite review snapshot")
        })
        .transpose()?;

    tx.execute_batch(NORMALIZED_RELATIONAL_SCHEMA)
        .context("creating normalized review database schema")?;
    match &store {
        Some(store) => write::replace_store_in_transaction(tx, store)?,
        None => write::clear_store_in_transaction(tx)?,
    }

    verify_relational_store(tx, store.as_ref())?;
    verify_database_integrity(tx)?;
    tx.execute_batch("DROP TABLE review_state;")
        .context("removing legacy sqlite review snapshot")?;
    verify_relational_store(tx, store.as_ref())?;
    verify_database_integrity(tx)
}

fn verify_relational_store(
    connection: &rusqlite::Connection,
    expected: Option<&ReviewStore>,
) -> Result<()> {
    let restored = read::load_store_from_connection(connection)?;
    match (expected, restored.as_ref()) {
        (Some(expected), Some(restored)) => ensure_same_store(expected, restored),
        (None, None) => Ok(()),
        (Some(_), None) => bail!("normalized review database contains no review settings"),
        (None, Some(_)) => bail!("normalized review database unexpectedly contains review state"),
    }
}

fn verify_database_integrity(connection: &rusqlite::Connection) -> Result<()> {
    let mut foreign_keys = connection
        .prepare("PRAGMA foreign_key_check")
        .context("preparing sqlite foreign key check")?;
    if foreign_keys
        .query([])
        .context("running sqlite foreign key check")?
        .next()
        .context("reading sqlite foreign key check")?
        .is_some()
    {
        bail!("normalized review database failed foreign key validation");
    }

    let mut quick_check = connection
        .prepare("PRAGMA quick_check")
        .context("preparing sqlite quick check")?;
    let messages = quick_check
        .query_map([], |row| row.get::<_, String>(0))
        .context("running sqlite quick check")?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if messages.as_slice() != ["ok"] {
        bail!(
            "normalized review database failed quick_check: {}",
            messages.join("; ")
        );
    }
    Ok(())
}
