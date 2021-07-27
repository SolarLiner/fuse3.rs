use std::path::PathBuf;

const WRAPPERS: [(&'static str, &'static str); 2] = [("wrapper.h", "bindings.rs"), ("wrapper_low.h", "bindings_low.rs")];

fn main() {
    let lib = pkg_config::Config::new()
        .atleast_version("0.29.0")
        .probe("fuse3")
        .expect("Couldn't find library fuse3 on the system");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("Unreachable: No OUT_DIR"));
    WRAPPERS.iter().for_each(|(input,  output)| {
        bindgen::builder()
            .header(*input)
            .clang_args(
                lib.include_paths
                    .iter()
                    .map(|f| format!("-I{}", f.display())),
            )
            .clang_args(lib.defines.iter().map(|(name, value)| match value {
                Some(v) => format!("-D{}={}", name, v),
                None => format!("-D{}", name),
            }))
            .clang_arg("-DFUSE_USE_VERSION=31")
            .derive_copy(true)
            .derive_debug(true)
            .derive_default(true)
            .generate_comments(true)
            .generate_block(true)
            .generate()
            .expect("Couldn't generate bindings to fuse3")
            .write_to_file(out_dir.join(*output))
            .expect("Couldn't write bindings")
    });
    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rustc-link-lib=fuse3");
}
