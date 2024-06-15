use super::raydium_sdk;
use super::utils;
use super::price;
use super::mongo;
use solana_client::nonblocking::rpc_client::RpcClient;
use price::get_current_sol_price;
use raydium_sdk::LiquidityPoolKeys;
use solana_sdk::instruction::Instruction;
use solana_sdk::signature::Keypair;
use solana_sdk::commitment_config::CommitmentConfig;
use mongo::{ TokenInfo, BuyTransaction, MongoHandler, TransactionType };
use std::time::Duration;
use utils::{ get_second_instruction_amount, get_token_metadata };
use solana_sdk::transaction::Transaction;
use solana_transaction_status::option_serializer::OptionSerializer;
use solana_transaction_status::UiInnerInstructions;
use solana_transaction_status::UiTransactionEncoding;
use mongodb::bson::DateTime;
use std::sync::Arc;
use spl_token_client::token::Token;
use spl_token_client::client::{ ProgramClient, ProgramRpcClient, ProgramRpcClientSendTransaction };
use solana_sdk::signature::Signer;
use raydium_contract_instructions::amm_instruction as amm;
use solana_client::rpc_config::RpcSendTransactionConfig;
use solana_sdk::program_error::ProgramError;
use utils::get_or_create_ata_for_token_in_and_out_with_balance;

#[derive(Debug, thiserror::Error)]
pub enum SwapError {
    #[error("Transaction error: {0}")] TransactionError(String),
    #[error("Token error: {0}")] TokenError(String),
    #[error("MongoDB error: {0}")] MongoDBError(String),
    #[error("Invalid transaction data")]
    InvalidTransactionData,
    #[error("Program error: {0}")] ProgramError(#[from] ProgramError), // Add conversion from ProgramError
}

