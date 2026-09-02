//! Series-statistics support: the `ComicBookSeriesStatistics` aggregate
//! values the `SmartListSeries*` matchers read, and the stats provider
//! that computes them per series (C# `ComicBookSeriesStatistics.Create`).
//!
//! The statistics engine lands with matcher evaluation; this module
//! defines the value kind so the matcher spec table can refer to it.

/// One aggregate value of `ComicBookSeriesStatistics`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatKind {
    Count,
    PageCount,
    PageReadCount,
    ReadPercentage,
    AverageRating,
    AverageCommunityRating,
    FirstNumber,
    LastNumber,
    FirstYear,
    LastYear,
    RunningTimeYears,
    MaxGapSize,
    GapCount,
    /// `YesNo` via `IsGapStart(book)`.
    GapStart,
    /// `YesNo` via `IsGapEnd(book)`.
    GapEnd,
    AllComplete,
    LastOpenedTime,
    LastAddedTime,
    LastPublishedTime,
    LastReleasedTime,
    MaxCount,
    MinCount,
}
