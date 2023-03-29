/// Set the environment variables `GIT_HASH` AND `CARGO_PKG_VERSION` to include
/// the current git revision.
pub fn git_hash(orig_version: &str) -> Result<(), Box<(dyn std::error::Error)>> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    let git_hash = String::from_utf8(output.stdout)?;
    println!("cargo:rustc-env=GIT_HASH={git_hash}");
    let version = format!("{orig_version}+{git_hash}");
    println!("cargo:rustc-env=CARGO_PKG_VERSION={version}"); // override default
    Ok(())
}

pub fn bui_backend_generate_code<P>(
    files_dir: P,
    generated_path: &str,
) -> Result<(), Box<(dyn std::error::Error)>>
where
    P: AsRef<std::path::Path>,
{
    match bui_backend_codegen::codegen(&files_dir, generated_path) {
        Ok(()) => Ok(()),
        Err(e) => Err(format!(
            "Error in the process of generating '{generated_path}' when attempting to read {} : {e}",
            files_dir.as_ref().display()
        )
        .into()),
    }
}
