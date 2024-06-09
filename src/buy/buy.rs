use super::raydium_sdk;
use super::utils;
use super::spltoken;
use super::price;
use super::mongo;
use solana_client::rpc_client::RpcClient;
use price::get_current_sol_price;
use raydium_sdk::LiquidityPoolKeys;
use raydium_sdk::make_swap_fixed_in_instruction;
use raydium_sdk::UserKeys;
use raydium_sdk::LiquiditySwapFixedInInstructionParamsV4;
use solana_sdk::signature::Keypair;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use spltoken::get_or_create_associated_token_account;
use mongo::{ TokenInfo, BuyTransaction, MongoHandler, TransactionType };
use std::time::Duration;
use std::str::FromStr;
use thiserror::Error;
use utils::{ get_second_instruction_amount, get_token_metadata };
use spl_associated_token_account::instruction::create_associated_token_account;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::transaction::Transaction;
use solana_transaction_status::option_serializer::OptionSerializer;
use solana_transaction_status::UiInnerInstructions;
use solana_transaction_status::UiTransactionEncoding;
use mongodb::bson::DateTime;
use std::sync::Arc;

#[derive(Debug, Error)]
pub enum SwapError {
    #[error("Transaction error: {0}")] TransactionError(String),
    #[error("Token error: {0}")] TokenError(String),
    #[error("MongoDB error: {0}")] MongoDBError(String),
    #[error("Invalid transaction data")]
    InvalidTransactionData,
}

