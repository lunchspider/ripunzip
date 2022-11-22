// Copyright 2022 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

mod cloneable_seekable_reader;

use std::{
    borrow::Cow,
    fs::{create_dir_all, File},
    io::ErrorKind,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use rayon::prelude::*;
use zip::read::ZipFile;

use crate::cloneable_seekable_reader::CloneableSeekableReader;

/// Unzip all files within a zip file as quickly as possible.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, value_name = "DIRECTORY")]
    output_directory: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// unzips a zip file
    File {
        /// Zip file to unzip
        #[arg(value_name = "FILE")]
        zipfile: PathBuf,
    },
    /// downloads and unzips a zip file
    Uri {
        /// URI of zip file to download and unzip
        #[arg(value_name = "URI")]
        uri: String,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    match &args.command {
        Commands::File { zipfile } => {
            let zipfile = File::open(zipfile)?;
            unzip_file(zipfile, &args.output_directory)
        }
        Commands::Uri { uri } => {
            println!("Downloading URI {}", uri);
            let mut response = reqwest::blocking::get(uri)?;
            let mut tempfile = tempfile::tempfile()?;
            std::io::copy(&mut response, &mut tempfile)?;
            unzip_file(tempfile, &args.output_directory)
        }
    }
}

fn unzip_file(zipfile: File, output_directory: &Option<PathBuf>) -> Result<()> {
    // The following line doesn't actually seem to make any significant
    // performance difference.
    // let zipfile = BufReader::new(zipfile);
    let zipfile = CloneableSeekableReader::new(zipfile);
    let zip = zip::ZipArchive::new(zipfile)?;
    let file_count = zip.len();
    println!("Zip has {} files", file_count);
    let errors: Vec<_> = (0..file_count)
        .into_par_iter()
        .map(|i| extract_file(zip.clone(), i, output_directory))
        .filter_map(Result::err)
        .collect();
    // Output any errors we found on any file
    for error in &errors {
        eprintln!("Error: {}", error)
    }
    // Return the first error code, if any.
    errors.into_iter().next().map(Result::Err).unwrap_or(Ok(()))
}

/// Extracts a file from a zip file, attaching diagnostics to any errors where
/// possible.
fn extract_file(
    mut myzip: zip::ZipArchive<CloneableSeekableReader<File>>,
    i: usize,
    output_directory: &Option<PathBuf>,
) -> Result<()> {
    let file = myzip.by_index(i)?;
    let name = file
        .enclosed_name()
        .map(Path::to_string_lossy)
        .unwrap_or_else(|| Cow::Borrowed("<unprintable>"))
        .to_string();
    extract_file_inner(file, output_directory)
        .with_context(|| format!("Failed to extract {}", name))
}

/// Extracts a file from a zip file.
fn extract_file_inner(mut file: ZipFile, output_directory: &Option<PathBuf>) -> Result<()> {
    let name = file
        .enclosed_name()
        .ok_or_else(|| std::io::Error::new(ErrorKind::Unsupported, "path not safe to extract"))?;
    let name = name.to_path_buf();
    // let name_for_error = name.clone();
    // let add_context = || format!("Failed to extract {}", name_for_error);
    if name.is_dir() {
        println!("Skipping directory {}", name.display());
    } else {
        println!("Extracting: {}", name.display());
        let out_file = match output_directory {
            Some(output_directory) => output_directory.join(file.name()),
            None => PathBuf::from(file.name()),
        };
        if let Some(parent) = out_file.parent() {
            create_dir_all(parent)?;
        }
        let mut out_file = File::create(out_file)?;
        std::io::copy(&mut file, &mut out_file)?;
    }
    Ok(())
}
