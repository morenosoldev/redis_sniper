use crate::sell::confirm::confirm_sell;
use super::utils;
use super::mongo::TokenMetadata;
use helius::error::HeliusError;
use spl_token_client::client::{ ProgramClient, ProgramRpcClient, ProgramRpcClientSendTransaction };
use spl_token_client::token::Token;
use solana_sdk::signature::Signer;
use solana_sdk::signature::Signature;
use std::sync::Arc;
use raydium_contract_instructions::amm_instruction as amm;
use utils::get_liquidity_pool;
use solana_sdk::transaction::Transaction;
use solana_client::nonblocking::rpc_client::RpcClient;
use spl_token_client::token::TokenError;
use solana_sdk::signature::Keypair;
use solana_client::rpc_config::RpcSendTransactionConfig;
use serde::{ Serialize, Deserialize };
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::time::Duration;
use solana_transaction_status::UiTransactionEncoding;
use helius::types::*;
use helius::Helius;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SellTransaction {
    pub metadata: TokenMetadata,
    pub mint: String,
    pub current_token_price_usd: f64,
    pub current_token_price_sol: f64,
    pub amount: u64,
    pub sol_amount: f64,
    pub entry: f64,
    pub base_vault: String,
    pub quote_vault: String,
}

pub async fn sell_swap(
    sell_transaction: &SellTransaction
) -> Result<Signature, Box<dyn std::error::Error>> {
    let api_key: String = std::env
        ::var("HELIUS_API_KEY")
        .expect("You must set the PRIVATE_KEY environment variable!");
    let cluster: Cluster = Cluster::MainnetBeta;
    let helius: Helius = Helius::new(&api_key, cluster).unwrap();
    let private_key = std::env
        ::var("PRIVATE_KEY")
        .expect("You must set the PRIVATE_KEY environment variable!");
    let keypair = Keypair::from_base58_string(&private_key);

    let min_amount_out = 0;
    let rpc_endpoint = std::env
        ::var("RPC_URL")
        .expect("You must set the RPC_URL environment variable!");

    let client = Arc::new(RpcClient::new(rpc_endpoint.to_string()));

    let program_client: Arc<dyn ProgramClient<ProgramRpcClientSendTransaction>> = Arc::new(
        ProgramRpcClient::new(client.clone(), ProgramRpcClientSendTransaction)
    );

    let out_token: Pubkey = Pubkey::from_str(
        "So11111111111111111111111111111111111111112"
    ).unwrap();

    // Clone the keypair to be able to use it multiple times
    let keypair_arc = Arc::new(keypair);

    let in_token_client = Token::new(
        Arc::clone(&program_client),
        &spl_token::ID,
        &Pubkey::from_str(&sell_transaction.mint).unwrap(),
        None,
        keypair_arc.clone()
    );
    let out_token_client = Token::new(
        Arc::clone(&program_client),
        &spl_token::ID,
        &out_token,
        None,
        keypair_arc.clone()
    );

    let user = keypair_arc.pubkey();

    let pool_info = match
        get_liquidity_pool(client.clone(), &Pubkey::from_str(&sell_transaction.mint).unwrap()).await
    {
        Ok(Some(info)) => info,
        Ok(None) => {
            dbg!("Pool info not found for the given tokens.");
            return Err("Pool info not found for the given tokens.".into());
        }
        Err(e) => {
            return Err(e.into());
        }
    };

    // Get the user's ATA. We don't try to create it as it is expected to exist.
    let user_in_token_account = in_token_client.get_associated_token_address(&user);
    dbg!("User input-tokens ATA={}", user_in_token_account);
    let user_in_acct = in_token_client.get_account_info(&user_in_token_account).await?;

    // TODO: If input tokens is the native mint(wSOL) and the balance is inadequate, attempt to
    // convert SOL to wSOL.
    let balance = user_in_acct.base.amount;

    if balance == 0 {
        return Err("User has no balance in the input token account".into());
    }
    dbg!("User input-tokens ATA balance={}", balance);
    if in_token_client.is_native() && balance < (sell_transaction.amount as u64) {
        let transfer_amt = (sell_transaction.amount as u64) - balance;
        let blockhash = client.get_latest_blockhash().await?;
        let transfer_instruction = solana_sdk::system_instruction::transfer(
            &user,
            &user_in_token_account,
            transfer_amt
        );
        let sync_instruction = spl_token::instruction::sync_native(
            &spl_token::ID,
            &user_in_token_account
        )?;
        let tx = Transaction::new_signed_with_payer(
            &[transfer_instruction, sync_instruction],
            Some(&user),
            &[&keypair_arc],
            blockhash
        );
        client.send_and_confirm_transaction(&tx).await.unwrap();
    }

    let user_out_token_account = out_token_client.get_associated_token_address(&user);
    dbg!("User's output-tokens ATA={}", user_out_token_account);
    match out_token_client.get_account_info(&user_out_token_account).await {
        Ok(_) => {
            dbg!("User's ATA for output tokens exists. Skipping creation.");
        }
        Err(TokenError::AccountNotFound) | Err(TokenError::AccountInvalidOwner) => {
            dbg!("User's output-tokens ATA does not exist. Creating..");
            out_token_client.create_associated_token_account(&user).await?;
        }
        Err(err) => {
            // Changed variable name to 'err'
            return Err(err.into()); // Return the error to handle it properly
        }
    }

    let mut instructions = vec![];

    let swap_amount_in = sell_transaction.amount;

    if pool_info.base_mint == Pubkey::from_str(&sell_transaction.mint).unwrap() {
        dbg!("Initializing swap with input tokens as pool base token");
        debug_assert!(pool_info.quote_mint == out_token);
        let swap_instruction = amm::swap_base_in(
            &amm::ID,
            &pool_info.id,
            &pool_info.authority,
            &pool_info.open_orders,
            &pool_info.target_orders,
            &pool_info.base_vault,
            &pool_info.quote_vault,
            &pool_info.market_program_id,
            &pool_info.market_id,
            &pool_info.market_bids,
            &pool_info.market_asks,
            &pool_info.market_event_queue,
            &pool_info.market_base_vault,
            &pool_info.market_quote_vault,
            &pool_info.market_authority,
            &user_in_token_account,
            &user_out_token_account,
            &user,
            swap_amount_in,
            min_amount_out
        )?;

        instructions.push(swap_instruction);
    } else {
        dbg!("Initializing swap with input tokens as pool quote token");
        debug_assert!(
            pool_info.quote_mint == Pubkey::from_str(&sell_transaction.mint).unwrap() &&
                pool_info.base_mint == out_token
        );
        let swap_instruction = amm::swap_base_out(
            &amm::ID,
            &pool_info.id,
            &pool_info.authority,
            &pool_info.open_orders,
            &pool_info.target_orders,
            &pool_info.base_vault,
            &pool_info.quote_vault,
            &pool_info.market_program_id,
            &pool_info.market_id,
            &pool_info.market_bids,
            &pool_info.market_asks,
            &pool_info.market_event_queue,
            &pool_info.market_base_vault,
            &pool_info.market_quote_vault,
            &pool_info.market_authority,
            &user_in_token_account,
            &user_out_token_account,
            &user,
            swap_amount_in,
            min_amount_out
        )?;
        instructions.push(swap_instruction);
    }

    let max_retries = 12;
    let retry_delay = Duration::from_secs(5);

    // Create the SmartTransactionConfig
    let config: SmartTransactionConfig = SmartTransactionConfig {
        create_config: CreateSmartTransactionConfig {
            instructions,
            signers: vec![&keypair_arc],
            lookup_tables: None,
            fee_payer: None,
        },
        send_options: RpcSendTransactionConfig {
            skip_preflight: true,
            preflight_commitment: None,
            encoding: None,
            max_retries: Some(4),
            min_context_slot: None,
        },
    };

    match helius.send_smart_transaction(config).await {
        Ok(signature) => {
            dbg!("Transaction sent successfully: {}", signature);
            let mut retry_count = 0;
            let mut confirmed = false;

            while !confirmed && retry_count <= max_retries {
                match client.get_transaction(&signature, UiTransactionEncoding::JsonParsed).await {
                    Ok(_confirmed_transaction) => {
                        confirm_sell(&signature, sell_transaction).await?;
                        confirmed = true;
                    }
                    Err(err) => {
                        eprintln!("Error getting confirmed transaction: {:?}", err);
                        if
                            err.to_string().contains("not confirmed") ||
                            err.to_string().contains("invalid type: null")
                        {
                            retry_count += 1;
                            tokio::time::sleep(retry_delay).await;
                        } else {
                            break;
                        }
                    }
                }
            }

            return Ok(signature);
        }
        Err(e) => {
            // Check if the error is a timeout (code 408)
            if e.to_string().contains("408 Request Timeout") {
                // Attempt to save transaction details even if there was a timeout
                if let Some(signature) = extract_signature_from_error(&e) {
                    dbg!("Extracted signature: {}", &signature);
                    let sig = Signature::from_str(&signature)
                        .map_err(|err| {
                            return Box::new(err) as Box<dyn std::error::Error>;
                        })
                        .unwrap();

                    confirm_sell(&sig, sell_transaction).await?;
                    return Ok(sig);
                } else {
                    return Err(e.into());
                }
            } else {
                return Err(e.into());
            }
        }
    }
}

fn extract_signature_from_error(error: &HeliusError) -> Option<String> {
    let error_message = error.to_string();
    let start_marker =
        "Transaction confirmation timed out with error code 408 Request Timeout: Transaction ";
    let end_marker = "'s confirmation timed out";

    // Find the start and end positions
    let start = error_message.find(start_marker)?;
    let end = error_message.find(end_marker)?;

    // Calculate the start of the actual signature (after the start marker)
    let start_signature = start + start_marker.len();

    // Extract the substring containing the signature
    let signature = &error_message[start_signature..end];
    Some(signature.to_string())
}
