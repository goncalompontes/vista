use serde_json::Value;

const FIXTURE: &str = include_str!("../../docs/fixtures/csr-enrollment-v1.json");

#[test]
fn enrollment_fixture_pins_the_cross_platform_identity_profile() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("valid enrollment fixture");
    assert_eq!(fixture["profile"], "vista-client-v1");
    assert_eq!(fixture["protocol"], "TLS 1.3");
    assert_eq!(fixture["key"]["algorithm"], "EC");
    assert_eq!(fixture["key"]["curve"], "P-256");
    assert_eq!(fixture["key"]["private_key_export"], false);
    assert_eq!(fixture["csr"]["format"], "PKCS#10");
    assert_eq!(fixture["csr"]["signature_required"], true);
    assert_eq!(fixture["csr"]["requested_extensions_are_untrusted"], true);
    let cert = &fixture["issued_certificate"];
    assert_eq!(cert["basic_constraints"]["ca"], false);
    assert_eq!(cert["key_usage"][0], "digitalSignature");
    assert_eq!(cert["extended_key_usage"][0], "clientAuth");
    assert_eq!(cert["subject_public_key_must_equal_csr"], true);
    assert_eq!(cert["issuer_must_be_host_ca"], true);
    assert_eq!(cert["private_key_transmitted"], false);
    let server = &fixture["server_acceptance"];
    assert_eq!(server["tls_minimum"], "1.3");
    assert_eq!(server["early_data"], false);
    assert_eq!(server["require_client_certificate"], true);
    assert_eq!(server["verify_chain"], true);
    assert_eq!(server["verify_expiry"], true);
    assert_eq!(server["verify_client_auth_eku"], true);
    assert_eq!(server["require_explicit_trust_record"], true);
    assert_eq!(server["reject_revoked_identity"], true);
}

#[test]
fn enrollment_fixture_requires_fail_closed_negative_cases() {
    let fixture: Value = serde_json::from_str(FIXTURE).expect("valid enrollment fixture");
    let cases = fixture["negative_cases"]
        .as_array()
        .expect("negative_cases array");
    let names: Vec<&str> = cases.iter().map(|case| case.as_str().unwrap()).collect();
    for required in [
        "malformed_csr",
        "unsigned_or_invalid_csr_signature",
        "wrong_curve",
        "csr_requests_ca_true",
        "csr_requests_server_auth",
        "csr_subject_key_mismatch",
        "untrusted_issuer",
        "expired_certificate",
        "missing_client_auth_eku",
        "unknown_trust_record",
        "revoked_client",
        "wrong_host_spki_pin",
        "tls_early_data",
        "public_certificate_without_private_key",
    ] {
        assert!(
            names.contains(&required),
            "missing negative case: {required}"
        );
    }
}
