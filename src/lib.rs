mod call;
pub mod lookup;
mod message;
pub mod twiml;
pub mod webhook;

use bytes::Bytes;
pub use call::{Call, OutboundCall};
use headers::authorization::{Authorization, Basic};
use headers::{ContentType, HeaderMapExt};
use http_body_util::{BodyExt as _, Either, Empty, Full};
use hyper::body::Incoming;
use hyper::{Method, StatusCode};
use hyper_tls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
pub use message::{Message, MessageStatus, OutboundMessage};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use url::form_urlencoded;

pub const GET: Method = Method::GET;
pub const POST: Method = Method::POST;
pub const PUT: Method = Method::PUT;

#[derive(Clone)]
pub struct Client {
    account_id: String,
    auth_token: String,
    auth_header: Authorization<Basic>,
    http_client: hyper_util::client::legacy::Client<
        HttpsConnector<HttpConnector>,
        Either<Empty<Bytes>, Full<Bytes>>,
    >,
}

fn url_encode(params: &[(&str, &str)]) -> String {
    let mut url = form_urlencoded::Serializer::new(String::new());
    for (k, v) in params {
        url.append_pair(k, v);
    }

    url.finish()
}

#[derive(Debug)]
pub enum TwilioError {
    RequestError(hyper_util::client::legacy::Error),
    ReadResponseError(hyper::Error),
    HTTPError(StatusCode),
    ParsingError,
    AuthError,
    BadRequest,
}

impl Display for TwilioError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match *self {
            TwilioError::RequestError(ref e) => e.fmt(f),
            TwilioError::ReadResponseError(ref e) => e.fmt(f),
            TwilioError::HTTPError(ref s) => write!(f, "Invalid HTTP status code: {}", s),
            TwilioError::ParsingError => f.write_str("Parsing error"),
            TwilioError::AuthError => f.write_str("Missing `X-Twilio-Signature` header in request"),
            TwilioError::BadRequest => f.write_str("Bad request"),
        }
    }
}

impl Error for TwilioError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match *self {
            TwilioError::RequestError(ref e) => Some(e),
            TwilioError::ReadResponseError(ref e) => Some(e),
            _ => None,
        }
    }
}

impl TwilioError {
    #[inline]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RequestError(_) => true,
            Self::ReadResponseError(_) => true,
            Self::HTTPError(s) => s.is_server_error(),
            _ => false,
        }
    }
}

pub trait FromMap {
    fn from_map(m: BTreeMap<String, String>) -> Result<Box<Self>, TwilioError>;
}

impl Client {
    pub fn new(account_id: &str, auth_token: &str) -> Self {
        Client {
            account_id: account_id.to_string(),
            auth_token: auth_token.to_string(),
            auth_header: Authorization::basic(account_id, auth_token),
            http_client: hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .build(HttpsConnector::new()),
        }
    }

    /// For account that need to provide a different SID in their URLs than they do in their
    /// Authorization header, this method will override the SID in the URL, but not the auth
    /// header.
    pub fn set_account_sid(&mut self, account_sid: String) {
        self.account_id = account_sid;
    }

    async fn send_request<T>(
        &self,
        method: hyper::Method,
        endpoint: &str,
        params: &[(&str, &str)],
    ) -> Result<T, TwilioError>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/{}.json",
            self.account_id, endpoint
        );
        let mut req = hyper::Request::builder()
            .method(method)
            .uri(&*url)
            .body(Either::Right(Full::from(url_encode(params))))
            .unwrap();

        req.headers_mut()
            .typed_insert(ContentType::from(mime::APPLICATION_WWW_FORM_URLENCODED));
        req.headers_mut().typed_insert(self.auth_header.clone());

        let resp = self
            .http_client
            .request(req)
            .await
            .map_err(TwilioError::RequestError)?;

        match resp.status() {
            StatusCode::CREATED | StatusCode::OK => {}
            other => return Err(TwilioError::HTTPError(other)),
        };

        let decoded: T = resp
            .into_body()
            .collect()
            .await
            .map_err(TwilioError::ReadResponseError)
            .and_then(|body| {
                let bytes = body.to_bytes();
                serde_json::from_slice(&bytes).map_err(|_| TwilioError::ParsingError)
            })?;

        Ok(decoded)
    }

    async fn message_status<T>(&self, message_sid: &str) -> Result<T, TwilioError>
    where
        T: serde::de::DeserializeOwned,
    {
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages/{}.json",
            self.account_id, message_sid,
        );
        let mut req = hyper::Request::get(url)
            .body(Either::Left(Empty::new()))
            .unwrap();

        req.headers_mut().typed_insert(self.auth_header.clone());

        let resp = self
            .http_client
            .request(req)
            .await
            .map_err(TwilioError::RequestError)?;

        match resp.status() {
            StatusCode::OK => {}
            other => return Err(TwilioError::HTTPError(other)),
        };

        let decoded: T = resp
            .into_body()
            .collect()
            .await
            .map_err(TwilioError::ReadResponseError)
            .and_then(|body| {
                let bytes = body.to_bytes();
                serde_json::from_slice(&bytes).map_err(|_| TwilioError::ParsingError)
            })?;

        Ok(decoded)
    }

    pub async fn respond_to_webhook<T: FromMap, F>(
        &self,
        req: hyper::Request<Incoming>,
        mut logic: F,
    ) -> hyper::Response<Full<Bytes>>
    where
        F: FnMut(T) -> twiml::Twiml,
    {
        let o: T = match self.parse_request::<T>(req).await {
            Ok(obj) => *obj,
            Err(_) => {
                let mut res = hyper::Response::new(Full::from("Error."));
                *res.status_mut() = StatusCode::BAD_REQUEST;
                return res;
            }
        };

        let t = logic(o);
        let body = t.as_twiml();
        let len = body.len() as u64;
        let mut res = hyper::Response::new(Full::from(body));
        res.headers_mut().typed_insert(headers::ContentType::xml());
        res.headers_mut().typed_insert(headers::ContentLength(len));
        res
    }
}
