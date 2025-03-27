use core::num::NonZeroU32;

use headers::HeaderMapExt;
use hyper::Body;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

use crate::{Client, TwilioError};

impl Client {
    pub async fn lookup_phone_number(&self, number: u64) -> Result<PhoneNumberInfo, TwilioError> {
        // TODO:  Accept Fields as an argument
        let url = format!(
            "https://lookups.twilio.com/v2/PhoneNumbers/+{number}?Fields=line_type_intelligence",
        );

        let mut req = hyper::Request::get(url).body(Body::empty()).unwrap();
        req.headers_mut().typed_insert(self.auth_header.clone());

        let resp = self
            .http_client
            .request(req)
            .await
            .map_err(TwilioError::NetworkError)?;

        let status = resp.status();
        if !status.is_success() {
            return Err(TwilioError::HTTPError(status));
        }

        let decoded = hyper::body::to_bytes(resp.into_body())
            .await
            .map_err(TwilioError::NetworkError)
            .and_then(|bytes| {
                serde_json::from_slice(&bytes).map_err(|_| TwilioError::ParsingError)
            })?;

        Ok(decoded)
    }
}

#[derive(Debug, Deserialize)]
pub struct PhoneNumberInfo {
    // pub call_forwarding: object|null,
    // pub caller_name: object|null,
    pub calling_country_code: String,
    pub country_code: String,
    // pub identity_match: object|null,
    // pub line_status: object|null,
    pub line_type_intelligence: Option<LineTypeIntelligence>,
    pub national_format: String,
    pub phone_number: String,
    // pub phone_number_quality_score: object|null,
    // pub pre_fill: object|null,
    // pub reassigned_number: object|null,
    // pub sim_swap: object|null,
    // pub sms_pumping_risk: object|null,
    pub url: String,
    pub valid: bool,
    #[serde(default)]
    // TODO:  bitflags
    pub validation_errors: Vec<ValidationError>,
}

#[derive(Debug, Deserialize)]
pub struct LineTypeIntelligence {
    pub carrier_name: String,
    pub error_code: Option<NonZeroU32>,
    pub mobile_country_code: String,
    pub mobile_network_code: String,
    #[serde(rename = "type")]
    pub kind: NumberType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberType {
    Landline,
    Mobile,
    FixedVoip,
    NonFixedVoip,
    Personal,
    TollFree,
    Premium,
    SharedCost,
    UniversalAccessNumber,
    Voicemail,
    Pager,
    Unknown,
}

impl<'de> Deserialize<'de> for NumberType {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(de)?;
        let this = match s {
            "landline" => Self::Landline,
            "mobile" => Self::Mobile,
            "fixedVoip" => Self::FixedVoip,
            "nonFixedVoip" => Self::NonFixedVoip,
            "personal" => Self::Personal,
            "tollFree" => Self::TollFree,
            "premium" => Self::Premium,
            "sharedCost" => Self::SharedCost,
            "uan" => Self::UniversalAccessNumber,
            "voicemail" => Self::Voicemail,
            "pager" => Self::Pager,
            "unknown" => Self::Unknown,
            s => return Err(D::Error::custom(format!("Unknown number type '{s}'"))),
        };
        Ok(this)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    TooShort,
    TooLong,
    InvalidButPossible,
    InvalidCountryCode,
    InvalidLength,
    NotANumber,
}

impl<'de> Deserialize<'de> for ValidationError {
    #[inline]
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(de)?;
        let this = match s {
            "TOO_SHORT" => Self::TooShort,
            "TOO_LONG" => Self::TooLong,
            "INVALID_BUT_POSSIBLE" => Self::InvalidButPossible,
            "INVALID_COUNTRY_CODE" => Self::InvalidCountryCode,
            "INVALID_LENGTH" => Self::InvalidLength,
            "NOT_A_NUMBER" => Self::NotANumber,
            s => return Err(D::Error::custom(format!("Unknown validation error '{s}'"))),
        };
        Ok(this)
    }
}
