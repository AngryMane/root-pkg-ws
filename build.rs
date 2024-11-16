use std::process::Command;

fn main() {
    let cargo_version_output = Command::new("cargo")
        .arg("--version")
        .output()
        .expect("Failed to execute cargo");

    let cargo_version = String::from_utf8(cargo_version_output.stdout)
        .expect("Invalid UTF-8 sequence in cargo version output");

    let version_number = cargo_version.split_whitespace().nth(1).unwrap_or("0.0.0");

    let version_parts: Vec<&str> = version_number.split('.').collect();
    let major: u32 = version_parts.get(0).unwrap_or(&"0").parse().unwrap_or(0);
    let minor: u32 = version_parts.get(1).unwrap_or(&"0").parse().unwrap_or(0);

    if (major > REQUIRED_MAJOR_VERSION)
        || (major == REQUIRED_MAJOR_VERSION && minor >= REQUIRED_MINOR_VERSION)
    {
        println!("cargo:rustc-cfg=feature=\"cargo_util_schemas\"");
    }
}

// if 1.77 or newer, use cargo_util_schemas.
// See https://github.com/rust-lang/cargo/pull/12914 for more detail.
const REQUIRED_MAJOR_VERSION: u32 = 1; // Replace with the required major version
const REQUIRED_MINOR_VERSION: u32 = 77; // Replace with the required minor version
