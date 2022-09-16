use super::registry::get_light_client_by_client_id;
use crate::light_client::LightClientHandlerError as Error;
use commitments::prover::prove_state_commitment;
use context::Context;
use enclave_commands::{
    LightClientResult, VerifyChannelInput, VerifyChannelResult, VerifyClientConsensusInput,
    VerifyClientConsensusResult, VerifyClientInput, VerifyClientResult, VerifyConnectionInput,
    VerifyConnectionResult,
};
use light_client::LightClientSource;
use store::KVStore;

pub fn verify_client<'l, S: KVStore, L: LightClientSource<'l>>(
    ctx: &mut Context<S>,
    input: VerifyClientInput,
) -> Result<LightClientResult, Error> {
    let ek = ctx.get_enclave_key();
    let lc = get_light_client_by_client_id::<_, L>(ctx, &input.client_id)?;

    let res = lc.verify_client(
        ctx,
        input.client_id,
        input.target_any_client_state.into(),
        input.prefix,
        input.counterparty_client_id,
        input.proof.0,
        input.proof.1,
    )?;

    Ok(LightClientResult::VerifyClient(VerifyClientResult(
        prove_state_commitment(ek, res.state_commitment)?,
    )))
}

pub fn verify_client_consensus<'l, S: KVStore, L: LightClientSource<'l>>(
    ctx: &mut Context<S>,
    input: VerifyClientConsensusInput,
) -> Result<LightClientResult, Error> {
    let ek = ctx.get_enclave_key();
    let lc = get_light_client_by_client_id::<_, L>(ctx, &input.client_id)?;

    let res = lc.verify_client_consensus(
        ctx,
        input.client_id,
        input.target_any_client_consensus_state.into(),
        input.prefix,
        input.counterparty_client_id,
        input.counterparty_consensus_height,
        input.proof.0,
        input.proof.1,
    )?;

    Ok(LightClientResult::VerifyClientConsensus(
        VerifyClientConsensusResult(prove_state_commitment(ek, res.state_commitment)?),
    ))
}

pub fn verify_connection<'l, S: KVStore, L: LightClientSource<'l>>(
    ctx: &mut Context<S>,
    input: VerifyConnectionInput,
) -> Result<LightClientResult, Error> {
    let ek = ctx.get_enclave_key();
    let lc = get_light_client_by_client_id::<_, L>(ctx, &input.client_id)?;

    let res = lc.verify_connection(
        ctx,
        input.client_id,
        input.expected_connection,
        input.prefix,
        input.counterparty_connection_id,
        input.proof.0,
        input.proof.1,
    )?;

    Ok(LightClientResult::VerifyConnection(VerifyConnectionResult(
        prove_state_commitment(ek, res.state_commitment)?,
    )))
}

pub fn verify_channel<'l, S: KVStore, L: LightClientSource<'l>>(
    ctx: &mut Context<S>,
    input: VerifyChannelInput,
) -> Result<LightClientResult, Error> {
    let ek = ctx.get_enclave_key();
    let lc = get_light_client_by_client_id::<_, L>(ctx, &input.client_id)?;

    let res = lc.verify_channel(
        ctx,
        input.client_id,
        input.expected_channel,
        input.prefix,
        input.counterparty_port_id,
        input.counterparty_channel_id,
        input.proof.0,
        input.proof.1,
    )?;

    Ok(LightClientResult::VerifyChannel(VerifyChannelResult(
        prove_state_commitment(ek, res.state_commitment)?,
    )))
}
