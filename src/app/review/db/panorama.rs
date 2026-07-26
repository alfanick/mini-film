use super::{ReviewDatabase, entities::*, *};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};

impl ReviewDatabase {
    pub(crate) async fn load_panorama_projects(&self) -> Result<Vec<ReviewPanoramaProject>> {
        let projects = panorama_projects::Entity::find()
            .order_by_desc(panorama_projects::Column::UpdatedAt)
            .all(&self.connection)
            .await
            .context("reading panorama projects")?;
        let sources = panorama_project_images::Entity::find()
            .order_by_asc(panorama_project_images::Column::PanoramaId)
            .order_by_asc(panorama_project_images::Column::Position)
            .all(&self.connection)
            .await
            .context("reading panorama project sources")?;
        let previews = panorama_previews::Entity::find()
            .order_by_asc(panorama_previews::Column::PanoramaId)
            .order_by_asc(panorama_previews::Column::MatchingMode)
            .order_by_asc(panorama_previews::Column::Projection)
            .all(&self.connection)
            .await
            .context("reading panorama previews")?;

        projects
            .into_iter()
            .map(|project| {
                let panorama_id = project.panorama_id;
                let image_ids = sources
                    .iter()
                    .filter(|source| source.panorama_id == panorama_id)
                    .map(|source| db_u64(source.image_id, "panorama source image id"))
                    .collect::<Result<Vec<_>>>()?;
                let previews = previews
                    .iter()
                    .filter(|preview| preview.panorama_id == panorama_id)
                    .map(|preview| preview_from_row(preview, &self.roots))
                    .collect::<Result<Vec<_>>>()?;
                project_from_row(project, image_ids, previews, &self.roots)
            })
            .collect()
    }

    pub(crate) async fn create_panorama_project(
        &self,
        project: &mut ReviewPanoramaProject,
    ) -> Result<()> {
        let transaction = self
            .connection
            .begin()
            .await
            .context("starting panorama project transaction")?;
        let inserted = project_active_model(project, false, &self.roots)?
            .insert(&transaction)
            .await
            .context("creating panorama project")?;
        project.id = db_u64(inserted.panorama_id, "new panorama project id")?;
        if let Err(error) = replace_project_children(&transaction, project, &self.roots).await {
            transaction.rollback().await.ok();
            return Err(error);
        }
        transaction
            .commit()
            .await
            .context("committing panorama project creation")
    }

    pub(crate) async fn save_panorama_project(
        &self,
        project: &ReviewPanoramaProject,
    ) -> Result<()> {
        let transaction = self
            .connection
            .begin()
            .await
            .context("starting panorama project transaction")?;
        let result = async {
            project_active_model(project, true, &self.roots)?
                .update(&transaction)
                .await
                .with_context(|| format!("updating panorama project {}", project.id))?;
            replace_project_children(&transaction, project, &self.roots).await
        }
        .await;
        match result {
            Ok(()) => transaction
                .commit()
                .await
                .context("committing panorama project update"),
            Err(error) => {
                transaction.rollback().await.ok();
                Err(error)
            }
        }
    }
}

fn project_from_row(
    row: panorama_projects::Model,
    image_ids: Vec<u64>,
    previews: Vec<ReviewPanoramaPreview>,
    roots: &ReviewPathRoots,
) -> Result<ReviewPanoramaProject> {
    Ok(ReviewPanoramaProject {
        id: db_u64(row.panorama_id, "panorama project id")?,
        name: row.name,
        status: ReviewPanoramaStatus::parse(&row.status)?,
        matching_mode: parse_matching_mode(&row.matching_mode)?,
        selected_projection: row
            .selected_projection
            .as_deref()
            .map(parse_projection)
            .transpose()?,
        output_path: row
            .output_path
            .as_deref()
            .map(|path| roots.source_from_storage(path, "panorama_projects.output_path"))
            .transpose()?,
        result_image_id: row
            .result_image_id
            .map(|value| db_u64(value, "panorama result image id"))
            .transpose()?,
        progress_stage: row.progress_stage,
        progress_completed: db_usize(row.progress_completed, "panorama progress completed")?,
        progress_total: db_usize(row.progress_total, "panorama progress total")?,
        error: row.error,
        created_at: row.created_at,
        updated_at: row.updated_at,
        image_ids,
        previews,
    })
}

fn preview_from_row(
    row: &panorama_previews::Model,
    roots: &ReviewPathRoots,
) -> Result<ReviewPanoramaPreview> {
    Ok(ReviewPanoramaPreview {
        matching_mode: parse_matching_mode(&row.matching_mode)?,
        projection: parse_projection(&row.projection)?,
        status: ReviewPanoramaPreviewStatus::parse(&row.status)?,
        path: row
            .preview_path
            .as_deref()
            .map(|path| roots.output_from_storage(path, "panorama_previews.preview_path"))
            .transpose()?,
        cache_key: row.cache_key.clone(),
        duration_ms: row
            .duration_ms
            .map(|value| db_u64(value, "panorama preview duration"))
            .transpose()?,
        error: row.error.clone(),
        updated_at: row.updated_at.clone(),
    })
}

