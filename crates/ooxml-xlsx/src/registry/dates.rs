//! Excel date serials (ADR-0006 §6): the 1900 system with Excel's false
//! 1900-02-29 (serial 60), and the 1904 system.

/// A date and time (no time zone), as Excel stores it.
///
/// The fields are private: every value comes from a validating
/// constructor, so a `set_datetime` can never write an impossible date or a
/// non-finite serial (PR #50 review M1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExcelDateTime {
    /// 1899–9999; `0` marks a time-only value (crate-internal).
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: f64,
}

impl ExcelDateTime {
    /// A date at midnight. `None` if the date does not exist.
    pub fn from_ymd(year: u16, month: u8, day: u8) -> Option<Self> {
        Self::from_ymd_hms(year, month, day, 0, 0, 0.0)
    }

    /// A date and time. `None` if it does not exist.
    pub fn from_ymd_hms(year: u16, month: u8, day: u8, hour: u8, minute: u8, second: f64) -> Option<Self> {
        let ok = (1..=12).contains(&month)
            && day >= 1
            && day <= days_in_month(year, month)
            && hour < 24
            && minute < 60
            && second.is_finite()
            && (0.0..60.0).contains(&second);
        ok.then_some(ExcelDateTime { year, month, day, hour, minute, second })
    }

    /// A time of day only (serial in [0, 1)); the date part is the epoch.
    pub fn from_hms(hour: u8, minute: u8, second: f64) -> Option<Self> {
        let t = Self::from_ymd_hms(1899, 12, 31, hour, minute, second)?;
        Some(ExcelDateTime { year: 0, ..t })
    }

    /// The year (1899–9999), or `None` for a time-only value.
    pub fn year(&self) -> Option<u16> {
        (self.year != 0).then_some(self.year)
    }
    /// The month (1–12). For a time-only value ([`Self::year`] is `None`)
    /// the date part is the 1900 epoch, 1899-12-31: month 12.
    pub fn month(&self) -> u8 {
        self.month
    }
    /// The day of the month (31 for a time-only value; see [`Self::month`]).
    pub fn day(&self) -> u8 {
        self.day
    }
    /// The hour (0–23).
    pub fn hour(&self) -> u8 {
        self.hour
    }
    /// The minute (0–59).
    pub fn minute(&self) -> u8 {
        self.minute
    }
    /// The seconds (0 ≤ s < 60).
    pub fn second(&self) -> f64 {
        self.second
    }

    fn time_fraction(&self) -> f64 {
        (f64::from(self.hour) * 3600.0 + f64::from(self.minute) * 60.0 + self.second) / 86_400.0
    }

    /// The serial number, or `None` outside the system's range: 1900 system
    /// 1899-12-31 (serial 0) to 9999-12-31; 1904 system 1904-01-01 to
    /// 9999-12-31; a time-only value is valid in both.
    pub fn serial(&self, date1904: bool) -> Option<f64> {
        let serial = self.serial_unchecked(date1904)?;
        serial.is_finite().then_some(serial)
    }

    fn serial_unchecked(&self, date1904: bool) -> Option<f64> {
        if self.year == 0 {
            return Some(self.time_fraction());
        }
        let days = days_from_civil(i64::from(self.year), u32::from(self.month), u32::from(self.day));
        let serial_days = if date1904 {
            let d = days - days_from_civil(1904, 1, 1);
            if d < 0 || self.year > 9999 {
                return None;
            }
            d
        } else {
            let d = days - days_from_civil(1899, 12, 31);
            if d < 0 || self.year > 9999 {
                return None;
            }
            // Excel's false 1900-02-29 is serial 60: later dates shift by one
            if d >= 60 { d + 1 } else { d }
        };
        Some(serial_days as f64 + self.time_fraction())
    }

