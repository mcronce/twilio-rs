use crate::{Client, FromMap, TwilioError};
use core::fmt;
use core::str::FromStr;
use headers::{HeaderMapExt, Host};
use hmac::{Hmac, Mac};
use http_body_util::BodyExt as _;
use hyper::body::Incoming;
use hyper::{Method, Request};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Queued,
    Sending,
    Sent,
    Failed,
    Delivered,
    Undelivered,
    Receiving,
    Received,
    Accepted,
    Scheduled,
    Read,
    PartiallyDelivered,
    Canceled,
}

impl AsRef<str> for MessageStatus {
    #[inline]
    fn as_ref(&self) -> &str {
        match self {
            Self::Queued => "queued",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Failed => "failed",
            Self::Delivered => "delivered",
            Self::Undelivered => "undelivered",
            Self::Receiving => "receiving",
            Self::Received => "received",
            Self::Accepted => "accepted",
            Self::Scheduled => "scheduled",
            Self::Read => "read",
            Self::PartiallyDelivered => "partially_delivered",
            Self::Canceled => "canceled",
        }
    }
}

impl fmt::Display for MessageStatus {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl MessageStatus {
    #[inline]
    pub fn from_bytes(s: &[u8]) -> Result<Self, InvalidMessageStatus> {
        let this = match s {
            b"queued" => Self::Queued,
            b"sending" => Self::Sending,
            b"sent" => Self::Sent,
            b"failed" => Self::Failed,
            b"delivered" => Self::Delivered,
            b"undelivered" => Self::Undelivered,
            b"receiving" => Self::Receiving,
            b"received" => Self::Received,
            b"accepted" => Self::Accepted,
            b"scheduled" => Self::Scheduled,
            b"read" => Self::Read,
            b"partially_delivered" => Self::PartiallyDelivered,
            b"canceled" => Self::Canceled,
            _ => return Err(InvalidMessageStatus(String::from_utf8_lossy(s).to_string())),
        };
        Ok(this)
    }
}

impl FromStr for MessageStatus {
    type Err = InvalidMessageStatus;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_bytes(s.as_bytes())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid Twilio message status '{0}'")]
pub struct InvalidMessageStatus(String);

fn get_args(path: &str) -> BTreeMap<String, String> {
    let url_segments: Vec<&str> = path.split('?').collect();
    if url_segments.len() != 2 {
        return BTreeMap::new();
    }
    let query_string = url_segments[1];
    args_from_urlencoded(query_string.as_bytes())
}

fn args_from_urlencoded(enc: &[u8]) -> BTreeMap<String, String> {
    url::form_urlencoded::parse(enc).into_owned().collect()
}

impl Client {
    pub async fn parse_request<T: FromMap>(
        &self,
        req: Request<Incoming>,
    ) -> Result<Box<T>, TwilioError> {
        let expected = req
            .headers()
            .get("X-Twilio-Signature")
            .ok_or_else(|| TwilioError::AuthError)
            .and_then(|d| base64::decode(d.as_bytes()).map_err(|_| TwilioError::BadRequest))?;

        let (parts, body) = req.into_parts();
        let body = body
            .collect()
            .await
            .unwrap() // Full::Error is Infallible
            .to_bytes();
        let host = match parts.headers.typed_get::<Host>() {
            None => return Err(TwilioError::BadRequest),
            Some(h) => h.hostname().to_string(),
        };
        let request_path = match parts.uri.path() {
            "*" => return Err(TwilioError::BadRequest),
            path => path,
        };
        let (args, post_append) = match parts.method {
            Method::GET => (get_args(request_path), "".to_string()),
            Method::POST => {
                let postargs = args_from_urlencoded(&body);
                let append = postargs
                    .iter()
                    .map(|(k, v)| format!("{}{}", k, v))
                    .collect();
                (postargs, append)
            }
            _ => return Err(TwilioError::BadRequest),
        };

        let effective_uri = format!("https://{}{}{}", host, request_path, post_append);
        let mut hasher = Hmac::<Sha1>::new_from_slice(self.auth_token.as_bytes()).unwrap();
        hasher.update(effective_uri.as_bytes());

        let result = hasher.finalize().into_bytes().to_vec();
        if result != expected {
            return Err(TwilioError::AuthError);
        }

        T::from_map(args)
    }
}
