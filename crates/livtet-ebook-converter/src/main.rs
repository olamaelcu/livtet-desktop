use std::fs::File;
use std::path::PathBuf;
use std::process::Command;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "livtet-ebook-converter")]
struct Args {
    #[arg(long, default_value = "azw3")]
    input_type: String,
    #[arg(long, default_value = "epub")]
    output_type: String,
    input: PathBuf,
    output: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let library_ok = try_library(&args);
    if library_ok {
        return Ok(());
    }
    let status = Command::new("boko")
        .arg("convert")
        .arg(&args.input)
        .arg(&args.output)
        .status()?;
    if !status.success() {
        return Err(format!("boko CLI failed: {:?}", status).into());
    }
    Ok(())
}

fn try_library(args: &Args) -> bool {
    use boko::{
        Book,
        export::{EpubExporter, Exporter},
    };
    let input_path = args.input.to_string_lossy();
    let mut book = match Book::open(&*input_path) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let mut file = match File::create(&args.output) {
        Ok(f) => f,
        Err(_) => return false,
    };
    match EpubExporter::new().export(&mut book, &mut file) {
        Ok(_) => true,
        Err(_) => false,
    }
}
