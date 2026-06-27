use super::{model::*, prelude::*, store::*};
use rusqlite::{Connection, OptionalExtension, Transaction, named_params, params};

pub(super) const SQLITE_STATE_FILE: &str = "mini-film-review.sqlite";
pub(super) const LEGACY_JSON_STATE_FILE: &str = "mini-film-review.json";

struct ReviewMigration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[ReviewMigration] = &[ReviewMigration {
    version: 1,
    name: "initial_review_state",
    sql: INITIAL_SCHEMA,
}];

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

pub(super) fn review_state_path(output_root: &Path) -> PathBuf {
    output_root.join(SQLITE_STATE_FILE)
}

pub(super) fn legacy_json_state_path(output_root: &Path) -> PathBuf {
    output_root.join(LEGACY_JSON_STATE_FILE)
}

pub(super) fn load_or_migrate_store(output_root: &Path) -> Result<(ReviewStore, PathBuf)> {
    let sqlite_path = review_state_path(output_root);
    let legacy_path = legacy_json_state_path(output_root);
    if sqlite_path.exists() {
        if let Some(store) = load_store(&sqlite_path)? {
            return Ok((store, sqlite_path));
        }
        if legacy_path.exists() {
            let store = migrate_legacy_json(&legacy_path, &sqlite_path)?;
            return Ok((store, sqlite_path));
        }
        return Ok((ReviewStore::new(Vec::new()), sqlite_path));
    }

    if legacy_path.exists() {
        let store = migrate_legacy_json(&legacy_path, &sqlite_path)?;
        return Ok((store, sqlite_path));
    }

    Ok((ReviewStore::new(Vec::new()), sqlite_path))
}

pub(super) fn resolve_review_state_for_publish(
    state: &Path,
    output_root: &Path,
) -> Result<PathBuf> {
    if state.exists() {
        let state = fs::canonicalize(state)
            .with_context(|| format!("canonicalizing review state {}", state.display()))?;
        ensure_path_within(&state, output_root)?;
        return Ok(state);
    }

    if state.file_name().and_then(|name| name.to_str()) == Some(LEGACY_JSON_STATE_FILE) {
        let sqlite_path = state.with_file_name(SQLITE_STATE_FILE);
        if sqlite_path.exists() {
            let sqlite_path = fs::canonicalize(&sqlite_path).with_context(|| {
                format!("canonicalizing review state {}", sqlite_path.display())
            })?;
            ensure_path_within(&sqlite_path, output_root)?;
            return Ok(sqlite_path);
        }
    }

    let state = fs::canonicalize(state)
        .with_context(|| format!("canonicalizing review state {}", state.display()))?;
    ensure_path_within(&state, output_root)?;
    Ok(state)
}

pub(super) fn load_store_for_publish(state: &Path) -> Result<Option<ReviewStore>> {
    if state.file_name().and_then(|name| name.to_str()) == Some(LEGACY_JSON_STATE_FILE) {
        let sqlite_path = state.with_file_name(SQLITE_STATE_FILE);
        if sqlite_path.exists() {
            return load_store(&sqlite_path);
        }
        if state.exists() {
            return migrate_legacy_json(state, &sqlite_path).map(Some);
        }
    }
    load_store(state)
}

