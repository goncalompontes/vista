use serde::Deserialize;
use vista_host::protocol::{
    ControlMessage, Envelope, PermissionSet, ProtocolVersion, VersionRange,
};

#[derive(Deserialize)]
struct Fixtures {
    protocol: String,
    client_frames: Vec<Envelope>,
    host_frames: Vec<Envelope>,
}

#[test]
fn swift_compatible_fixtures_decode_in_rust() {
    let fixtures: Fixtures =
        serde_json::from_str(include_str!("../../docs/fixtures/protocol-v1.json")).unwrap();
    assert_eq!(fixtures.protocol, "1.0");

    assert!(matches!(
        &fixtures.client_frames[0].body,
        ControlMessage::ClientHello { versions, .. } if *versions == VersionRange::current()
    ));
    assert!(matches!(
        &fixtures.host_frames[0].body,
        ControlMessage::ServerHello { selected, policy, .. }
            if *selected == ProtocolVersion { major: 1, minor: 0 }
                && *policy == PermissionSet::video_only()
    ));
    assert!(matches!(
        &fixtures.client_frames[2].body,
        ControlMessage::SessionOpen { permissions, .. } if permissions.video && !permissions.input
    ));
    assert!(matches!(
        &fixtures.host_frames[3].body,
        ControlMessage::Pong { nonce: 42 }
    ));

    for envelope in fixtures
        .client_frames
        .iter()
        .chain(fixtures.host_frames.iter())
    {
        let bytes = envelope.encode().unwrap();
        assert_eq!(&Envelope::decode(&bytes).unwrap(), envelope);
    }
}
