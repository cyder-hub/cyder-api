use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use anyhow::{bail, Context, Result};

fn main() -> Result<()> {
    let mut args = pico_args::Arguments::from_env();

    // Help flags
    if args.contains(["-h", "--help"]) {
        print_help();
        return Ok(());
    }

    let subcommand = args.subcommand()?.unwrap_or_else(|| "default".into());

    // Ensure all remaining arguments are processed or fail if unexpected ones are found
    let remaining = args.finish();
    if !remaining.is_empty() {
        eprintln!("Error: Unexpected arguments: {:?}", remaining);
        print_help();
        std::process::exit(1); // Use exit code 1 for errors
    }

    // Set working directory to project root before running commands that need it
    let root = project_root();
    println!("Running xtask in: {}", root.display());
    // Note: Setting global CWD might not be ideal if commands need different CWDs.
    // We'll handle CWD within specific run_* functions where needed.
    // env::set_current_dir(&root).context("Failed to set working directory to project root")?;

    match subcommand.as_str() {
        "dev" => cmd_dev()?,
        "build" => cmd_build()?,
        "dev-backend" => cmd_dev_backend()?,
        "build-backend" => cmd_build_backend()?,
        "dev-front" => cmd_dev_front()?,
        "build-front" => cmd_build_front()?,
        "install-front-deps" => cmd_install_front_deps()?, // Add this line
        "default" => {
            // Optional: Define a default behavior, e.g., print help or run combined dev
            println!("Default task: Running combined dev server (backend + frontend).");
            cmd_dev()?;
        }
        _ => {
            eprintln!("Error: Unknown command '{}'", subcommand);
            print_help();
            std::process::exit(1); // Use exit code 1 for errors
        }
    }

    Ok(())
}

fn print_help() {
    println!(
        r#"
Usage: cargo xtask <COMMAND>

Commands:
  dev             Runs the backend and frontend development servers concurrently.
  build           Builds the backend and frontend projects in release mode.
  dev-backend     Runs the backend development server using 'cargo run'.
  build-backend        Builds the backend project in release mode using 'cargo build --release'.
  dev-front            Installs deps and runs the frontend development server using 'npm run dev' in './front'.
  build-front          Installs deps and builds the frontend project using 'npm run build' in './front'.
  install-front-deps   Installs frontend dependencies using 'npm install' in './front'.
  default              Runs the 'dev' command.
"#
    );
}


// New combined dev command
fn cmd_dev() -> Result<()> {
    println!("🚀 Starting backend and frontend development servers...");

    let backend_handle = thread::spawn(|| {
        println!("▶️ Starting backend dev server...");
        // Call the dedicated backend dev function
        if let Err(e) = cmd_dev_backend() { // Change this line
             eprintln!("Backend dev server failed: {}", e);
        }
    });

    let frontend_handle = thread::spawn(|| {
        println!("▶️ Starting frontend dev server (will install deps if needed)..."); // Update print statement slightly
         // Call the dedicated frontend dev function (which includes dep install)
        if let Err(e) = cmd_dev_front() { // Change this line
             eprintln!("Frontend dev server failed: {}", e);
        }
    });

    // Wait for both threads to complete.
    // Note: Dev servers usually run indefinitely, so join() might block forever
    // unless the servers exit or error out. This setup assumes you'll manually
    // stop the combined process (Ctrl+C), which should terminate the child processes.
    let backend_res = backend_handle.join();
    let frontend_res = frontend_handle.join();

    if backend_res.is_err() {
        eprintln!("Error joining backend thread.");
    }
     if frontend_res.is_err() {
        eprintln!("Error joining frontend thread.");
    }

    // Check if either thread panicked or the underlying command failed (if error handling inside thread was more robust)
    // Depending on the exact behavior desired, you might want to bail out here.
    // For now, just print that they were launched.
    println!("✅ Development servers launched (running concurrently). Press Ctrl+C to stop.");

    Ok(())
}


// New combined build command
fn cmd_build() -> Result<()> {
    println!("🏗️ Building backend and frontend projects...");

    // Build backend first
    cmd_build_backend()?;

    // Then build frontend
    cmd_build_front()?;

    println!("✅ Combined build complete.");
    Ok(())
}

// Renamed from cmd_dev
fn cmd_dev_backend() -> Result<()> {
    println!("🚀 Starting backend development server...");
    let server_dir = project_root().join("server");
    // Run 'cargo run' within the server directory
    run_cargo("run", &[], &server_dir)?; // Remove -p flag, update directory
    Ok(())
}

// Renamed from cmd_build
fn cmd_build_backend() -> Result<()> {
    println!("🏗️ Building backend project in release mode...");
    let server_dir = project_root().join("server");
    // Run 'cargo build --release' within the server directory
    run_cargo("build", &["--release"], &server_dir)?; // Remove -p flag, update directory
    println!("✅ Backend build complete.");
    Ok(())
}

fn cmd_dev_front() -> Result<()> {
    // Install dependencies first
    cmd_install_front_deps()?; // Add this line

    println!("🚀 Starting frontend development server...");
    run_npm("run", &["dev"], &project_root().join("front"))?;
    Ok(())
}

fn cmd_build_front() -> Result<()> {
    // Install dependencies first
    cmd_install_front_deps()?; // Add this line

    println!("🏗️ Building frontend project...");
    let front_dir = project_root().join("front");
    // Remove the following two lines:
    // println!("▶️ Running: npm install (in ./front)");
    // run_npm("install", &[], &front_dir)?;
    println!("▶️ Running: npm run build (in ./front)");
    run_npm("run", &["build"], &front_dir)?;
    println!("✅ Frontend build complete.");
    Ok(())
}

fn cmd_install_front_deps() -> Result<()> {
    println!("📦 Installing frontend dependencies...");
    let front_dir = project_root().join("front");
    run_npm("install", &[], &front_dir)?;
    println!("✅ Frontend dependencies installed.");
    Ok(())
}

fn project_root() -> PathBuf {
    // Assumes xtask is directly inside the workspace root
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}

fn run_cargo(command: &str, args: &[&str], directory: &Path) -> Result<ExitStatus> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(cargo);
    cmd.arg(command);
    cmd.args(args);
    cmd.current_dir(directory); // Set the working directory

    println!("▶️ Running: {:?} in {:?}", cmd, directory.display());

    // Inherit stdio to see output/errors directly, useful for dev servers
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let status = cmd.status().with_context(|| format!("Failed to execute: {:?} in {:?}", cmd, directory.display()))?;

    if !status.success() {
        bail!("Command `{:?}` failed with status {}", cmd, status);
    }
    Ok(status)
}

fn run_npm(npm_command: &str, args: &[&str], directory: &Path) -> Result<ExitStatus> {
    let npm_executable = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let mut cmd = Command::new(npm_executable);
    cmd.arg(npm_command);
    cmd.args(args);
    cmd.current_dir(directory); // Set the working directory

    println!("▶️ Running: {:?} in {:?}", cmd, directory.display());

    // Inherit stdio to see output/errors directly
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let status = cmd.status().with_context(|| format!("Failed to execute: {:?} in {:?}", cmd, directory.display()))?;

    if !status.success() {
        bail!("npm command `{:?}` failed with status {}", cmd, status);
    }

    Ok(status)
}
