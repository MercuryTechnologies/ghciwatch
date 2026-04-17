use std::sync::OnceLock;
use std::time::Duration;

use miette::miette;
use miette::Context;
use miette::IntoDiagnostic;

const TIMEOUT_MULT_VAR: &str = "TIMEOUT_MULT";
const DEFAULT_TIMEOUT_MULT: f64 = 1.0;

static TIMEOUT_MULT: OnceLock<miette::Result<f64>> = OnceLock::new();

/// Multiply a [`Duration`] by the `$TIMEOUT_MULT`.
///
/// See: [`get_timeout_mult`].
pub fn timeout_mult(duration: Duration) -> miette::Result<Duration> {
    match get_timeout_mult() {
        Ok(mult) => Ok(duration.mul_f64(*mult)),
        // lol
        Err(err) => Err(miette!("{err}")),
    }
}

/// Get the `$TIMEOUT_MULT` environment variable.
///
/// This is a multiplier for all test timeout durations; it's used to make tests more reliable in CI
/// and under load.
pub fn get_timeout_mult() -> &'static miette::Result<f64> {
    TIMEOUT_MULT.get_or_init(get_timeout_mult_inner)
}

fn get_timeout_mult_inner() -> miette::Result<f64> {
    match std::env::var(TIMEOUT_MULT_VAR) {
        Ok(raw) => raw
            .parse()
            .into_diagnostic()
            .wrap_err("Failed to parse `$TIMEOUT_MULT`"),
        Err(std::env::VarError::NotPresent) => Ok(DEFAULT_TIMEOUT_MULT),
        Err(err) => Err(err)
            .into_diagnostic()
            .wrap_err("Failed to get `$TIMEOUT_MULT`"),
    }
}
