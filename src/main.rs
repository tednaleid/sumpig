use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "sumpig",
    about = "Merkle tree directory fingerprinting and comparison"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a fingerprint manifest for a directory tree
    Fingerprint {
        /// Directory to fingerprint
        path: PathBuf,

        /// Output depth (controls manifest granularity, not hashing depth)
        #[arg(short, long, default_value = "6")]
        depth: usize,

        /// Output file (default: <path>/.sync-fingerprints/<hostname>.txt)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Worker thread count (default: number of CPU cores)
        #[arg(short, long)]
        jobs: Option<usize>,

        /// Disable default skip list (hash everything)
        #[arg(long)]
        no_skip: bool,
    },
    // Compare subcommand added in Phase 2
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Fingerprint {
            path,
            depth,
            output,
            jobs,
            no_skip,
        } => {
            if let Err(e) = run_fingerprint(&path, depth, output.as_deref(), jobs, no_skip) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn run_fingerprint(
    path: &std::path::Path,
    depth: usize,
    output: Option<&std::path::Path>,
    jobs: Option<usize>,
    no_skip: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();

    // Validate the path.
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("{}: not a directory", path.display()).into());
    }

    // Walk the directory tree.
    let walk_options = sumpig::walk::WalkOptions {
        skip_defaults: !no_skip,
        num_threads: jobs.unwrap_or(0),
    };
    let walk_entries = sumpig::walk::walk_directory(&canonical, &walk_options);

    // Hash files in parallel, collect (relative_path, FileHash) pairs.
    let hashed_entries: Vec<(PathBuf, sumpig::hash::FileHash)> = walk_entries
        .into_iter()
        .filter(|e| !e.is_dir)
        .map(|e| {
            let full_path = canonical.join(&e.path);
            let file_hash = sumpig::hash::hash_file(&full_path);
            (e.path, file_hash)
        })
        .collect();

    // Count files and dirs for the header.
    let total_files = hashed_entries.len();

    // Sort by path (should already be sorted from walk, but ensure it).
    let mut sorted_entries = hashed_entries;
    sorted_entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Compute Merkle tree and produce flat entries.
    let (flat_entries, root_hash) = sumpig::merkle::compute_manifest(&sorted_entries, depth);

    let total_dirs = flat_entries
        .iter()
        .filter(|e| e.entry_type == sumpig::merkle::EntryType::Dir)
        .count();

    // Build the manifest header.
    let header = sumpig::manifest::ManifestHeader {
        host: sumpig::manifest::get_hostname(),
        path: canonical.to_string_lossy().into_owned(),
        depth,
        date: sumpig::manifest::get_iso_date(),
        total_files,
        total_dirs,
        root_hash: sumpig::hash::hash_to_hex(&root_hash),
    };

    // Determine output path.
    let output_path = match output {
        Some(p) => p.to_path_buf(),
        None => {
            let sync_dir = canonical.join(".sync-fingerprints");
            fs::create_dir_all(&sync_dir)?;
            let hostname = sumpig::manifest::get_hostname();
            sync_dir.join(format!("{hostname}.txt"))
        }
    };

    // Write the manifest.
    let mut file = fs::File::create(&output_path)?;
    sumpig::manifest::write_manifest(&mut file, &header, &flat_entries)?;

    // Print summary to stderr.
    let elapsed = start.elapsed();
    eprintln!(
        "{} files, {} dirs in {:.2}s | root: {} | {}",
        total_files,
        total_dirs,
        elapsed.as_secs_f64(),
        sumpig::hash::hash_to_hex(&root_hash),
        output_path.display(),
    );

    Ok(())
}
