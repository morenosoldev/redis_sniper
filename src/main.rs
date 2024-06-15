mod sell;
mod buy;
use redis::RedisResult;
use futures_util::StreamExt;
use serde::{ Serialize, Deserialize };
use dotenv::dotenv;
use buy::raydium_sdk::{ LiquidityPoolKeys, LiquidityPoolKeysString };
use sell::sell::SellTransaction;
use tokio::time::{ sleep, Duration };

#[derive(Debug, Serialize, Deserialize)]
struct BuyTransaction {
    pub type_: String,
    in_token: String,
    out_token: String,
    amount_in: f64,
    key_z: LiquidityPoolKeysString,
    lp_decimals: u8,
}

async fn handle_trade_message(payload: String) {
    let trade_info: serde_json::Value = match serde_json::from_str(&payload) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("Failed to parse trade info: {}", e);
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
                    match
                        buy::buy::buy_swap(
                            LiquidityPoolKeys::from(tx.key_z),
                            tx.lp_decimals,
                            tx.amount_in
                        ).await
                    {
                        Ok(result) => {
                            println!("Buy swap successful: {}", result);
                            // Proceed with any further processing if needed
                        }
                        Err(err) => {
                            eprintln!("Buy swap error: {:?}", err);
                            // Handle the error as per your application's logic
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to deserialize BuyTransaction: {}", e);
                    return;
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
                    match sell::confirm::confirm_sell(&tx).await {
                        Ok(_) => {
                            println!("Sell confirmed successfully");
                            // Proceed with any further processing if needed
                        }
                        Err(err) => {
                            eprintln!("Sell confirmation error: {:?}", err);
                            // Handle the error as per your application's logic
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to deserialize SellTransaction: {}", e);
                    return;
                }
            }
        }
        _ => {
            eprintln!("Invalid transaction type or missing 'type_' field");
            return;
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
                    println!("Received message: {}", payload);
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
