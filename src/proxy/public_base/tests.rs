use rama::http::HeaderValue;

use super::*;

fn server() -> ServerConfig {
    ServerConfig {
        host: "127.0.0.1".to_string(),
        port: 8346,
        workers: 1,
    }
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.insert(*name, HeaderValue::from_str(value).unwrap());
    }
    map
}

#[test]
fn the_client_facing_authority_wins_over_the_bind() {
    const CASES: &[(&[(&str, &str)], &str)] = &[
        // Behind mothership: the Host the client sent.
        (&[("host", "192.168.1.20:8080")], "http://192.168.1.20:8080"),
        // A TLS-terminating proxy that says so.
        (
            &[
                ("host", "127.0.0.1:8346"),
                ("x-forwarded-proto", "https"),
                ("x-forwarded-host", "gems.example.com"),
            ],
            "https://gems.example.com",
        ),
        // Chained proxies: the first (client-facing) entry.
        (
            &[
                ("host", "vein.internal"),
                ("x-forwarded-proto", "HTTPS, http"),
            ],
            "https://vein.internal",
        ),
        (&[("host", "[::1]:8346")], "http://[::1]:8346"),
        // No Host at all: the bind address.
        (&[], "http://127.0.0.1:8346"),
        // Anything that could break out of HTML or JSON is ignored.
        (
            &[("host", "evil.com\"><script>"), ("x-forwarded-host", "a b")],
            "http://127.0.0.1:8346",
        ),
        (
            &[("host", "ok.example"), ("x-forwarded-host", "x/../y")],
            "http://ok.example",
        ),
    ];
    for (pairs, expected) in CASES {
        assert_eq!(
            public_base(&headers(pairs), &server()),
            *expected,
            "{pairs:?}"
        );
    }
}