pub async fn buy_swap(
    key_z: LiquidityPoolKeys,
    direction: bool,
    lp_decimals: u8,
    sol_amount: f64,
    key_payer: &Keypair,
    wallet_address: &Pubkey
) -> Result<String, SwapError> {
    let mut retry_count = 0;
    let max_retries = 24;
    let retry_delay = Duration::from_secs(2);

    let rpc_endpoint = std::env
        ::var("RPC_URL")
        .expect("You must set the RPC_URL environment variable!");
    let client: Arc<RpcClient> = Arc::new(RpcClient::new(rpc_endpoint.to_string()));

    if
        key_z.base_mint.to_string() == "So11111111111111111111111111111111111111112".to_string() ||
        key_z.quote_mint.to_string() != "So11111111111111111111111111111111111111112".to_string()
    {
        return Err(SwapError::InvalidTransactionData);
    }

    let token_account_in = match
        get_or_create_associated_token_account(
            &client,
            key_payer,
            wallet_address,
            &key_z.quote_mint
        )
    {
        Ok(account) => account,
        Err(err) => {
            return Err(
                SwapError::TokenError(
                    format!("Error getting or creating associated token account: {:?}", err)
                )
            );
        }
    };

    let token_account_out = match
        get_or_create_associated_token_account(&client, key_payer, wallet_address, &key_z.base_mint)
    {
        Ok(account) => account,
        Err(err) => {
            return Err(
                SwapError::TokenError(
                    format!("Error getting or creating associated token account: {:?}", err)
                )
            );
        }
    };

    let amount_in: u64 = (sol_amount * 1_000_000_000.0) as u64;
    let min_amount_out: u64 = 0;

    create_associated_token_account(
        &wallet_address,
        &wallet_address,
        &key_z.base_mint,
        &Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").expect("TOKEN_ID")
    );
    let v: u8 = key_z.version;

    let user_keys = UserKeys::new(
        if direction {
            token_account_in
        } else {
            token_account_out
        },
        if direction {
            token_account_out
        } else {
            token_account_in
        },
        wallet_address.clone()
    );

    let params = LiquiditySwapFixedInInstructionParamsV4::new(
        key_z.clone(),
        user_keys,
        amount_in,
        min_amount_out
    );

    let the_swap_instruction = make_swap_fixed_in_instruction(params, v); // Pass params by reference

    let instructions = vec![
        ComputeBudgetInstruction::set_compute_unit_price(25000),
        ComputeBudgetInstruction::set_compute_unit_limit(600000),
        the_swap_instruction
    ];

    let mut transaction = Transaction::new_with_payer(&instructions, Some(&wallet_address));

    // Adding a delay to ensure pool readiness
    tokio::time::sleep(Duration::from_secs(2)).await;

    loop {
        if retry_count > max_retries {
            return Err(SwapError::TransactionError("Max retries exceeded".to_string()));
        }
        let recent_blockhash = match client.get_latest_blockhash() {
            Ok(blockhash) => blockhash,
            Err(err) => {
                return Err(
                    SwapError::TransactionError(
                        format!("Error getting latest blockhash: {:?}", err)
                    )
                );
            }
        };
        transaction.sign(&[&key_payer], recent_blockhash);

        let result = client.send_and_confirm_transaction_with_spinner_and_commitment(
            &transaction,
            CommitmentConfig::confirmed()
        );

        match result {
            Ok(signature) => {
                let transaction_signature = signature.to_string();
                // Retry loop for confirming the transaction
                let mut confirmed = false;
                while !confirmed && retry_count <= max_retries {
                    match client.get_transaction(&signature, UiTransactionEncoding::JsonParsed) {
                        Ok(confirmed_transaction) => {
                            let inner_instructions: Vec<UiInnerInstructions> =
                                confirmed_transaction.transaction.meta
                                    .as_ref()
                                    .and_then(|data| {
                                        match &data.inner_instructions {
                                            OptionSerializer::Some(inner) => Some(inner.clone()),
                                            _ => None,
                                        }
                                    })
                                    .unwrap_or_else(|| Vec::new());

                            let amount: Option<String> = get_second_instruction_amount(
                                &inner_instructions
                            );

                            if let Some(ref amount_str) = amount {
                                // Parse the amount as f64
                                let amount = amount_str.parse::<f64>().unwrap_or_default();

                                // Assume `lp_decimals` is of type u8
                                let token_decimals = lp_decimals as f64;

                                // Adjust the token amount using the decimals
                                let adjusted_token_amount = amount / (10f64).powf(token_decimals);

                                // Calculate the buy price per token in SOL
                                let buy_price_per_token_in_sol = sol_amount / adjusted_token_amount;

                                // Fetch the current SOL price in USD
                                let current_sol_price =
                                    get_current_sol_price().await.unwrap_or_default();

                                // Calculate the buy price in USD
                                let buy_price_usd = buy_price_per_token_in_sol * current_sol_price;

                                let token_info = TokenInfo {
                                    base_mint: key_z.base_mint.to_string(),
                                    quote_mint: key_z.quote_mint.to_string(),
                                    base_vault: key_z.base_vault.to_string(),
                                    quote_vault: key_z.quote_vault.to_string(),
                                };

                                // Initialize MongoDB handler
                                let mongo_handler = match MongoHandler::new().await {
                                    Ok(handler) => handler,
                                    Err(err) => {
                                        return Err(
                                            SwapError::MongoDBError(
                                                format!("Error creating MongoDB handler: {:?}", err)
                                            )
                                        );
                                    }
                                };

                                let token_metadata = match
                                    get_token_metadata(
                                        &key_z.base_mint.to_string(),
                                        adjusted_token_amount,
                                        &client
                                    ).await
                                {
                                    Ok(metadata) => metadata,
                                    Err(err) => {
                                        return Err(
                                            SwapError::MongoDBError(
                                                format!("Error getting token metadata: {:?}", err)
                                            )
                                        );
                                    }
                                };

                                let buy_transaction: BuyTransaction = BuyTransaction {
                                    transaction_signature: transaction_signature.clone(),
                                    token_info: token_info.clone(),
                                    amount,
                                    sol_amount,
                                    sol_price: current_sol_price,
                                    token_metadata: token_metadata.clone(),
                                    entry_price: buy_price_usd,
                                    transaction_type: TransactionType::LongTermHold,
                                    created_at: DateTime::now(),
                                };

                                // Store transaction info in MongoDB
                                if
                                    let Err(e) = mongo_handler.store_buy_transaction_info(
                                        buy_transaction,
                                        "solsniper",
                                        "buy_transactions"
                                    ).await
                                {
                                    eprintln!("Error storing transaction info: {:?}", e);
                                }

                                if
                                    let Err(e) = mongo_handler.store_token(
                                        token_metadata,
                                        "solsniper",
                                        "tokens"
                                    ).await
                                {
                                    eprintln!("Error storing transaction info: {:?}", e);
                                }
                            } else {
                                eprintln!("Error getting amount from inner instructions");
                            }

                            confirmed = true;
                        }
                        Err(err) => {
                            eprintln!("Error getting confirmed transaction: {:?}", err);
                            if
                                err.to_string().contains("not confirmed") ||
                                err.to_string().contains("invalid type: null")
                            {
                                retry_count += 1;

                                tokio::time::sleep(retry_delay).await;
                            } else {
                                break;
                            }
                        }
                    }
                }

                if confirmed {
                    return Ok(signature.to_string());
                }
            }
            Err(_e) => {
                retry_count += 1;
                tokio::time::sleep(retry_delay).await;
            }
        }
    }
}
