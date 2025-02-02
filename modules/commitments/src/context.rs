use crate::prelude::*;
use crate::{Error, EthABIEncoder};
use core::{fmt::Display, time::Duration};
use lcp_types::{nanos_to_duration, Time};
use serde::{Deserialize, Serialize};

pub const COMMITMENT_CONTEXT_TYPE_EMPTY: u16 = 0;
pub const COMMITMENT_CONTEXT_TYPE_WITHIN_TRUSTING_PERIOD: u16 = 1;
pub const COMMITMENT_CONTEXT_HEADER_SIZE: usize = 32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CommitmentContext {
    Empty,
    TrustingPeriod(TrustingPeriodContext),
}

impl CommitmentContext {
    pub fn validate(&self, current_timestamp: Time) -> Result<(), Error> {
        match self {
            CommitmentContext::Empty => Ok(()),
            CommitmentContext::TrustingPeriod(ctx) => ctx.validate(current_timestamp),
        }
    }

    // MSB first
    // 0-1:  type
    // 2-31: reserved
    pub fn header(&self) -> [u8; COMMITMENT_CONTEXT_HEADER_SIZE] {
        let mut header = [0u8; COMMITMENT_CONTEXT_HEADER_SIZE];

        match self {
            CommitmentContext::Empty => {
                header[0..=1].copy_from_slice(&COMMITMENT_CONTEXT_TYPE_EMPTY.to_be_bytes());
            }
            CommitmentContext::TrustingPeriod(_) => {
                header[0..=1]
                    .copy_from_slice(&COMMITMENT_CONTEXT_TYPE_WITHIN_TRUSTING_PERIOD.to_be_bytes());
            }
        }
        header
    }

    fn parse_context_type_from_header(header_bytes: &[u8]) -> Result<u16, Error> {
        if header_bytes.len() != COMMITMENT_CONTEXT_HEADER_SIZE {
            return Err(Error::invalid_commitment_context_header(format!(
                "invalid commitment context header length: expected={} actual={}",
                COMMITMENT_CONTEXT_HEADER_SIZE,
                header_bytes.len()
            )));
        }

        let mut header = [0u8; COMMITMENT_CONTEXT_HEADER_SIZE];
        header.copy_from_slice(header_bytes);

        Ok(u16::from_be_bytes([header[0], header[1]]))
    }
}

impl EthABIEncoder for CommitmentContext {
    fn ethabi_encode(self) -> Vec<u8> {
        let header = self.header().as_ref().try_into().unwrap();
        match self {
            CommitmentContext::Empty => EthABICommitmentContext {
                header,
                context_bytes: vec![],
            }
            .encode(),
            CommitmentContext::TrustingPeriod(ctx) => EthABICommitmentContext {
                header,
                context_bytes: ctx.ethabi_encode(),
            }
            .encode(),
        }
    }
    fn ethabi_decode(bz: &[u8]) -> Result<Self, Error> {
        let EthABICommitmentContext {
            header,
            context_bytes,
        } = EthABICommitmentContext::decode(bz)?;

        match CommitmentContext::parse_context_type_from_header(&header)? {
            COMMITMENT_CONTEXT_TYPE_EMPTY => {
                assert!(context_bytes.is_empty());
                Ok(CommitmentContext::Empty)
            }
            COMMITMENT_CONTEXT_TYPE_WITHIN_TRUSTING_PERIOD => {
                let ctx = TrustingPeriodContext::ethabi_decode(&context_bytes)?;
                Ok(CommitmentContext::TrustingPeriod(ctx))
            }
            type_ => Err(Error::invalid_commitment_context_header(format!(
                "unknown commitment context type: {}",
                type_
            ))),
        }
    }
}

pub(crate) struct EthABICommitmentContext {
    header: ethabi::FixedBytes,   // bytes32
    context_bytes: ethabi::Bytes, // bytes
}

impl EthABICommitmentContext {
    fn encode(&self) -> Vec<u8> {
        use ethabi::Token;
        ethabi::encode(&[Token::Tuple(vec![
            Token::FixedBytes(self.header.clone()),
            Token::Bytes(self.context_bytes.clone()),
        ])])
    }
    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        use ethabi::ParamType;
        let tuple = ethabi::decode(
            &[ParamType::Tuple(vec![
                ParamType::FixedBytes(32),
                ParamType::Bytes,
            ])],
            bytes,
        )?
        .into_iter()
        .next()
        .unwrap()
        .into_tuple()
        .unwrap();

