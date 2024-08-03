use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{ AccountMeta, Instruction },
    pubkey::Pubkey,
    signature::{ Keypair, Signer },
};
use super::sell;
use solana_sdk::system_program;
use helius::types::*;
use helius::Helius;
use spl_associated_token_account::get_associated_token_address;
use spl_associated_token_account::instruction::create_associated_token_account;
use std::str::FromStr;
use serde::Deserialize;
use reqwest::header::*;
use sell::SellTransaction;

use crate::sell::confirm::confirm_sell;

const GLOBAL: &str = "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf";
const FEE_RECIPIENT: &str = "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const EVENT_AUTHORITY: &str = "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1";
pub const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const RENT: &str = "SysvarRent111111111111111111111111111111111";
const PUMP_FUN_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const ASSOCIATED_TOKEN_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const PUMP_FUN_ACCOUNT: &str = "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1";
const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";
use std::error::Error;
use solana_sdk::signature::Signature;
use solana_sdk::instruction::Instruction as SolanaInstruction;
use solana_client::rpc_config::RpcSendTransactionConfig;

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
            instructions,
            signers: vec![&keypair],
            lookup_tables: None,
            fee_payer: None,
        },
        send_options: RpcSendTransactionConfig {
            skip_preflight: true,
            preflight_commitment: None,
            encoding: None,
            max_retries: Some(4),
            min_context_slot: None,
        },
    };

    match helius.send_smart_transaction(config).await {
        Ok(signature) => {
            dbg!("Transaction sent successfully: {}", signature);
            Ok(signature)
        }
        Err(e) => {
            dbg!("Failed to send transaction: {:?}", &e);
            Err(Box::new(e)) // Convert the error to a boxed dynamic error
        }
    }
}

pub async fn pump_fun_sell(
    mint_str: &str,
    token_amount: u64,
    slippage_decimal: f64,
    sell_transaction: &SellTransaction
) -> Result<Signature, Box<dyn Error>> {
    let rpc_endpoint = std::env
        ::var("RPC_URL")
        .expect("You must set the RPC_URL environment variable!");
    let connection = RpcClient::new_with_commitment(
        rpc_endpoint.to_string(),
        CommitmentConfig::confirmed()
    );

    let coin_data = match get_coin_data(mint_str).await {
        Ok(data) => data,
        Err(_) => {
            eprintln!("Failed to retrieve coin data...");
            return Err("Failed to retrieve coin data...".into());
        }
    };

    let private_key = std::env
        ::var("PRIVATE_KEY")
        .expect("You must set the PRIVATE_KEY environment variable!");
    let payer = Keypair::from_base58_string(&private_key);
    let owner = payer.pubkey();
    let mint = Pubkey::from_str(mint_str).unwrap();

    let mut instructions = vec![];

    let token_account_address = get_associated_token_address(&owner, &mint);

    if connection.get_account(&token_account_address).is_err() {
        let create_account_instruction = create_associated_token_account(
            &payer.pubkey(),
            &payer.pubkey(),
            &mint,
            &Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap()
        );
        instructions.push(create_account_instruction);
    }

    let virtual_token_reserves = coin_data.virtual_token_reserves as u128;
    let virtual_sol_reserves = coin_data.virtual_sol_reserves as u128;
    let token_amount = token_amount as u128;

    // Calculate SOL output with u128
    let sol_out = (token_amount * virtual_sol_reserves) / virtual_token_reserves;

    // Calculate minimum SOL received with slippage using integer arithmetic
    let slippage_multiplier = 1_000_000_000u128 + ((slippage_decimal * 1_000_000_000.0) as u128);
    let min_sol_received = (sol_out * slippage_multiplier) / 1_000_000_000;

    // Convert back to u64 safely
    let sol_out_u64: u64 = sol_out.try_into().map_err(|_| "Overflow")?;
    let min_sol_received_u64: u64 = min_sol_received.try_into().map_err(|_| "Overflow")?;

    let sol_out_f64 = (sol_out as f64) / 1_000_000_000.0;
    dbg!(sol_out_f64);

    let keys = vec![
        AccountMeta::new_readonly(Pubkey::from_str(GLOBAL).unwrap(), false),
        AccountMeta::new(Pubkey::from_str(FEE_RECIPIENT).unwrap(), false),
        AccountMeta::new_readonly(mint, false),
        AccountMeta::new(Pubkey::from_str(&coin_data.bonding_curve).unwrap(), false),
        AccountMeta::new(Pubkey::from_str(&coin_data.associated_bonding_curve).unwrap(), false),
        AccountMeta::new(token_account_address, false),
        AccountMeta::new(owner, true),
        AccountMeta::new_readonly(system_program::ID, false),
        AccountMeta::new_readonly(Pubkey::from_str(ASSOCIATED_TOKEN_PROGRAM)?, false),
        AccountMeta::new_readonly(Pubkey::from_str(TOKEN_PROGRAM)?, false),
        AccountMeta::new_readonly(Pubkey::from_str(EVENT_AUTHORITY)?, false),
        AccountMeta::new_readonly(Pubkey::from_str(PUMP_FUN_PROGRAM)?, false)
    ];

    let sell: u64 = 12502976635542562355; // Replace with your specific instruction data
    let mut data = vec![];
    data.extend_from_slice(&sell.to_le_bytes());
    data.extend_from_slice(&token_amount.to_le_bytes());
    data.extend_from_slice(&min_sol_received_u64.to_le_bytes());

    let instruction = Instruction {
        program_id: Pubkey::from_str(PUMP_FUN_PROGRAM).unwrap(),
        accounts: keys,
        data,
    };

    instructions.push(instruction);

    match create_transaction(instructions, payer).await {
        Ok(tx) => {
            println!("Transaction sent successfully: {}", tx);
            confirm_sell(&tx, sell_transaction, Some(sol_out_f64)).await?;

            Ok(tx)
        }
        Err(e) => {
            eprintln!("Failed to create transaction...");
            Err(e)
        }
    }
}

// Struct for CoinData
#[derive(Deserialize)]
struct CoinData {
    virtual_token_reserves: u64,
    virtual_sol_reserves: u64,
    bonding_curve: String,
    associated_bonding_curve: String,
}

// Fetch coin data
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
