//! The grain overlay's strength. Shared by design: the grain is a body-level
//! layer every format's page sits under, and no format pipeline gets to own it.

use crate::appearance::Appearance;

/// `--noise-opacity`, the grain strength dial as 0..=1. Written on `body`,
/// where the overlay resolves it.
pub fn css_vars(a: &Appearance) -> Vec<(&'static str, String)> {
    vec![(
        "--noise-opacity",
        format!("{}", a.noise_intensity.min(100) as f64 / 100.0),
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_opacity_var_is_a_unit_fraction() {
        let a = Appearance { noise_intensity: 65, ..Default::default() };
        let vars = css_vars(&a);
        assert_eq!(vars[0].0, "--noise-opacity");
        assert_eq!(vars[0].1, "0.65");
    }
}
