use super::raydium_sdk;
use super::utils;
use super::service;
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
use service::TokenVaults;
use std::str::FromStr;
use solana_sdk::pubkey::Pubkey;
use solana_client::client_error::ClientError;
use std::time::Duration;
use spl_associated_token_account::instruction;

#[derive(Debug, thiserror::Error)]
pub enum SwapError {
    #[error("Transaction error: {0}")] TransactionError(String),
    #[error("Token error: {0}")] TokenError(String),
    #[error("Invalid transaction data")]
    InvalidTransactionData,
    #[error("Program error: {0}")] ProgramError(#[from] ProgramError),
    #[error("Client error: {0}")] ClientError(#[from] ClientError),
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

    let user = Pubkey::from_str("4VZscJb8n2iiV4VyUSeaXPFw53mqqwH55BX6JRFaveia").unwrap();

    let rpc_endpoint = std::env
        ::var("RPC_URL")
        .expect("You must set the RPC_URL environment variable!");
    let client: Arc<RpcClient> = Arc::new(RpcClient::new(rpc_endpoint.to_string()));

    let program_client: Arc<dyn ProgramClient<ProgramRpcClientSendTransaction>> = Arc::new(
        ProgramRpcClient::new(client.clone(), ProgramRpcClientSendTransaction)
    );
    dbg!("Køber nu");

    let keypair_arc = Arc::new(keypair);

    let amount_in: u64 = (sol_amount * 1_000_000_000.0) as u64;

    // Determine if SOL is the input or output token
    let (token_in_mint, token_out_mint, _is_sol_input) = if
        key_z.base_mint.to_string() == "So11111111111111111111111111111111111111112"
    {
        (key_z.base_mint.clone(), key_z.quote_mint.clone(), true)
    } else if key_z.quote_mint.to_string() == "So11111111111111111111111111111111111111112" {
        (key_z.quote_mint.clone(), key_z.base_mint.clone(), false)
    } else {
        return Err(SwapError::InvalidTransactionData);
    };

    dbg!("Token in mint: {}", token_in_mint);
    dbg!("Token out mint: {}", token_out_mint);

    let token_in = Token::new(
        program_client.clone(),
        &spl_token::ID,
        &token_in_mint,
        None,
        keypair_arc.clone()
    );
    let token_out = Token::new(
        program_client.clone(),
        &spl_token::ID,
        &token_out_mint,
        None,
        keypair_arc.clone()
    );

    // Get the user's ATA or create it if it does not exist
    let user_in_token_account = token_in.get_associated_token_address(&user);
    dbg!("User input-tokens ATA={}", user_in_token_account);