fn project_active_model(
    project: &ReviewPanoramaProject,
    include_id: bool,
    roots: &ReviewPathRoots,
) -> Result<panorama_projects::ActiveModel> {
    Ok(panorama_projects::ActiveModel {
        panorama_id: if include_id {
            Set(db_i64(project.id))
        } else {
            sea_orm::NotSet
        },
        name: Set(project.name.clone()),
        status: Set(project.status.as_str().to_string()),
        matching_mode: Set(project.matching_mode.to_string()),
        selected_projection: Set(project.selected_projection.map(|value| value.to_string())),
        output_path: Set(project
            .output_path
            .as_deref()
            .map(|path| roots.source_to_storage(path, "panorama_projects.output_path"))
            .transpose()?),
        result_image_id: Set(project.result_image_id.map(db_i64)),
        progress_stage: Set(project.progress_stage.clone()),
        progress_completed: Set(db_i64_usize(project.progress_completed)),
        progress_total: Set(db_i64_usize(project.progress_total)),
        error: Set(project.error.clone()),
        created_at: Set(project.created_at.clone()),
        updated_at: Set(project.updated_at.clone()),
    })
}

async fn replace_project_children(
    transaction: &sea_orm::DatabaseTransaction,
    project: &ReviewPanoramaProject,
    roots: &ReviewPathRoots,
) -> Result<()> {
    let panorama_id = db_i64(project.id);
    panorama_project_images::Entity::delete_many()
        .filter(panorama_project_images::Column::PanoramaId.eq(panorama_id))
        .exec(transaction)
        .await
        .with_context(|| format!("replacing sources for panorama project {}", project.id))?;
    panorama_previews::Entity::delete_many()
        .filter(panorama_previews::Column::PanoramaId.eq(panorama_id))
        .exec(transaction)
        .await
        .with_context(|| format!("replacing previews for panorama project {}", project.id))?;

    for (position, image_id) in project.image_ids.iter().copied().enumerate() {
        panorama_project_images::ActiveModel {
            panorama_id: Set(panorama_id),
            position: Set(db_i64_usize(position)),
            image_id: Set(db_i64(image_id)),
        }
        .insert(transaction)
        .await
        .with_context(|| {
            format!(
                "writing source {position} for panorama project {}",
                project.id
            )
        })?;
    }
    for preview in &project.previews {
        panorama_previews::ActiveModel {
            panorama_id: Set(panorama_id),
            matching_mode: Set(preview.matching_mode.to_string()),
            projection: Set(preview.projection.to_string()),
            status: Set(preview.status.as_str().to_string()),
            preview_path: Set(preview
                .path
                .as_deref()
                .map(|path| roots.output_to_storage(path, "panorama_previews.preview_path"))
                .transpose()?),
            cache_key: Set(preview.cache_key.clone()),
            duration_ms: Set(preview.duration_ms.map(db_i64)),
            error: Set(preview.error.clone()),
            updated_at: Set(preview.updated_at.clone()),
        }
        .insert(transaction)
        .await
        .with_context(|| {
            format!(
                "writing {} {} preview for panorama project {}",
                preview.matching_mode, preview.projection, project.id
            )
        })?;
    }
    Ok(())
}

fn parse_matching_mode(value: &str) -> Result<PanoramaMatchingMode> {
    match value {
        "automatic" => Ok(PanoramaMatchingMode::Automatic),
        "sequential" => Ok(PanoramaMatchingMode::Sequential),
        "multi-row" => Ok(PanoramaMatchingMode::MultiRow),
        "flat-mosaic" => Ok(PanoramaMatchingMode::FlatMosaic),
        _ => bail!("invalid panorama matching mode {value:?}"),
    }
}

fn parse_projection(value: &str) -> Result<PanoramaProjection> {
    match value {
        "rectilinear" => Ok(PanoramaProjection::Rectilinear),
        "cylindrical" => Ok(PanoramaProjection::Cylindrical),
        "equirectangular" => Ok(PanoramaProjection::Equirectangular),
        "panini" => Ok(PanoramaProjection::Panini),
        _ => bail!("invalid panorama projection {value:?}"),
    }
}

fn db_i64(value: u64) -> i64 {
    i64::try_from(value).expect("review identifiers fit SQLite INTEGER")
}

fn db_i64_usize(value: usize) -> i64 {
    i64::try_from(value).expect("review counts fit SQLite INTEGER")
}

fn db_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).with_context(|| format!("{field} is negative: {value}"))
}

fn db_usize(value: i64, field: &str) -> Result<usize> {
    usize::try_from(value).with_context(|| format!("{field} is invalid: {value}"))
}
