use super::*;

fn at(rfc3339: &str) -> Timestamp {
    rfc3339.parse().unwrap()
}

#[test]
fn the_default_schedule_fires_at_minute_five_of_every_hour() {
    let cron = parse_schedule(DEFAULT_PROMOTION_SCHEDULE).unwrap();
    for (now, wait) in [
        ("2026-10-23T09:04:30Z", Duration::from_secs(30)),
        // Exactly on time: that run is happening, the next is an hour away.
        ("2026-10-23T09:05:00Z", Duration::from_hours(1)),
        ("2026-10-23T09:06:00Z", Duration::from_mins(59)),
        // Across midnight, in UTC whatever the host's zone.
        ("2026-10-23T23:59:00Z", Duration::from_mins(6)),
    ] {
        assert_eq!(until_next(&cron, at(now)), Some(wait), "{now}");
    }
}

#[test]
fn five_field_schedules_work_and_bad_ones_are_refused() {
    let cron = parse_schedule("30 2 * * *").unwrap();
    assert_eq!(
        until_next(&cron, at("2026-10-23T02:00:00Z")),
        Some(Duration::from_mins(30))
    );

    let err = parse_schedule("every hour").unwrap_err();
    assert!(
        format!("{err:#}").contains("`every hour`"),
        "names the schedule: {err:#}"
    );
}
