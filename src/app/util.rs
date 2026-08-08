use std::{fs, io, path::Path};

use anyhow::{Context, Result};

pub(crate) use crate::app::timestamps::{
    OutputEditMetadata, extract_capture_iso, restore_output_color_profile,
    sync_output_metadata_from_image_with_color_profile,
    sync_output_metadata_from_raw_with_color_profile, sync_output_timestamps_from_exif,
};
pub(crate) use crate::util::{
    InputFileFilter, coalesce_due_input_sidecars, coalesce_input_sidecars, configure_threads,
    cpu_thread_count, default_hald_dir, half_cpu_thread_count, input_filter_name,
    is_heic_input_file, is_internal_staging_input_file, is_jpeg_input_file, is_raw_input_file,
    is_rendered_input_file, is_supported_input_file, is_supported_raw_file, is_tiff_input_file,
    matching_raw_for_sidecar, matching_sidecar_for_raw, remove_temp_file, time_of_day_seed,
};

pub(crate) fn create_missing_input_directory(path: &Path) -> Result<()> {
    if !path.exists() {
        eprintln!(
            "Input directory {} does not exist. Create it? [y/N]",
            path.display()
        );
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            fs::create_dir_all(path).with_context(|| format!("creating {}", path.display()))?;
        }
    }
    Ok(())
}
