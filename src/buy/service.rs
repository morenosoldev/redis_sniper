use super::raydium_sdk;
use super::utils;
use super::price;
use super::mongo;
use price::get_current_sol_price;
use mongo::{ TokenInfo, BuyTransaction, MongoHandler, TransactionType };
use solana_sdk::signature::Signature;
use utils::{ get_second_instruction_amount, get_token_metadata };
use solana_transaction_status::option_serializer::OptionSerializer;
use solana_transaction_status::UiInnerInstructions;
use solana_transaction_status::UiTransactionEncoding;
use solana_client::nonblocking::rpc_client::RpcClient;
use mongodb::bson::DateTime;
use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

pub async fn save_buy_details(
    client: Arc<RpcClient>,
    signature: &Signature,
    sol_amount: f64,
    lp_decimals: u8,
    key_z: raydium_sdk::LiquidityPoolKeys
) -> Result<(), Box<dyn Error>> {
    let max_retries = 8;
    let retry_delay = Duration::from_secs(20);
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

                let amount: Option<String> = get_second_instruction_amount(&inner_instructions);

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
                    let current_sol_price = get_current_sol_price().await.unwrap_or_default();

                    let usd_amount = sol_amount * current_sol_price;

                    let fee = confirmed_transaction.transaction.meta.unwrap().fee;

                    let fee_sol = (fee as f64) / 1_000_000_000.0;
                    let fee_usd = fee_sol * current_sol_price;

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
                            return Err(err.into());
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
                            return Err(err.into());
                        }
                    };

                    let buy_transaction: BuyTransaction = BuyTransaction {
                        transaction_signature: signature.to_string().clone(),
                        token_info: token_info.clone(),
                        amount,
                        sol_amount,
                        sol_price: current_sol_price,
                        usd_amount,
                        token_metadata: token_metadata.clone(),
                        entry_price: buy_price_usd,
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

                    if
                        let Err(e) = mongo_handler.store_token(
                            token_metadata,
                            "solsniper",
                            "tokens"
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
