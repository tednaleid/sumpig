use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;

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

        /// Suppress progress bars and summary output
        #[arg(short, long)]
        quiet: bool,
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
            quiet,
        } => {
            if let Err(e) = run_fingerprint(&path, depth, output.as_deref(), jobs, no_skip, quiet)
            {
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
    quiet: bool,
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
    let spinner = if !quiet {
        let sp = ProgressBar::new_spinner();
        sp.set_style(
            ProgressStyle::with_template("  {spinner} Scanning...").unwrap(),
        );
        sp.enable_steady_tick(Duration::from_millis(120));
        Some(sp)
    } else {
        None
    };

    let walk_options = sumpig::walk::WalkOptions {
        skip_defaults: !no_skip,
        num_threads: jobs.unwrap_or(0),
    };
    let walk_entries = sumpig::walk::walk_directory(&canonical, &walk_options);

    if let Some(sp) = &spinner {
        sp.finish_and_clear();
    }

    // Separate files from directories for hashing.
    let files_to_hash: Vec<sumpig::walk::WalkEntry> = walk_entries
        .into_iter()
        .filter(|e| !e.is_dir)
        .collect();
    let file_count = files_to_hash.len();

    // Hash files in parallel with progress bar.
    let pb = if !quiet {
        let pb = ProgressBar::new(file_count as u64);
        pb.set_style(
            ProgressStyle::with_template(
                "  Hashing  [{bar:30}] {pos}/{len}  {percent}%  {eta} remaining",
            )
            .unwrap()
            .progress_chars("##-"),
        );
        Some(pb)
    } else {
        None
    };

    let hashed_entries: Vec<(PathBuf, sumpig::hash::FileHash)> = files_to_hash
        .into_par_iter()
        .map(|e| {
            let full_path = canonical.join(&e.path);
            let file_hash = sumpig::hash::hash_file(&full_path);
            if let Some(pb) = &pb {
                pb.inc(1);
            }
            (e.path, file_hash)
        })
        .collect();

    if let Some(pb) = &pb {
        pb.finish_and_clear();
    }

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
        total_files: file_count,
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
    if !quiet {
        let elapsed = start.elapsed();
        eprintln!(
            "{} files, {} dirs in {:.2}s | root: {} | {}",
            file_count,
            total_dirs,
            elapsed.as_secs_f64(),
            sumpig::hash::hash_to_hex(&root_hash),
            output_path.display(),
        );
    }

    Ok(())
}
