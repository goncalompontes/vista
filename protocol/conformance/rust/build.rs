fn main() {
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let generated = std::path::PathBuf::from("../generated/rust");
    if std::env::var_os("VISTA_PROTO_REGENERATE").is_some() {
        std::fs::create_dir_all(&generated).unwrap();
        prost_build::Config::new()
            .out_dir(&generated)
            .compile_protos(&["../../../proto/vista/v1/vista.proto"], &["../../../proto"])
            .unwrap();
    }
    std::fs::copy(generated.join("vista.v1.rs"), output.join("vista.v1.rs")).unwrap();
    println!("cargo:rerun-if-env-changed=VISTA_PROTO_REGENERATE");
    println!("cargo:rerun-if-env-changed=PROTOC");
    println!("cargo:rerun-if-changed=../../../proto/vista/v1/vista.proto");
    println!("cargo:rerun-if-changed=../generated/rust/vista.v1.rs");
}