pub(super) fn load_store(path: &Path) -> Result<Option<ReviewStore>> {
    if !path.exists() {
        return Ok(None);
    }
    if path.file_name().and_then(|name| name.to_str()) == Some(LEGACY_JSON_STATE_FILE) {
        return load_legacy_json(path).map(Some);
    }

    let connection = open_database(path)?;
    let store_json = connection
        .query_row(
            "SELECT store_json FROM review_state WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .with_context(|| format!("reading review state from {}", path.display()))?;
    store_json
        .map(|text| {
            serde_json::from_str(&text)
                .with_context(|| format!("parsing review state in {}", path.display()))
        })
        .transpose()
}

pub(super) fn save_store(path: &Path, store: &ReviewStore) -> Result<()> {
    let mut connection = open_database(path)?;
    replace_store(&mut connection, store)
        .with_context(|| format!("saving review state to {}", path.display()))
}

fn open_database(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut connection =
        Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
    connection
        .busy_timeout(Duration::from_secs(30))
        .context("setting sqlite busy timeout")?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .context("enabling sqlite foreign keys")?;
    run_migrations(&mut connection)?;
    Ok(connection)
}

fn run_migrations(connection: &mut Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );",
        )
        .context("creating sqlite migration ledger")?;
    for migration in MIGRATIONS {
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
        tx.execute_batch(migration.sql)
            .with_context(|| format!("applying sqlite migration {}", migration.name))?;
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

fn migrate_legacy_json(legacy_path: &Path, sqlite_path: &Path) -> Result<ReviewStore> {
    let store = load_legacy_json(legacy_path)?;
    let temp_path = sqlite_temp_path(sqlite_path);
    if temp_path.exists() {
        fs::remove_file(&temp_path)
            .with_context(|| format!("removing stale {}", temp_path.display()))?;
    }

    {
        let mut connection = open_database(&temp_path)?;
        replace_store(&mut connection, &store)?;
        let restored = load_store_from_connection(&connection)?;
        ensure_same_store(&store, &restored)?;
    }

    if let Some(parent) = sqlite_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    if sqlite_path.exists() {
        fs::remove_file(sqlite_path).with_context(|| {
            format!("replacing empty review database {}", sqlite_path.display())
        })?;
    }
    fs::rename(&temp_path, sqlite_path).with_context(|| {
        format!(
            "installing migrated review database {}",
            sqlite_path.display()
        )
    })?;
    let backup = migrated_json_backup_path(legacy_path);
    fs::rename(legacy_path, &backup).with_context(|| {
        format!(
            "renaming migrated review state {} to {}",
            legacy_path.display(),
            backup.display()
        )
    })?;
    Ok(store)
}

fn load_legacy_json(path: &Path) -> Result<ReviewStore> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn sqlite_temp_path(sqlite_path: &Path) -> PathBuf {
    let file_name = sqlite_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(SQLITE_STATE_FILE);
    sqlite_path.with_file_name(format!("{file_name}.tmp"))
}

fn migrated_json_backup_path(legacy_path: &Path) -> PathBuf {
    let timestamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    for suffix in 0.. {
        let candidate = if suffix == 0 {
            legacy_path.with_file_name(format!("{LEGACY_JSON_STATE_FILE}.migrated-{timestamp}"))
        } else {
            legacy_path.with_file_name(format!(
                "{LEGACY_JSON_STATE_FILE}.migrated-{timestamp}-{suffix}"
            ))
        };
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn load_store_from_connection(connection: &Connection) -> Result<ReviewStore> {
    let store_json = connection
        .query_row(
            "SELECT store_json FROM review_state WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .context("reading migrated review state")?;
    serde_json::from_str(&store_json).context("parsing migrated review state")
}

fn ensure_same_store(expected: &ReviewStore, restored: &ReviewStore) -> Result<()> {
    let expected = canonical_store_value(expected)?;
    let restored = canonical_store_value(restored)?;
    if expected != restored {
        bail!(
            "sqlite migration verification failed: restored review state differs from legacy JSON"
        );
    }
    Ok(())
}

fn replace_store(connection: &mut Connection, store: &ReviewStore) -> Result<()> {
    let tx = connection
        .transaction()
        .context("starting review state transaction")?;
    tx.execute_batch(
        "DELETE FROM image_profile_renders;
         DELETE FROM image_publish_profiles;
         DELETE FROM image_labels;
         DELETE FROM image_tags;
         DELETE FROM image_exif_tags;
         DELETE FROM images;
         DELETE FROM profile_tone_curve_points;
         DELETE FROM profile_hsl_values;
         DELETE FROM profile_sharpening;
         DELETE FROM profile_adjustments;
         DELETE FROM profiles;
         DELETE FROM review_state;",
    )
    .context("clearing previous review state")?;

    let store_json = serde_json::to_string_pretty(store).context("serializing review state")?;
    tx.execute(
        "INSERT INTO review_state(
            id, next_id, current_image_id, min_rating, exif_schema_version,
            store_json, store_sha1, updated_at
        ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            u64_to_i64(store.next_id, "review next_id")?,
            optional_u64_to_i64(store.ui.current_image_id, "current image id")?,
            u8_to_i64(store.ui.min_rating),
            u32_to_i64(store.exif_schema_version),
            store_json,
            sha1_hex(store_json.as_bytes()),
            now_string(),
        ],
    )
    .context("writing review state snapshot")?;

    for (position, profile) in store.profiles.iter().enumerate() {
        insert_profile(&tx, position, profile)?;
    }
    for (position, image) in store.images.iter().enumerate() {
        insert_image(&tx, position, image)?;
    }

    tx.commit().context("committing review state transaction")
}

fn insert_profile(tx: &Transaction<'_>, position: usize, profile: &ReviewProfile) -> Result<()> {
    let metadata = profile.metadata.as_ref();
    let metadata_json = metadata.map(serde_json::to_string).transpose()?;
    let profile_name = metadata.map(|metadata| metadata.profile_name.as_str());
    let profile_uuid = metadata.and_then(|metadata| metadata.profile_uuid.as_deref());
    let look_name = metadata.and_then(|metadata| metadata.look_name.as_deref());
    let look_uuid = metadata.and_then(|metadata| metadata.look_uuid.as_deref());
    let source_profile_name = metadata.and_then(|metadata| metadata.source_profile_name.as_deref());
    let source_profile_uuid = metadata.and_then(|metadata| metadata.source_profile_uuid.as_deref());
    let grain = metadata.and_then(|metadata| metadata.grain.as_ref());
    tx.execute(
        "INSERT INTO profiles(
            profile_index, position, selector, stem,
            retouch_exposure, retouch_highlights, retouch_shadows, retouch_whites,
            retouch_blacks, retouch_temperature, retouch_offset, retouch_clarity,
            metadata_present, profile_name, profile_uuid, look_name, look_uuid,
            source_profile_name, source_profile_uuid, has_camera_raw_settings,
            grain_amount, grain_size, grain_frequency, has_hald, has_pp3,
            pp3_name, metadata_json
        ) VALUES (
            ?1, ?2, ?3, ?4,
            ?5, ?6, ?7, ?8,
            ?9, ?10, ?11, ?12,
            ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20,
            ?21, ?22, ?23, ?24, ?25,
            ?26, ?27
        )",
        params![
            usize_to_i64(profile.index, "profile index")?,
            usize_to_i64(position, "profile position")?,
            profile.selector,
            profile.stem,
            profile.retouch_base.exposure,
            profile.retouch_base.highlights,
            profile.retouch_base.shadows,
            profile.retouch_base.whites,
            profile.retouch_base.blacks,
            profile.retouch_base.temperature,
            profile.retouch_base.offset,
            profile.retouch_base.clarity,
            bool_to_i64(metadata.is_some()),
            profile_name,
            profile_uuid,
            look_name,
            look_uuid,
            source_profile_name,
            source_profile_uuid,
            bool_to_i64(
                metadata
                    .map(|metadata| metadata.has_camera_raw_settings)
                    .unwrap_or(false)
            ),
            grain.map(|grain| u8_to_i64(grain.amount)),
            grain.map(|grain| u8_to_i64(grain.size)),
            grain.map(|grain| u8_to_i64(grain.frequency)),
            bool_to_i64(metadata.map(|metadata| metadata.has_hald).unwrap_or(false)),
            bool_to_i64(metadata.map(|metadata| metadata.has_pp3).unwrap_or(false)),
            metadata.and_then(|metadata| metadata.pp3_name.as_deref()),
            metadata_json,
        ],
    )
    .context("writing review profile")?;

    if let Some(metadata) = metadata {
        insert_profile_metadata(tx, profile.index, metadata)?;
    }
    Ok(())
}

fn insert_profile_metadata(
    tx: &Transaction<'_>,
    profile_index: usize,
    metadata: &ReviewProfileMetadata,
) -> Result<()> {
    insert_profile_adjustments(
        tx,
        profile_index,
        "source",
        &metadata.source_adjustments,
        &metadata.source_sharpening,
    )?;
    insert_profile_adjustments(
        tx,
        profile_index,
        "emulation",
        &metadata.emulation_adjustments,
        &metadata.emulation_sharpening,
    )
}

fn insert_profile_adjustments(
    tx: &Transaction<'_>,
    profile_index: usize,
    scope: &str,
    adjustments: &ReviewProfileAdjustments,
    sharpening: &ReviewProfileSharpening,
) -> Result<()> {
    tx.execute(
        "INSERT INTO profile_adjustments(
            profile_index, scope,
            exposure, contrast, highlights, shadows, whites, blacks, saturation,
            vibrance, clarity, parametric_shadows, parametric_darks,
            parametric_lights, parametric_highlights, parametric_shadow_split,
            parametric_midtone_split, parametric_highlight_split,
            calibration_red_hue, calibration_red_saturation, calibration_green_hue,
            calibration_green_saturation, calibration_blue_hue, calibration_blue_saturation
        ) VALUES (
            ?1, ?2,
            ?3, ?4, ?5, ?6, ?7, ?8, ?9,
            ?10, ?11, ?12, ?13,
            ?14, ?15, ?16,
            ?17, ?18,
            ?19, ?20, ?21,
            ?22, ?23, ?24
        )",
        params![
            usize_to_i64(profile_index, "profile index")?,
            scope,
            adjustments.exposure,
            adjustments.contrast,
            adjustments.highlights,
            adjustments.shadows,
            adjustments.whites,
            adjustments.blacks,
            adjustments.saturation,
            adjustments.vibrance,
            adjustments.clarity,
            adjustments.parametric.shadows,
            adjustments.parametric.darks,
            adjustments.parametric.lights,
            adjustments.parametric.highlights,
            adjustments.parametric.shadow_split,
            adjustments.parametric.midtone_split,
            adjustments.parametric.highlight_split,
            adjustments.calibration.red_hue,
            adjustments.calibration.red_saturation,
            adjustments.calibration.green_hue,
            adjustments.calibration.green_saturation,
            adjustments.calibration.blue_hue,
            adjustments.calibration.blue_saturation,
        ],
    )
    .context("writing profile adjustments")?;

    tx.execute(
        "INSERT INTO profile_sharpening(
            profile_index, scope, present, amount, radius, detail, masking
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            usize_to_i64(profile_index, "profile index")?,
            scope,
            bool_to_i64(sharpening.present),
            sharpening.amount,
            sharpening.radius,
            sharpening.detail,
            sharpening.masking,
        ],
    )
    .context("writing profile sharpening")?;

    for (channel, values) in [
        ("hue", &adjustments.hsl.hue),
        ("saturation", &adjustments.hsl.saturation),
        ("luminance", &adjustments.hsl.luminance),
    ] {
        for (position, value) in values.iter().enumerate() {
            tx.execute(
                "INSERT INTO profile_hsl_values(
                    profile_index, scope, channel, value_index, value
                ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    usize_to_i64(profile_index, "profile index")?,
                    scope,
                    channel,
                    usize_to_i64(position, "hsl value index")?,
                    *value,
                ],
            )
            .context("writing profile hsl value")?;
        }
    }

    for (channel, points) in [
        ("composite", &adjustments.tone_curve.composite),
        ("red", &adjustments.tone_curve.red),
        ("green", &adjustments.tone_curve.green),
        ("blue", &adjustments.tone_curve.blue),
    ] {
        for (position, [x, y]) in points.iter().enumerate() {
            tx.execute(
                "INSERT INTO profile_tone_curve_points(
                    profile_index, scope, channel, point_index, x, y
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    usize_to_i64(profile_index, "profile index")?,
                    scope,
                    channel,
                    usize_to_i64(position, "tone curve point index")?,
                    *x,
                    *y,
                ],
            )
            .context("writing profile tone curve point")?;
        }
    }
    Ok(())
}

