use std::process::Command;

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_owned())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUST_AGISERVER_BUILD_DATE");
    println!("cargo:rerun-if-env-changed=RUST_AGISERVER_GIT_HASH");

    let build_date = std::env::var("RUST_AGISERVER_BUILD_DATE")
        .ok()
        .or_else(|| command_output("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]))
        .unwrap_or_else(|| "unknown".to_owned());
    let git_hash = std::env::var("RUST_AGISERVER_GIT_HASH")
        .ok()
        .or_else(|| command_output("git", &["rev-parse", "--short", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());

    println!("cargo:rustc-env=RUST_AGISERVER_BUILD_DATE={build_date}");
    println!("cargo:rustc-env=RUST_AGISERVER_GIT_HASH={git_hash}");
}
