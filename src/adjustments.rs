/// Return whether a Lightroom tone curve leaves values unchanged.
///
/// XMP curves are stored as control points. An empty curve and a curve whose
/// points all lie on `y = x` are both identity operations, so they do not need a
/// generated RawTherapee curve.
pub(crate) fn curve_is_identity(points: &[(f32, f32)]) -> bool {
    points.is_empty() || points.iter().all(|(x, y)| (*x - *y).abs() < f32::EPSILON)
}
