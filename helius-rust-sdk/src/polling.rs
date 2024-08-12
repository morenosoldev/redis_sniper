use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_sdk::signature::Signature;
use futures::stream::StreamExt;
use solana_client::rpc_response::RpcSignatureResult;
use tokio::time::{ timeout, Duration };
use solana_transaction_status::UiTransactionEncoding;

pub async fn poll_transaction(
    rpc_client: Arc<RpcClient>,
    pub_subclient: PubsubClient,
    signature: Signature
) -> Result<bool, Box<dyn std::error::Error + 'static>> {
    let (mut stream, _) = pub_subclient.signature_subscribe(&signature, None).await?;
    println!("Subscribed to transaction status for signature: {:?}", signature);

    let mut checked_sent = false;

    loop {
        match timeout(Duration::from_secs(30), stream.next()).await {
            Ok(Some(response)) => {
                let value: RpcSignatureResult = response.value;
                println!("Transaction status: {:?}", value);

                match value {
                    RpcSignatureResult::ProcessedSignature(processed_result) => {
                        if let Some(err) = processed_result.err {
                            println!("Transaction failed with error: {:?}", err);
                            return Err(Box::new(err));
                        } else {
                            println!("Transaction processed successfully.");
                            return Ok(true);
                        }
                    }
                    RpcSignatureResult::ReceivedSignature(_) => {
                        println!("Transaction signature received, but not yet processed.");
                        // Continue polling as the transaction is in progress
                    }
                }
            }
            Ok(None) => {
                // If the stream ends unexpectedly
                println!("End of stream encountered unexpectedly.");
                return Ok(false);
            }
            Err(_) => {
                if !checked_sent {
                    // Check if the transaction has been sent after 15 seconds
                    println!(
                        "No transaction status received within 15 seconds. Checking if the transaction has been sent..."
                    );

                    if
                        let Err(_) = rpc_client.get_transaction(
                            &signature,
                            UiTransactionEncoding::JsonParsed
                        ).await
                    {
                        println!("Transaction has not been sent yet.");
                        return Ok(false);
                    } else {
                        println!("Transaction has been sent but no updates received yet.");
                        // Set flag to indicate the transaction has been sent
                        checked_sent = true;
                    }
                } else {
                    // If already checked, wait for a maximum of 1 minute
                    println!("Waiting for a maximum of 1 minute for transaction updates...");

                    if let Err(_) = timeout(Duration::from_secs(45), stream.next()).await {
                        println!("No transaction updates received within 1 minute after sending.");
                        return Ok(false);
                    }
                }
            }
        }
    }
}