fn insert_image(tx: &Transaction<'_>, position: usize, image: &ReviewImage) -> Result<()> {
    let image_json = serde_json::to_string(image).context("serializing review image")?;
    let preview_status = enum_text(&image.preview.status)?;
    let rating_source = enum_text(&image.rating_source)?;
    let tags_source = enum_text(&image.tags_source)?;
    let notes_source = enum_text(&image.notes_source)?;
    let codex_status = enum_text(&image.codex.status)?;
    let label = review_label_name(image.label);
    let retouch_crop_x = image.retouch.crop.map(|crop| crop.x);
    let retouch_crop_y = image.retouch.crop.map(|crop| crop.y);
    let retouch_crop_width = image.retouch.crop.map(|crop| crop.width);
    let retouch_crop_height = image.retouch.crop.map(|crop| crop.height);
    tx.execute(
        "INSERT INTO images(
            image_id, position, raw_path, sooc_sidecar_path, relative_path, file_name,
            exif_capture_timestamp, exif_rating, exif_focal_length, exif_aperture,
            exif_shutter_speed, exif_iso, exif_camera_model, exif_lens_model,
            exif_shooting_mode, exif_exposure_compensation, exif_flash, exif_note,
            preview_status, preview_path, preview_error, preview_duration_ms,
            preview_render_key, preview_updated_at, selected_profile_index, rating,
            label, notes, rating_source, tags_source, notes_source, codex_status,
            codex_flags_tags, codex_flags_note, codex_flags_rating, codex_model,
            codex_analysis_key, codex_error, codex_updated_at, retouch_exposure,
            retouch_highlights, retouch_shadows, retouch_whites, retouch_blacks,
            retouch_temperature, retouch_offset, retouch_clarity, retouch_crop_x,
            retouch_crop_y, retouch_crop_width, retouch_crop_height,
            retouch_rotation_degrees, publish_profiles_default, updated_at, image_json
        ) VALUES (
            :image_id, :position, :raw_path, :sooc_sidecar_path, :relative_path, :file_name,
            :exif_capture_timestamp, :exif_rating, :exif_focal_length, :exif_aperture,
            :exif_shutter_speed, :exif_iso, :exif_camera_model, :exif_lens_model,
            :exif_shooting_mode, :exif_exposure_compensation, :exif_flash, :exif_note,
            :preview_status, :preview_path, :preview_error, :preview_duration_ms,
            :preview_render_key, :preview_updated_at, :selected_profile_index, :rating,
            :label, :notes, :rating_source, :tags_source, :notes_source, :codex_status,
            :codex_flags_tags, :codex_flags_note, :codex_flags_rating, :codex_model,
            :codex_analysis_key, :codex_error, :codex_updated_at, :retouch_exposure,
            :retouch_highlights, :retouch_shadows, :retouch_whites, :retouch_blacks,
            :retouch_temperature, :retouch_offset, :retouch_clarity, :retouch_crop_x,
            :retouch_crop_y, :retouch_crop_width, :retouch_crop_height,
            :retouch_rotation_degrees, :publish_profiles_default, :updated_at, :image_json
        )",
        named_params! {
            ":image_id": u64_to_i64(image.id, "image id")?,
            ":position": usize_to_i64(position, "image position")?,
            ":raw_path": path_text(&image.raw_path),
            ":sooc_sidecar_path": option_path_text(image.sooc_sidecar_path.as_deref()),
            ":relative_path": image.relative_path,
            ":file_name": image.file_name,
            ":exif_capture_timestamp": image.exif.capture_timestamp,
            ":exif_rating": image.exif.rating.map(u8_to_i64),
            ":exif_focal_length": image.exif.focal_length,
            ":exif_aperture": image.exif.aperture,
            ":exif_shutter_speed": image.exif.shutter_speed,
            ":exif_iso": image.exif.iso,
            ":exif_camera_model": image.exif.camera_model,
            ":exif_lens_model": image.exif.lens_model,
            ":exif_shooting_mode": image.exif.shooting_mode,
            ":exif_exposure_compensation": image.exif.exposure_compensation,
            ":exif_flash": image.exif.flash,
            ":exif_note": image.exif.note,
            ":preview_status": preview_status,
            ":preview_path": option_path_text(image.preview.path.as_deref()),
            ":preview_error": image.preview.error,
            ":preview_duration_ms": optional_u64_to_i64(image.preview.duration_ms, "preview duration")?,
            ":preview_render_key": image.preview.render_key,
            ":preview_updated_at": image.preview.updated_at,
            ":selected_profile_index": usize_to_i64(image.selected_profile_index, "selected profile index")?,
            ":rating": u8_to_i64(image.rating),
            ":label": label,
            ":notes": image.notes,
            ":rating_source": rating_source,
            ":tags_source": tags_source,
            ":notes_source": notes_source,
            ":codex_status": codex_status,
            ":codex_flags_tags": bool_to_i64(image.codex.flags.tags),
            ":codex_flags_note": bool_to_i64(image.codex.flags.note),
            ":codex_flags_rating": bool_to_i64(image.codex.flags.rating),
            ":codex_model": image.codex.model,
            ":codex_analysis_key": image.codex.analysis_key,
            ":codex_error": image.codex.error,
            ":codex_updated_at": image.codex.updated_at,
            ":retouch_exposure": image.retouch.adjustments.exposure,
            ":retouch_highlights": image.retouch.adjustments.highlights,
            ":retouch_shadows": image.retouch.adjustments.shadows,
            ":retouch_whites": image.retouch.adjustments.whites,
            ":retouch_blacks": image.retouch.adjustments.blacks,
            ":retouch_temperature": image.retouch.adjustments.temperature,
            ":retouch_offset": image.retouch.adjustments.offset,
            ":retouch_clarity": image.retouch.adjustments.clarity,
            ":retouch_crop_x": retouch_crop_x,
            ":retouch_crop_y": retouch_crop_y,
            ":retouch_crop_width": retouch_crop_width,
            ":retouch_crop_height": retouch_crop_height,
            ":retouch_rotation_degrees": image.retouch.rotation_degrees,
            ":publish_profiles_default": bool_to_i64(image.publish_profile_indexes.is_none()),
            ":updated_at": image.updated_at,
            ":image_json": image_json,
        },
    )
    .context("writing review image")?;

    insert_text_list(tx, "image_exif_tags", image.id, &image.exif.tags)?;
    insert_text_list(tx, "image_tags", image.id, &image.tags)?;
    for (position, label) in image.labels.iter().enumerate() {
        tx.execute(
            "INSERT INTO image_labels(image_id, position, label) VALUES (?1, ?2, ?3)",
            params![
                u64_to_i64(image.id, "image id")?,
                usize_to_i64(position, "image label position")?,
                review_label_name(*label),
            ],
        )
        .context("writing review image label")?;
    }
    if let Some(indexes) = &image.publish_profile_indexes {
        for (position, profile_index) in indexes.iter().enumerate() {
            tx.execute(
                "INSERT INTO image_publish_profiles(image_id, position, profile_index)
                 VALUES (?1, ?2, ?3)",
                params![
                    u64_to_i64(image.id, "image id")?,
                    usize_to_i64(position, "publish profile position")?,
                    usize_to_i64(*profile_index, "publish profile index")?,
                ],
            )
            .context("writing review image publish profile")?;
        }
    }
    for (position, render) in image.profiles.iter().enumerate() {
        insert_profile_render(tx, image.id, position, render)?;
    }
    Ok(())
}

