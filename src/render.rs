//! Render a docker image.

// Docker image format is specified at
// https://github.com/moby/moby/blob/v17.05.0-ce/image/spec/v1.md

use libflate::gzip;
use std::io::{BufReader, Read};
use std::path::{Path, StripPrefixError};
use std::{fs, path};
use tar;

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("wrong target path {}: must be absolute path to existing directory", _0.display())]
    WrongTargetPath(path::PathBuf),
    #[error("io error")]
    Io(#[from] std::io::Error),
}

/// Unpack an ordered list of layers to a target directory.
///
/// Layers must be provided as gzip-compressed tar archives, with lower layers
/// coming first. Target directory must be an existing absolute path.
pub fn unpack(layers: &[Vec<u8>], target_dir: &path::Path) -> Result<(), RenderError> {
    if !target_dir.is_absolute() || !target_dir.exists() || !target_dir.is_dir() {
        return Err(RenderError::WrongTargetPath(target_dir.to_path_buf()));
    }
    for l in layers {
        // Unpack layers
        let mut input = std::io::BufReader::new(l.as_slice());
        let gz_dec = gzip::Decoder::new(&mut input)?;
        let mut archive = tar::Archive::new(gz_dec);
        archive.set_preserve_permissions(true);
        archive.set_unpack_xattrs(true);
        archive.unpack(target_dir)?;

        // Clean whiteouts
        clean_whiteouts(target_dir, input)?;
    }
    Ok(())
}

pub fn unpack_files(files: Vec<String>, target_dir: &path::Path) -> Result<(), RenderError> {
    if !target_dir.is_absolute() || !target_dir.exists() || !target_dir.is_dir() {
        return Err(RenderError::WrongTargetPath(target_dir.to_path_buf()));
    }
    for file in files {
        // Unpack layers
        let path = Path::new(&file);
        if let Ok(f) = std::fs::OpenOptions::new().read(true).open(path) {
            let mut input = std::io::BufReader::new(f);

            let gz_dec = gzip::Decoder::new(&mut input)?;
            let mut archive = tar::Archive::new(gz_dec);
            archive.set_preserve_permissions(true);
            archive.set_unpack_xattrs(true);
            archive.unpack(target_dir)?;

            // Clean whiteouts
            clean_whiteouts(target_dir, input)?;
        };
    }
    Ok(())
}

pub fn unpack_partial(
    layers: &[Vec<u8>],
    target_dir: &path::Path,
    filter: String,
) -> Result<(), RenderError> {
    if !target_dir.is_absolute() || !target_dir.exists() || !target_dir.is_dir() {
        return Err(RenderError::WrongTargetPath(target_dir.to_path_buf()));
    }
    for l in layers {
        // Unpack layers
        let mut input = std::io::BufReader::new(l.as_slice());
        let gz_dec = gzip::Decoder::new(&mut input)?;
        let mut archive = tar::Archive::new(gz_dec);
        archive.set_preserve_permissions(true);
        archive.set_unpack_xattrs(true);
        for file in archive.entries().unwrap() {
            let mut f = file.unwrap();
            match f.path().unwrap().strip_prefix(&filter) {
                Ok(path) => {}
                Err(_) => {
                    // Not in the prefix
                }
            }
        }

        // Clean whiteouts
        clean_whiteouts(target_dir, input)?;
    }
    Ok(())
}

fn clean_whiteouts<R: Read>(target_dir: &Path, l: BufReader<R>) -> Result<(), RenderError> {
    let gz_dec = gzip::Decoder::new(l)?;
    let mut archive = tar::Archive::new(gz_dec);
    for entry in archive.entries()? {
        let file = entry?;
        let path = file.path()?;
        let parent = path.parent().unwrap_or_else(|| path::Path::new("/"));
        if let Some(fname) = path.file_name() {
            let wh_name = fname.to_string_lossy();
            if wh_name == ".wh..wh..opq" {
                //TODO: opaque whiteout, dir removal
            } else if wh_name.starts_with(".wh.") {
                let rel_parent = path::PathBuf::from("./".to_string() + &parent.to_string_lossy());

                // Remove real file behind whiteout
                let real_name = wh_name.trim_start_matches(".wh.");
                let abs_real_path = target_dir.join(&rel_parent).join(real_name);
                fs::remove_dir_all(abs_real_path)?;

                // Remove whiteout place-holder
                let abs_wh_path = target_dir.join(&rel_parent).join(fname);
                fs::remove_dir_all(abs_wh_path)?;
            };
        }
    }
    Ok(())
}
