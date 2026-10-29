use std::fs::File;
use std::process::Command;

use camino::Utf8PathBuf;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "livtet-ebook-converter")]
struct Args {
    #[arg(long, default_value = "azw3")]
    input_type: String,
    #[arg(long, default_value = "epub")]
    output_type: String,
    input: Utf8PathBuf,
    output: Utf8PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if try_library(&args) {
        return Ok(());
    }
    let status = Command::new("boko")
        .arg("convert")
        .arg(&args.input)
        .arg(&args.output)
        .status()?;
    if !status.success() {
        return Err(format!("boko CLI failed: {status:?}").into());
    }
    Ok(())
}

fn try_library(args: &Args) -> bool {
    use boko::{
        Book,
        export::{EpubExporter, Exporter},
    };
    let Ok(book) = Book::open(args.input.as_str()) else {
        return false;
    };
    let Ok(mut file) = File::create(&args.output) else {
        return false;
    };
    EpubExporter::new().export(&book, &mut file).is_ok()
}
