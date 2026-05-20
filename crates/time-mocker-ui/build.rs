//! Embeds a Windows UAC manifest so the controller prompts for admin
//! elevation on launch (required for `CreateRemoteThread` into other users'
//! processes and for hooking system DLLs).

fn main() {
    // Only embed the UAC manifest in release builds — otherwise `cargo test`
    // and other dev workflows would fail with ERROR_ELEVATION_REQUIRED (740).
    println!("cargo:rerun-if-changed=build.rs");
    let profile = std::env::var("PROFILE").unwrap_or_default();
    if profile != "release" {
        return;
    }
    #[cfg(windows)]
    {
        use embed_manifest::manifest::ExecutionLevel;
        use embed_manifest::{embed_manifest, new_manifest};
        let manifest = new_manifest("TimeMocker.UI")
            .requested_execution_level(ExecutionLevel::RequireAdministrator);
        embed_manifest(manifest).expect("failed to embed UAC manifest");
    }
}
