//! Locale-aware integer formatting for human-readable output.
//!
//! Rustloc keeps canonical count and diff responses numeric. This module is
//! only used by the human table presentation adapter, where a reader may ask
//! for grouped digits. Locale lookup happens once for that render: the active
//! system locale is parsed against `num-format`'s supported locale table, and
//! unsupported or absent locales fall back to `en` so counting and diffing never
//! fail because of locale discovery.

use num_format::{Locale, ToFormattedString};

/// Deterministic fallback used when the active locale is absent or unsupported.
pub const FALLBACK_LOCALE: Locale = Locale::en;

/// Integer display policy for human tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    enabled: bool,
    locale: Locale,
}

impl NumberFormat {
    /// Return the ungrouped display policy.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            locale: FALLBACK_LOCALE,
        }
    }

    /// Resolve the active system locale once and return a grouping policy.
    pub fn active() -> Self {
        Self::from_locale_name(sys_locale::get_locale().as_deref()).unwrap_or_else(Self::fallback)
    }

    /// Return the documented deterministic grouping fallback.
    pub fn fallback() -> Self {
        Self {
            enabled: true,
            locale: FALLBACK_LOCALE,
        }
    }

    /// Build a grouping policy from an explicit locale name.
    ///
    /// Locale names may arrive as BCP 47 (`en-US`) or POSIX-ish
    /// (`en_US.UTF-8`, `de_DE@euro`). Encoding and modifier suffixes are not
    /// part of `num-format` locale names, so they are stripped before parsing.
    pub fn from_locale_name(name: Option<&str>) -> Option<Self> {
        let name = name?.trim();
        if name.is_empty() {
            return None;
        }

        let name = normalize_locale_name(name);
        Locale::from_name(name)
            .or_else(|_| Locale::from_name(language_subtag(name)))
            .ok()
            .map(|locale| Self {
                enabled: true,
                locale,
            })
    }

    /// Format an unsigned count for display.
    pub fn u64(self, value: u64) -> String {
        if self.enabled {
            value.to_formatted_string(&self.locale)
        } else {
            value.to_string()
        }
    }

    /// Format a signed count for display.
    pub fn i64(self, value: i64) -> String {
        if self.enabled {
            value.to_formatted_string(&self.locale)
        } else {
            value.to_string()
        }
    }
}

fn normalize_locale_name(name: &str) -> &str {
    name.split(['.', '@']).next().unwrap_or(name)
}

fn language_subtag(name: &str) -> &str {
    name.split(['-', '_']).next().unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_format_uses_plain_digits() {
        assert_eq!(NumberFormat::disabled().u64(3805), "3805");
        assert_eq!(NumberFormat::disabled().i64(-3805), "-3805");
    }

    #[test]
    fn explicit_comma_grouping_locale_formats_integers() {
        let format = NumberFormat::from_locale_name(Some("en-US")).unwrap();

        assert_eq!(format.u64(3805), "3,805");
        assert_eq!(format.i64(-3805), "-3,805");
    }

    #[test]
    fn explicit_non_comma_grouping_locale_formats_integers() {
        let format = NumberFormat::from_locale_name(Some("de-DE")).unwrap();

        assert_eq!(format.u64(3805), "3.805");
        assert_eq!(format.i64(-3805), "-3.805");
    }

    #[test]
    fn locale_name_normalization_accepts_posix_suffixes() {
        let format = NumberFormat::from_locale_name(Some("en_US.UTF-8@posix")).unwrap();

        assert_eq!(format.u64(3805), "3,805");
    }

    #[test]
    fn fallback_is_english_style_grouping() {
        assert_eq!(NumberFormat::fallback().u64(3805), "3,805");
    }

    #[test]
    fn unsupported_locale_returns_none_for_callers_to_fallback() {
        assert!(NumberFormat::from_locale_name(Some("zz-ZZ")).is_none());
        assert!(NumberFormat::from_locale_name(None).is_none());
    }
}
