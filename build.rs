use std::path::Path;
use std::process::Command;

fn main() {
    // Check if frontend build should be skipped (Docker pre-builds it)
    if std::env::var("CODASAURUS_SKIP_FRONTEND_BUILD").is_ok() {
        println!("cargo:info=Skipping Svelte build (CODASAURUS_SKIP_FRONTEND_BUILD=1)");
        return;
    }

    let svelte_dir = Path::new("svelte-dashboard");
    if !svelte_dir.exists() {
        println!("cargo:warning=svelte-dashboard/ not found, skipping frontend build");
        return;
    }

    // Check if node is available
    let node_check = Command::new("node").arg("--version").output();
    if node_check.is_err() {
        println!("cargo:warning=Node.js not found, skipping Svelte build. Run 'cd svelte-dashboard && npm install && npm run build' manually.");
        return;
    }

    // Install dependencies if needed
    let node_modules = svelte_dir.join("node_modules");
    if !node_modules.exists() {
        println!("cargo:info=Installing Svelte dependencies...");
        let status = Command::new("npm")
            .args(["install"])
            .current_dir(svelte_dir)
            .status()
            .expect("npm install failed");
        if !status.success() {
            panic!("npm install failed");
        }
    }

    // Build Svelte SPA
    println!("cargo:info=Building Svelte SPA...");
    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(svelte_dir)
        .status()
        .expect("npm run build failed");
    if !status.success() {
        panic!("Svelte build failed");
    }

    // Tell cargo to re-run if Svelte source or deps change
    println!("cargo:rerun-if-changed=svelte-dashboard/src/");
    println!("cargo:rerun-if-changed=svelte-dashboard/package.json");
    println!("cargo:rerun-if-changed=svelte-dashboard/package-lock.json");

    // Ensure the dist directory exists
    let dist = svelte_dir.join("dist");
    if !dist.exists() {
        panic!("Svelte build did not produce dist/ output");
    }
}
