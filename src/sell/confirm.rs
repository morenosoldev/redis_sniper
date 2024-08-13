use super::mongo;
use super::sell::SellTransaction;
use super::price;
use super::utils;
use mongo::{ MongoHandler, SellTransaction as SellTransactionMongo, TokenInfo };
use redis::{ Commands, RedisResult };
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::Duration;
use price::get_current_sol_price;
use utils::calculate_sol_amount_received;
use std::error::Error;
use solana_transaction_status::UiTransactionEncoding;
use mongodb::bson::DateTime;
use solana_sdk::signature::Signature;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

pub async fn confirm_sell(
    signature: &Signature,
    sell_transaction: &SellTransaction,
    sol_received: Option<f64>
) -> Result<(), Box<dyn Error>> {
    let rpc_endpoint = std::env
        ::var("RPC_URL")
        .expect("You must set the RPC_URL environment variable!");
    let rpc_client: Arc<RpcClient> = Arc::new(RpcClient::new(rpc_endpoint.to_string()));
    let mongo_handler = MongoHandler::new().await.map_err(|err| {
        format!("Error creating MongoDB handler: {:?}", err)
    })?;

    let mut retry_count = 0;
    let max_retries = 3;
    let retry_delay = Duration::from_secs(10);

    let usd_sol_price = get_current_sol_price().await?;

    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::JsonParsed),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };

    let mut confirmed = false;
    while !confirmed && retry_count <= max_retries {
        match rpc_client.get_transaction_with_config(&signature, config.clone()).await {
            Ok(confirmed_transaction) => {
                let sell_price = sell_transaction.current_token_price_usd;
                let sol_amount = match sol_received {
                    Some(amount) => amount,
                    None => {
                        // Calculate sol_amount if sol_received is not provided
                        calculate_sol_amount_received(
                            &confirmed_transaction,
                            &rpc_client,
                            &Pubkey::from_str(&sell_transaction.mint).unwrap()
                        ).await? as f64
                    }
                };
                let profit = (sol_amount as f64) - (sell_transaction.sol_amount as f64);
                let profit_usd = profit * usd_sol_price;
                let profit_percentage = (profit / (sell_transaction.sol_amount as f64)) * 100.0;
                let fee = confirmed_transaction.transaction.meta.unwrap().fee;

                let fee_sol = (fee as f64) / 1_000_000_000.0;
                let fee_usd = fee_sol * usd_sol_price;

                // Format and print profit percentage
                let profit_percentage_str = format!("{:.4}", profit_percentage);

                // If you need to use the profit percentage as a number
                let profit_percentage_value: f64 = profit_percentage_str
                    .parse()
                    .unwrap_or_default();

                let mut trade_state = mongo_handler.fetch_trade_state(
                    &sell_transaction.mint.clone()
                ).await?;

                let mut buy_transaction = mongo_handler.get_buy_transaction_from_token(
                    &sell_transaction.mint.clone(),
                    "solsniper",
                    "buy_transactions"
                ).await?;

                buy_transaction.amount = buy_transaction.amount - (sell_transaction.amount as f64);
                mongo_handler.update_buy_transaction(&buy_transaction).await?;

                if buy_transaction.amount == 0.0 {
                    mongo_handler.update_token_metadata_sold_field(
                        &sell_transaction.mint,
                        "solsniper",
                        "tokens"
                    ).await?;
                }

                trade_state.taken_out += sol_amount;
                trade_state.remaining -= 0.0;

                mongo_handler.update_trade_state(&trade_state).await?;

                let sell_transaction_mongo = SellTransactionMongo {
                    transaction_signature: signature.to_string(),
                    token_info: TokenInfo {
                        base_mint: sell_transaction.mint.clone(),
                        quote_mint: "So11111111111111111111111111111111111111112".to_string(),
                        base_vault: sell_transaction.base_vault.clone(),
                        quote_vault: sell_transaction.quote_vault.clone(),
                    },
                    amount: sell_transaction.amount as f64,
                    sol_amount: sol_amount as f64,
                    sol_price: sell_transaction.current_token_price_sol,
                    sell_price,
                    entry_price: sell_transaction.entry.clone(),
                    token_metadata: sell_transaction.metadata.clone(),
                    fee_sol: fee_sol,
                    fee_usd: fee_usd,
                    profit,
                    profit_usd,
                    profit_percentage: profit_percentage_value,
                    created_at: DateTime::now(),
                };

                decrease_buy_counter().await?;

                mongo_handler.store_sell_transaction_info(
                    sell_transaction_mongo,
                    "solsniper",
                    "sell_transactions"
                ).await?;

                confirmed = true;
            }
            Err(_err) => {
                retry_count += 1;
                tokio::time::sleep(retry_delay).await;
            }
        }
    }

    if confirmed {
        return Ok(());
    } else {
        return Err("Transaction not confirmed after 3 retries".into());
    }
}

pub async fn decrease_buy_counter() -> RedisResult<()> {
    let redis_url = std::env
        ::var("REDIS_URL")
        .expect("You must set the REDIS_URL environment variable");

    let client = redis::Client::open(redis_url.clone()).expect("Failed to create Redis client");

    let mut con = client.get_connection().expect("Failed to connect to Redis");

    // Get the current value of buy_transaction_count
    let current_count: i32 = con.get("buy_transaction_count").unwrap_or(0);
    println!("Current count: {}", current_count);

    // Calculate the new value by subtracting 1
    let new_count = current_count - 1;
    println!("New count: {}", new_count);

    // Set the new value back to Redis
    let _: () = con
        .set("buy_transaction_count", new_count)
        .expect("Failed to set buy_transaction_count");

    Ok(())
}
