//! `GET /docs/...`: package documentation as markdown.
//!
//! `/docs/{gems|crates|npm}/{name}/{version}` is a version's README;
//! `.../api` its API docs, `.../api/{path}` one module or item
//! (`/docs/crates/state-machines/0.32.1/api/runtime::Runner`,
//! `/docs/gems/state_machines/0.202.0/api/StateMachines::Machine%23draw`:
//! the path is percent-decoded). Scoped npm names go in as they are:
//! `/docs/npm/@types/node/26.6.5`.

use anyhow::Result;
use rama::http::{Body, Response, StatusCode, header};
use vein_adapter::AssetKind;

use super::{Docs, NoApiDocs, NotPublished, Package};
use crate::registry::NotBuilt;

/// What a docs path asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Readme(Package),
    /// The API docs, at a module or item path when one is given.
    Api(Package, Option<String>),
}

/// What a `/docs/...` path asks for; `None` for any other path.
#[must_use]
pub fn parse(path: &str) -> Option<Result<Request>> {
    let rest = path.strip_prefix("/docs/")?;
    let (registry, rest) = rest.split_once('/')?;
    let kind = match registry {
        "gems" => AssetKind::Gem,
        "crates" => AssetKind::Crate,
        "npm" => AssetKind::NpmPackage,
        _ => return None,
    };
    let segments: Vec<&str> = rest.split('/').collect();
    // A scoped npm name spans two segments.
    let name_len = if kind == AssetKind::NpmPackage && rest.starts_with('@') {
        2
    } else {
        1
    };
    let name = segments.get(..name_len)?.join("/");
    let version = *segments.get(name_len)?;
    let tail = &segments[name_len + 1..];
    let package = match Package::new(kind, &name, version) {
        Ok(package) => package,
        Err(err) => return Some(Err(err)),
    };
    Some(match tail {
        [] => Ok(Request::Readme(package)),
        ["api"] | ["api", ""] => Ok(Request::Api(package, None)),
        ["api", path @ ..] => Ok(Request::Api(
            package,
            Some(
                percent_encoding::percent_decode_str(&path.join("/"))
                    .decode_utf8_lossy()
                    .into_owned(),
            ),
        )),
        _ => Err(anyhow::anyhow!(
            "`{path}` is not a docs path: …/{{version}}, …/api or …/api/{{path}}"
        )),
    })
}

/// The answer to `path`, if it is a docs path.
pub async fn respond(docs: &Docs, path: &str) -> Option<Result<Response<Body>>> {
    let request = match parse(path)? {
        Ok(request) => request,
        // A registry built out is not here, rather than asked wrong.
        Err(err) if err.downcast_ref::<NotBuilt>().is_some() => {
            return Some(text(StatusCode::NOT_FOUND, &format!("{err}\n")));
        }
        Err(err) => return Some(text(StatusCode::BAD_REQUEST, &format!("{err}\n"))),
    };
    Some(match request {
        Request::Readme(package) => match docs.readme(&package).await {
            Ok(Some(readme)) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, markdown_type(&readme.file))
                .header("x-vein-docs-file", readme.file)
                .body(Body::from(readme.text))
                .map_err(Into::into),
            Ok(None) => text(
                StatusCode::NOT_FOUND,
                &format!("{} has no README\n", package.label()),
            ),
            Err(err) => failed(&err),
        },
        Request::Api(package, at) => match docs.api(&package, at.as_deref()).await {
            Ok(Some(api)) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
                .header("x-vein-docs-source", api.source)
                .body(Body::from(api.text))
                .map_err(Into::into),
            Ok(None) => text(
                StatusCode::NOT_FOUND,
                &format!(
                    "{} has no public `{}`\n",
                    package.label(),
                    at.unwrap_or_default()
                ),
            ),
            Err(err) => failed(&err),
        },
    })
}

/// 404 for what the registry or docs host does not have, 502 otherwise.
fn failed(err: &anyhow::Error) -> Result<Response<Body>> {
    if err.downcast_ref::<NotPublished>().is_some() || err.downcast_ref::<NoApiDocs>().is_some() {
        text(StatusCode::NOT_FOUND, &format!("{err}\n"))
    } else {
        text(StatusCode::BAD_GATEWAY, &format!("{err:#}\n"))
    }
}

/// Markdown unless the file says otherwise (`README.rdoc`, `README.txt`).
fn markdown_type(file: &str) -> &'static str {
    let markdown = std::path::Path::new(file)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"));
    if markdown {
        "text/markdown; charset=utf-8"
    } else {
        "text/plain; charset=utf-8"
    }
}

fn text(status: StatusCode, body: &str) -> Result<Response<Body>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(body.to_owned()))
        .map_err(Into::into)
}

#[cfg(test)]
mod tests;