pub async fn buy_swap(
    key_z: LiquidityPoolKeys,
    lp_decimals: u8,
    sol_amount: f64
) -> Result<String, SwapError> {
    let mut retry_count = 0;
    let max_retries = 24;
    let retry_delay = Duration::from_secs(8);

    let private_key = std::env
        ::var("PRIVATE_KEY")
        .expect("You must set the PRIVATE_KEY environment variable!");
    let keypair = Keypair::from_base58_string(&private_key);

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
    let mut instructions: Vec<Instruction> = vec![];
    let ata_creation_bundle = get_or_create_ata_for_token_in_and_out_with_balance(
        &token_in,
        &token_out,
        keypair_arc.clone()
    ).await.unwrap();

    {
        instructions.push(
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(25000)
        );
        instructions.push(
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(600000)
        );
    }
    //Create input ATAs if instruction exist
    if ata_creation_bundle.token_in.instruction.is_some() {
        println!(
            "Creating ata for token-in {:?}. ATA is {:?}",
            token_in.get_address(),
            ata_creation_bundle.token_in.ata_pubkey
        );
        instructions.push(ata_creation_bundle.token_in.instruction.unwrap());
    }
    if ata_creation_bundle.token_out.instruction.is_some() {
        println!(
            "Creating ata for token-out {:?} ATA is {:?}",
            token_out.get_address(),
            ata_creation_bundle.token_out.ata_pubkey
        );
        instructions.push(ata_creation_bundle.token_out.instruction.unwrap());
    }

    let amount_in: u64 = (sol_amount * 1_000_000_000.0) as u64;

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

    loop {
        if retry_count > max_retries {
            return Err(SwapError::TransactionError("Max retries exceeded".to_string()));
        }

        let recent_blockhash = match client.get_latest_blockhash().await {
            Ok(blockhash) => blockhash,
            Err(err) => {
                return Err(
                    SwapError::TransactionError(
                        format!("Error getting latest blockhash: {:?}", err)
                    )
                );
            }
        };

        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&keypair_arc.pubkey()),
            &[&keypair_arc],
            recent_blockhash
        );

        let result = client.send_and_confirm_transaction_with_spinner_and_config(
            &transaction,
            CommitmentConfig::confirmed(),
            RpcSendTransactionConfig::default()
        ).await;

        match result {
            Ok(signature) => {
                let transaction_signature = signature.to_string();
                // Retry loop for confirming the transaction
                let mut confirmed = false;
                while !confirmed && retry_count <= max_retries {
                    match
                        client.get_transaction(&signature, UiTransactionEncoding::JsonParsed).await
                    {
                        Ok(confirmed_transaction) => {
                            let inner_instructions: Vec<UiInnerInstructions> =
                                confirmed_transaction.transaction.meta
                                    .as_ref()
                                    .and_then(|data| {
                                        match &data.inner_instructions {
                                            OptionSerializer::Some(inner) => Some(inner.clone()),
                                            _ => None,
                                        }
                                    })
                                    .unwrap_or_else(|| Vec::new());

                            let amount: Option<String> = get_second_instruction_amount(
                                &inner_instructions
                            );

                            if let Some(ref amount_str) = amount {
                                // Parse the amount as f64
                                let amount = amount_str.parse::<f64>().unwrap_or_default();

                                // Assume `lp_decimals` is of type u8
                                let token_decimals = lp_decimals as f64;

                                // Adjust the token amount using the decimals
                                let adjusted_token_amount = amount / (10f64).powf(token_decimals);

                                // Calculate the buy price per token in SOL
                                let buy_price_per_token_in_sol = sol_amount / adjusted_token_amount;

                                // Fetch the current SOL price in USD
                                let current_sol_price =
                                    get_current_sol_price().await.unwrap_or_default();

                                let usd_amount = sol_amount * current_sol_price;

                                // Calculate the buy price in USD
                                let buy_price_usd = buy_price_per_token_in_sol * current_sol_price;

                                let token_info = TokenInfo {
                                    base_mint: key_z.base_mint.to_string(),
                                    quote_mint: key_z.quote_mint.to_string(),
                                    base_vault: key_z.base_vault.to_string(),
                                    quote_vault: key_z.quote_vault.to_string(),
                                };

                                // Initialize MongoDB handler
                                let mongo_handler = match MongoHandler::new().await {
                                    Ok(handler) => handler,
                                    Err(err) => {
                                        return Err(
                                            SwapError::MongoDBError(
                                                format!("Error creating MongoDB handler: {:?}", err)
                                            )
                                        );
                                    }
                                };

                                let token_metadata = match
                                    get_token_metadata(
                                        &key_z.base_mint.to_string(),
                                        adjusted_token_amount,
                                        &client
                                    ).await
                                {
                                    Ok(metadata) => metadata,
                                    Err(err) => {
                                        return Err(
                                            SwapError::MongoDBError(
                                                format!("Error getting token metadata: {:?}", err)
                                            )
                                        );
                                    }
                                };

                                let buy_transaction: BuyTransaction = BuyTransaction {
                                    transaction_signature: transaction_signature.clone(),
                                    token_info: token_info.clone(),
                                    amount,
                                    sol_amount,
                                    sol_price: current_sol_price,
                                    usd_amount,
                                    token_metadata: token_metadata.clone(),
                                    entry_price: buy_price_usd,
                                    transaction_type: TransactionType::LongTermHold,
                                    created_at: DateTime::now(),
                                };

                                // Store transaction info in MongoDB
                                if
                                    let Err(e) = mongo_handler.store_buy_transaction_info(
                                        buy_transaction,
                                        "solsniper",
                                        "buy_transactions"
                                    ).await
                                {
                                    eprintln!("Error storing transaction info: {:?}", e);
                                }

                                if
                                    let Err(e) = mongo_handler.store_token(
                                        token_metadata,
                                        "solsniper",
                                        "tokens"
                                    ).await
                                {
                                    eprintln!("Error storing transaction info: {:?}", e);
                                }
                            } else {
                                eprintln!("Error getting amount from inner instructions");
                            }

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

                if confirmed {
                    return Ok(signature.to_string());
                }
            }
            Err(e) => {
                retry_count += 1;
                dbg!("Error sending transaction. Retrying...", e);
                tokio::time::sleep(retry_delay).await;
            }
        }
    }
}