fn insert_text_list(
    tx: &Transaction<'_>,
    table: &str,
    image_id: u64,
    values: &[String],
) -> Result<()> {
    let sql = format!("INSERT INTO {table}(image_id, position, tag) VALUES (?1, ?2, ?3)");
    for (position, value) in values.iter().enumerate() {
        tx.execute(
            &sql,
            params![
                u64_to_i64(image_id, "image id")?,
                usize_to_i64(position, "text position")?,
                value,
            ],
        )
        .with_context(|| format!("writing {table}"))?;
    }
    Ok(())
}

fn insert_profile_render(
    tx: &Transaction<'_>,
    image_id: u64,
    position: usize,
    render: &ReviewProfileRender,
) -> Result<()> {
    let status = enum_text(&render.status)?;
    let render_json = serde_json::to_string(render).context("serializing profile render")?;
    tx.execute(
        "INSERT INTO image_profile_renders(
            image_id, position, profile_index, profile_stem, display_name, status,
            output_path, error, duration_ms, render_key, width, height, updated_at,
            render_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            u64_to_i64(image_id, "image id")?,
            usize_to_i64(position, "profile render position")?,
            usize_to_i64(render.profile_index, "profile render index")?,
            render.profile_stem,
            render.display_name,
            status,
            option_path_text(render.output_path.as_deref()),
            render.error,
            optional_u64_to_i64(render.duration_ms, "profile render duration")?,
            render.render_key,
            render.width.map(u32_to_i64),
            render.height.map(u32_to_i64),
            render.updated_at,
            render_json,
        ],
    )
    .context("writing review profile render")?;
    Ok(())
}

