#![cfg_attr(target_arch = "x86_64", feature(portable_simd))]

mod adjustments;
mod diffusion;
mod grain;
mod grain_rfgr;
mod hald;
mod model;
mod nikon;
mod pp3;
mod rgb_table;
pub mod util;
mod xmp;

pub use diffusion::{
    DiffusionMethod, DiffusionPreset, DiffusionSettings, apply_diffusion, render_diffusion_rgb16,
};
pub use grain::{
    apply_grain, apply_grain_8bit, apply_grain_8bit_with_engine, apply_grain_8bit_with_options,
    apply_grain_with_engine, apply_grain_with_options,
};
pub use hald::{
    convert_dir, convert_path, convert_xmp_to_hald, profile_display_name, profile_info_line,
    try_convert_dir, write_hald_png, write_hald_png_with_adjustments,
};
pub use model::{
    BatchSummary, CalibrationAdjustments, ConvertedProfile, DEFAULT_GRAIN_REFERENCE_MPIX,
    GrainEngine, GrainRenderOptions, GrainSettings, HaldOptions, HslAdjustments, ParametricTone,
    ProfileAdjustments, RgbTable, SharpeningSettings, ToneCurves, XmpFilmRecipe, XmpRgbTable,
    dummy_converted_profile, non_default_adjustments,
};
pub use nikon::{
    NikonPictureControl, NikonReport, fit_nikon_picture_control,
    fit_nikon_picture_control_from_hald, write_ncp, write_report,
};
pub use pp3::{
    rawtherapee_contrast_clarity_profile_text, rawtherapee_hald_clut_profile_text,
    rawtherapee_local_contrast_profile_text, rawtherapee_profile_text,
    rawtherapee_resize_profile_text, rawtherapee_tone_equalizer_profile_text,
    write_rawtherapee_contrast_clarity_profile, write_rawtherapee_profile,
    write_rawtherapee_resize_profile,
};
pub use rgb_table::{decode_rgb_table, parse_rgb_table, sample_rgb_table};
pub use util::{
    SUPPORTED_RAW_EXTENSIONS, configure_threads, cpu_thread_count, default_hald_dir,
    half_cpu_thread_count, is_supported_raw_file, remove_temp_file, time_of_day_seed,
};
pub use xmp::{extract_film_recipe, extract_rgb_table};