    // Check if the user's token account exists
    let user_in_acct = match token_in.get_account_info(&user_in_token_account).await {
        Ok(account_info) => account_info,
        Err(_) => {
            // Create the user's token account if it does not exist
            dbg!("Creating user's input-tokens ATA");
            let create_account_instruction = instruction::create_associated_token_account(
                &keypair_arc.pubkey(),
                &user,
                &token_in_mint,
                &spl_token::id()
            );

            let config = SmartTransactionConfig {
                create_config: CreateSmartTransactionConfig {
                    instructions: [create_account_instruction].to_vec(),
                    signers: vec![&keypair_arc],
                    lookup_tables: None,
                    fee_payer: None,
                },
                send_options: RpcSendTransactionConfig {
                    skip_preflight: true,
                    preflight_commitment: None,
                    encoding: None,
                    max_retries: None,
                    min_context_slot: None,
                },
            };

            match helius.send_smart_transaction_with_tip(config, Some(64000), Some("NY")).await {
                Ok(signature) => {
                    dbg!("Transaction sent successfully: {}", signature);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    // Retry fetching the user's token account info
                    token_in
                        .get_account_info(&user_in_token_account).await
                        .map_err(|err| {
                            SwapError::TransactionError(
                                format!("Failed to fetch user's input-tokens ATA after creation: {}", err)
                            )
                        })?
                }
                Err(e) => {
                    return Err(SwapError::TransactionError(e.to_string()));
                }
            }
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

        let intructions_set = vec![transfer_instruction, sync_instruction];
        // Create the SmartTransactionConfig
        let config = SmartTransactionConfig {
            create_config: CreateSmartTransactionConfig {
                instructions: intructions_set,
                signers: vec![&keypair_arc],
                lookup_tables: None,
                fee_payer: None,
            },
            send_options: RpcSendTransactionConfig {
                skip_preflight: true,
                preflight_commitment: None,
                encoding: None,
                max_retries: None,
                min_context_slot: None,
            },
        };

        match helius.send_smart_transaction_with_tip(config, Some(64000), Some("NY")).await {
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

    // Send some sol from account to the ata and then call sync native
    if token_in.is_native() && ata_creation_bundle.token_in.balance < amount_in {
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
        // An SPL token is an input. If the ATA token address does not exist, it means that the balance is definitely 0.
        if ata_creation_bundle.token_in.balance < amount_in {
            dbg!("Input token not native. Checking sufficient balance");
            return Err(
                SwapError::TokenError("Insufficient balance in input token account".to_string())
            );
        }
    }

    let mut slippage = 30.0;

    let mut retries = 0;
    let max_retries = 3;

    loop {
        // Calculate amount out
        let (_amount_out, minimum_amount_out) = utils::get_out_amount(
            &token_out_mint.to_string(),
            amount_in,
            &slippage
        ).await?;

        if minimum_amount_out == 0 {
            return Err(
                SwapError::TokenError("Minimum expected output amount is zero.".to_string())
            );
        }

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
            minimum_amount_out as u64
        )?;
        instructions.push(swap_instruction);

        let mut token_vaults = TokenVaults {
            base_vault: "".to_string(),
            quote_vault: "".to_string(),
            base_mint: key_z.quote_mint.to_string(),
            quote_mint: "So11111111111111111111111111111111111111112".to_string(),
        };

        if key_z.quote_mint.to_string() == "So11111111111111111111111111111111111111112" {
            // Swap base_mint and quote_mint if quote_mint is SOL
            token_vaults = TokenVaults {
                base_vault: key_z.quote_vault.to_string(),
                quote_vault: key_z.base_vault.to_string(),
                base_mint: key_z.base_mint.to_string(),
                quote_mint: "So11111111111111111111111111111111111111112".to_string(),
            };
        }

        // Create the SmartTransactionConfig
        let config = SmartTransactionConfig {
            create_config: CreateSmartTransactionConfig {
                instructions: instructions.clone(),
                signers: vec![&keypair_arc],
                lookup_tables: None,
                fee_payer: None,
            },
            send_options: RpcSendTransactionConfig {
                skip_preflight: true,
                preflight_commitment: None,
                encoding: None,
                max_retries: None,
                min_context_slot: None,
            },
        };

        match helius.send_smart_transaction_with_tip(config, Some(64000), Some("NY")).await {
            Ok(signature) => {
                dbg!("Transaction sent successfully: {}", signature);
                let saved_details = save_buy_details(
                    client.clone(),
                    &signature,
                    lp_decimals,
                    &token_out_mint.to_string(),
                    token_vaults,
                    false
                ).await;

                if let Err(e) = saved_details {
                    return Err(SwapError::TransactionError(e.to_string()));
                }

                return Ok(signature.to_string());
            }
            Err(e) => {
                // Log the error for further investigation
                dbg!("Error sending transaction: {:?}", &e);
                // Check if the error is "Error fetching compute units for the instructions provided"
                if
                    e.to_string() ==
                    "InvalidInput(\"Error fetching compute units for the instructions provided\")"
                {
                    if retries < max_retries {
                        retries += 1;
                        dbg!(
                            "Retrying transaction attempt DER SKET DEN SPECIALLE FEJL HER {}/{} after 15 seconds",
                            retries,
                            max_retries
                        );
                        tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
                        // Increase slippage and rebuild the transaction if necessary
                        let user_out_token_account = token_out.get_associated_token_address(&user);
                        let token_balance = client
                            .get_balance(&user_out_token_account).await
                            .unwrap();

                        if token_balance == 0 {
                            slippage = 8.0;
                            continue;
                        } else {
                            return Err(SwapError::TransactionError(e.to_string()));
                        }
                    } else {
                        return Err(SwapError::TransactionError(e.to_string()));
                    }
                } else {
                    if retries < max_retries {
                        retries += 1;
                        dbg!(
                            "Retrying transaction attempt {}/{} immediately",
                            retries,
                            max_retries
                        );
                        // Increase slippage and rebuild the transaction if necessary
                        slippage = 5.0;

                        continue;
                    } else {
                        return Err(SwapError::TransactionError(e.to_string()));
                    }
                }
            }
        }
    }
}
