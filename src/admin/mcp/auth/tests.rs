use super::*;

fn subject(callers: &Callers, authorization: Option<&str>) -> Option<String> {
    callers
        .identify(authorization)
        .ok()
        .map(|principal| principal.subject)
}

/// Without a read token, readers come in anonymously; the write token makes
/// the admin; any other credential is refused.
#[test]
fn the_write_token_makes_the_admin() {
    let callers = Callers::new(None, "w-token".into());
    assert_eq!(
        subject(&callers, Some("Bearer w-token")).as_deref(),
        Some(ADMIN)
    );
    assert_eq!(subject(&callers, None).as_deref(), Some("anonymous"));
    assert_eq!(subject(&callers, Some("Bearer w-tokeN")), None);
    assert_eq!(subject(&callers, Some("Bearer w-token-and-more")), None);
    assert_eq!(subject(&callers, Some("Basic w-token")), None);
}

/// With a read token set, every caller still needs a token: reads keep
/// the protection they had before write tools existed.
#[test]
fn a_read_token_still_guards_reads() {
    let callers = Callers::new(Some("r-token".into()), "w-token".into());
    assert_eq!(
        subject(&callers, Some("Bearer r-token")).as_deref(),
        Some("agent")
    );
    assert_eq!(
        subject(&callers, Some("Bearer w-token")).as_deref(),
        Some(ADMIN)
    );
    assert_eq!(subject(&callers, None), None);
}

#[test]
fn debug_output_never_shows_the_tokens() {
    let callers = Callers::new(Some("r-token".into()), "w-token".into());
    let shown = format!("{callers:?}");
    assert!(
        !shown.contains("r-token") && !shown.contains("w-token"),
        "{shown}"
    );
}
