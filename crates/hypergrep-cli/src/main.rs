use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use clap::Parser;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

use hypergrep_core::index::Index;

#[derive(Parser)]
#[command(
    name = "hypergrep",
    version,
    about = "Hypergrep - A codebase intelligence engine for AI agents",
    long_about = "Hypergrep is a code search tool built for AI coding agents.\n\n\
        Unlike grep/ripgrep which return raw text lines, Hypergrep understands code \
        structure. It builds a trigram index for fast text search, parses ASTs with \
        tree-sitter for structural awareness, and maintains a live call graph for \
        impact analysis.\n\n\
        SEARCH MODES:\n  \
        (default)     Text search (ripgrep-compatible output)\n  \
        -s            Structural search (return full function/class bodies)\n  \
        --layer N     Semantic compression (0=names, 1=signatures+calls, 2=full code)\n  \
        --callers     Reverse call graph (who calls this symbol?)\n  \
        --callees     Forward call graph (what does this symbol call?)\n  \
        --impact      Impact analysis (what breaks if this symbol changes?)\n  \
        --exists      Existence check via bloom filter (O(1), no false negatives)\n  \
        --model       Codebase mental model (compact summary for orientation)\n\n\
        EXAMPLES:\n  \
        hypergrep authenticate src/           Search for \"authenticate\"\n  \
        hypergrep -s authenticate src/        Return full function bodies\n  \
        hypergrep --layer 1 --budget 500 authenticate src/\n                                          \
        Best results in 500 tokens\n  \
        hypergrep --layer 1 --json authenticate src/\n                                          \
        JSON output for agent consumption\n  \
        hypergrep --callers authenticate src/ Who calls authenticate()?\n  \
        hypergrep --impact authenticate src/  What breaks if it changes?\n  \
        hypergrep --exists redis src/         Does this project use Redis?\n  \
        hypergrep --model \"\" src/             Codebase overview (~500 tokens)\n\n\
        LANGUAGES: Rust, Python, JavaScript, TypeScript, Go, Java, C, C++",
    after_help = "Hypergrep builds an in-memory trigram index on first run.\n\
        Subsequent queries against the same codebase are 50-500x faster.\n\
        In daemon mode (hypergrep-daemon), the index persists across queries\n\
        and updates incrementally via filesystem watching."
)]
struct Cli {
    /// Regex pattern to search for, or symbol name for graph queries
    #[arg(help = "Pattern (regex for search, symbol name for --callers/--impact)")]
    pattern: String,

    /// Directory or file to search
    #[arg(default_value = ".", help = "Directory to search [default: .]")]
    path: PathBuf,

    // -- Output modes --
    /// Show index statistics (files, trigrams, symbols, graph edges, bloom filter size)
    #[arg(long, help_heading = "Output modes")]
    stats: bool,

    /// Print the codebase mental model -- a compact structural summary (~500 tokens)
    /// that an agent can load once at session start to skip orientation searches
    #[arg(long, help_heading = "Output modes")]
    model: bool,

    /// Check if a concept/technology exists in the codebase.
    /// Uses a bloom filter: "NO" is guaranteed correct, "YES" may have ~1% false positives
    #[arg(long, help_heading = "Output modes")]
    exists: bool,

    // -- Search options --
    /// Return complete enclosing functions/classes instead of matching lines.
    /// Deduplicates: multiple matches in the same function return it only once
    #[arg(short = 's', long, help_heading = "Search options")]
    structural: bool,

    /// Semantic compression layer:
    ///   0 = file path + symbol name + kind (~15 tokens per result)
    ///   1 = signature + calls + called_by (~80-120 tokens per result)
    ///   2 = full source code of enclosing function (~200-800 tokens per result)
    #[arg(long, value_name = "0|1|2", help_heading = "Search options")]
    layer: Option<u8>,

    /// Maximum token budget for results. Hypergrep selects the best results
    /// that fit within this budget. Requires --layer
    #[arg(long, value_name = "TOKENS", help_heading = "Search options")]
    budget: Option<usize>,

    // -- Graph queries --
    /// Show all functions/methods that call the given symbol (reverse call graph)
    #[arg(long, help_heading = "Graph queries")]
    callers: bool,

