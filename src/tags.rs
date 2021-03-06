use crate::errors::Result;
use crate::Client;
use reqwest::{self, header, Url};
use std::fmt::Debug;

/// A chunk of tags for an image.
///
/// This contains a non-strict subset of the whole list of tags
/// for an image, depending on pagination option at request time.
#[derive(Debug, Default, Deserialize, Serialize)]
struct TagsChunk {
    /// Image repository name.
    name: String,
    /// Subset of tags.
    tags: Vec<String>,
}

impl Client {
    /// List existing tags for an image.
    pub fn get_tags<'a, 'b: 'a, 'c: 'a>(
        &'b self,
        name: &'c str,
        paginate: Option<u32>,
    ) -> Result<Vec<String>> {
        let base_url = format!("{}/v2/{}/tags/list", self.base_url, name);
        let mut link: Option<String> = None;

        let mut result: Vec<String> = Vec::new();

        loop {
            let (tags_chunk, last) = self.fetch_tags_chunk(paginate, &base_url, &link)?;
            for tag in tags_chunk.tags {
                result.push(tag);
            }

            link = match last {
                None => break,
                Some(ref s) if s.is_empty() => None,
                s => s,
            };
        }

        Ok(result)
    }

    fn fetch_tags_chunk(
        &self,
        paginate: Option<u32>,
        base_url: &str,
        link: &Option<String>,
    ) -> Result<(TagsChunk, Option<String>)> {
        let url_paginated = match (paginate, link) {
            (Some(p), None) => format!("{}?n={}", base_url, p),
            (None, Some(l)) => format!("{}?next_page={}", base_url, l),
            (Some(p), Some(l)) => format!("{}?n={}&next_page={}", base_url, p, l),
            _ => base_url.to_string(),
        };
        let url = Url::parse(&url_paginated)?;

        let resp = self
            .build_reqwest(reqwest::Method::GET, url)
            .header(header::ACCEPT, "application/json")
            .send()?
            .error_for_status()?;

        // ensure the CONTENT_TYPE header is application/json
        let ct_hdr = resp.headers().get(header::CONTENT_TYPE).cloned();

        trace!("page url {:?}", ct_hdr);

        let ok = match ct_hdr {
            None => false,
            Some(ref ct) => ct.to_str()?.starts_with("application/json"),
        };
        if !ok {
            // TODO: Make this an error once Satellite
            // returns the content type correctly
            debug!("get_tags: wrong content type '{:?}', ignoring...", ct_hdr);
        }

        // extract the response body and parse the LINK header
        let next = parse_link(resp.headers().get(header::LINK));
        trace!("next_page {:?}", next);

        let tags_chunk = resp.json::<TagsChunk>()?;
        Ok((tags_chunk, next))
    }
}

/// Parse a `Link` header.
///
/// Format is described at https://docs.docker.com/registry/spec/api/#listing-image-tags#pagination.
fn parse_link(hdr: Option<&header::HeaderValue>) -> Option<String> {
    // TODO: this a brittle string-matching parser. Investigate
    // whether there is a a common library to do this, in the future.

    // Raw Header value bytes.
    let hval = match hdr {
        Some(v) => v,
        None => return None,
    };

    // Header value string.
    let sval = match hval.to_str() {
        Ok(v) => v.to_owned(),
        _ => return None,
    };

    // Query parameters for next page URL.
    let uri = sval.trim_end_matches(">; rel=\"next\"");
    let query: Vec<&str> = uri.splitn(2, "next_page=").collect();
    let params = match query.get(1) {
        Some(v) if !(*v).is_empty() => v,
        _ => return None,
    };

    // Last item in current page (pagination parameter).
    let last: Vec<&str> = params.splitn(2, '&').collect();
    match last.get(0).cloned() {
        Some(v) if !v.is_empty() => Some(v.to_string()),
        _ => None,
    }
}
