use std::error::Error;
use solana_sdk::pubkey::Pubkey;
use borsh::{ BorshDeserialize, BorshSerialize };
use serde::{ Deserialize, Serialize };
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

pub async fn get_current_sol_price() -> Result<f64, Box<dyn Error>> {
    let url = "https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd";
    let response = reqwest::get(url).await?;

    let json: serde_json::Value = response.json().await?;
    let price = json["solana"]["usd"].as_f64().unwrap_or(0.0);

    Ok(price)
}
