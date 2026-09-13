use std::process::Command;

fn main() {
    let web_dir = std::path::Path::new("web");
    let dist_dir = web_dir.join("dist");

    // Allow skipping frontend build during development
    if std::env::var("SKIP_WEB_BUILD").is_ok() {
        // Create a minimal dist if it doesn't exist so rust-embed doesn't fail
        if !dist_dir.exists() {
            std::fs::create_dir_all(&dist_dir).unwrap();
            std::fs::write(
                dist_dir.join("index.html"),
                "<html><body><h1>Run with SKIP_WEB_BUILD unset to build frontend</h1></body></html>",
            )
            .unwrap();
        }
        return;
    }

    if !web_dir.exists() {
        println!("cargo:warning=web/ directory not found, skipping frontend build");
        if !dist_dir.exists() {
            std::fs::create_dir_all(&dist_dir).unwrap();
            std::fs::write(dist_dir.join("index.html"), "<html><body>No frontend</body></html>").unwrap();
        }
        return;
    }

    // Install deps if needed
    if !web_dir.join("node_modules").exists() {
        println!("cargo:warning=Installing frontend dependencies...");
        let status = Command::new("pnpm")
            .arg("install")
            .current_dir(web_dir)
            .status()
            .expect("Failed to run pnpm install. Is pnpm installed?");
        assert!(status.success(), "pnpm install failed");
    }

    // Build frontend
    println!("cargo:warning=Building frontend...");
    let status = Command::new("pnpm")
        .arg("run")
        .arg("build")
        .current_dir(web_dir)
        .status()
        .expect("Failed to run pnpm build");
    assert!(status.success(), "pnpm build failed");

    // Tell cargo to re-run if frontend sources change
    println!("cargo:rerun-if-changed=web/src/");
    println!("cargo:rerun-if-changed=web/index.html");
    println!("cargo:rerun-if-changed=web/package.json");
}
