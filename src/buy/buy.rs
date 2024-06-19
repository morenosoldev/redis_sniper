use super::raydium_sdk;
use super::utils;
use super::service;
use helius::error::HeliusError;
use solana_client::nonblocking::rpc_client::RpcClient;
use raydium_sdk::LiquidityPoolKeys;
use solana_sdk::instruction::Instruction;
use solana_sdk::signature::Keypair;
use std::sync::Arc;
use spl_token_client::token::Token;
use spl_token_client::client::{ ProgramClient, ProgramRpcClient, ProgramRpcClientSendTransaction };
use solana_sdk::signature::Signer;
use raydium_contract_instructions::amm_instruction as amm;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_sdk::program_error::ProgramError;
use utils::get_or_create_ata_for_token_in_and_out_with_balance;
use helius::types::*;
use helius::Helius;
use service::save_buy_details;
use solana_sdk::signature::Signature;
use std::str::FromStr;
use solana_sdk::pubkey::Pubkey;

#[derive(Debug, thiserror::Error)]
pub enum SwapError {
    #[error("Transaction error: {0}")] TransactionError(String),
    #[error("Token error: {0}")] TokenError(String),
    #[error("Invalid transaction data")]
    InvalidTransactionData,
    #[error("Program error: {0}")] ProgramError(#[from] ProgramError),
}

