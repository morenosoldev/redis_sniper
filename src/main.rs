mod sell;
mod buy;
use redis::RedisResult;
use futures_util::StreamExt;
use serde::{ Serialize, Deserialize };
use dotenv::dotenv;
use buy::raydium_sdk::LiquidityPoolKeysString;
use buy::pump::pump_fun_buy;
use tokio::time::{ sleep, Duration };
use std::time::Instant;
use buy::utils::get_liquidity_pool;
use sell::utils::get_liquidity_pool as get_sell_liquidity_pool;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use sell::sell::SellTransaction;
use sell::pump::pump_fun_sell;
use sell::sell::sell_swap;

#[derive(Debug, Serialize, Deserialize, Clone)]
struct BuyTransaction {
    pub type_: String,
    in_token: String,
    out_token: String,
    amount_in: f64,
    key_z: Option<LiquidityPoolKeysString>,
    lp_decimals: u8,
}

async fn handle_trade_message(payload: String) {
    let trade_info: serde_json::Value = match serde_json::from_str(&payload) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("Failed to parse trade info, check the other repo: {}", e);
            return;
        }
    };

    match trade_info["type_"].as_str() {
        Some("buy") => {
            let buy_transaction: Result<BuyTransaction, serde_json::Error> = serde_json::from_value(
                trade_info.clone()
            );
            match buy_transaction {
                Ok(tx) => {
                    // Measure time taken for buy swap
                    let start_time = Instant::now();

                    let rpc_endpoint = std::env
                        ::var("RPC_URL")
                        .expect("You must set the RPC_URL environment variable!");
                    let client: Arc<RpcClient> = Arc::new(RpcClient::new(rpc_endpoint));

                    let in_token_pubkey = Pubkey::from_str(&tx.in_token).unwrap();

                    let buy_pool_result = get_liquidity_pool(
                        client.clone(),
                        &in_token_pubkey
                    ).await;

                    if let Ok(Some(buy_pool)) = buy_pool_result {
                        // Proceed with the buy swap
                        match buy::buy::buy_swap(buy_pool, tx.lp_decimals, tx.amount_in).await {
                            Ok(result) => {
                                let elapsed = start_time.elapsed();
                                println!(
                                    "Buy swap successful: {}. Time taken: {:?}",
                                    result,
                                    elapsed
                                );
                            }
                            Err(err) => {
                                eprintln!("Buy swap error: {:?}", err);
                            }
                        }
                    } else {
                        // Treat as pump token
                        dbg!("Running pump_fun_buy");
                        let mint_str = &tx.in_token;
                        let slippage_decimal = 15.0; // Update as necessary

                        match pump_fun_buy(mint_str, tx.amount_in, slippage_decimal).await {
                            Ok(_) => {
                                let elapsed = start_time.elapsed();
                                println!("Pump fun buy successful. Time taken: {:?}", elapsed);
                            }
                            Err(err) => {
                                eprintln!("Pump fun buy error: {:?}", err);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to deserialize BuyTransaction: {}", e);
                }
            }
        }
        Some("sell") => {
            let sell_transaction: Result<
                SellTransaction,
                serde_json::Error
            > = serde_json::from_value(trade_info.clone());
            match sell_transaction {
                Ok(tx) => {
                    // Measure time taken for sell transaction
                    let start_time = Instant::now();

                    let rpc_endpoint = std::env
                        ::var("RPC_URL")
                        .expect("You must set the RPC_URL environment variable!");
                    let client: Arc<RpcClient> = Arc::new(RpcClient::new(rpc_endpoint));

                    let in_token_pubkey = Pubkey::from_str(&tx.mint).unwrap();

                    let sell_pool_result = get_sell_liquidity_pool(
                        client.clone(),
                        &in_token_pubkey
                    ).await;

                    if let Ok(Some(_sell_pool)) = sell_pool_result {
                        // Proceed with the sell swap
                        match sell_swap(&tx).await {
                            Ok(_) => {
                                let elapsed = start_time.elapsed();
                                dbg!("Sell confirmed successfully. Time taken: {:?}", elapsed);
                            }
                            Err(err) => {
                                eprintln!("Sell confirmation error: {:?}", err);
                                // Handle the error as per your application's logic
                            }
                        }
                    } else {
                        // Treat as pump token
                        dbg!("Running pump_fun_sell");
                        let mint_str = &tx.mint;
                        let slippage_decimal = 0.15; // Update as necessary

                        match pump_fun_sell(mint_str, tx.amount, slippage_decimal, &tx).await {
                            Ok(_) => {
                                let elapsed = start_time.elapsed();
                                println!("Pump fun sell successful. Time taken: {:?}", elapsed);
                            }
                            Err(err) => {
                                eprintln!("Pump fun sell error: {:?}", err);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to deserialize SellTransaction: {}", e);
                }
            }
        }
        _ => {
            eprintln!("Invalid transaction type or missing 'type_' field");
        }
    }
}

async fn receive_trades() -> RedisResult<()> {
    let redis_url = std::env
        ::var("REDIS_URL")
        .expect("You must set the REDIS_URL environment variable");

    loop {
        let client = redis::Client::open(redis_url.clone()).expect("Failed to create Redis client");

        match client.get_multiplexed_async_connection().await {
            Ok(_connection) => {
                let mut pubsub = client.get_async_pubsub().await.unwrap();
                if let Err(e) = pubsub.subscribe("trading").await {
                    eprintln!("Failed to subscribe to 'trading': {}", e);
                    sleep(Duration::from_secs(5)).await;
                    continue;
                }

                let mut pubsub_stream = pubsub.on_message();
                while let Some(msg) = pubsub_stream.next().await {
                    let payload: String = match msg.get_payload() {
                        Ok(p) => p,
                        Err(e) => {
                            eprintln!("Failed to get payload from message: {}", e);
                            continue;
                        }
                    };
                    handle_trade_message(payload).await;
                }
            }
            Err(e) => {
                eprintln!("Error connecting to Redis: {}", e);
            }
        }

        sleep(Duration::from_secs(5)).await;
    }
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    if let Err(e) = receive_trades().await {
        eprintln!("Error receiving trades: {}", e);
    }
}
