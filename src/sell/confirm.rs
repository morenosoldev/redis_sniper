use super::mongo;
use super::sell::SellTransaction;
use super::price;
use super::utils;
use mongo::{ MongoHandler, SellTransaction as SellTransactionMongo, TokenInfo };
use solana_client::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::Duration;
use price::get_current_sol_price;
use utils::calculate_sol_amount_received;
use std::error::Error;
use solana_transaction_status::UiTransactionEncoding;
use mongodb::bson::DateTime;
use solana_sdk::signature::Signature;

pub async fn confirm_sell(
    signature: &Signature,
    sell_transaction: &SellTransaction
) -> Result<(), Box<dyn Error>> {
    let rpc_endpoint = std::env
        ::var("RPC_URL")
        .expect("You must set the RPC_URL environment variable!");
    let rpc_client: Arc<RpcClient> = Arc::new(RpcClient::new(rpc_endpoint.to_string()));
    let mongo_handler = MongoHandler::new().await.map_err(|err| {
        format!("Error creating MongoDB handler: {:?}", err)
    })?;

    let mut retry_count = 0;
    let max_retries = 6;
    let retry_delay = Duration::from_secs(18);

    let usd_sol_price = get_current_sol_price().await?;

    let mut confirmed = false;
    while !confirmed && retry_count <= max_retries {
        match rpc_client.get_transaction(&signature, UiTransactionEncoding::JsonParsed) {
            Ok(confirmed_transaction) => {
                let sell_price = sell_transaction.current_token_price_usd;
                let sol_amount = calculate_sol_amount_received(&confirmed_transaction).await?;
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

                mongo_handler.store_sell_transaction_info(
                    sell_transaction_mongo,
                    "solsniper",
                    "sell_transactions"
                ).await?;

                mongo_handler.update_token_metadata_sold_field(
                    &sell_transaction.mint,
                    "solsniper",
                    "tokens"
                ).await?;

                confirmed = true;
            }
            Err(err) => {
                eprintln!("Error fetching transaction: {:?}", err);
                retry_count += 1;
                tokio::time::sleep(retry_delay).await;
            }
        }
    }

    if confirmed {
        return Ok(());
    } else {
        return Err("Transaction not confirmed after 8 retries".into());
    }
}