    /// The date for a serial. `None` for negative or too-large serials, and
    /// for serial 60 in the 1900 system (the nonexistent 1900-02-29).
    pub fn from_serial(serial: f64, date1904: bool) -> Option<Self> {
        // beyond 9999-12-31 in either system (2,958,465 / 2,957,003): refused
        // before the integer arithmetic, which would overflow for huge
        // serials (PR #50 review round 2, m1)
        if !serial.is_finite() || !(0.0..3_000_000.0).contains(&serial) {
            return None;
        }
        let mut days = serial.floor() as i64;
        let mut secs = ((serial - serial.floor()) * 86_400.0 * 1000.0).round() / 1000.0;
        if secs >= 86_400.0 {
            days += 1;
            secs = 0.0;
        }
        let base = if date1904 {
            days_from_civil(1904, 1, 1)
        } else {
            if days == 60 {
                return None;
            }
            if days > 60 {
                days -= 1;
            }
            days_from_civil(1899, 12, 31)
        };
        let (y, m, d) = civil_from_days(base + days);
        if !(1899..=9999).contains(&y) {
            return None;
        }
        let hour = (secs / 3600.0).floor();
        let minute = ((secs - hour * 3600.0) / 60.0).floor();
        let second = secs - hour * 3600.0 - minute * 60.0;
        Some(ExcelDateTime { year: y as u16, month: m as u8, day: d as u8, hour: hour as u8, minute: minute as u8, second })
    }
}

fn is_leap(y: u16) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

fn days_in_month(y: u16, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Hinnant's algorithm).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serials_1900() {
        let s = |y, m, d| ExcelDateTime::from_ymd(y, m, d).unwrap().serial(false).unwrap();
        assert_eq!(s(1899, 12, 31), 0.0);
        assert_eq!(s(1900, 1, 1), 1.0);
        assert_eq!(s(1900, 2, 28), 59.0);
        assert_eq!(s(1900, 3, 1), 61.0, "after the false 1900-02-29 (60)");
        assert_eq!(s(2026, 11, 16), 46342.0);
        assert_eq!(s(9999, 12, 31), 2_958_465.0);
        assert!(ExcelDateTime::from_ymd(1900, 2, 29).is_none(), "the real calendar has no 1900-02-29");
        assert_eq!(ExcelDateTime::from_serial(60.0, false), None);
        assert_eq!(ExcelDateTime::from_serial(61.0, false), ExcelDateTime::from_ymd(1900, 3, 1));
        assert_eq!(ExcelDateTime::from_serial(46342.5, false), ExcelDateTime::from_ymd_hms(2026, 11, 16, 12, 0, 0.0));
        let t = ExcelDateTime::from_hms(6, 0, 0.0).unwrap();
        assert_eq!(t.serial(false), Some(0.25));
        assert_eq!(t.serial(true), Some(0.25));
    }

    #[test]
    fn serials_1904() {
        let s = |y, m, d| ExcelDateTime::from_ymd(y, m, d).map(|x| x.serial(true));
        assert_eq!(s(1904, 1, 1), Some(Some(0.0)));
        assert_eq!(s(2026, 11, 16), Some(Some(46342.0 - 1462.0)));
        assert_eq!(s(9999, 12, 31), Some(Some(2_957_003.0)));
        assert_eq!(s(1903, 12, 31), Some(None));
        assert_eq!(ExcelDateTime::from_serial(0.0, true), ExcelDateTime::from_ymd(1904, 1, 1));
    }

    #[test]
    fn constructors_validate() {
        assert!(ExcelDateTime::from_ymd(2026, 2, 31).is_none());
        assert!(ExcelDateTime::from_ymd_hms(2026, 2, 1, 99, 0, 0.0).is_none());
        assert!(ExcelDateTime::from_ymd_hms(2026, 2, 1, 0, 60, 0.0).is_none());
        assert!(ExcelDateTime::from_ymd_hms(2026, 2, 1, 0, 0, f64::NAN).is_none());
        assert!(ExcelDateTime::from_ymd_hms(2026, 2, 1, 0, 0, 60.0).is_none());
        assert!(ExcelDateTime::from_ymd(10_000, 1, 1).is_none_or(|d| d.serial(false).is_none()));
        for huge in [3.0e6, 9.3e18, 1e19, 1e300, f64::MAX] {
            assert_eq!(ExcelDateTime::from_serial(huge, false), None, "{huge}");
            assert_eq!(ExcelDateTime::from_serial(huge, true), None, "{huge}");
        }
        assert!(ExcelDateTime::from_serial(2_958_465.5, false).is_some(), "9999-12-31 12:00");
        let t = ExcelDateTime::from_hms(1, 2, 3.5).unwrap();
        assert_eq!((t.year(), t.hour(), t.minute(), t.second()), (None, 1, 2, 3.5));
    }
}
