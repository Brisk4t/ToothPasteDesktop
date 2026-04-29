fn main() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let proto_root = manifest_dir.join("ToothPaste/shared");

    println!(
        "cargo:rerun-if-changed={}",
        proto_root.join("toothpacket.proto").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        proto_root.join("toothpacket.options").display()
    );

    prost_build::compile_protos(&[proto_root.join("toothpacket.proto")], &[&proto_root]).unwrap();
}
