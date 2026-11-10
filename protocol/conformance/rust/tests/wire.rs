use prost::Message;
use vista_protobuf_candidate::*;

fn fixtures() -> Vec<(String, String, Vec<u8>)> {
    include_str!("../../fixtures/golden.tsv").lines().map(|line| {
        let mut p = line.split('\t');
        (p.next().unwrap().to_owned(), p.next().unwrap().to_owned(), hex(p.next().unwrap()))
    }).collect()
}
fn hex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i+2], 16).unwrap()).collect()
}
fn roundtrip<M: Message + Default>(bytes: &[u8]) {
    let m = M::decode(bytes).unwrap();
    assert_eq!(m.encode_to_vec(), bytes);
}
#[test]
fn compiler_generated_vectors_roundtrip() {
    for (id, _, bytes) in fixtures() {
        match id.as_str() {
            "version" => roundtrip::<ProtocolVersion>(&bytes),
            "optional-zero" => roundtrip::<VistaError>(&bytes),
            "grant-approved" | "unknown-enum" | "enum-zero" => roundtrip::<PermissionGrant>(&bytes),
            "grant-revoked" => roundtrip::<PermissionUpdate>(&bytes),
            "catalog" | "action" => roundtrip::<ControlEnvelope>(&bytes),
            "picker" | "verified-binding" => roundtrip::<CaptureSelection>(&bytes),
            "enroll" => roundtrip::<BootstrapEnvelope>(&bytes),
            "certificate" => roundtrip::<PairingResult>(&bytes),
            "lease" => roundtrip::<LeaseResult>(&bytes),
            "input" => roundtrip::<InputEnvelope>(&bytes),
            "required-feature" | "additive-v1.1" => { let _: ControlHello = ControlHello::decode(bytes.as_slice()).unwrap(); },
            "missing-domain" => roundtrip::<LeaseRequest>(&bytes),
            "unknown-field" => { let x = ProtocolVersion::decode(bytes.as_slice()).unwrap(); assert_eq!(x.major, 1); },
            _ => panic!("unhandled fixture {id}"),
        }
    }
}
#[test]
fn explicit_presence_and_closed_domain_validation() {
    let lookup = |id: &str| fixtures().into_iter().find(|x| x.0 == id).unwrap().2;
    assert_eq!(VistaError::decode(lookup("optional-zero").as_slice()).unwrap().retry_after_ms, Some(0));
    let approved = PermissionGrant::decode(lookup("grant-approved").as_slice()).unwrap();
    assert_eq!(approved.epoch, 9_007_199_254_740_993);
    assert_eq!(approved.state, PermissionState::PermissionGranted as i32);
    let unknown = PermissionGrant::decode(lookup("unknown-enum").as_slice()).unwrap();
    assert_eq!(unknown.state, 123);
    assert!(PermissionState::try_from(unknown.state).is_err());
    let zero = PermissionGrant::decode(lookup("enum-zero").as_slice()).unwrap();
    assert_eq!(zero.state, 0); // A caller must reject unspecified; protobuf alone cannot.
    assert!(LeaseRequest::decode(lookup("missing-domain").as_slice()).unwrap().target.is_none());
    let future = ControlHello::decode(lookup("required-feature").as_slice()).unwrap();
    assert!(future.required_features.iter().any(|f| f.must_support && f.id == "future.privileged"));
    assert!(ProtocolVersion::decode([0x08].as_slice()).is_err());
    assert!(ProtocolVersion::decode([0xff; 11].as_slice()).is_err());
}
