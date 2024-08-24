use super::service;
use solana_sdk::{
    instruction::{ AccountMeta, Instruction },
    pubkey::Pubkey,
    signature::{ Keypair, Signer },
};
use service::TokenVaults;
use helius::types::*;
use helius::Helius;
use spl_associated_token_account::get_associated_token_address;
use spl_associated_token_account::instruction::create_associated_token_account;
use std::str::FromStr;
use serde::Deserialize;
use reqwest::header::*;
use solana_client::nonblocking::rpc_client::RpcClient;
use service::save_buy_details;
use std::sync::Arc;
use std::error::Error;
use solana_sdk::signature::Signature;
use solana_sdk::instruction::Instruction as SolanaInstruction;
use solana_client::rpc_config::RpcSendTransactionConfig;

const GLOBAL: &str = "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf";
const FEE_RECIPIENT: &str = "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const RENT: &str = "SysvarRent111111111111111111111111111111111";
const PUMP_FUN_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
const PUMP_FUN_ACCOUNT: &str = "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1";
const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";

const MAX_RETRIES: usize = 4;

async fn create_transaction(
    instructions: Vec<SolanaInstruction>,
    keypair: Keypair
) -> Result<Signature, Box<dyn Error>> {
    let api_key: String = std::env
        ::var("HELIUS_API_KEY")
        .expect("You must set the HELIUS_API_KEY environment variable!");
    let cluster: Cluster = Cluster::MainnetBeta;
    let helius: Helius = Helius::new(&api_key, cluster).unwrap();

    let config = SmartTransactionConfig {
        create_config: CreateSmartTransactionConfig {
            instructions: instructions.clone(),
            signers: vec![&keypair],
            lookup_tables: None,
            fee_payer: None,
        },
        send_options: RpcSendTransactionConfig {
            skip_preflight: true,
            preflight_commitment: None,
            encoding: None,
            max_retries: None,
            min_context_slot: None,
        },
    };

    match helius.send_smart_transaction_with_tip(config, Some(600000), Some("NY")).await {
        Ok(signature) => {
            dbg!("Transaction sent successfully: {}", &signature);
            return Ok(signature);
        }
        Err(e) => {
            dbg!("Failed to send transaction on attempt {}: {:?}", &e);
            return Err("Failed to send transaction".into());
        }
    }
}

