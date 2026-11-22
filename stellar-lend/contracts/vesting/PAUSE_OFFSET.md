# Per-Grant Pause Offsets

The contract's cumulative pause duration is applied only to pauses that overlap
a grant's lifetime. Each grant's pause snapshot is recorded separately from the
`Grant` record, keyed by grantee and grant index.

For a grant created with snapshot `S`, schedule time at wall-clock timestamp `t`
is:

`t - (cumulative_paused_at(t) - S)`

`cumulative_paused_at(t)` includes the elapsed portion of the current pause, so
schedule time remains fixed throughout an ongoing pause. Subtracting `S` also
means pauses completed before a grant was created do not delay it.

## Worked example

An existing grant is created at day 0. The contract pauses at day 10 and resumes
at day 30, a 20-day pause. At day 70, the existing grant's effective schedule
time is day 50, so a 100-day linear grant has vested 50%. If a second grant is
created at day 30 after resuming, its snapshot includes the completed 20-day
pause. At day 70 its effective schedule time is day 70, so it has vested 40%
(40 of its 100 days). The earlier pause does not delay this later cohort.

During a pause, an early grant created before the pause has the same effective
schedule time at every timestamp in the paused interval. A grant created during
the pause snapshots the elapsed paused time on creation; it remains frozen at
its creation-time schedule position until resume.

## Milestone schedules

Milestones remain stored as absolute timestamps. The same per-grant
pause-adjusted schedule time is compared with those timestamps, so a milestone
on an existing grant is delayed by pauses overlapping that grant. Pauses before
a newly created milestone grant do not affect it.

## Storage compatibility

`Grant`'s XDR layout is unchanged. Pause snapshots use the additional persistent
key `GrantPauseSnapshots(grantee)`, a vector aligned with the grantee's grant
vector. This avoids making old `Grant` records unreadable. When the sidecar key
or an individual index is absent, its snapshot defaults to zero. Such legacy
grants therefore retain the previous global-offset behavior for pauses that
occurred before this upgrade; newly created grants receive the current
cumulative pause total. When old grants are followed by new grants, missing
legacy indices are initialized to zero.

## Pause state and edge cases

`TotalPausedSecs` accumulates completed pause intervals. While paused,
`PausedAt` supplies the current interval for schedule-time calculations.
`pause` while already paused and `resume` while not paused remain idempotent.
Zero-length pauses add no time. Claim and revoke remain unavailable while
paused.
