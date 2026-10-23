use super::*;

fn admin(networks: &[&str]) -> AdminConfig {
    AdminConfig {
        trusted_networks: networks.iter().map(ToString::to_string).collect(),
        ..AdminConfig::default()
    }
}

#[test]
fn peers_are_admitted_by_network() {
    const CASES: &[(Option<&str>, bool)] = &[
        (Some("192.168.1.42"), true),
        (Some("10.0.9.7"), true),
        // Harbor's own tools: loopback is always added.
        (Some("127.0.0.1"), true),
        (Some("::1"), true),
        // A dual-stack listener reports IPv4 clients this way.
        (Some("::ffff:192.168.1.42"), true),
        (Some("10.0.1.1"), false),
        (Some("192.168.4.1"), false),
        // No recorded peer: refused, not waved through.
        (None, false),
    ];
    let networks = trusted_networks(&admin(&["192.168.1.0/24", "10.0.9.7"])).unwrap();
    for (peer, expected) in CASES {
        let ip = peer.map(|ip| ip.parse::<IpAddr>().unwrap());
        assert_eq!(admitted(ip, &networks), *expected, "{peer:?}");
    }
}

#[test]
fn no_networks_means_no_gate() {
    let networks = trusted_networks(&admin(&[])).unwrap();
    assert_eq!(networks, Vec::<IpNet>::new());
    assert!(admitted(None, &networks));
}

#[test]
fn a_bad_entry_is_a_startup_error() {
    let err = trusted_networks(&admin(&["192.168.1.0/33"])).unwrap_err();
    assert!(err.to_string().contains("192.168.1.0/33"), "{err}");
}
