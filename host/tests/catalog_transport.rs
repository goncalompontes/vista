use std::thread;
use vista_host::catalog_wire::CatalogEntry;
use vista_host::protocol::{PairingSecret, PermissionSet, unix_now};
use vista_host::transport::{CatalogSetup, HostControlServer, connect};

#[test]
fn catalog_snapshot_is_sent_after_pairing() {
    let secret = PairingSecret::generate().unwrap();
    let setup = CatalogSetup::new(
        vec![CatalogEntry {
            window_id: 5,
            title: Some("Browser".into()),
            app_id: Some("firefox".into()),
            workspace_id: Some(2),
        }],
        "niri-test",
        1,
    )
    .unwrap();
    let (server, offer) = HostControlServer::bind_with_catalog(
        "127.0.0.1:0".parse().unwrap(),
        secret.clone(),
        unix_now(),
        PermissionSet::video_only(),
        Some(setup),
    )
    .unwrap();
    let handle = thread::spawn(move || server.accept_and_run());
    let client = connect(offer.address, secret).unwrap();
    let server_stats = handle.join().unwrap().unwrap();

    assert_eq!(client.catalog_entries, 1);
    assert!(client.reached_active);
    assert!(client.pong_verified);
    assert!(server_stats.catalog_sent);
    assert_eq!(server_stats.catalog_entries, 1);
}

#[test]
fn empty_catalog_is_still_sent() {
    let secret = PairingSecret::generate().unwrap();
    let setup = CatalogSetup::new(Vec::new(), "niri-test", 1).unwrap();
    let (server, offer) = HostControlServer::bind_with_catalog(
        "127.0.0.1:0".parse().unwrap(),
        secret.clone(),
        unix_now(),
        PermissionSet::video_only(),
        Some(setup),
    )
    .unwrap();
    let handle = thread::spawn(move || server.accept_and_run());
    let client = connect(offer.address, secret).unwrap();
    let server_stats = handle.join().unwrap().unwrap();
    assert_eq!(client.catalog_entries, 0);
    assert!(server_stats.catalog_sent);
}

#[test]
fn invalid_catalog_setup_is_rejected_before_listening() {
    let duplicate = CatalogEntry {
        window_id: 5,
        title: None,
        app_id: None,
        workspace_id: None,
    };
    let error = CatalogSetup::new(vec![duplicate.clone(), duplicate], "niri-test", 1)
        .unwrap_err()
        .to_string();
    assert!(error.contains("duplicate window"), "{error}");
    assert!(CatalogSetup::new(Vec::new(), "niri-test", 0).is_err());
}
