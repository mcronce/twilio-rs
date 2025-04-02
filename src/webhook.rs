use crate::{Client, FromMap, TwilioError};
use core::fmt;
use core::str::FromStr;
use headers::{HeaderMapExt, Host};
use hmac::{Hmac, Mac};
use hyper::{Body, Method, Request};
use sha1::Sha1;
use std::collections::BTreeMap;

#[derive(Debug)]
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

impl fmt::Display for MessageStatus {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
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
        };
        f.write_str(s)
    }
}

impl FromStr for MessageStatus {
    type Err = InvalidMessageStatus;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let this = match s {
            "queued" => Self::Queued,
            "sending" => Self::Sending,
            "sent" => Self::Sent,
            "failed" => Self::Failed,
            "delivered" => Self::Delivered,
            "undelivered" => Self::Undelivered,
            "receiving" => Self::Receiving,
            "received" => Self::Received,
            "accepted" => Self::Accepted,
            "scheduled" => Self::Scheduled,
            "read" => Self::Read,
            "partially_delivered" => Self::PartiallyDelivered,
            "canceled" => Self::Canceled,
            _ => return Err(InvalidMessageStatus(s.to_owned())),
        };
        Ok(this)
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
        req: Request<Body>,
    ) -> Result<Box<T>, TwilioError> {
        let expected = req
            .headers()
            .get("X-Twilio-Signature")
            .ok_or_else(|| TwilioError::AuthError)
            .and_then(|d| base64::decode(d.as_bytes()).map_err(|_| TwilioError::BadRequest))?;

        let (parts, body) = req.into_parts();
        let body = hyper::body::to_bytes(body)
            .await
            .map_err(TwilioError::NetworkError)?;
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
