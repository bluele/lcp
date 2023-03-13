use crate::prelude::*;
use core::time::Duration;
use flex_error::*;
use light_client::LightClientSpecificError;

define_error! {
    #[derive(Debug, Clone, PartialEq, Eq)]
    Error {
        UnexpectedClientType {
            type_url: String
        }
        |e| {
            format_args!("unexpected client_type: type_url={}", e.type_url)
        },

        UnexpectedHeaderType {
            type_url: String
        }
        |e| {
            format_args!("unexpected header type: type_url={}", e.type_url)
        },

        ExpiredAvr {
            current_timestamp: lcp_types::Time,
            attestation_time: lcp_types::Time,
            client_state_key_expiration: Duration
        }
        |e| {
            format_args!("Expired AVR: current_timestamp= {:?} attestation_time={:?} client_state_key_expiration={:?}", e.current_timestamp, e.attestation_time, e.client_state_key_expiration)
        },

        MrenclaveMismatch {
            expected: Vec<u8>,
            actual: Vec<u8>
        }
        |e| {
            format_args!("Mrenclave mismatch: expected={:?} actual={:?}", e.expected, e.actual)
        },

        AttestationReport
        [attestation_report::Error]
        |_| { "Attestation report error" },

        Time
        [lcp_types::TimeError]
        |_| { "Time error" },

        IbcProto
        [TraceError<ibc_proto::protobuf::Error>]
        |_| { "IBCProto error" }
    }
}

impl From<attestation_report::Error> for Error {
    fn from(err: attestation_report::Error) -> Self {
        Error::attestation_report(err)
    }
}

impl From<lcp_types::TimeError> for Error {
    fn from(err: lcp_types::TimeError) -> Self {
        Error::time(err)
    }
}

impl LightClientSpecificError for Error {}