        // if the decoding is successful, the length of the tuple should be 2
        assert!(tuple.len() == 2);
        let mut values = tuple.into_iter();
        Ok(Self {
            header: values.next().unwrap().into_fixed_bytes().unwrap(),
            context_bytes: values.next().unwrap().into_bytes().unwrap(),
        })
    }
}

impl Default for CommitmentContext {
    fn default() -> Self {
        CommitmentContext::Empty
    }
}

impl Display for CommitmentContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CommitmentContext::Empty => write!(f, "Empty"),
            CommitmentContext::TrustingPeriod(ctx) => write!(f, "TrustingPeriod {{{}}}", ctx),
        }
    }
}

/// NOTE: time precision is in seconds (i.e. nanoseconds are truncated)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustingPeriodContext {
    /// How long a validator set is trusted for (must be shorter than the chain's
    /// unbonding period)
    trusting_period: Duration,

    /// Correction parameter dealing with only approximately synchronized clocks.
    /// The local clock should always be ahead of timestamps from the blockchain; this
    /// is the maximum amount that the local clock may drift behind a timestamp from the
    /// blockchain.
    clock_drift: Duration,

    /// The timestamp of the untrusted header
    /// NOTE: The header is used to update the state of the light client.
    untrusted_header_timestamp: Time,

    /// The timestamp of the trusted state
    /// NOTE: The state is a previously verified state of the light client.
    trusted_state_timestamp: Time,
}

impl TrustingPeriodContext {
    pub fn new(
        trusting_period: Duration,
        clock_drift: Duration,
        untrusted_header_timestamp: Time,
        trusted_state_timestamp: Time,
    ) -> Self {
        Self {
            trusting_period,
            clock_drift,
            untrusted_header_timestamp,
            trusted_state_timestamp,
        }
    }

    pub fn validate(&self, current_timestamp: Time) -> Result<(), Error> {
        // ensure that trusted consensus state's timestamp hasn't passed the trusting period
        Self::ensure_within_trust_period(
            current_timestamp,
            self.trusted_state_timestamp,
            self.trusting_period,
        )?;

        // ensure the header isn't from a future time
        Self::ensure_header_from_past(
            current_timestamp,
            self.untrusted_header_timestamp,
            self.clock_drift,
        )?;

        Ok(())
    }

    fn ensure_within_trust_period(
        now: Time,
        trusted_state_time: Time,
        trusting_period: Duration,
    ) -> Result<(), Error> {
        let trusting_period_end = (trusted_state_time + trusting_period)?;
        if trusting_period_end > now {
            Ok(())
        } else {
            Err(Error::out_of_trusting_period(now, trusting_period_end))
        }
    }

    fn ensure_header_from_past(
        now: Time,
        untrusted_header_time: Time,
        clock_drift: Duration,
    ) -> Result<(), Error> {
        let current = (now + clock_drift)?;
        if current > untrusted_header_time {
            Ok(())
        } else {
            Err(Error::header_from_future(now, untrusted_header_time))
        }
    }
}

impl Display for TrustingPeriodContext {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "trusting_period={} clock_drift={} untrusted_header_timestamp={} trusted_state_timestamp={}",
            self.trusting_period.as_secs(), self.clock_drift.as_secs(), self.untrusted_header_timestamp, self.trusted_state_timestamp
        )
    }
}

impl EthABIEncoder for TrustingPeriodContext {
    fn ethabi_encode(self) -> Vec<u8> {
        let mut timestamps = [0u8; 32];
        timestamps[0..=15].copy_from_slice(
            &self
                .untrusted_header_timestamp
                .as_unix_timestamp_nanos()
                .to_be_bytes(),
        );
        timestamps[16..=31].copy_from_slice(
            &self
                .trusted_state_timestamp
                .as_unix_timestamp_nanos()
                .to_be_bytes(),
        );
        let mut params = [0u8; 32];
        params[0..=15].copy_from_slice(&self.trusting_period.as_nanos().to_be_bytes());
        params[16..=31].copy_from_slice(&self.clock_drift.as_nanos().to_be_bytes());
        EthABITrustingPeriodContext {
            timestamps: timestamps.to_vec(),
            params: params.to_vec(),
        }
        .encode()
    }
    fn ethabi_decode(bz: &[u8]) -> Result<Self, Error> {
        let c = EthABITrustingPeriodContext::decode(bz)?;
        let trusting_period =
            nanos_to_duration(u128::from_be_bytes(c.params[0..=15].try_into().unwrap()))?;
        let clock_drift =
            nanos_to_duration(u128::from_be_bytes(c.params[16..=31].try_into().unwrap()))?;
        let untrusted_header_timestamp = Time::from_unix_timestamp_nanos(u128::from_be_bytes(
            c.timestamps[0..=15].try_into().unwrap(),
        ))?;
        let trusted_state_timestamp = Time::from_unix_timestamp_nanos(u128::from_be_bytes(
            c.timestamps[16..=31].try_into().unwrap(),
        ))?;
        Ok(Self {
            trusting_period,
            clock_drift,
            untrusted_header_timestamp,
            trusted_state_timestamp,
        })
    }
}

