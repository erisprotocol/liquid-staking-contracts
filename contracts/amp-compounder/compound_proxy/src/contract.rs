use crate::error::ContractError;
use crate::execute::{compound, handle_callback, multi_swap, update_config};
use crate::queries::{get_lp, get_lp_state, get_lps, get_routes, query_config};
use crate::simulation::query_compound_simulation;
use crate::state::{Config, State};

use astroport::asset::addr_opt_validate;
use astroport::common::{claim_ownership, drop_ownership_proposal, propose_new_owner};
use cosmwasm_std::{
    entry_point, to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdError, StdResult,
};
use eris::adapters::factory::Factory;
use eris::compound_proxy::{ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};

/// ## Description
/// Creates a new contract with the specified parameters in the [`InstantiateMsg`].
/// Returns the [`Response`] with the specified attributes if the operation was successful, or a [`ContractError`] if the contract was not created.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    mut deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State::default();

    let factory = if let Some(factory) = msg.factory {
        Some(Factory(deps.api.addr_validate(&factory)?))
    } else {
        None
    };

    state.config.save(
        deps.storage,
        &Config {
            factory,
            owner: deps.api.addr_validate(&msg.owner)?,
        },
    )?;

    for lp in msg.lps {
        state.add_lp(&mut deps, lp)?;
    }

    for route in msg.routes {
        state.add_route(&mut deps, route)?;
    }

    Ok(Response::new())
}

/// ## Description
/// Exposes execute functions available in the contract.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        // ExecuteMsg::Receive(cw20_msg) => receive(deps, env, info, cw20_msg),
        ExecuteMsg::Compound {
            rewards,
            receiver,
            no_swap,
            slippage_tolerance,
            lp_token,
        } => {
            let receiver_addr = addr_opt_validate(deps.api, &receiver)?;

            compound(
                deps,
                env,
                info.clone(),
                rewards,
                receiver_addr.unwrap_or(info.sender),
                no_swap,
                slippage_tolerance,
                lp_token,
            )
        },

        ExecuteMsg::MultiSwap {
            into,
            assets,
            receiver,
        } => {
            let receiver_addr = addr_opt_validate(deps.api, &receiver)?;

            multi_swap(deps, env, info.clone(), into, assets, receiver_addr.unwrap_or(info.sender))
        },

        ExecuteMsg::UpdateConfig {
            ..
        } => update_config(deps, env, info, msg),

        ExecuteMsg::ProposeNewOwner {
            owner,
            expires_in,
        } => {
            let state = State::default();
            let config: Config = state.config.load(deps.storage)?;

            propose_new_owner(
                deps,
                info,
                env,
                owner,
                expires_in,
                config.owner,
                state.ownership_proposal,
            )
            .map_err(|e| e.into())
        },
        ExecuteMsg::DropOwnershipProposal {} => {
            let state = State::default();
            let config: Config = state.config.load(deps.storage)?;

            drop_ownership_proposal(deps, info, config.owner, state.ownership_proposal)
                .map_err(|e| e.into())
        },
        ExecuteMsg::ClaimOwnership {} => {
            let state = State::default();

            claim_ownership(deps, info, env, state.ownership_proposal, |deps, new_owner| {
                let state = State::default();
                state.config.update::<_, StdError>(deps.storage, |mut v| {
                    v.owner = new_owner;
                    Ok(v)
                })?;

                Ok(())
            })
            .map_err(|e| e.into())
        },
        ExecuteMsg::Callback(msg) => handle_callback(deps, env, info, msg),
        // ExecuteMsg::Swap {
        //     ..
        // } => Err(StdError::generic_err("not supported ".to_string()).into()),
    }
}

// fn receive(
//     deps: DepsMut,
//     env: Env,
//     info: MessageInfo,
//     cw20_msg: Cw20ReceiveMsg,
// ) -> Result<Response, ContractError> {
//     let api = deps.api;
//     match from_binary(&cw20_msg.msg)? {
//         ReceiveMsg::Split {
//             into,
//             receiver,
//             slippage_tolerance,
//         } => split(
//             deps,
//             env,
//             info,
//             token_asset(info.sender, cw20_msg.amount),
//             into,
//             deps.api.addr_validate( &receiver.unwrap_or(cw20_msg.sender))?,
//             slippage_tolerance,
//         ),
//     }
// }

/// ## Description
/// Exposes all the queries available in the contract.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
        QueryMsg::GetLp {
            lp_addr,
        } => to_binary(&get_lp(deps, lp_addr)?),
        QueryMsg::GetLpState {
            lp_addr,
        } => to_binary(&get_lp_state(deps, lp_addr)?),
        QueryMsg::GetLps {
            start_after,
            limit,
        } => to_binary(&get_lps(deps, start_after, limit)?),
        QueryMsg::GetRoutes {
            start_after,
            limit,
        } => to_binary(&get_routes(deps, start_after, limit)?),
        QueryMsg::CompoundSimulation {
            rewards,
            lp_token,
        } => to_binary(&query_compound_simulation(deps, rewards, lp_token)?),
    }
}

/// ## Description
/// Used for migration of contract. Returns the default object of type [`Response`].
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> StdResult<Response> {
    Ok(Response::default())
}
