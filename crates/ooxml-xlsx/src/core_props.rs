//! `docProps/core.xml` for workbooks created from the template (ADR-0001
//! R2-m2 / R3-m1: regenerated wholesale and installed with `set_part`).

use std::time::{SystemTime, UNIX_EPOCH};

/// W3CDTF UTC timestamp, e.g. `2026-11-16T09:30:00Z`.
pub(crate) fn w3cdtf(t: SystemTime) -> String {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(duration) => i128::from(duration.as_secs()),
        Err(error) => {
            let duration = error.duration();
            -i128::from(duration.as_secs()) - i128::from(duration.subsec_nanos() != 0)
        }
    };
    let (days, rem) = (secs.div_euclid(86_400) as i64, secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

/// A fresh `core.xml`: created and modified set to `t`, no creator.
pub(crate) fn generate(t: SystemTime) -> Vec<u8> {
    let now = w3cdtf(t);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" \
         xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
         <dcterms:created xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:created>\
         <dcterms:modified xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:modified>\
         </cp:coreProperties>"
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn dates() {
        assert_eq!(w3cdtf(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(w3cdtf(UNIX_EPOCH + Duration::from_secs(951_782_400)), "2000-02-29T00:00:00Z");
        assert_eq!(w3cdtf(UNIX_EPOCH + Duration::from_secs(1_794_822_645)), "2026-11-16T09:50:45Z");
        let x = generate(UNIX_EPOCH);
        ooxml_core::XmlDocument::parse(&x).unwrap();
    }
}
