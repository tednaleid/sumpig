use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};

#[derive(Parser)]
#[command(
    name = "sumpig",
    version,
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

        /// Output depth (controls manifest granularity, not hashing depth) [default: 10]
        #[arg(short, long)]
        depth: Option<usize>,

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

        /// Use fast metadata-only hashing instead of content hashing
        #[arg(short = 'M', long)]
        metadata: bool,

        /// Tag the output file with a name (or timestamp if no name given)
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        tag: Option<String>,

        /// Use settings (depth, mode) from an existing manifest file
        #[arg(short = 'm', long, conflicts_with_all = ["depth", "metadata"])]
        match_settings: Option<PathBuf>,

        /// Force-download cloud-only (dataless) files before hashing
        #[arg(long)]
        hydrate: bool,
    },
    /// Compare two fingerprint manifests and report differences
    #[command(after_long_help = "\
EXAMPLES:
  sumpig compare machine-a.txt machine-b.txt
  sumpig compare ~/Documents   (auto-discovers 2 files in .sumpig-fingerprints/)

Output (stdout, tab-separated):
  !\t./path/to/changed-file.txt
  <\t./path/only-in-first.txt
  >\t./path/only-in-second.txt

Prefixes (use cut -f2 to extract paths):
  !  file or directory differs between the two manifests
  <  entry only in the first manifest
  >  entry only in the second manifest

Directories appear when they are at the manifest depth boundary
(where individual files are not listed). Summary and warnings
are printed to stderr.

Exit codes: 0 = identical, 1 = differences found, 2 = error")]
    Compare {
        /// Fingerprint file(s) or directory containing .sumpig-fingerprints/
        #[arg(num_args = 1..=2)]
        paths: Vec<PathBuf>,
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
            metadata,
            tag,
            match_settings,
            hydrate,
        } => {
            if let Err(e) = run_fingerprint(&FingerprintOptions {
                path: &path,
                depth,
                output: output.as_deref(),
                jobs,
                no_ignore,
                quiet,
                metadata,
                tag: tag.as_deref(),
                match_settings: match_settings.as_deref(),
                hydrate,
            }) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Compare { paths } => {
            let (file1, file2) = match resolve_compare_paths(&paths) {
                Ok(pair) => pair,
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(2);
                }
            };
            match run_compare(&file1, &file2) {
                Ok(identical) => {
                    if !identical {
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(2);
                }
            }
        }
    }
}

fn resolve_compare_paths(
    paths: &[PathBuf],
) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    match paths.len() {
        2 => Ok((paths[0].clone(), paths[1].clone())),
        1 => {
            let dir = &paths[0];
            if !dir.is_dir() {
                return Err(format!(
                    "{}: not a directory (provide two file paths or a single directory)",
                    dir.display()
                )
                .into());
            }
            let fp_dir = dir.join(".sumpig-fingerprints");
            if !fp_dir.is_dir() {
                return Err(format!(
                    "{}/.sumpig-fingerprints: directory not found",
                    dir.display()
                )
                .into());
            }
            let mut files: Vec<PathBuf> = fs::read_dir(&fp_dir)?
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "txt"))
                .collect();
            files.sort();
            if files.len() != 2 {
                return Err(format!(
                    "expected exactly 2 files in {}/.sumpig-fingerprints/, found {}",
                    dir.display(),
                    files.len()
                )
                .into());
            }
            Ok((files[0].clone(), files[1].clone()))
        }
        _ => Err("compare requires 1 or 2 arguments".into()),
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

    let depth = header1.depth.min(header2.depth);
    let result = sumpig::compare::compare_manifests(&entries1, &entries2, &label1, &label2, depth);

    let report = sumpig::compare::format_report(&result);
    print!("{}", report.stdout);
    eprint!("{}", report.stderr);

    Ok(result.identical)
}

struct FingerprintOptions<'a> {
    path: &'a std::path::Path,
    depth: Option<usize>,
    output: Option<&'a std::path::Path>,
    jobs: Option<usize>,
    no_ignore: bool,
    quiet: bool,
    metadata: bool,
    tag: Option<&'a str>,
    match_settings: Option<&'a std::path::Path>,
    hydrate: bool,
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

    // Resolve settings: --match-settings overrides depth and mode from a reference manifest.
    let (depth, metadata) = if let Some(ref_path) = opts.match_settings {
        let file = fs::File::open(ref_path)
            .map_err(|e| format!("--match-settings: {}: {e}", ref_path.display()))?;
        let reader = std::io::BufReader::new(file);
        let header = sumpig::manifest::parse_manifest_header(reader)
            .map_err(|e| format!("--match-settings: {}: {e}", ref_path.display()))?;

        let use_metadata = header.mode == "fast";
        if !opts.quiet {
            eprintln!(
                "  Using settings from {}: depth={}, mode={}",
                ref_path.display(),
                header.depth,
                header.mode,
            );
        }
        (header.depth, use_metadata)
    } else {
        (opts.depth.unwrap_or(10), opts.metadata)
    };

    // Walk and hash files in parallel (pipelined).
    let pb: Option<ProgressBar> = if !opts.quiet {
        let pb = ProgressBar::new_spinner();
        pb.set_style(ProgressStyle::with_template("  Hashing  {pos} files...").unwrap());
        pb.enable_steady_tick(Duration::from_millis(120));
        Some(pb)
    } else {
        None
    };

    let walk_options = sumpig::walk::WalkOptions {
        use_default_ignores: !opts.no_ignore,
        num_threads: opts.jobs.unwrap_or(0),
    };
    let pb_clone = pb.clone();
    let pipeline = sumpig::walk::walk_and_hash(
        &canonical,
        &walk_options,
        !metadata,
        opts.hydrate,
        move |_size| {
            if let Some(ref pb) = pb_clone {
                pb.inc(1);
            }
        },
    );

    if let Some(pb) = &pb {
        pb.finish_and_clear();
    }

    let file_count = pipeline.file_count;
    let total_bytes = pipeline.total_bytes;

    // Merge walk errors into hashed entries, then sort.
    let mut sorted_entries = pipeline.hashed;
    sorted_entries.extend(pipeline.errors);
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
        total_bytes,
        root_hash: sumpig::hash::hash_to_hex(&root_hash),
        mode: if metadata { "fast" } else { "content" }.to_string(),
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
        if let Some(ref_path) = opts.match_settings {
            eprintln!(
                "  To compare: sumpig compare {} {}",
                ref_path.display(),
                output_path.display(),
            );
        }
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
