use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::{ AccountMeta, Instruction },
    pubkey::Pubkey,
    signature::{ Keypair, Signer },
};
use super::sell;
use solana_sdk::system_program;
use super::mongo;
use mongo::MongoHandler;
use helius::types::*;
use helius::Helius;
use spl_associated_token_account::get_associated_token_address;
use spl_associated_token_account::instruction::create_associated_token_account;
use std::str::FromStr;
use serde::Deserialize;
use reqwest::header::*;
use sell::SellTransaction;
use sell::find_sell_signature;

use crate::sell::confirm::confirm_sell;

const GLOBAL: &str = "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf";
const FEE_RECIPIENT: &str = "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM";
const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
pub const EVENT_AUTHORITY: &str = "Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1";
pub const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const PUMP_FUN_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const ASSOCIATED_TOKEN_PROGRAM: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
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
            dbg!("Failed to send transaction on attempt {:?}", &e);
            return Err(Box::new(e)); // Return the error after the last retry
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

    let mongo_handler = MongoHandler::new().await.map_err(|err| {
        format!("Error creating MongoDB handler: {:?}", err)
    })?;

    let private_key = std::env
        ::var("PRIVATE_KEY")
        .expect("You must set the PRIVATE_KEY environment variable!");
    let payer = Keypair::from_base58_string(&private_key);
    let owner = payer.pubkey();
    let mint = Pubkey::from_str(mint_str).unwrap();

    let token_account_address = get_associated_token_address(&owner, &mint);

    let token_balance_result = connection.get_token_account_balance(&token_account_address);

    if let Ok(token_balance) = token_balance_result {
        // Convert the token_amount to the equivalent in decimal form
        let token_amount_decimals =
            (token_amount as f64) / ((10u64).pow(token_balance.decimals as u32) as f64);

        // Retrieve the balance in decimal form
        let balance = token_balance.ui_amount.unwrap_or(0.0);

        // Define a larger epsilon value to consider balances like 0.247686 as effectively zero
        let epsilon = 1.0; // Adjusted to consider balances <= 1.0 as zero

        let buy_transaction = mongo_handler.get_buy_transaction_from_token(
            &sell_transaction.mint,
            "solsniper",
            "buy_transactions"
        ).await?;

        // Convert the buy transaction amount to decimal form
        let buy_transaction_amount_decimals =
            (buy_transaction.amount as f64) / ((10u64).pow(token_balance.decimals as u32) as f64);

        println!("Balance: {}", balance);
        println!("Token Amount: {}", token_amount_decimals);
        println!("Buy Transaction Amount: {}", buy_transaction_amount_decimals);

        if balance == 0.0 {
            match mongo_handler.is_token_sold("solsniper", "tokens", &sell_transaction.mint).await {
                Ok(true) => {
                    return Err("Token already sold".into());
                }
                Ok(false) => {
                    let signature = find_sell_signature(&sell_transaction.mint).await?;

                    if let Err(err) = confirm_sell(&signature, sell_transaction, true).await {
                        return Err(err.into());
                    }
                    return Ok(signature);
                }
                Err(err) => {
                    return Err(err.into());
                }
            }
        }

        // 2. Check if the balance is close to zero
        if balance <= epsilon {
            // If the balance is close to zero, handle the sell signature
            match mongo_handler.is_token_sold("solsniper", "tokens", &sell_transaction.mint).await {
                Ok(true) => {
                    return Err("Token already sold".into());
                }
                Ok(false) => {
                    let signature = find_sell_signature(&sell_transaction.mint).await?;

                    if let Err(err) = confirm_sell(&signature, sell_transaction, true).await {
                        return Err(err.into());
                    }
                    return Ok(signature);
                }
                Err(err) => {
                    return Err(err.into());
                }
            }
        }

        // 1. Check if the balance is sufficient to sell the requested amount
        if balance < token_amount_decimals {
            if buy_transaction_amount_decimals > token_amount_decimals {
                let signature = find_sell_signature(&sell_transaction.mint).await?;

                if let Err(err) = confirm_sell(&signature, sell_transaction, true).await {
                    return Err(err.into());
                }
                return Err("Token amount does not match the buy transaction".into());
            }

            return Err(
                format!(
                    "Insufficient balance. Attempting to sell {} tokens, but only {} tokens are available.",
                    token_amount_decimals,
                    balance
                ).into()
            );
        }

        // 3. Proceed with the normal sell process if none of the above conditions are met
        let mut instructions = vec![];

        if connection.get_account(&token_account_address).is_err() {
            let create_account_instruction = create_associated_token_account(
                &payer.pubkey(),
                &payer.pubkey(),
                &mint,
                &Pubkey::from_str(TOKEN_PROGRAM_ID).unwrap()
            );
            instructions.push(create_account_instruction);
        }

        let max_retries = 4;
        for _ in 0..max_retries {
            let coin_data = match get_coin_data(mint_str).await {
                Ok(data) => data,
                Err(_) => {
                    return Err("Failed to retrieve coin data...".into());
                }
            };

            let virtual_token_reserves = coin_data.virtual_token_reserves as u128;
            let virtual_sol_reserves = coin_data.virtual_sol_reserves as u128;
            let token_amount = token_amount as u128;

            // Calculate SOL output with u128
            let sol_out = (token_amount * virtual_sol_reserves) / virtual_token_reserves;

            // Calculate minimum SOL received with slippage using integer arithmetic
            let slippage_multiplier =
                1_000_000_000u128 + ((slippage_decimal * 1_000_000_000.0) as u128);
            let min_sol_received = (sol_out * slippage_multiplier) / 1_000_000_000;

            // Convert back to u64 safely
            let min_sol_received_u64: u64 = min_sol_received.try_into().map_err(|_| "Overflow")?;

            let sol_out_f64 = (sol_out as f64) / 1_000_000_000.0;
            dbg!(sol_out_f64);

            let keys = vec![
                AccountMeta::new_readonly(Pubkey::from_str(GLOBAL).unwrap(), false),
                AccountMeta::new(Pubkey::from_str(FEE_RECIPIENT).unwrap(), false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new(Pubkey::from_str(&coin_data.bonding_curve).unwrap(), false),
                AccountMeta::new(
                    Pubkey::from_str(&coin_data.associated_bonding_curve).unwrap(),
                    false
                ),
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

            match create_transaction(instructions.clone(), payer.insecure_clone()).await {
                Ok(tx) => {
                    confirm_sell(&tx, sell_transaction, true).await?;
                    return Ok(tx);
                }
                Err(_e) => {
                    instructions.clear(); // Clear instructions to recalculate in the next iteration
                }
            }
        }
        return Err("Failed to create transaction after retries".into());
    } else {
        // Handle the case where fetching the token balance fails
        return Err("Failed to fetch token balance".into());
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
