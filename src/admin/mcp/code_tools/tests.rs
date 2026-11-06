use super::*;

#[test]
fn listing_backoff_doubles_from_first_retry_up_to_the_cap() {
    let mut backoff = listing_backoff();
    let ms = |wait: Duration| u64::try_from(wait.as_millis()).unwrap();

    // 20% jitter: each wait lands between 80% and 100% of its step.
    let first = backoff.failed().unwrap();
    assert!((ms(FIRST_RETRY) * 8 / 10..=ms(FIRST_RETRY)).contains(&first));
    let second = backoff.failed().unwrap();
    assert!((ms(FIRST_RETRY) * 16 / 10..=ms(FIRST_RETRY) * 2).contains(&second));

    // A provider down for hours is still retried, never more than MAX_RETRY apart.
    for _ in 0..300 {
        let wait = backoff.failed().expect("listing never gives up");
        assert!(wait <= ms(MAX_RETRY));
    }

    backoff.succeeded();
    assert!(backoff.failed().unwrap() <= ms(FIRST_RETRY));
}
