use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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

        /// Output file (default: <path>/.sumpig-fingerprints/<hostname>.txt)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Worker thread count (default: number of CPU cores)
        #[arg(short, long)]
        jobs: Option<usize>,

        /// Disable default ignore list (hash everything)
        #[arg(long)]
        no_ignore: bool,

        /// Suppress progress bars and summary output
        #[arg(short, long)]
        quiet: bool,

        /// Hash file contents instead of metadata (slower, detects silent corruption)
        #[arg(short = 'C', long)]
        verify_contents: bool,

        /// Tag the output file with a name (or timestamp if no name given)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        tag: Option<String>,
    },
    /// Compare two fingerprint manifests and report differences
    Compare {
        /// First fingerprint file
        file1: PathBuf,
        /// Second fingerprint file
        file2: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Fingerprint {
            path,
            depth,
            output,
            jobs,
            no_ignore,
            quiet,
            verify_contents,
            tag,
        } => {
            if let Err(e) = run_fingerprint(&FingerprintOptions {
                path: &path,
                depth,
                output: output.as_deref(),
                jobs,
                no_ignore,
                quiet,
                verify_contents,
                tag: tag.as_deref(),
            }) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Compare { file1, file2 } => match run_compare(&file1, &file2) {
            Ok(identical) => {
                if !identical {
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(2);
            }
        },
    }
}

fn run_compare(
    file1: &std::path::Path,
    file2: &std::path::Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let reader1 = std::io::BufReader::new(fs::File::open(file1)?);
    let reader2 = std::io::BufReader::new(fs::File::open(file2)?);

    let (header1, entries1) = sumpig::manifest::parse_manifest(reader1)
        .map_err(|e| format!("{}: {e}", file1.display()))?;
    let (header2, entries2) = sumpig::manifest::parse_manifest(reader2)
        .map_err(|e| format!("{}: {e}", file2.display()))?;

    // Warn about mismatches that might confuse the user.
    if header1.depth != header2.depth {
        eprintln!(
            "warning: depth mismatch ({} vs {}), comparing available entries",
            header1.depth, header2.depth,
        );
    }
    if header1.path != header2.path {
        eprintln!(
            "warning: different root paths ({} vs {})",
            header1.path, header2.path,
        );
    }
    if header1.mode != header2.mode {
        eprintln!(
            "warning: mode mismatch ({} vs {}), results may not be meaningful",
            header1.mode, header2.mode,
        );
    }

    let label1 = format!("{} ({})", header1.host, header1.date);
    let label2 = format!("{} ({})", header2.host, header2.date);

    let result = sumpig::compare::compare_manifests(&entries1, &entries2, &label1, &label2);

    let report = sumpig::compare::format_report(&result);
    print!("{report}");

    // Print warnings to stderr.
    if !result.dataless_warnings.is_empty() || !result.error_warnings.is_empty() {
        let warn_count = result.dataless_warnings.len() + result.error_warnings.len();
        eprintln!("{warn_count} entries could not be fully verified (dataless/error)");
    }

    Ok(result.identical)
}

struct FingerprintOptions<'a> {
    path: &'a std::path::Path,
    depth: usize,
    output: Option<&'a std::path::Path>,
    jobs: Option<usize>,
    no_ignore: bool,
    quiet: bool,
    verify_contents: bool,
    tag: Option<&'a str>,
}

