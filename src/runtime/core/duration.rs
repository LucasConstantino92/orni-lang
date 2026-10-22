//! The native implementation behind Orni's `Duration` core class.
//!
//! `Duration` is a value class rather than a timer that starts on its own. It
//! describes an amount of elapsed time for APIs such as token lifetimes,
//! connection timeouts, retry windows, and route protection. The HTTP runtime
//! decides when to apply that value.
//!
//! ```orni
//! let retry_after: duration = Duration(seconds: 30);
//! app.protect(
//!     route: "/auth/login",
//!     attempts: 5,
//!     lock_time: Duration(minutes: 15)
//! );
//! ```
//!
//! Exactly one explicit unit is accepted by the constructor:
//!
//! - `milliseconds` is stored directly.
//! - `seconds` is multiplied by 1,000.
//! - `minutes` is multiplied by 60,000.
//! - `hours` is multiplied by 3,600,000.
//! - `days` is multiplied by 86,400,000.
//!
//! Keeping the normalized value in milliseconds makes comparisons predictable
//! and prevents APIs from accepting an ambiguous raw integer.

/// The Orni core class created by `Duration(unit: amount)`.
///
/// This zero-sized Rust type is the implementation namespace for the Orni
/// value class. Its constructor returns a checked millisecond count, which the
/// interpreter stores as `Value::Duration`.
pub(crate) struct Duration;

/// Why a native duration value could not be created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DurationError {
    /// The constructor accepts elapsed time, never a negative amount.
    Negative,
    /// An integer literal cannot be represented by the normalized value type.
    OutOfRange,
    /// The selected unit would overflow after conversion to milliseconds.
    Overflow,
}

impl Duration {
    /// Creates a checked duration in milliseconds from the parser-selected unit.
    ///
    /// `multiplier` is intentionally supplied by the parser rather than by
    /// user code. This keeps `Duration(seconds: 2)` explicit at the language
    /// boundary while allowing the core class to own checked normalization.
    pub(crate) fn from_unit(amount: i128, multiplier: i128) -> Result<u64, DurationError> {
        if amount < 0 {
            return Err(DurationError::Negative);
        }
        let amount = u64::try_from(amount).map_err(|_| DurationError::OutOfRange)?;
        let multiplier = u64::try_from(multiplier).map_err(|_| DurationError::OutOfRange)?;
        amount
            .checked_mul(multiplier)
            .ok_or(DurationError::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::{Duration, DurationError};

    #[test]
    fn normalizes_explicit_time_units_to_milliseconds() {
        assert_eq!(Duration::from_unit(250, 1), Ok(250));
        assert_eq!(Duration::from_unit(2, 1_000), Ok(2_000));
        assert_eq!(Duration::from_unit(3, 60_000), Ok(180_000));
        assert_eq!(Duration::from_unit(1, 3_600_000), Ok(3_600_000));
    }

    #[test]
    fn rejects_negative_and_overflowing_values() {
        assert_eq!(Duration::from_unit(-1, 1), Err(DurationError::Negative));
        assert_eq!(
            Duration::from_unit(i128::MAX, 1),
            Err(DurationError::OutOfRange)
        );
        assert_eq!(
            Duration::from_unit(u64::MAX as i128, 2),
            Err(DurationError::Overflow)
        );
    }
}