pub async fn buy_swap(
    key_z: LiquidityPoolKeys,
    lp_decimals: u8,
    sol_amount: f64
) -> Result<String, SwapError> {
    let api_key: String = std::env
        ::var("HELIUS_API_KEY")
        .expect("You must set the HELIUS_API_KEY environment variable!");
    let cluster: Cluster = Cluster::MainnetBeta;
    let helius: Helius = Helius::new(&api_key, cluster).unwrap();

    let private_key = std::env
        ::var("PRIVATE_KEY")
        .expect("You must set the PRIVATE_KEY environment variable!");
    let keypair = Keypair::from_base58_string(&private_key);

    let user = Pubkey::from_str("3rzKBn91t3ttL23by55oo9h5Ag89nCdvFbHwvs58Uj52").unwrap();

    let rpc_endpoint = std::env
        ::var("RPC_URL")
        .expect("You must set the RPC_URL environment variable!");
    let client: Arc<RpcClient> = Arc::new(RpcClient::new(rpc_endpoint.to_string()));

    let program_client: Arc<dyn ProgramClient<ProgramRpcClientSendTransaction>> = Arc::new(
        ProgramRpcClient::new(client.clone(), ProgramRpcClientSendTransaction)
    );
    dbg!("Køber nu");

    if
        key_z.base_mint.to_string() == "So11111111111111111111111111111111111111112".to_string() ||
        key_z.quote_mint.to_string() != "So11111111111111111111111111111111111111112".to_string()
    {
        return Err(SwapError::InvalidTransactionData);
    }

    let keypair_arc = Arc::new(keypair);

    let token_in = Token::new(
        program_client.clone(),
        &spl_token::ID,
        &key_z.quote_mint,
        None,
        keypair_arc.clone()
    );
    let token_out = Token::new(
        program_client.clone(),
        &spl_token::ID,
        &key_z.base_mint,
        None,
        keypair_arc.clone()
    );

    let amount_in: u64 = (sol_amount * 1_000_000_000.0) as u64;

    // Get the user's ATA. We don't try to create it as it is expected to exist.
    let user_in_token_account = token_in.get_associated_token_address(&user);
    dbg!("User input-tokens ATA={}", user_in_token_account);
    let user_in_acct = match token_in.get_account_info(&user_in_token_account).await {
        Ok(account_info) => account_info,
        Err(err) => {
            return Err(
                SwapError::TransactionError(format!("Failed to get user input-tokens ATA: {}", err))
            );
        }
    };

    // TODO: If input tokens is the native mint(wSOL) and the balance is inadequate, attempt to
    // convert SOL to wSOL.
    let balance = user_in_acct.base.amount;
    dbg!("User input-tokens ATA balance={}", balance);
    if token_in.is_native() && balance < amount_in {
        let transfer_amt = amount_in - balance;

        let transfer_instruction = solana_sdk::system_instruction::transfer(
            &user,
            &user_in_token_account,
            transfer_amt
        );
        let sync_instruction = spl_token::instruction::sync_native(
            &spl_token::ID,
            &user_in_token_account
        )?;

        let intructions = vec![transfer_instruction, sync_instruction];
        // Create the SmartTransactionConfig
        let config = SmartTransactionConfig {
            instructions: intructions,
            signers: vec![&keypair_arc],
            send_options: RpcSendTransactionConfig {
                skip_preflight: true,
                preflight_commitment: None,
                encoding: None,
                max_retries: Some(2),
                min_context_slot: None,
            },
            lookup_tables: None,
        };

        match helius.send_smart_transaction(config).await {
            Ok(signature) => {
                dbg!("Transaction sent successfully: {}", signature);
            }
            Err(e) => {
                return Err(SwapError::TransactionError(e.to_string()));
            }
        }
    }

    let mut instructions: Vec<Instruction> = vec![];
    let ata_creation_bundle = get_or_create_ata_for_token_in_and_out_with_balance(
        &token_in,
        &token_out,
        keypair_arc.clone()
    ).await.unwrap();

    //Create input ATAs if instruction exist
    if ata_creation_bundle.token_in.instruction.is_some() {
        instructions.push(ata_creation_bundle.token_in.instruction.unwrap());
    }
    if ata_creation_bundle.token_out.instruction.is_some() {
        instructions.push(ata_creation_bundle.token_out.instruction.unwrap());
    }

    //Send some sol from account to the ata and then call sync native
    if token_in.is_native() && ata_creation_bundle.token_in.balance < amount_in {
        println!("Input token is native");
        let transfer_amount = amount_in - ata_creation_bundle.token_in.balance;
        let transfer_instruction = solana_sdk::system_instruction::transfer(
            &keypair_arc.pubkey().clone(),
            &ata_creation_bundle.token_in.ata_pubkey,
            transfer_amount
        );
        let sync_instruction = spl_token::instruction::sync_native(
            &spl_token::ID,
            &ata_creation_bundle.token_in.ata_pubkey
        )?;
        instructions.push(transfer_instruction);
        instructions.push(sync_instruction);
    } else {
        //An SPL token is an input. If the ATA token address does not exist, it means that the balance is definately 0.
        if ata_creation_bundle.token_in.balance < amount_in {
            dbg!("Input token not native. Checking sufficient balance");
            return Err(
                SwapError::TokenError("Insufficient balance in input token account".to_string())
            );
        }
    }

    // Here we are ensuring that the swap is done from SOL to SPL token (quote to base)
    dbg!("Initializing swap with input tokens as pool quote token");
    let swap_instruction = amm::swap_base_in(
        &amm::ID,
        &key_z.id,
        &key_z.authority,
        &key_z.open_orders,
        &key_z.target_orders,
        &key_z.base_vault,
        &key_z.quote_vault,
        &key_z.market_program_id,
        &key_z.market_id,
        &key_z.market_bids,
        &key_z.market_asks,
        &key_z.market_event_queue,
        &key_z.market_base_vault,
        &key_z.market_quote_vault,
        &key_z.market_authority,
        &ata_creation_bundle.token_in.ata_pubkey,
        &ata_creation_bundle.token_out.ata_pubkey,
        &keypair_arc.pubkey().clone(),
        amount_in,
        0
    )?;
    instructions.push(swap_instruction);

    // Create the SmartTransactionConfig
    let config = SmartTransactionConfig {
        instructions,
        signers: vec![&keypair_arc],
        send_options: RpcSendTransactionConfig {
            skip_preflight: true,
            preflight_commitment: None,
            encoding: None,
            max_retries: Some(4),
            min_context_slot: None,
        },
        lookup_tables: None,
    };

    match helius.send_smart_transaction(config).await {
        Ok(signature) => {
            dbg!("Transaction sent successfully: {}", signature);
            let saved_details = save_buy_details(
                client,
                &signature,
                sol_amount,
                lp_decimals,
                key_z
            ).await;

            if let Err(e) = saved_details {
                return Err(SwapError::TransactionError(e.to_string()));
            }

            return Ok(signature.to_string());
        }
        Err(e) => {
            // Check if the error is a timeout (code 408)
            if e.to_string().contains("408 Request Timeout") {
                // Attempt to save transaction details even if there was a timeout
                if let Some(signature) = extract_signature_from_error(&e) {
                    dbg!("Extracted signature: {}", &signature);
                    let sig = Signature::from_str(&signature).map_err(|err|
                        SwapError::TransactionError(err.to_string())
                    )?;

                    let saved_details = save_buy_details(
                        client,
                        &sig,
                        sol_amount,
                        lp_decimals,
                        key_z
                    ).await;

                    if let Err(save_err) = saved_details {
                        return Err(SwapError::TransactionError(save_err.to_string()));
                    }

                    Ok(signature)
                } else {
                    Err(
                        SwapError::TransactionError(
                            "Failed to extract signature from error".to_string()
                        )
                    )
                }
            } else {
                Err(SwapError::TransactionError(e.to_string()))
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
