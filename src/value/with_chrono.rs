use super::impl_timestamp;
use crate::{DbErr, TryGetError, prelude::ChronoDateTimeUtc};
use std::ops::{Deref, DerefMut};

/// A `DateTime<Utc>` stored as a Unix timestamp (seconds) `i64` in the database.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct ChronoUnixTimestamp(pub ChronoDateTimeUtc);

/// A `DateTime<Utc>` stored as a Unix timestamp in milliseconds (`i64`) in the database.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct ChronoUnixTimestampMillis(pub ChronoDateTimeUtc);

// Both print as the timestamp they hold.
impl std::fmt::Debug for ChronoUnixTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl std::fmt::Debug for ChronoUnixTimestampMillis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, f)
    }
}

impl_timestamp!(
    ChronoUnixTimestamp,
    ChronoDateTimeUtc,
    from_timestamp,
    to_timestamp
);

impl_timestamp!(
    ChronoUnixTimestampMillis,
    ChronoDateTimeUtc,
    from_timestamp_millis,
    to_timestamp_millis
);

fn from_timestamp(ts: i64) -> Option<ChronoUnixTimestamp> {
    ChronoDateTimeUtc::from_timestamp(ts, 0).map(ChronoUnixTimestamp)
}

fn to_timestamp(ts: ChronoUnixTimestamp) -> i64 {
    ts.0.timestamp()
}

fn from_timestamp_millis(ts: i64) -> Option<ChronoUnixTimestampMillis> {
    ChronoDateTimeUtc::from_timestamp_millis(ts).map(ChronoUnixTimestampMillis)
}

fn to_timestamp_millis(ts: ChronoUnixTimestampMillis) -> i64 {
    ts.0.timestamp_millis()
}