impl From<TrustingPeriodContext> for CommitmentContext {
    fn from(ctx: TrustingPeriodContext) -> Self {
        CommitmentContext::TrustingPeriod(ctx)
    }
}

pub(crate) struct EthABITrustingPeriodContext {
    /// bytes32 in solidity
    /// MSB first
    /// 0-15: untrusted_header_timestamp
    /// 16-31: trusted_state_timestamp
    pub timestamps: ethabi::FixedBytes,
    /// bytes32 in solidity
    /// MSB first
    /// 0-15: trusting_period
    /// 16-31: clock_drift
    pub params: ethabi::FixedBytes,
}

impl EthABITrustingPeriodContext {
    fn encode(self) -> Vec<u8> {
        use ethabi::Token;
        ethabi::encode(&[Token::Tuple(vec![
            Token::FixedBytes(self.timestamps),
            Token::FixedBytes(self.params),
        ])])
    }
    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        use ethabi::ParamType;
        let tuple = ethabi::decode(
            &[ParamType::Tuple(vec![
                ParamType::FixedBytes(32),
                ParamType::FixedBytes(32),
            ])],
            bytes,
        )?
        .into_iter()
        .next()
        .unwrap()
        .into_tuple()
        .unwrap();
        assert!(tuple.len() == 2);
        let mut values = tuple.into_iter();
        Ok(Self {
            timestamps: values.next().unwrap().into_fixed_bytes().unwrap(),
            params: values.next().unwrap().into_fixed_bytes().unwrap(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::ErrorDetail;
    use lcp_types::MAX_UNIX_TIMESTAMP_NANOS;
    use proptest::prelude::*;
    use time::{macros::datetime, OffsetDateTime};

    proptest! {
        #[test]
        fn pt_trusting_period_context(
            trusting_period in ..=MAX_UNIX_TIMESTAMP_NANOS,
            clock_drift in ..=MAX_UNIX_TIMESTAMP_NANOS,
            untrusted_header_timestamp in ..=MAX_UNIX_TIMESTAMP_NANOS,
            trusted_state_timestamp in ..=MAX_UNIX_TIMESTAMP_NANOS
        ) {
            let ctx: CommitmentContext = TrustingPeriodContext::new(
                nanos_to_duration(trusting_period).unwrap(),
                nanos_to_duration(clock_drift).unwrap(),
                Time::from_unix_timestamp_nanos(untrusted_header_timestamp).unwrap(),
                Time::from_unix_timestamp_nanos(trusted_state_timestamp).unwrap(),
            ).into();
            let bz = ctx.clone().ethabi_encode();
            let ctx2 = CommitmentContext::ethabi_decode(&bz).unwrap();
            assert_eq!(ctx, ctx2);
        }
    }

    #[test]
    fn test_empty_context_serialization() {
        let ctx = CommitmentContext::Empty;
        let bz = ctx.clone().ethabi_encode();
        let ctx2 = CommitmentContext::ethabi_decode(&bz).unwrap();
        assert_eq!(ctx, ctx2);
    }

    #[test]
    fn test_trusting_period_context_serialization() {
        let ctx = CommitmentContext::TrustingPeriod(TrustingPeriodContext::new(
            Duration::new(60 * 60 * 24, 0),
            Duration::new(60 * 60, 0),
            Time::now(),
            Time::now(),
        ));
        let bz = ctx.clone().ethabi_encode();
        let ctx2 = CommitmentContext::ethabi_decode(&bz).unwrap();
        assert_eq!(ctx, ctx2);
    }

    #[test]
    fn test_context_header() {
        let ctx = CommitmentContext::Empty;
        let header = ctx.header();
        assert_eq!(
            COMMITMENT_CONTEXT_TYPE_EMPTY,
            CommitmentContext::parse_context_type_from_header(&header).unwrap()
        );

        let ctx = CommitmentContext::TrustingPeriod(TrustingPeriodContext::new(
            Duration::new(60 * 60 * 24, 0),
            Duration::new(60 * 60, 0),
            Time::now(),
            Time::now(),
        ));
        let header = ctx.header();
        assert_eq!(
            COMMITMENT_CONTEXT_TYPE_WITHIN_TRUSTING_PERIOD,
            CommitmentContext::parse_context_type_from_header(&header).unwrap()
        );
    }

    fn build_trusting_period_context(
        trusting_period_nanos: u128,
        clock_drift_nanos: u128,
        untrusted_header_timestamp: OffsetDateTime,
        trusted_state_timestamp: OffsetDateTime,
    ) -> TrustingPeriodContext {
        TrustingPeriodContext::new(
            nanos_to_duration(trusting_period_nanos).unwrap(),
            nanos_to_duration(clock_drift_nanos).unwrap(),
            Time::from_unix_timestamp_nanos(
                untrusted_header_timestamp.unix_timestamp_nanos() as u128
            )
            .unwrap(),
            Time::from_unix_timestamp_nanos(trusted_state_timestamp.unix_timestamp_nanos() as u128)
                .unwrap(),
        )
    }

    fn validate_and_assert_no_error(ctx: TrustingPeriodContext, current_timestamp: OffsetDateTime) {
        let res = ctx.validate(
            Time::from_unix_timestamp_nanos(current_timestamp.unix_timestamp_nanos() as u128)
                .unwrap(),
        );
        assert!(res.is_ok(), "{:?}", res);
    }

    fn validate_and_assert_trusting_period_error(
        ctx: TrustingPeriodContext,
        current_timestamp: OffsetDateTime,
    ) {
        let res = ctx.validate(
            Time::from_unix_timestamp_nanos(current_timestamp.unix_timestamp_nanos() as u128)
                .unwrap(),
        );
        assert!(res.is_err());
        if let ErrorDetail::OutOfTrustingPeriod(_) = res.as_ref().err().unwrap().detail() {
        } else {
            panic!("{:?}", res);
        }
    }

    fn validate_and_assert_clock_drift_error(
        ctx: TrustingPeriodContext,
        current_timestamp: OffsetDateTime,
    ) {
        let res = ctx.validate(
            Time::from_unix_timestamp_nanos(current_timestamp.unix_timestamp_nanos() as u128)
                .unwrap(),
        );
        assert!(res.is_err());
        if let ErrorDetail::HeaderFromFuture(_) = res.as_ref().err().unwrap().detail() {
        } else {
            panic!("{:?}", res);
        }
    }

    #[test]
    fn test_trusting_period_context() {
        {
            let current_timestamp = datetime!(2023-08-20 0:00 UTC);
            let untrusted_header_timestamp = datetime!(2023-08-20 0:00 UTC);
            let trusted_state_timestamp = datetime!(2023-08-20 0:00 UTC);
            let ctx = build_trusting_period_context(
                1,
                1,
                untrusted_header_timestamp,
                trusted_state_timestamp,
            );
            validate_and_assert_no_error(ctx, current_timestamp);
        }

        // trusting_period
        {
            let current_timestamp = datetime!(2023-08-20 0:00 UTC);
            let untrusted_header_timestamp = current_timestamp - Duration::new(0, 1);
            let trusted_state_timestamp = untrusted_header_timestamp - Duration::new(0, 1);

            let ctx = build_trusting_period_context(
                1,
                0,
                untrusted_header_timestamp,
                trusted_state_timestamp,
            );
            validate_and_assert_trusting_period_error(ctx, current_timestamp);

            let ctx = build_trusting_period_context(
                2,
                0,
                untrusted_header_timestamp,
                trusted_state_timestamp,
            );
            validate_and_assert_trusting_period_error(ctx, current_timestamp);

            let ctx = build_trusting_period_context(
                3,
                0,
                untrusted_header_timestamp,
                trusted_state_timestamp,
            );
            validate_and_assert_no_error(ctx, current_timestamp);
        }

        // clock drift
        {
            let current_timestamp = datetime!(2023-08-20 0:00 UTC);
            let untrusted_header_timestamp = current_timestamp + Duration::new(0, 1);
            let trusted_state_timestamp = current_timestamp;
            let ctx = build_trusting_period_context(
                1,
                0,
                untrusted_header_timestamp,
                trusted_state_timestamp,
            );
            validate_and_assert_clock_drift_error(ctx, current_timestamp);
            let ctx = build_trusting_period_context(
                1,
                1,
                untrusted_header_timestamp,
                trusted_state_timestamp,
            );
            validate_and_assert_clock_drift_error(ctx, current_timestamp);
            let ctx = build_trusting_period_context(
                1,
                2,
                untrusted_header_timestamp,
                trusted_state_timestamp,
            );
            validate_and_assert_no_error(ctx, current_timestamp);
        }
    }
}
