use std::{env, path::PathBuf, process::Command};
fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    for shader in ["city.vert", "city.frag", "sky.vert", "sky.frag"] {
        let input = PathBuf::from("../../shaders").join(shader);
        println!("cargo:rerun-if-changed={}", input.display());
        let status = Command::new("glslc")
            .arg("--target-env=vulkan1.1")
            .arg("-O")
            .arg(&input)
            .arg("-o")
            .arg(out.join(format!("{shader}.spv")))
            .status()
            .expect("glslc est requis pour compiler les shaders Vulkan (paquet Fedora glslc)");
        assert!(status.success(), "Échec de compilation du shader {shader}");
    }
}
