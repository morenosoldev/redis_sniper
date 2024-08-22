mod sell;
mod buy;
use redis::RedisResult;
use futures_util::StreamExt;
use serde::{ Serialize, Deserialize };
use dotenv::dotenv;
use buy::raydium_sdk::LiquidityPoolKeysString;
use buy::pump::pump_fun_buy;
use buy::buy::buy_swap;
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
use serde_json::json;
use redis::AsyncCommands;

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
                    // Measure time taken for buy transaction
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

                    let buy_success = if let Ok(Some(buy_pool)) = buy_pool_result {
                        let redis_url = std::env
                            ::var("REDIS_URL")
                            .expect("You must set the REDIS_URL environment variable!");
                        let client = redis::Client
                            ::open(redis_url)
                            .expect("Failed to create Redis client");
                        let mut connection = client
                            .get_multiplexed_async_connection().await
                            .expect("Failed to get Redis connection");

                        // Proceed with the buy swap using the existing logic
                        match buy_swap(buy_pool, tx.lp_decimals, tx.amount_in).await {
                            Ok(_) => {
                                let elapsed = start_time.elapsed();

                                let confirmation_message =
                                    json!({
                        "status": "success",
                        "mint": tx.in_token,
                    }).to_string();

                                let _: () = connection
                                    .publish("trading_confirmation", confirmation_message).await
                                    .expect("Failed to send confirmation");

                                dbg!("Buy confirmed successfully. Time taken: {:?}", elapsed);
                                true
                            }
                            Err(err) => {
                                eprintln!("Buy confirmation error: {:?}", err);
                                false
                            }
                        }
                    } else {
                        // Treat as pump token
                        dbg!("Running pump_fun_buy");
                        let mint_str = &tx.in_token;
                        let slippage_decimal = 65.0; // Update as necessary

                        match
                            pump_fun_buy(
                                mint_str,
                                tx.amount_in,
                                slippage_decimal,
                                tx.lp_decimals
                            ).await
                        {
                            Ok(_) => {
                                let elapsed = start_time.elapsed();
                                println!("Pump fun buy successful. Time taken: {:?}", elapsed);
                                true
                            }
                            Err(err) => {
                                eprintln!("Pump fun buy error: {:?}", err);
                                false
                            }
                        }
                    };

                    // Send confirmation message back
                    if buy_success {
                        let redis_url = std::env
                            ::var("REDIS_URL")
                            .expect("You must set the REDIS_URL environment variable!");
                        let client = redis::Client
                            ::open(redis_url)
                            .expect("Failed to create Redis client");
                        let mut connection = client
                            .get_multiplexed_async_connection().await
                            .expect("Failed to get Redis connection");

                        let confirmation_message =
                            json!({
                            "status": "success",
                            "mint": tx.in_token,
                        }).to_string();

                        let _: () = connection
                            .publish("trading_confirmation", confirmation_message).await
                            .expect("Failed to send confirmation");
                    } else {
                        let redis_url = std::env
                            ::var("REDIS_URL")
                            .expect("You must set the REDIS_URL environment variable!");
                        let client = redis::Client
                            ::open(redis_url)
                            .expect("Failed to create Redis client");
                        let mut connection = client
                            .get_multiplexed_async_connection().await
                            .expect("Failed to get Redis connection");

                        let confirmation_message =
                            json!({
                            "status": "fail",
                            "mint": tx.in_token,
                        }).to_string();

                        let _: () = connection
                            .publish("trading_confirmation", confirmation_message).await
                            .expect("Failed to send confirmation");
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

                    let sell_success = if let Ok(Some(_sell_pool)) = sell_pool_result {
                        match sell_swap(&tx).await {
                            Ok(_) => {
                                let elapsed = start_time.elapsed();
                                dbg!("Sell confirmed successfully. Time taken: {:?}", elapsed);
                                true
                            }
                            Err(err) => {
                                eprintln!("Sell confirmation error: {:?}", err);
                                false
                            }
                        }
                    } else {
                        let mint_str = &tx.mint;
                        let slippage_decimal = 50.0;

                        match pump_fun_sell(mint_str, tx.amount, slippage_decimal, &tx).await {
                            Ok(_) => {
                                let elapsed = start_time.elapsed();
                                println!("Pump fun sell successful. Time taken: {:?}", elapsed);
                                true
                            }
                            Err(err) => {
                                eprintln!("Pump fun sell error: {:?}", err);
                                false
                            }
                        }
                    };

                    let redis_url = std::env
                        ::var("REDIS_URL")
                        .expect("You must set the REDIS_URL environment variable!");
                    let client = redis::Client
                        ::open(redis_url)
                        .expect("Failed to create Redis client");
                    let mut connection = client
                        .get_multiplexed_async_connection().await
                        .expect("Failed to get Redis connection");

                    let confirmation_message =
                        json!({
                        "status": if sell_success { "success" } else { "fail" },
                        "mint": tx.mint,
                    }).to_string();

                    let _: () = connection
                        .publish("trading_confirmation", confirmation_message).await
                        .expect("Failed to send confirmation");
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
