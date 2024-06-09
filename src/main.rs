use redis::Client;
use std::thread;
mod sell;
mod buy;
use solana_sdk::{ signature::Keypair, signer::Signer };
use serde::{ Serialize, Deserialize };
use buy::raydium_sdk::{ LiquidityPoolKeys, LiquidityPoolKeysString };
use sell::sell::SellTransaction;
use dotenv::dotenv;

#[derive(Debug, Serialize, Deserialize)]
struct BuyTransaction {
    pub type_: String,
    in_token: String,
    out_token: String,
    amount_in: f64,
    key_z: LiquidityPoolKeysString,
    lp_decimals: u8,
}

async fn receive_trades() {
    let client = Client::open("redis://127.0.0.1:6379/").unwrap();
    let mut connection = client.get_connection().unwrap();

    let mut pubsub = connection.as_pubsub();
    let private_key = std::env
        ::var("PRIVATE_KEY")
        .expect("You must set the PRIVATE_KEY environment variable!");
    let keypair = Keypair::from_base58_string(&private_key);

    pubsub.subscribe("trading").unwrap();

    loop {
        let message = pubsub.get_message().unwrap();
        let payload: String = message.get_payload().unwrap();

        let trade_info: serde_json::Value = serde_json::from_str(&payload).unwrap();
        println!("Received trade: {}", trade_info);

        match trade_info["type_"].as_str() {
            Some("buy") => {
                println!("Buying token...");
                let buy_transaction: BuyTransaction = serde_json::from_value(trade_info).unwrap();
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
                println!("Selling token...");
                let sell_transaction: SellTransaction = serde_json::from_value(trade_info).unwrap();
                println!("Sell transaction: {:?}", sell_transaction);
                let _signature = sell::confirm::confirm_sell(&sell_transaction).await;
            }
            _ => println!("Unknown trade type"),
        }
    }
}
fn main() {
    dotenv().ok();
    // Spawn a new thread for receiving trades
    let receive_handle = thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(receive_trades());
    });

    // Keep the main thread alive
    loop {
        std::thread::park();
    }
}