pub async fn pump_fun_buy(
    mint_str: &str,
    sol_in: f64,
    slippage_decimal: f64,
    lp_decimals: u8,
    group_title: String,
    user_name: String
) -> Result<Signature, Box<dyn Error>> {
    let rpc_endpoint = std::env
        ::var("RPC_URL")
        .expect("You must set the RPC_URL environment variable!");
    let client: Arc<RpcClient> = Arc::new(RpcClient::new(rpc_endpoint.to_string()));

    let private_key = std::env
        ::var("PRIVATE_KEY")
        .expect("You must set the PRIVATE_KEY environment variable!");
    let payer = Keypair::from_base58_string(&private_key);
    let owner = payer.pubkey();
    let mint = Pubkey::from_str(mint_str).unwrap();

    let mut instructions = vec![];

    let token_account_address = get_associated_token_address(&owner, &mint);

    let token_account_exists = client.get_account(&token_account_address).await.is_ok();
    if !token_account_exists {
        let create_account_instruction = create_associated_token_account(
            &payer.pubkey(),
            &payer.pubkey(),
            &mint,
            &Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap()
        );
        instructions.push(create_account_instruction);
    }

    for _ in 0..MAX_RETRIES {
        let coin_data = match get_coin_data(mint_str).await {
            Ok(data) => data,
            Err(_) => {
                return Err("Failed to retrieve coin data...".into());
            }
        };

        let sol_in_lamports = (sol_in * 1_000_000_000.0) as u128;
        let token_out =
            (sol_in_lamports * (coin_data.virtual_token_reserves as u128)) /
            (coin_data.virtual_sol_reserves as u128);
        println!("Token out: {}", token_out);
        let sol_in_with_slippage = sol_in * (1.0 + slippage_decimal);
        let max_sol_cost = (sol_in_with_slippage * 1_000_000_000.0) as u128;

        let token_out_u64: u64 = token_out.try_into().map_err(|_| "Overflow")?;
        let max_sol_cost_u64: u64 = max_sol_cost.try_into().map_err(|_| "Overflow")?;

        let keys = vec![
            AccountMeta::new_readonly(Pubkey::from_str(GLOBAL).unwrap(), false),
            AccountMeta::new(Pubkey::from_str(FEE_RECIPIENT).unwrap(), false),
            AccountMeta::new_readonly(mint, false),
            AccountMeta::new(Pubkey::from_str(&coin_data.bonding_curve).unwrap(), false),
            AccountMeta::new(Pubkey::from_str(&coin_data.associated_bonding_curve).unwrap(), false),
            AccountMeta::new(token_account_address, false),
            AccountMeta::new(owner, true),
            AccountMeta::new_readonly(Pubkey::from_str(SYSTEM_PROGRAM_ID).unwrap(), false),
            AccountMeta::new_readonly(Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap(), false),
            AccountMeta::new_readonly(Pubkey::from_str(RENT).unwrap(), false),
            AccountMeta::new_readonly(Pubkey::from_str(PUMP_FUN_ACCOUNT).unwrap(), false),
            AccountMeta::new_readonly(Pubkey::from_str(PUMP_FUN_PROGRAM).unwrap(), false)
        ];

        let buy: u64 = 16927863322537952870;
        let mut data = vec![];
        data.extend_from_slice(&buy.to_le_bytes());
        data.extend_from_slice(&token_out_u64.to_le_bytes());
        data.extend_from_slice(&max_sol_cost_u64.to_le_bytes());

        let instruction = Instruction {
            program_id: Pubkey::from_str(PUMP_FUN_PROGRAM).unwrap(),
            accounts: keys,
            data,
        };

        instructions.push(instruction);

        match create_transaction(instructions.clone(), payer.insecure_clone()).await {
            Ok(tx) => {
                let key_z = TokenVaults {
                    base_vault: "".to_string(),
                    quote_vault: "".to_string(),
                    base_mint: mint_str.to_string(),
                    quote_mint: "So11111111111111111111111111111111111111112".to_string(),
                };

                let _saved_details = save_buy_details(
                    client.clone(),
                    &tx,
                    lp_decimals,
                    mint_str,
                    key_z,
                    true,
                    group_title,
                    user_name
                ).await;
                return Ok(tx);
            }
            Err(e) => {
                dbg!("Failed to send transaction: {:?}", e);
                instructions.clear(); // Clear instructions to recalculate in the next iteration
            }
        }
    }

    Err("Failed to create transaction after retries".into())
}

#[derive(Deserialize)]
struct CoinData {
    virtual_token_reserves: u64,
    virtual_sol_reserves: u64,
    bonding_curve: String,
    associated_bonding_curve: String,
}

async fn get_coin_data(mint_str: &str) -> Result<CoinData, Box<dyn std::error::Error>> {
    let url = format!("https://frontend-api.pump.fun/coins/{}", mint_str);
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header(
            USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:125.0) Gecko/20100101 Firefox/125.0"
        )
        .header(ACCEPT, "*/*")
        .header(ACCEPT_LANGUAGE, "en-US,en;q=0.5")
        .header(ACCEPT_ENCODING, "gzip, deflate, br")
        .header(REFERER, "https://www.pump.fun/")
        .header(ORIGIN, "https://www.pump.fun")
        .header(CONNECTION, "keep-alive")
        .send().await?;

    if response.status().is_success() {
        let coin_data: CoinData = response.json().await?;
        Ok(coin_data)
    } else {
        Err(format!("Failed to retrieve coin data: {}", response.status()).into())
    }
}
