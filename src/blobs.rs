use crate::errors::{Error, Result};
use crate::Client;
use reqwest;
use reqwest::{Method, StatusCode};
use sha2::Digest;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

impl Client {
    /// Check if a blob exists.
    pub fn has_blob(&self, name: &str, digest: &str) -> Result<bool> {
        let url = {
            let ep = format!("{}/v2/{}/blobs/{}", self.base_url, name, digest);
            reqwest::Url::parse(&ep)?
        };

        let res = self.build_reqwest(Method::HEAD, url.clone()).send()?;

        trace!("Blob HEAD status: {:?}", res.status());

        match res.status() {
            StatusCode::OK => Ok(true),
            _ => Ok(false),
        }
    }

    /// Retrieve blob.
    pub fn get_blob(&self, name: &str, digest: &str) -> Result<Vec<u8>> {
        let digest = crate::ContentDigest::try_new(digest.to_string())?;

        let blob = {
            let ep = format!("{}/v2/{}/blobs/{}", self.base_url, name, digest);
            let url = reqwest::Url::parse(&ep)?;

            let res = self.build_reqwest(Method::GET, url.clone()).send()?;

            trace!("GET {} status: {}", res.url(), res.status());
            let status = res.status();

            // Let client errors through to populate them with the body
            if !(status.is_success() || status.is_client_error()) {
                return Err(Error::UnexpectedHttpStatus(status));
            }

            let status = res.status();
            let body_vec = res.bytes()?.to_vec();
            let len = body_vec.len();

            if status.is_success() {
                trace!("Successfully received blob with {} bytes ", len);
                Ok(body_vec)
            } else if status.is_client_error() {
                Err(Error::Client {
                    status,
                    len,
                    body: body_vec,
                })
            } else {
                // We only want to handle success and client errors here
                error!(
                    "Received unexpected HTTP status '{}' after fetching the body. Please submit a bug report.",
                    status
                );
                Err(Error::UnexpectedHttpStatus(status))
            }
        }?;

        digest.try_verify(&blob)?;
        Ok(blob.to_vec())
    }

    /// Retrieve blob with progress
    pub fn get_blob_with_progress(
        &self,
        name: &str,
        digest: &str,
        sender: Option<Sender<u64>>,
    ) -> Result<Vec<u8>> {
        let digest = crate::ContentDigest::try_new(digest.to_string())?;
        let mut hash = digest.start_hash();
        let blob = {
            let ep = format!("{}/v2/{}/blobs/{}", self.base_url, name, digest);
            let url = reqwest::Url::parse(&ep)?;

            let mut res = self.build_reqwest(Method::GET, url.clone()).send()?;

            trace!("GET {} status: {}", res.url(), res.status());
            let status = res.status();
            // Let client errors through to populate them with the body
            if !(status.is_success() || status.is_client_error()) {
                if let Some(send) = sender {
                    drop(send);
                };
                return Err(Error::UnexpectedHttpStatus(status));
            }

            let status = res.status();

            let mut buffer: [u8; 1024] = [0; 1024];
            let mut body_vec: Vec<u8> = Vec::new();

            loop {
                match res.read(&mut buffer) {
                    Ok(size) => {
                        if size > 0 {
                            if let Some(send) = &sender {
                                send.send(size as u64).unwrap();
                            };
                            Digest::update(&mut hash, &buffer[0..size]);
                            body_vec.append(&mut buffer[0..size].to_vec());
                        } else {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("Download error: {:?}", e);
                        break;
                    }
                }
            }
            let len = body_vec.len();

            if let Some(send) = sender {
                drop(send);
            };
            if status.is_success() {
                trace!("Successfully received blob with {} bytes ", len);
                Ok(body_vec)
            } else if status.is_client_error() {
                Err(Error::Client {
                    status,
                    len,
                    body: body_vec,
                })
            } else {
                // We only want to handle success and client errors here
                error!(
                    "Received unexpected HTTP status '{}' after fetching the body. Please submit a bug report.",
                    status
                );
                Err(Error::UnexpectedHttpStatus(status))
            }
        }?;

        digest.try_verify_hash(&hash)?;
        Ok(blob.to_vec())
    }

    /// Retrieve blob with progress
    pub fn get_blob_with_progress_file(
        &self,
        name: &str,
        hash: &str,
        sender: Option<Sender<u64>>,
        target_dir: &Path,
    ) -> Result<PathBuf> {
        let digest = crate::ContentDigest::try_new(hash.to_string())?;
        let mut target = target_dir.to_path_buf();
        std::fs::create_dir_all(&target).unwrap();
        target.push(hash);
        trace!("Going to downloaad to: {:?}", target);

        let ep = format!("{}/v2/{}/blobs/{}", self.base_url, name, digest);
        let url = reqwest::Url::parse(&ep)?;

        let mut res = match self.build_reqwest(Method::GET, url.clone()).send() {
            Ok(res) => res,
            Err(e) => {
                println!("Unable to create request: {:?}", e);
                return Err(Error::DownloadFailed);
            }
        };

        trace!("GET {} status: {}", res.url(), res.status());
        let status = res.status();
        // Let client errors through to populate them with the body
        if !(status.is_success() || status.is_client_error()) {
            if let Some(send) = sender {
                drop(send);
            };
            return Err(Error::UnexpectedHttpStatus(status));
        }

        let status = res.status();

        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&target)
            .unwrap();
        let mut len: usize = 0;
        let mut buffer: [u8; 1024] = [0; 1024];
        let mut hash = digest.start_hash();
        loop {
            match res.read(&mut buffer) {
                Ok(size) => {
                    if size > 0 {
                        if let Some(send) = &sender {
                            send.send(size as u64).unwrap();
                        };
                        len += size;
                        Digest::update(&mut hash, &buffer[0..size]);
                        file.write_all(&buffer[0..size]).unwrap();
                    } else {
                        break;
                    }
                }
                Err(e) => {
                    error!("Download error: {:?}", e);
                    break;
                }
            }
        }

        if let Some(send) = sender {
            drop(send);
        };
        if status.is_success() {
            trace!("Successfully received blob with {} bytes ", len);
            digest.try_verify_hash(&hash)?;
            Ok(target.clone())
        } else if status.is_client_error() {
            Err(Error::Client {
                status,
                len,
                body: vec![],
            })
        } else {
            // We only want to handle success and client errors here
            error!(
                    "Received unexpected HTTP status '{}' after fetching the body. Please submit a bug report.",
                    status
                );
            Err(Error::UnexpectedHttpStatus(status))
        }
    }
}
