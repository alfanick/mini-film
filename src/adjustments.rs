/// Return whether a Lightroom tone curve leaves values unchanged.
///
/// XMP curves are stored as control points. An empty curve and a curve whose
/// points all lie on `y = x` are both identity operations, so they do not need a
/// generated RawTherapee curve.
pub(crate) fn curve_is_identity(points: &[(f32, f32)]) -> bool {
    points.is_empty() || points.iter().all(|(x, y)| (*x - *y).abs() < f32::EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_curve_accepts_empty_and_exact_diagonal_points() {
        assert!(curve_is_identity(&[]));
        assert!(curve_is_identity(&[
            (0.0, 0.0),
            (128.0, 128.0),
            (255.0, 255.0)
        ]));
    }

    #[test]
    fn identity_curve_rejects_any_non_diagonal_point() {
        assert!(!curve_is_identity(&[(0.0, 0.0), (128.0, 129.0)]));
    }
}
