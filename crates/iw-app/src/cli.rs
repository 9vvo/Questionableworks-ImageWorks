//! Command-line interface. Everything here runs without opening a window.

use iw_engine::format::{self, ExportOptions, FileFormat};
use std::path::Path;

const USAGE: &str = "\
Usage:
  imageworks                               Open the application
  imageworks convert <input> <output> [--quality <1-100>]
                                           Convert between .iwdoc, .png and .jpg
  imageworks --version
  imageworks --help

The output format is chosen by the output file's extension. Converting to
PNG or JPEG flattens the layers; --quality applies to JPEG only.";

/// Runs a command-line action if the arguments ask for one. Returns the
/// process exit code, or `None` to start the application.
pub fn run(args: &[String]) -> Option<i32> {
    match args.first().map(String::as_str) {
        None => None,
        Some("--version") => {
            println!(
                "ImageWorks {} (engine {})",
                env!("CARGO_PKG_VERSION"),
                iw_engine::VERSION
            );
            Some(0)
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            Some(0)
        }
        Some("convert") => Some(match convert(&args[1..]) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("imageworks: {message}");
                1
            }
        }),
        Some(other) => {
            eprintln!("imageworks: unknown argument \"{other}\"\n\n{USAGE}");
            Some(2)
        }
    }
}

fn convert(args: &[String]) -> Result<(), String> {
    let mut paths = Vec::new();
    let mut options = ExportOptions::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--quality" {
            let value = iter
                .next()
                .ok_or("--quality needs a number from 1 to 100")?;
            options.jpeg_quality = value
                .parse::<u8>()
                .ok()
                .filter(|q| (1..=100).contains(q))
                .ok_or_else(|| format!("--quality must be from 1 to 100, not \"{value}\""))?;
        } else if arg.starts_with("--") {
            return Err(format!("unknown option \"{arg}\""));
        } else {
            paths.push(Path::new(arg));
        }
    }
    let [input, output] = paths[..] else {
        return Err("convert needs exactly one input and one output file".into());
    };
    let target = FileFormat::from_path(output).ok_or_else(|| {
        format!(
            "cannot tell the output format from \"{}\"; use .iwdoc, .png or .jpg",
            output.display()
        )
    })?;
    let document =
        format::open(input).map_err(|e| format!("cannot open {}: {e}", input.display()))?;
    format::save(&document, output, target, &options)
        .map_err(|e| format!("cannot write {}: {e}", output.display()))
}
