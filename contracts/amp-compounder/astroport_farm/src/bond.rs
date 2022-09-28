use astroport::asset::{addr_validate_to_lower, token_asset, Asset};
use astroport::querier::query_token_balance;
use cosmwasm_std::{
    attr, Addr, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Response, Uint128,
};

use crate::error::ContractError;
use crate::state::{Config, ScalingOperation, CONFIG, STATE};

use cw20::Expiration;

use eris::adapters::asset::AssetEx;
use eris::astroport_farm::CallbackMsg;

/// ## Description
/// Send assets to compound proxy to create LP token and bond received LP token on behalf of sender.
pub fn bond_assets(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    assets: Vec<Asset>,
    minimum_receive: Option<Uint128>,
    no_swap: Option<bool>,
    slippage_tolerance: Option<Decimal>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let staking_token = config.lp_token;

    let mut messages: Vec<CosmosMsg> = vec![];
    let mut funds: Vec<Coin> = vec![];

    for asset in assets.iter() {
        asset.deposit_asset(&info, &env.contract.address, &mut messages)?;
        if !asset.amount.is_zero() {
            if asset.is_native_token() {
                funds.push(Coin {
                    denom: asset.info.to_string(),
                    amount: asset.amount,
                });
            } else {
                messages.push(asset.increase_allowance_msg(
                    config.compound_proxy.0.to_string(),
                    Some(Expiration::AtHeight(env.block.height + 1)),
                )?);
            }
        }
    }

    let compound =
        config.compound_proxy.compound_msg(assets, funds, no_swap, slippage_tolerance)?;
    messages.push(compound);

    let prev_balance = query_token_balance(&deps.querier, staking_token, &env.contract.address)?;
    messages.push(
        CallbackMsg::BondTo {
            to: info.sender,
            prev_balance,
            minimum_receive,
        }
        .into_cosmos_msg(&env.contract.address)?,
    );

    Ok(Response::new().add_messages(messages).add_attribute("action", "bond_assets"))
}

/// ## Description
/// Bond available LP token on the contract on behalf of the user.
pub fn bond_to(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    to: Addr,
    prev_balance: Uint128,
    minimum_receive: Option<Uint128>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let balance = query_token_balance(&deps.querier, &config.lp_token, &env.contract.address)?;
    let amount = balance - prev_balance;

    if let Some(minimum_receive) = minimum_receive {
        if amount < minimum_receive {
            return Err(ContractError::AssertionMinimumReceive {
                minimum_receive,
                amount,
            });
        }
    }

    bond_internal(deps, env, config, to, amount)
}

/// ## Description
/// Bond received LP token on behalf of the user.
pub fn bond(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sender_addr: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let staker_addr = addr_validate_to_lower(deps.api, &sender_addr)?;

    let config = CONFIG.load(deps.storage)?;

    // only staking token contract can execute this message
    if config.lp_token != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    bond_internal(deps, env, config, staker_addr, amount)
}

/// Internal bond function used by bond and bond_to
fn bond_internal(
    deps: DepsMut,
    env: Env,
    config: Config,
    staker_addr: Addr,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let lp_balance = config.staking_contract.query_deposit(
        &deps.querier,
        &config.lp_token,
        &env.contract.address,
    )?;

    let mut messages: Vec<CosmosMsg> = vec![];

    let mut state = STATE.load(deps.storage)?;

    //TODO: withdraw reward to pending reward; before changing share

    // calculate share
    let bond_share = state.calc_bond_share(amount, lp_balance, ScalingOperation::Truncate);
    state.total_bond_share += bond_share;
    messages.push(state.amp_lp_token.mint(bond_share, staker_addr)?);

    STATE.save(deps.storage, &state)?;

    messages.push(config.staking_contract.deposit_msg(config.lp_token.to_string(), amount)?);
    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "bond"),
        attr("amount", amount),
        attr("bond_amount", amount),
    ]))
}

/// ## Description
/// Unbond LP token of sender
pub fn unbond(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    sender_addr: String,
    amount: Uint128,
) -> Result<Response, ContractError> {
    let staker_addr = addr_validate_to_lower(deps.api, &sender_addr)?;
    let mut state = STATE.load(deps.storage)?;

    // only amp LP token contract can execute this message
    if state.amp_lp_token.0 != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    let config = CONFIG.load(deps.storage)?;
    let staking_token = config.lp_token;

    let lp_balance = config.staking_contract.query_deposit(
        &deps.querier,
        &staking_token,
        &env.contract.address,
    )?;

    let bond_amount = state.calc_bond_amount(lp_balance, amount);
    state.total_bond_share = state.total_bond_share.checked_sub(amount)?;

    // update state
    STATE.save(deps.storage, &state)?;

    Ok(Response::new()
        .add_messages(vec![
            state.amp_lp_token.burn(amount)?,
            config.staking_contract.withdraw_msg(staking_token.to_string(), bond_amount)?,
            token_asset(staking_token, bond_amount).transfer_msg(&staker_addr)?,
        ])
        .add_attributes(vec![
            attr("action", "unbond"),
            attr("staker_addr", staker_addr),
            attr("amount", bond_amount),
        ]))
}
