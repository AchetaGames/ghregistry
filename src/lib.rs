use std::collections::HashMap;
use std::io::Read;

#[macro_use]
extern crate serde;
#[macro_use]
extern crate log;
#[macro_use]
extern crate strum_macros;

pub mod mediatypes;

mod config;
mod errors;

use errors::{Error, Result};
use serde::{Deserialize, Serialize};

// use crate::errors::*; use reqwest::{Method, StatusCode, Url};

pub use crate::config::Config;

// mod catalog;

mod auth;
mod tags;

pub use auth::WwwHeaderParseError;

pub mod manifest;

mod blobs;

mod content_digest;
mod render;

pub(crate) use self::content_digest::ContentDigest;
pub use self::content_digest::ContentDigestError;

pub static USER_AGENT: &str = "acheta-ghregistry/0.0";

/// Get registry credentials from a JSON config reader.
///
/// This is a convenience decoder for docker-client credentials
/// typically stored under `~/.docker/config.json`.
pub fn get_credentials<T: Read>(
    reader: T,
    index: &str,
) -> Result<(Option<String>, Option<String>)> {
    let map: Auths = serde_json::from_reader(reader)?;
    let real_index = match index {
        // docker.io has some special casing in config.json
        "docker.io" | "registry-1.docker.io" => "https://index.docker.io/v1/",
        other => other,
    };
    let auth = match map.auths.get(real_index) {
        Some(x) => base64::decode(x.auth.as_str())?,
        None => return Err(Error::AuthInfoMissing(real_index.to_string())),
    };
    let s = String::from_utf8(auth)?;
    let creds: Vec<&str> = s.splitn(2, ':').collect();
    let up = match (creds.get(0), creds.get(1)) {
        (Some(&""), Some(p)) => (None, Some(p.to_string())),
        (Some(u), Some(&"")) => (Some(u.to_string()), None),
        (Some(u), Some(p)) => (Some(u.to_string()), Some(p.to_string())),
        (_, _) => (None, None),
    };
    trace!("Found credentials for user={:?} on {}", up.0, index);
    Ok(up)
}

#[derive(Debug, Deserialize, Serialize)]
struct Auths {
    auths: HashMap<String, AuthObj>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct AuthObj {
    auth: String,
}

/// A Client to make outgoing API requests to a registry.
#[derive(Clone, Debug)]
pub struct Client {
    base_url: String,
    credentials: Option<(String, String)>,
    index: String,
    user_agent: Option<String>,
    auth: Option<auth::Auth>,
    client: reqwest::blocking::Client,
}

impl Client {
    pub fn configure() -> Config {
        Config::default()
    }

    /// Ensure remote registry supports v2 API.
    pub fn ensure_v2_registry(self) -> Result<Self> {
        if !self.is_v2_supported()? {
            Err(Error::V2NotSupported)
        } else {
            Ok(self)
        }
    }

    /// Check whether remote registry supports v2 API.
    pub fn is_v2_supported(&self) -> Result<bool> {
        match self.is_v2_supported_and_authorized() {
            Ok((v2_supported, _)) => Ok(v2_supported),
            Err(crate::Error::UnexpectedHttpStatus(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Check whether remote registry supports v2 API and `self` is authorized.
    /// Authorized means to successfully GET the `/v2` endpoint on the remote registry.
    pub fn is_v2_supported_and_authorized(&self) -> Result<(bool, bool)> {
        let api_header = "Docker-Distribution-API-Version";
        let api_version = "registry/2.0";

        // GET request to bare v2 endpoint.
        let v2_endpoint = format!("{}/v2/", self.base_url);
        let request = reqwest::Url::parse(&v2_endpoint).map(|url| {
            trace!("GET {:?}", url);
            self.build_reqwest(reqwest::Method::GET, url)
        })?;

        let response = request.send()?;

        let b = match (response.status(), response.headers().get(api_header)) {
            (reqwest::StatusCode::OK, Some(x)) => Ok((x == api_version, true)),
            (reqwest::StatusCode::UNAUTHORIZED, Some(x)) => Ok((x == api_version, false)),
            (s, v) => {
                trace!("Got unexpected status {}, header version {:?}", s, v);
                return Err(crate::Error::UnexpectedHttpStatus(s));
            }
        };

        b
    }

    /// Takes reqwest's async RequestBuilder and injects an authentication header if a token is present
    fn build_reqwest(
        &self,
        method: ::reqwest::Method,
        url: reqwest::Url,
    ) -> reqwest::blocking::RequestBuilder {
        let mut builder = self.client.request(method, url);

        if let Some(auth) = &self.auth {
            builder = auth.add_auth_headers(builder);
        };

        if let Some(ua) = &self.user_agent {
            builder = builder.header(reqwest::header::USER_AGENT, ua.as_str());
        };

        builder
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct ApiError {
    code: String,
    message: String,
    detail: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Errors {
    errors: Vec<ApiError>,
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
