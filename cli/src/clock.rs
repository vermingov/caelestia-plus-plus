//! Local wall-clock time, for naming screenshots and recordings.
//!
//! The names have to match what the Python CLI produced, and they are local
//! time, not UTC — a screenshot taken at 21:40 is named 21:40. That is one
//! libc call away, so there is no reason to carry a date crate for it.

/// `struct tm` as the C library fills it in.
#[repr(C)]
struct Tm {
    sec: i32,
    min: i32,
    hour: i32,
    mday: i32,
    mon: i32,
    year: i32,
    wday: i32,
    yday: i32,
    isdst: i32,
    gmtoff: i64,
    zone: *const i8,
}

pub struct LocalTime {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
}

pub fn now() -> LocalTime {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut tm = Tm {
        sec: 0, min: 0, hour: 0, mday: 1, mon: 0, year: 70,
        wday: 0, yday: 0, isdst: -1, gmtoff: 0, zone: std::ptr::null(),
    };
    unsafe {
        tzset();
        localtime_r(&secs, &mut tm);
    }

    LocalTime {
        year: tm.year + 1900,
        month: tm.mon + 1,
        day: tm.mday,
        hour: tm.hour,
        minute: tm.min,
        second: tm.sec,
    }
}

impl LocalTime {
    /// `20260914213000` — the screenshot cache name.
    pub fn compact(&self) -> String {
        format!(
            "{:04}{:02}{:02}{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// `20260914_21-30-00` — the recording name.
    pub fn recording(&self) -> String {
        format!(
            "{:04}{:02}{:02}_{:02}-{:02}-{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

extern "C" {
    fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
    fn tzset();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrees_with_the_system_clock() {
        // `date` reads the same zone from the same environment, so the two
        // must agree — to the second, unless we straddle one.
        let ours = now().compact();
        let theirs = String::from_utf8(
            std::process::Command::new("date")
                .arg("+%Y%m%d%H%M%S")
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let theirs = theirs.trim();
        assert_eq!(ours.len(), 14);
        assert_eq!(&ours[..12], &theirs[..12], "{ours} vs {theirs}");
    }

    #[test]
    fn the_recording_name_is_the_shape_the_python_cli_wrote() {
        let stamp = now().recording();
        assert_eq!(stamp.len(), 17);
        assert_eq!(stamp.as_bytes()[8], b'_');
        assert_eq!(stamp.as_bytes()[11], b'-');
        assert_eq!(stamp.as_bytes()[14], b'-');
    }
}
