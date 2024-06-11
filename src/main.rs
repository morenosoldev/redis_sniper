mod sell;
mod buy;
use redis::RedisResult;
use futures_util::StreamExt;
use serde::{ Serialize, Deserialize };
use solana_sdk::{ signature::Keypair, signer::Signer };
use dotenv::dotenv;
use buy::raydium_sdk::{ LiquidityPoolKeys, LiquidityPoolKeysString };
use sell::sell::SellTransaction;

#[derive(Debug, Serialize, Deserialize)]
struct BuyTransaction {
    pub type_: String,
    in_token: String,
    out_token: String,
    amount_in: f64,
    key_z: LiquidityPoolKeysString,
    lp_decimals: u8,
}

async fn handle_trade_message(payload: String, keypair: Keypair) {
    let trade_info: serde_json::Value = match serde_json::from_str(&payload) {
        Ok(info) => info,
        Err(e) => {
            eprintln!("Failed to parse trade info: {}", e);
            return;
        }
    };

    match trade_info["type_"].as_str() {
        Some("buy") => {
            let buy_transaction: BuyTransaction = match serde_json::from_value(trade_info) {
                Ok(tx) => tx,
                Err(e) => {
                    eprintln!("Failed to deserialize BuyTransaction: {}", e);
                    return;
                }
            };

            let _ = buy::buy::buy_swap(
                LiquidityPoolKeys::from(buy_transaction.key_z),
                true,
                buy_transaction.lp_decimals,
                buy_transaction.amount_in,
                &keypair,
                &keypair.pubkey()
            ).await;
        }
        Some("sell") => {
            let sell_transaction: SellTransaction = match serde_json::from_value(trade_info) {
                Ok(tx) => tx,
                Err(e) => {
                    eprintln!("Failed to deserialize SellTransaction: {}", e);
                    return;
                }
            };

            let _signature = sell::confirm::confirm_sell(&sell_transaction).await;
        }
        _ => println!("Unknown trade type"),
    }
}

async fn receive_trades() -> RedisResult<()> {
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".to_string());
    let client = redis::Client::open(redis_url)?;

    let pubsub_conn = client.get_async_connection().await?;

    let mut pubsub = pubsub_conn.into_pubsub();

    pubsub.subscribe("trading").await?;
    let mut pubsub_stream = pubsub.on_message();

    let private_key = std::env
        ::var("PRIVATE_KEY")
        .expect("You must set the PRIVATE_KEY environment variable!");
    let keypair = Keypair::from_base58_string(&private_key);

    while let Some(msg) = pubsub_stream.next().await {
        let payload: String = msg.get_payload()?;
        println!("Received message: {}", payload);
        handle_trade_message(payload, keypair.insecure_clone()).await;
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    if let Err(e) = receive_trades().await {
        eprintln!("Error receiving trades: {}", e);
    }
}
