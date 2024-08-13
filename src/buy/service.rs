use super::utils;
use super::price;
use super::price::calculate_pump_price;
use super::mongo;
use price::get_current_sol_price;
use mongo::{ TokenInfo, BuyTransaction, MongoHandler, TransactionType };
use solana_sdk::signature::Signature;
use utils::{ get_second_instruction_amount, calculate_sol_amount_spent, get_token_metadata };
use solana_transaction_status::option_serializer::OptionSerializer;
use solana_transaction_status::UiInnerInstructions;
use solana_transaction_status::UiTransactionEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use mongodb::bson::DateTime;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;
use redis::{ Commands, RedisResult };

#[derive(Debug, Clone)]
pub struct TokenVaults {
    pub base_vault: String,
    pub quote_vault: String,
    pub base_mint: String,
    pub quote_mint: String,
}

pub async fn save_buy_details(
    client: Arc<RpcClient>,
    signature: &Signature,
    lp_decimals: u8,
    mint: &str,
    token_vaults: TokenVaults,
    pump: bool
) -> Result<(), Box<dyn Error>> {
    let max_retries = 3;
    let retry_delay = Duration::from_secs(10);
    let mut retries = 0;

    while retries <= max_retries {
        match client.get_transaction(&signature, UiTransactionEncoding::JsonParsed).await {
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
                    &inner_instructions,
                    pump
                );

                let sol_amount = calculate_sol_amount_spent(&confirmed_transaction).await.unwrap();

                if let Some(ref amount_str) = amount {
                    // Parse the amount as f64
                    let amount = amount_str.parse::<f64>().unwrap_or_default();

                    // Assume `lp_decimals` is of type u8
                    let token_decimals = lp_decimals as f64;

                    // Adjust the token amount using the decimals
                    let adjusted_token_amount = amount / (10f64).powf(token_decimals);

                    let (buy_price_per_token_in_sol, current_sol_price, _buy_price_usd) = if pump {
                        // Use calculate_pump_price if the token_mint ends with "pump"
                        match calculate_pump_price(&client.clone(), mint.parse()?).await {
                            Ok(price) => {
                                let price_per_token_in_sol = price; // Adjust as necessary
                                let current_sol_price =
                                    get_current_sol_price().await.unwrap_or_default();
                                let buy_price_usd = price_per_token_in_sol * current_sol_price;
                                (price_per_token_in_sol, current_sol_price, buy_price_usd)
                            }
                            Err(e) => {
                                return Err(e.into()); // Skip this token on error
                            }
                        }
                    } else {
                        // Calculate the buy price per token in SOL
                        let buy_price_per_token_in_sol = sol_amount / adjusted_token_amount;
                        let current_sol_price = get_current_sol_price().await.unwrap_or_default();
                        let usd_amount = sol_amount * current_sol_price;

                        (buy_price_per_token_in_sol, current_sol_price, usd_amount)
                    };

                    let fee = confirmed_transaction.transaction.meta.unwrap().fee;

                    let fee_sol = (fee as f64) / 1_000_000_000.0;
                    let fee_usd = fee_sol * current_sol_price;

                    // Determine the vaults to use
                    let (base_vault, quote_vault) = if pump {
                        ("".to_string(), "".to_string())
                    } else {
                        (token_vaults.base_vault.to_string(), token_vaults.quote_vault.to_string())
                    };

                    // Determine the base mint and base vault based on whether base_mint is SOL
                    let base_mint_to_use = if
                        token_vaults.base_mint == "So11111111111111111111111111111111111111112"
                    {
                        // If base_mint is SOL, use quote_mint and quote_vault instead
                        token_vaults.quote_mint.to_string()
                    } else {
                        // Otherwise, use base_mint and base_vault
                        token_vaults.base_mint.to_string()
                    };

                    let token_info = TokenInfo {
                        base_mint: base_mint_to_use,
                        quote_mint: token_vaults.quote_mint.to_string(),
                        base_vault,
                        quote_vault,
                    };

                    // Initialize MongoDB handler
                    let mongo_handler = match MongoHandler::new().await {
                        Ok(handler) => handler,
                        Err(err) => {
                            return Err(err.into());
                        }
                    };

                    // Prepare token_metadata and ensure it's not None
                    let token_metadata = loop {
                        match get_token_metadata(&mint, adjusted_token_amount, &client).await {
                            Ok(metadata) => {
                                break metadata;
                            }
                            Err(err) => {
                                // You might want to retry or provide a default value here
                                tokio::time::sleep(retry_delay).await;
                                retries += 1;
                                if retries > max_retries {
                                    return Err(err.into());
                                }
                            }
                        }
                    };

                    let usd_amount = sol_amount * current_sol_price;

                    let buy_transaction: BuyTransaction = BuyTransaction {
                        transaction_signature: signature.to_string().clone(),
                        token_info: token_info.clone(),
                        initial_amount: amount,
                        amount,
                        sol_amount,
                        sol_price: current_sol_price,
                        highest_profit_percentage: 0.0,
                        usd_amount,
                        token_metadata: token_metadata.clone(),
                        entry_price: buy_price_per_token_in_sol,
                        fee_sol,
                        fee_usd,
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

                    increase_buy_counter().await?;

                    if
                        let Err(e) = mongo_handler.store_token(
                            token_metadata,
                            "solsniper",
                            "tokens",
                            sol_amount
                        ).await
                    {
                        eprintln!("Error storing token info: {:?}", e);
                    }
                } else {
                    eprintln!("Error getting amount from inner instructions");
                }

                return Ok(());
            }
            Err(e) => {
                eprintln!("Error getting transaction details: {:?}", e);
                if retries < max_retries {
                    retries += 1;
                    tokio::time::sleep(retry_delay).await;
                } else {
                    return Err(Box::new(e));
                }
            }
        }
    }

    Err("Failed to get transaction details after maximum retries".into())
}

pub async fn increase_buy_counter() -> RedisResult<()> {
    let redis_url = std::env
        ::var("REDIS_URL")
        .expect("You must set the REDIS_URL environment variable");

    let client = redis::Client::open(redis_url.clone()).expect("Failed to create Redis client");

    let mut con = client.get_connection().expect("Failed to connect to Redis");

    // Get the current value of buy_transaction_count
    let current_count: i32 = con.get("buy_transaction_count").unwrap_or(0);
    println!("Current count: {}", current_count);

    // Calculate the new value by subtracting 1
    let new_count = current_count + 1;
    println!("New count: {}", new_count);

    // Set the new value back to Redis
    let _: () = con
        .set("buy_transaction_count", new_count)
        .expect("Failed to set buy_transaction_count");

    Ok(())
}