fn run_fingerprint(opts: &FingerprintOptions) -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();

    // Validate the path.
    let canonical = opts
        .path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", opts.path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("{}: not a directory", opts.path.display()).into());
    }

    // Walk the directory tree.
    let spinner = if !opts.quiet {
        let sp = ProgressBar::new_spinner();
        sp.set_style(ProgressStyle::with_template("  {spinner} Scanning...").unwrap());
        sp.enable_steady_tick(Duration::from_millis(120));
        Some(sp)
    } else {
        None
    };

    let walk_options = sumpig::walk::WalkOptions {
        use_default_ignores: !opts.no_ignore,
        num_threads: opts.jobs.unwrap_or(0),
    };
    let walk_result = sumpig::walk::walk_directory(&canonical, &walk_options);

    if let Some(sp) = &spinner {
        sp.finish_and_clear();
    }

    // Convert walk errors to FileHash::Error entries so they appear in the manifest.
    let walk_error_entries: Vec<(PathBuf, sumpig::hash::FileHash)> = walk_result
        .errors
        .into_iter()
        .map(|e| (e.path, sumpig::hash::FileHash::Error(e.reason)))
        .collect();

    // Separate files from directories for hashing.
    let files_to_hash: Vec<sumpig::walk::WalkEntry> = walk_result
        .entries
        .into_iter()
        .filter(|e| !e.is_dir)
        .collect();
    let file_count = files_to_hash.len();

    // Hash files in parallel with progress bar.
    let pb = if !opts.quiet {
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

    let total_bytes = AtomicU64::new(0);
    let hashed_entries: Vec<(PathBuf, sumpig::hash::FileHash)> = files_to_hash
        .into_par_iter()
        .map(|e| {
            let full_path = canonical.join(&e.path);
            let (file_hash, size) = if opts.verify_contents {
                sumpig::hash::hash_file(&full_path)
            } else {
                sumpig::hash::hash_file_metadata(&full_path)
            };
            total_bytes.fetch_add(size, Ordering::Relaxed);
            if let Some(pb) = &pb {
                pb.inc(1);
            }
            (e.path, file_hash)
        })
        .collect();
    let total_bytes = total_bytes.into_inner();

    if let Some(pb) = &pb {
        pb.finish_and_clear();
    }

    // Merge walk errors into hashed entries, then sort.
    let mut sorted_entries = hashed_entries;
    sorted_entries.extend(walk_error_entries);
    sorted_entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Compute Merkle tree and produce flat entries.
    let (flat_entries, root_hash) = sumpig::merkle::compute_manifest(&sorted_entries, opts.depth);

    let total_dirs = flat_entries
        .iter()
        .filter(|e| e.entry_type == sumpig::merkle::EntryType::Dir)
        .count();

    // Build the manifest header.
    let header = sumpig::manifest::ManifestHeader {
        host: sumpig::manifest::get_hostname(),
        path: canonical.to_string_lossy().into_owned(),
        depth: opts.depth,
        date: sumpig::manifest::get_iso_date(),
        total_files: file_count,
        total_dirs,
        total_bytes,
        root_hash: sumpig::hash::hash_to_hex(&root_hash),
        mode: if opts.verify_contents {
            "content"
        } else {
            "fast"
        }
        .to_string(),
    };

    // Determine output path.
    let output_path = match opts.output {
        Some(p) => p.to_path_buf(),
        None => {
            let sync_dir = canonical.join(".sumpig-fingerprints");
            fs::create_dir_all(&sync_dir)?;
            let hostname = sumpig::manifest::get_hostname();
            let filename = match opts.tag {
                Some("") => {
                    // --tag with no value: use timestamp.
                    let ts = header.date.replace(':', "-");
                    format!("{hostname}-{ts}.txt")
                }
                Some(name) => format!("{hostname}-{name}.txt"),
                None => format!("{hostname}.txt"),
            };
            sync_dir.join(filename)
        }
    };

    // Write the manifest.
    let mut file = fs::File::create(&output_path)?;
    sumpig::manifest::write_manifest(&mut file, &header, &flat_entries)?;

    // Print summary to stderr.
    if !opts.quiet {
        let elapsed = start.elapsed();
        eprintln!(
            "{} files, {} dirs, {} in {:.2}s | root: {} | {}",
            file_count,
            total_dirs,
            format_bytes(total_bytes),
            elapsed.as_secs_f64(),
            sumpig::hash::hash_to_hex(&root_hash),
            output_path.display(),
        );
    }

    Ok(())
}

/// Format a byte count as a human-readable string (e.g., "2.4 GB", "156 MB").
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    const TB: u64 = 1024 * GB;

    if bytes >= TB {
        format!("{:.1} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