    /// Show all functions/methods that the given symbol calls (forward call graph)
    #[arg(long, help_heading = "Graph queries")]
    callees: bool,

    /// Impact analysis: BFS upstream through the call graph to find everything
    /// affected by a change to this symbol.
    ///   Depth 1 = WILL BREAK (direct callers)
    ///   Depth 2 = MAY BREAK (callers of callers)
    ///   Depth 3+ = REVIEW (transitive)
    #[arg(long, help_heading = "Graph queries")]
    impact: bool,

    /// Maximum depth for --impact analysis
    #[arg(
        long,
        default_value = "3",
        value_name = "N",
        help_heading = "Graph queries"
    )]
    depth: usize,

    // -- Output format --
    /// Output as JSON (for programmatic/agent consumption)
    #[arg(long, help_heading = "Output format")]
    json: bool,

    /// Suppress colored output
    #[arg(long, help_heading = "Output format")]
    no_color: bool,

    /// Print only the count of matches
    #[arg(short = 'c', long, help_heading = "Output format")]
    count: bool,

    /// Print only file paths that contain matches (no line content)
    #[arg(
        short = 'l',
        long = "files-with-matches",
        help_heading = "Output format"
    )]
    files_only: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("hypergrep=info")
        .with_writer(io::stderr)
        .init();

    let cli = Cli::parse();

    // Determine if we need the full structural pass or just fast trigram index
    let needs_structural = cli.structural
        || cli.layer.is_some()
        || cli.callers
        || cli.callees
        || cli.impact
        || cli.model
        || cli.stats;

    let start = Instant::now();
    let mut index = Index::build(&cli.path)?;

    if needs_structural {
        index.complete_index();
    }

    // Save updated index to disk for next run
    let _ = index.save();
    let build_time = start.elapsed();

    if cli.stats {
        eprintln!("Files indexed: {}", index.file_count());
        eprintln!("Unique trigrams: {}", index.trigram_count());
        eprintln!("Symbols parsed: {}", index.symbol_count());
        eprintln!("Graph edges: {}", index.graph.edge_count());
        eprintln!(
            "Bloom filter: {} concepts, {} bytes",
            index.bloom.len(),
            index.bloom.size_bytes()
        );
        eprintln!("Mental model: {} tokens", index.mental_model.tokens);
        eprintln!("Index build time: {:?}", build_time);
        return Ok(());
    }

    // Mental model: --model
    if cli.model {
        if cli.json {
            println!("{}", serde_json::to_string_pretty(&index.mental_model)?);
        } else {
            print!(
                "{}",
                hypergrep_core::mental_model::format_text(&index.mental_model)
            );
        }
        eprintln!(
            "Index: {:?} | Mental model: {} tokens",
            build_time, index.mental_model.tokens
        );
        return Ok(());
    }

    // Existence check: --exists
    if cli.exists {
        let found = index.bloom.might_contain(&cli.pattern);
        if cli.json {
            println!(
                "{{\"concept\": \"{}\", \"exists\": {}}}",
                cli.pattern, found
            );
        } else if found {
            println!("YES: '{}' is likely present in this codebase", cli.pattern);
        } else {
            println!("NO: '{}' is definitely not in this codebase", cli.pattern);
        }
        eprintln!("Index: {:?} | Bloom filter: O(1) lookup", build_time);
        return Ok(());
    }

    let color_choice = if cli.no_color {
        ColorChoice::Never
    } else {
        ColorChoice::Auto
    };

    // Graph queries: --callers, --callees, --impact
    if cli.callers || cli.callees || cli.impact {
        let mut stdout = StandardStream::stdout(color_choice);
        let mut sym_color = ColorSpec::new();
        sym_color.set_fg(Some(Color::Yellow)).set_bold(true);
        let mut file_color = ColorSpec::new();
        file_color.set_fg(Some(Color::Magenta));
        let mut severity_color = ColorSpec::new();

        if cli.callers {
            let results = index.graph.callers_of(&cli.pattern);
            if results.is_empty() {
                eprintln!("No callers found for '{}'", cli.pattern);
            } else {
                writeln!(stdout, "Callers of '{}':", cli.pattern)?;
                for sym in &results {
                    stdout.set_color(&file_color)?;
                    write!(stdout, "  {}", sym.file.display())?;
                    stdout.reset()?;
                    write!(stdout, ":")?;
                    stdout.set_color(&sym_color)?;
                    writeln!(stdout, "{}", sym.name)?;
                    stdout.reset()?;
                }
            }
        } else if cli.callees {
            let results = index.graph.callees_of(&cli.pattern);
            if results.is_empty() {
                eprintln!("No callees found for '{}'", cli.pattern);
            } else {
                writeln!(stdout, "Callees of '{}':", cli.pattern)?;
                for sym in &results {
                    stdout.set_color(&file_color)?;
                    write!(stdout, "  {}", sym.file.display())?;
                    stdout.reset()?;
                    write!(stdout, ":")?;
                    stdout.set_color(&sym_color)?;
                    writeln!(stdout, "{}", sym.name)?;
                    stdout.reset()?;
                }
            }
        } else if cli.impact {
            let results = index.graph.impact(&cli.pattern, cli.depth);
            if results.is_empty() {
                eprintln!("No impact detected for '{}'", cli.pattern);
            } else {
                writeln!(
                    stdout,
                    "Impact analysis for '{}' (depth {}):",
                    cli.pattern, cli.depth
                )?;
                writeln!(stdout)?;
                for r in &results {
                    severity_color.set_fg(Some(match r.severity {
                        hypergrep_core::graph::ImpactSeverity::WillBreak => Color::Red,
                        hypergrep_core::graph::ImpactSeverity::MayBreak => Color::Yellow,
                        hypergrep_core::graph::ImpactSeverity::Review => Color::Cyan,
                    }));
                    severity_color.set_bold(true);

                    write!(stdout, "  [depth {}] ", r.depth)?;
                    stdout.set_color(&severity_color)?;
                    write!(stdout, "{:<12}", format!("{}", r.severity))?;
                    stdout.reset()?;
                    stdout.set_color(&file_color)?;
                    write!(stdout, " {}", r.symbol.file.display())?;
                    stdout.reset()?;
                    write!(stdout, ":")?;
                    stdout.set_color(&sym_color)?;
                    writeln!(stdout, "{}", r.symbol.name)?;
                    stdout.reset()?;

                    severity_color = ColorSpec::new(); // reset for next iteration
                }
            }
        }

        eprintln!(
            "Index: {:?} | {} edges in graph",
            build_time,
            index.graph.edge_count()
        );
        return Ok(());
    }

    // Semantic search: --layer N [--budget M] [--json]
    if let Some(layer_num) = cli.layer {
        let layer = hypergrep_core::semantic::Layer::from_u8(layer_num);
        let search_start = Instant::now();
        let results = index.search_semantic(&cli.pattern, layer, cli.budget)?;
        let search_time = search_start.elapsed();

        let total_tokens: usize = results.iter().map(|r| r.tokens).sum();

        if cli.json {
            println!("{}", serde_json::to_string_pretty(&results)?);
        } else {
            let mut stdout = StandardStream::stdout(color_choice);
            let mut kind_color = ColorSpec::new();
            kind_color.set_fg(Some(Color::Cyan));
            let mut name_color = ColorSpec::new();
            name_color.set_fg(Some(Color::Yellow)).set_bold(true);
            let mut file_color = ColorSpec::new();
            file_color.set_fg(Some(Color::Magenta));
            let mut dim = ColorSpec::new();
            dim.set_dimmed(true);

            for r in &results {
                // Header
                stdout.set_color(&file_color)?;
                write!(stdout, "{}", r.file)?;
                stdout.reset()?;
                write!(stdout, ":")?;
                stdout.set_color(&kind_color)?;
                write!(stdout, "{}", r.kind)?;
                stdout.reset()?;
                write!(stdout, " ")?;
                stdout.set_color(&name_color)?;
                write!(stdout, "{}", r.name)?;
                stdout.reset()?;
                stdout.set_color(&dim)?;
                writeln!(stdout, " (~{} tokens)", r.tokens)?;
                stdout.reset()?;

                if let Some(sig) = &r.signature {
                    writeln!(stdout, "  sig: {}", sig)?;
                }
                if let Some(calls) = &r.calls {
                    writeln!(stdout, "  calls: {}", calls.join(", "))?;
                }
                if let Some(called_by) = &r.called_by {
                    writeln!(stdout, "  called_by: {}", called_by.join(", "))?;
                }
                if let Some(body) = &r.body {
                    writeln!(stdout, "{}", body)?;
                }
                writeln!(stdout)?;
            }
        }

        eprintln!(
            "Index: {:?} | Search: {:?} | {} results, {} total tokens (layer {}, budget {:?})",
            build_time,
            search_time,
            results.len(),
            total_tokens,
            layer_num,
            cli.budget,
        );
        return Ok(());
    }

    if cli.structural {
        let search_start = Instant::now();
        let matches = index.search_structural(&cli.pattern)?;
        let search_time = search_start.elapsed();

        if cli.count {
            println!("{}", matches.len());
        } else {
            let mut stdout = StandardStream::stdout(color_choice);
            let mut path_color = ColorSpec::new();
            path_color.set_fg(Some(Color::Magenta));
            let mut kind_color = ColorSpec::new();
            kind_color.set_fg(Some(Color::Cyan));
            let mut name_color = ColorSpec::new();
            name_color.set_fg(Some(Color::Yellow)).set_bold(true);
            let mut line_color = ColorSpec::new();
            line_color.set_fg(Some(Color::Green));

            for m in &matches {
                // Header: path:lines kind name
                stdout.set_color(&path_color)?;
                write!(stdout, "{}", m.path.display())?;
                stdout.reset()?;
                write!(stdout, ":")?;
                stdout.set_color(&line_color)?;
                write!(stdout, "{}-{}", m.line_range.0, m.line_range.1)?;
                stdout.reset()?;
                write!(stdout, " ")?;
                stdout.set_color(&kind_color)?;
                write!(stdout, "{}", m.symbol_kind)?;
                stdout.reset()?;
                write!(stdout, " ")?;
                stdout.set_color(&name_color)?;
                writeln!(stdout, "{}", m.symbol_name)?;
                stdout.reset()?;

                // Body
                writeln!(stdout, "{}", m.body)?;
                writeln!(stdout, "---")?;
            }
        }

        eprintln!(
            "Index: {:?} | Search: {:?} | {} structural matches ({} symbols in {} files)",
            build_time,
            search_time,
            matches.len(),
            index.symbol_count(),
            index.file_count()
        );
    } else {
        let search_start = Instant::now();
        let matches = index.search(&cli.pattern)?;
        let search_time = search_start.elapsed();

        if cli.count {
            println!("{}", matches.len());
        } else if cli.files_only {
            let mut seen = std::collections::HashSet::new();
            for m in &matches {
                if seen.insert(&m.path) {
                    println!("{}", m.path.display());
                }
            }
        } else {
            let mut stdout = StandardStream::stdout(color_choice);
            let mut path_color = ColorSpec::new();
            path_color.set_fg(Some(Color::Magenta));
            let mut line_num_color = ColorSpec::new();
            line_num_color.set_fg(Some(Color::Green));
            let mut match_color = ColorSpec::new();
            match_color.set_fg(Some(Color::Red)).set_bold(true);

            for m in &matches {
                stdout.set_color(&path_color)?;
                write!(stdout, "{}", m.path.display())?;
                stdout.reset()?;
                write!(stdout, ":")?;
                stdout.set_color(&line_num_color)?;
                write!(stdout, "{}", m.line_number)?;
                stdout.reset()?;
                write!(stdout, ":")?;

                let line = &m.line;
                write!(stdout, "{}", &line[..m.match_start])?;
                stdout.set_color(&match_color)?;
                write!(stdout, "{}", &line[m.match_start..m.match_end])?;
                stdout.reset()?;
                writeln!(stdout, "{}", &line[m.match_end..])?;
            }
        }

        eprintln!(
            "Index: {:?} | Search: {:?} | {} matches in {} files",
            build_time,
            search_time,
            matches.len(),
            index.file_count()
        );
    }

    Ok(())
}
