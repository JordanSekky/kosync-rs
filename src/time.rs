use chrono::{DateTime, Utc};

/// Current time in unix nanoseconds, the resolution all timestamps are stored at.
pub fn now_ns() -> i64 {
    Utc::now()
        .timestamp_nanos_opt()
        .expect("system clock is outside the range of i64 nanoseconds")
}

/// A stored unix-nanosecond timestamp as a [`DateTime`].
pub fn datetime_from_ns(ns: i64) -> DateTime<Utc> {
    DateTime::from_timestamp_nanos(ns)
}

/// A [`DateTime`] as unix nanoseconds. Instants outside of what an `i64` can
/// hold saturate, which keeps them ordered correctly against every stored
/// timestamp when used as a range bound.
pub fn datetime_to_ns(datetime: DateTime<Utc>) -> i64 {
    datetime.timestamp_nanos_opt().unwrap_or_else(|| {
        if datetime.timestamp() < 0 {
            i64::MIN
        } else {
            i64::MAX
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nanoseconds_round_trip() {
        let ns = 1_700_000_000_123_456_789;
        assert_eq!(datetime_to_ns(datetime_from_ns(ns)), ns);
    }

    #[test]
    fn out_of_range_instants_saturate_in_the_right_direction() {
        // Years 3000 and 1000, either side of what i64 nanoseconds can hold
        let far_future = DateTime::from_timestamp(32_503_680_000, 0).unwrap();
        let far_past = DateTime::from_timestamp(-30_610_224_000, 0).unwrap();

        assert_eq!(datetime_to_ns(far_future), i64::MAX);
        assert_eq!(datetime_to_ns(far_past), i64::MIN);
    }
}
