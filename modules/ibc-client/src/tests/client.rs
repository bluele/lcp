use crate::client_def::LCPClient;
use crate::client_state::{ClientState, LCP_CLIENT_STATE_TYPE_URL};
use crate::consensus_state::ConsensusState;
use crate::header::Header;
use crate::prelude::*;
use commitments::{gen_state_id_from_any, UpdateClientCommitment};
use lcp_types::{Any, ClientId, Height};
use light_client::{
    CreateClientResult, Error as LightClientError, HostClientReader, LightClient,
    StateVerificationResult, UpdateClientResult,
};
use light_client_registry::LightClientRegistry;
use validation_context::ValidationParams;

pub const LCP_CLIENT_TYPE: &str = "0000-lcp";

#[derive(Default)]
pub struct LCPLightClient;

// WARNING: This implementation is intended for testing purpose only
// each function always returns the default value as a commitment
impl LightClient for LCPLightClient {
    fn create_client(
        &self,
        _ctx: &dyn HostClientReader,
        any_client_state: Any,
        any_consensus_state: Any,
    ) -> Result<CreateClientResult, LightClientError> {
        let state_id = gen_state_id_from_any(&any_client_state, &any_consensus_state)
            .map_err(LightClientError::commitment)?;
        let client_state = ClientState::try_from(any_client_state)?;
        let consensus_state = ConsensusState::try_from(any_consensus_state)?;
        let height = client_state.latest_height;
        let timestamp = consensus_state.timestamp;

        LCPClient {}.initialise(&client_state, &consensus_state)?;

        Ok(CreateClientResult {
            height,
            commitment: UpdateClientCommitment {
                prev_state_id: None,
                new_state_id: state_id,
                new_state: Some(client_state.into()),
                prev_height: None,
                new_height: height,
                timestamp,
                validation_params: ValidationParams::Empty,
            },
            prove: false,
        })
    }

    fn update_client(
        &self,
        ctx: &dyn HostClientReader,
        client_id: ClientId,
        any_header: Any,
    ) -> Result<UpdateClientResult, LightClientError> {
        let header = Header::try_from(any_header)?;

        // Read client type from the host chain store. The client should already exist.
        let client_type = ctx.client_type(&client_id)?;

        assert!(client_type.eq(LCP_CLIENT_TYPE));

        // Read client state from the host chain store.
        let client_state = ctx.client_state(&client_id)?.try_into()?;

        // if client_state.is_frozen() {
        //     return Err(Error::ICS02Error(ICS02Error::client_frozen(client_id)).into());
        // }

        let height = header.get_height().unwrap_or_default();
        let header_timestamp = header.get_timestamp().unwrap();

        // Use client_state to validate the new header against the latest consensus_state.
        // This function will return the new client_state (its latest_height changed) and a
        // consensus_state obtained from header. These will be later persisted by the keeper.
        let (new_client_state, new_consensus_state) =
            LCPClient {}.check_header_and_update_state(ctx, client_id, client_state, header)?;

        Ok(UpdateClientResult {
            new_any_client_state: Any::from_any(new_client_state),
            new_any_consensus_state: Any::from_any(new_consensus_state),
            height,
            commitment: UpdateClientCommitment::default(),
            prove: false,
        })
    }

    fn client_type(&self) -> String {
        LCP_CLIENT_TYPE.to_owned()
    }

    fn latest_height(
        &self,
        ctx: &dyn HostClientReader,
        client_id: &ClientId,
    ) -> Result<Height, LightClientError> {
        let client_state: ClientState = ctx.client_state(client_id)?.try_into()?;
        Ok(client_state.latest_height)
    }

    fn verify_membership(
        &self,
        _ctx: &dyn HostClientReader,
        _client_id: ClientId,
        _prefix: Vec<u8>,
        _path: String,
        _value: Vec<u8>,
        _proof_height: Height,
        _proof: Vec<u8>,
    ) -> Result<StateVerificationResult, LightClientError> {
        todo!()
    }

    fn verify_non_membership(
        &self,
        _ctx: &dyn HostClientReader,
        _client_id: ClientId,
        _prefix: Vec<u8>,
        _path: String,
        _proof_height: Height,
        _proof: Vec<u8>,
    ) -> Result<StateVerificationResult, LightClientError> {
        todo!()
    }
}

#[allow(dead_code)]
pub fn register_implementations(registry: &mut dyn LightClientRegistry) {
    registry
        .put_light_client(
            LCP_CLIENT_STATE_TYPE_URL.to_string(),
            Box::new(LCPLightClient),
        )
        .unwrap()
}
