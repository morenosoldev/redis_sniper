use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_sdk::signature::Signature;
use futures::stream::StreamExt;
use solana_client::rpc_response::RpcSignatureResult;
use tokio::time::{ timeout, Duration };

pub async fn poll_transaction(
    rpc_client: Arc<RpcClient>,
    pub_subclient: PubsubClient,
    signature: Signature,
    last_valid_block_height: u64
) -> Result<bool, Box<dyn std::error::Error + 'static>> {
    let (mut stream, _) = pub_subclient.signature_subscribe(&signature, None).await?;
    println!("Subscribed to transaction status for signature: {:?}", signature);

    let mut checked_status = false;

    loop {
        // Check current block height
        let current_block_height = rpc_client.get_block_height().await?;
        if current_block_height > last_valid_block_height {
            println!("Transaction is invalid due to expired blockhash.");
            return Ok(false);
        }

        // Check signature status immediately
        if !checked_status {
            if let Some(status) = check_signature_status(&rpc_client, &signature).await? {
                return Ok(status);
            }
            checked_status = true;
        }

        // Poll for updates from the subscription stream
        match timeout(Duration::from_secs(25), stream.next()).await {
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
            Ok(None) | Err(_) => {
                // If stream ends or times out, continue checking manually
                if let Some(status) = check_signature_status(&rpc_client, &signature).await? {
                    return Ok(status);
                }
            }
        }
    }
}

async fn check_signature_status(
    rpc_client: &RpcClient,
    signature: &Signature
) -> Result<Option<bool>, Box<dyn std::error::Error + 'static>> {
    let statuses = rpc_client.get_signature_statuses(&[*signature]).await?;

    if let Some(Some(status)) = statuses.value.get(0) {
        if status.confirmation_status.is_some() {
            if let Some(ref err) = status.err {
                return Err(Box::new(err.clone())); // Clone the error if you need to return it
            } else {
                return Ok(Some(true));
            }
        }
    }
    Ok(None)
}
