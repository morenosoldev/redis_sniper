use std::error::Error;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::time::Duration;
use borsh::{ BorshDeserialize, BorshSerialize };
use serde::{ Deserialize, Serialize };
use solana_client::rpc_config::RpcAccountInfoConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_transaction_status::{
    EncodedConfirmedTransactionWithStatusMeta,
    EncodedTransaction,
    UiMessage,
    UiParsedMessage,
};
use tokio::time::sleep;
use solana_account_decoder::UiAccountEncoding;
use std::convert::TryInto;
use std::str::FromStr;
use super::utils::{ pubkey_to_string, string_to_pubkey };

#[derive(BorshSerialize, BorshDeserialize, Debug, Clone)]
pub struct BondingCurveLayout {
    pub blob1: u64,
    pub virtual_token_reserves: u64,
    pub virtual_sol_reserves: u64,
    pub real_token_reserves: u64,
    pub real_sol_reserves: u64,
    pub blob4: u64,
    pub complete: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct PumpAccounts {
    #[serde(serialize_with = "pubkey_to_string", deserialize_with = "string_to_pubkey")]
    pub mint: Pubkey,
    #[serde(serialize_with = "pubkey_to_string", deserialize_with = "string_to_pubkey")]
    pub bonding_curve: Pubkey,
    #[serde(serialize_with = "pubkey_to_string", deserialize_with = "string_to_pubkey")]
    pub associated_bonding_curve: Pubkey,
    #[serde(serialize_with = "pubkey_to_string", deserialize_with = "string_to_pubkey")]
    pub dev: Pubkey,
    #[serde(serialize_with = "pubkey_to_string", deserialize_with = "string_to_pubkey")]
    pub metadata: Pubkey,
}

pub async fn get_bonding_curve(
    rpc_client: &RpcClient,
    bonding_curve_pubkey: Pubkey
) -> Result<BondingCurveLayout, Box<dyn Error>> {
    const MAX_RETRIES: u32 = 5;
    const INITIAL_DELAY_MS: u64 = 200;
    let mut retries = 0;
    let mut delay = Duration::from_millis(INITIAL_DELAY_MS);

    loop {
        match
            rpc_client.get_account_with_config(&bonding_curve_pubkey, RpcAccountInfoConfig {
                encoding: Some(UiAccountEncoding::Base64),
                commitment: Some(CommitmentConfig::processed()),
                data_slice: None,
                min_context_slot: None,
            }).await
        {
            Ok(res) => {
                if let Some(account) = res.value {
                    let data_length = account.data.len();
                    let data: [u8; 49] = account.data
                        .try_into()
                        .map_err(|_| format!("Invalid data length: {}", data_length))?;
                    println!("Raw bytes: {:?}", data);

                    let layout = BondingCurveLayout {
                        blob1: u64::from_le_bytes(data[0..8].try_into()?),
                        virtual_token_reserves: u64::from_le_bytes(data[8..16].try_into()?),
                        virtual_sol_reserves: u64::from_le_bytes(data[16..24].try_into()?),
                        real_token_reserves: u64::from_le_bytes(data[24..32].try_into()?),
                        real_sol_reserves: u64::from_le_bytes(data[32..40].try_into()?),
                        blob4: u64::from_le_bytes(data[40..48].try_into()?),
                        complete: data[48] != 0,
                    };

                    println!("Parsed BondingCurveLayout: {:?}", layout);
                    return Ok(layout);
                } else {
                    if retries >= MAX_RETRIES {
                        dbg!("Max retries reached. Account not found.");
                        return Err("Account not found after max retries".into());
                    }
                    println!(
                        "Attempt {} failed: Account not found. Retrying in {:?}...",
                        retries + 1,
                        delay
                    );
                    sleep(delay).await;
                    retries += 1;
                    delay = Duration::from_millis(INITIAL_DELAY_MS * (2u64).pow(retries));
                    continue;
                }
            }
            Err(e) => {
                if retries >= MAX_RETRIES {
                    dbg!("Max retries reached. Last dbg: {}", &e);
                    return Err(format!("Max retries reached. Last dbg: {}", e).into());
                }
                println!("Attempt {} failed: {}. Retrying in {:?}...", retries + 1, e, delay);
                sleep(delay).await;
                retries += 1;
                delay = Duration::from_millis(INITIAL_DELAY_MS * (2u64).pow(retries));
            }
        }
    }
}

pub async fn mint_to_pump_accounts(mint: &Pubkey) -> Result<PumpAccounts, Box<dyn Error>> {
    // Constants
    const PUMP_FUN_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

    // Derive the bonding curve address
    let (bonding_curve, _) = Pubkey::find_program_address(
        &[b"bonding-curve", mint.as_ref()],
        &Pubkey::from_str(PUMP_FUN_PROGRAM)?
    );

    // Derive the associated bonding curve address
    let associated_bonding_curve = spl_associated_token_account::get_associated_token_address(
        &bonding_curve,
        mint
    );

    Ok(PumpAccounts {
        mint: *mint,
        bonding_curve,
        associated_bonding_curve,
        dev: Pubkey::default(),
        metadata: Pubkey::default(),
    })
}

pub fn parse_pump_accounts(
    tx: EncodedConfirmedTransactionWithStatusMeta
) -> Result<PumpAccounts, Box<dyn Error>> {
    if let EncodedTransaction::Json(tx) = &tx.transaction.transaction {
        if let UiMessage::Parsed(UiParsedMessage { account_keys, .. }) = &tx.message {
            println!("Account keys: {:?}", account_keys);
            if account_keys.len() >= 5 {
                let dev = account_keys[0].pubkey.parse()?;
                let mint = account_keys[1].pubkey.parse()?;
                let bonding_curve = account_keys[3].pubkey.parse()?;
                let associated_bonding_curve = account_keys[4].pubkey.parse()?;
                let metadata = account_keys[5].pubkey.parse()?;

                return Ok(PumpAccounts {
                    mint,
                    bonding_curve,
                    associated_bonding_curve,
                    dev,
                    metadata,
                });
            } else {
                return Err("Not enough account keys".into());
            }
        }
    }
    Err("Not a JSON transaction".into())
}

pub fn get_token_amount(
    virtual_sol_reserves: u64,
    virtual_token_reserves: u64,
    real_token_reserves: u64,
    lamports: u64
) -> Result<u64, Box<dyn Error>> {
    let virtual_sol_reserves = virtual_sol_reserves as u128;
    let virtual_token_reserves = virtual_token_reserves as u128;
    let amount_in = lamports as u128;

    let reserves_product = virtual_sol_reserves
        .checked_mul(virtual_token_reserves)
        .ok_or("Overflow in reserves product calculation")?;

    let new_virtual_sol_reserve = virtual_sol_reserves
        .checked_add(amount_in)
        .ok_or("Overflow in new virtual SOL reserve calculation")?;

    let new_virtual_token_reserve = reserves_product
        .checked_div(new_virtual_sol_reserve)
        .ok_or("Division by zero or overflow in new virtual token reserve calculation")?
        .checked_add(1)
        .ok_or("Overflow in new virtual token reserve calculation")?;

    let amount_out = virtual_token_reserves
        .checked_sub(new_virtual_token_reserve)
        .ok_or("Underflow in amount out calculation")?;

    let final_amount_out = std::cmp::min(amount_out, real_token_reserves as u128);

    Ok(final_amount_out as u64)
}

pub async fn calculate_pump_price(
    rpc_client: &RpcClient,
    mint: Pubkey
) -> Result<f64, Box<dyn Error>> {
    let pump_accounts = mint_to_pump_accounts(&mint).await?;
    let bonding_curve = get_bonding_curve(rpc_client, pump_accounts.bonding_curve).await?;

    // Directly use BondingCurveLayout in the calculation function
    let sol_price = calculate_pump_curve_price(&bonding_curve).await;

    Ok(sol_price)
}

pub async fn calculate_pump_curve_price(state: &BondingCurveLayout) -> f64 {
    if state.virtual_token_reserves == 0 || state.virtual_sol_reserves == 0 {
        panic!("Invalid reserve data");
    }

    let virtual_token_reserves = state.virtual_token_reserves;
    let virtual_sol_reserves = state.virtual_sol_reserves;

    // Constants for conversions
    const LAMPORTS_PER_SOL: u64 = 1_000_000_000; // Solana's conversion factor for lamports to SOL
    const PUMP_CURVE_TOKEN_DECIMALS: u32 = 6; // Decimals for the token

    // Adjust reserves using integer arithmetic
    let adjusted_virtual_sol_reserves = virtual_sol_reserves / LAMPORTS_PER_SOL;
    let adjusted_virtual_token_reserves =
        virtual_token_reserves / (10u64).pow(PUMP_CURVE_TOKEN_DECIMALS);

    // Calculate the sol price in terms of token and convert to f64 for final division
    let sol_price =
        (adjusted_virtual_sol_reserves as f64) / (adjusted_virtual_token_reserves as f64);

    sol_price
}

pub async fn get_current_sol_price() -> Result<f64, Box<dyn Error>> {
    let url =
        "https://public-api.birdeye.so/defi/price?address=So11111111111111111111111111111111111111112";
    let birdeye_api_key = std::env
        ::var("BIRDEYE_API")
        .expect("You must set the RPC_URL environment variable!");
    let response = reqwest::Client
        ::new()
        .get(url)
        .header("X-API-KEY", birdeye_api_key)
        .send().await?;

    if response.status().is_success() {
        let sol_price_json: serde_json::Value = response.json().await?;
        let sol_price_usd: f64 = sol_price_json["data"]["value"].as_f64().unwrap_or(0.0);
        Ok(sol_price_usd)
    } else {
        Err("Failed to fetch SOL price".into())
    }
}
