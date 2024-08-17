use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_sdk::signature::Signature;
use futures::stream::StreamExt;
use solana_client::rpc_response::RpcSignatureResult;
use tokio::time::{ timeout, Duration };
use solana_transaction_status::UiTransactionEncoding;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;

pub async fn poll_transaction(
    rpc_client: Arc<RpcClient>,
    pub_subclient: PubsubClient,
    signature: Signature
) -> Result<bool, Box<dyn std::error::Error + 'static>> {
    let (mut stream, _) = pub_subclient.signature_subscribe(&signature, None).await?;
    println!("Subscribed to transaction status for signature: {:?}", signature);

    let mut checked_sent = false;

    loop {
        match timeout(Duration::from_secs(38), stream.next()).await {
            Ok(Some(response)) => {
                let value: RpcSignatureResult = response.value;

                match value {
                    RpcSignatureResult::ProcessedSignature(processed_result) => {
                        if let Some(err) = processed_result.err {
                            return Err(Box::new(err));
                        } else {
                            return Ok(true);
                        }
                    }
                    RpcSignatureResult::ReceivedSignature(_) => {
                        // Continue polling as the transaction is in progress
                    }
                }
            }
            Ok(None) => {
                // If the stream ends unexpectedly
                return Ok(false);
            }
            Err(_) => {
                if !checked_sent {
                    let config = RpcTransactionConfig {
                        encoding: Some(UiTransactionEncoding::Json),
                        commitment: Some(CommitmentConfig::processed()),
                        max_supported_transaction_version: Some(0),
                    };
                    // Check if the transaction has been sent after 35 seconds
                    if
                        let Err(_) = rpc_client.get_transaction_with_config(
                            &signature,
                            config
                        ).await
                    {
                        println!("Transaction has not been sent yet.");
                        return Ok(false);
                    } else {
                        println!("Transaction has been sent but no updates received yet.");
                        checked_sent = true;
                    }
                } else {
                    // If already checked, wait for a maximum of 1 minute
                    if let Err(_) = timeout(Duration::from_secs(22), stream.next()).await {
                        println!("No transaction updates received within 1 minute after sending.");
                        return Ok(false);
                    }
                }
            }
        }
    }
}