#[cfg(test)]
fn insert_text_list_table_name_allowed(table: &str) -> bool {
    matches!(table, "image_exif_tags" | "image_tags")
}

fn ensure_path_within(path: &Path, root: &Path) -> Result<()> {
    if path.starts_with(root) {
        Ok(())
    } else {
        bail!(
            "review state {} is outside output root {}",
            path.display(),
            root.display()
        )
    }
}

fn canonical_store_value(store: &ReviewStore) -> Result<serde_json::Value> {
    serde_json::to_value(store).context("canonicalizing review state")
}

fn enum_text<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value).context("serializing enum value")? {
        serde_json::Value::String(text) => Ok(text),
        value => Ok(value.to_string()),
    }
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn option_path_text(path: Option<&Path>) -> Option<String> {
    path.map(path_text)
}

fn bool_to_i64(value: bool) -> i64 {
    i64::from(value)
}

fn u8_to_i64(value: u8) -> i64 {
    i64::from(value)
}

fn u32_to_i64(value: u32) -> i64 {
    i64::from(value)
}

fn u64_to_i64(value: u64, name: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{name} {value} does not fit sqlite INTEGER"))
}

fn optional_u64_to_i64(value: Option<u64>, name: &str) -> Result<Option<i64>> {
    value.map(|value| u64_to_i64(value, name)).transpose()
}

fn usize_to_i64(value: usize, name: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("{name} {value} does not fit sqlite INTEGER"))
}

#[cfg(test)]
pub(super) fn table_count(path: &Path, table: &str) -> Result<u64> {
    if !insert_text_list_table_name_allowed(table)
        && !matches!(
            table,
            "review_state"
                | "profiles"
                | "profile_adjustments"
                | "profile_sharpening"
                | "profile_hsl_values"
                | "profile_tone_curve_points"
                | "images"
                | "image_labels"
                | "image_publish_profiles"
                | "image_profile_renders"
        )
    {
        bail!("unsupported table count target {table}");
    }
    let connection = open_database(path)?;
    let mut statement = connection.prepare(&format!("SELECT COUNT(*) FROM {table}"))?;
    let count = statement.query_row([], |row| row.get::<_, i64>(0))?;
    u64::try_from(count).context("sqlite table count does not fit u64")
}
