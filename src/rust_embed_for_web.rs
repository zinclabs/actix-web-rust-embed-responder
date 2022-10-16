use crate::{helper::accepts_gzip, parse::parse_if_none_match_value};
use actix_web::{body::BoxBody, http::Method, HttpRequest, HttpResponse, Responder};

pub struct EmbeddedForWebFileResponse {
    embedded_file: rust_embed_for_web::EmbeddedFile,
}

impl From<rust_embed_for_web::EmbeddedFile> for EmbeddedForWebFileResponse {
    fn from(embedded_file: rust_embed_for_web::EmbeddedFile) -> Self {
        EmbeddedForWebFileResponse { embedded_file }
    }
}

impl Responder for EmbeddedForWebFileResponse {
    type Body = BoxBody;

    fn respond_to(self, req: &HttpRequest) -> HttpResponse<Self::Body> {
        // This responder can't respond to anything other than GET and HEAD requests.
        if req.method() != Method::GET && req.method() != Method::HEAD {
            return HttpResponse::NotImplemented().finish();
        }

        // For the ETag we are using the sha256 hash of the file, encoded with
        // base64. We surround it with quotes as per the spec.
        let etag = self.embedded_file.metadata.etag();

        let last_modified = self.embedded_file.metadata.last_modified();
        let last_modified_timestamp = self.embedded_file.metadata.last_modified_timestamp();

        // Handle If-None-Match requests. If the client has the file cached
        // already, it can send back the ETag to ask for the file only if it has
        // changed.
        //
        // We first check If-None-Match because the spec specifies that it gets
        // priority over If-Modified-Since.
        if let Some(req_etags) = req
            .headers()
            .get("If-None-Match")
            .and_then(parse_if_none_match_value)
        {
            if req_etags.contains(&etag) {
                return HttpResponse::NotModified().finish();
            } else {
                return respond(&self, req, &etag, last_modified);
            }
        }

        // Handle If-Unmodified-Since requests. As a fallback to ETag, the client
        // can also check if a file has been modified using the last modified
        // timestamp of the file.
        if let Some(last_modified_timestamp) = last_modified_timestamp {
            if let Some(if_unmodified_since) = req
                .headers()
                .get("If-Unmodified-Since")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| chrono::DateTime::parse_from_rfc2822(v).ok())
            {
                // It's been modified since then
                if last_modified_timestamp > if_unmodified_since.timestamp() {
                    return respond(&self, req, &etag, last_modified);
                } else {
                    return HttpResponse::NotModified().finish();
                }
            }
        }

        // Otherwise, the client doesn't have the file cached and we do need to
        // send a response.
        respond(&self, req, etag, last_modified)
    }
}

fn respond(
    file: &EmbeddedForWebFileResponse,
    req: &HttpRequest,
    etag: &str,
    last_modified: Option<&str>,
) -> HttpResponse {
    let mut resp = HttpResponse::Ok();
    resp.append_header(("ETag", etag));

    if let Some(last_modified) = last_modified {
        resp.append_header(("Last-Modified", last_modified));
    }

    // We respond with gzip if the client accepts it, and if gzipping the file
    // actually makes it smaller (otherwise the data_gzip would be None)
    if accepts_gzip(req) {
        if let Some(data_gzip) = file.embedded_file.data_gzip {
            resp.append_header(("Content-Encoding", "gzip"));
            return resp.body(data_gzip);
        }
    }

    resp.body(file.embedded_file.data)
}