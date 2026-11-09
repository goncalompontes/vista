use serde::Deserialize;
use vista_host::protocol::{
    ControlMessage, Envelope, PermissionSet, ProtocolVersion, VersionRange,
};
use vista_host::transport::{Direction, RecordCipher};

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

#[derive(Deserialize)]
struct RecordVector {
    key_hex: String,
    session_id: String,
    direction: String,
    counter: u64,
    plaintext_hex: String,
    ciphertext_hex: String,
}

#[derive(Deserialize)]
struct PairingFixture {
    record: RecordVector,
}

#[test]
fn shared_media_record_vector_matches_the_swift_implementation() {
    let fixture: PairingFixture =
        serde_json::from_str(include_str!("../../docs/fixtures/pairing-v1.json")).unwrap();
    let record = fixture.record;
    assert_eq!(record.direction, "client_to_host");
    assert_eq!(record.counter, 0);
    let key: [u8; 32] = hex::decode(&record.key_hex).unwrap().try_into().unwrap();
    let mut cipher = RecordCipher::new(&key, &record.session_id, Direction::ClientToHost).unwrap();
    let sealed = cipher
        .seal(&hex::decode(&record.plaintext_hex).unwrap())
        .unwrap();
    assert_eq!(hex::encode(sealed), record.ciphertext_hex);
    assert_eq!(cipher.counter(), 1);
}
