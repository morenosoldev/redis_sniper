use super::utils;
use super::mongo::TokenMetadata;
use spl_token_client::client::{ ProgramClient, ProgramRpcClient, ProgramRpcClientSendTransaction };
use spl_token_client::token::Token;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::signature::Signature;
use std::sync::Arc;
use raydium_contract_instructions::amm_instruction as amm;
use utils::get_liquidity_pool;
use solana_sdk::transaction::Transaction;
use solana_client::nonblocking::rpc_client::RpcClient;
use spl_token_client::token::TokenError;
use solana_sdk::signature::Keypair;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_client::rpc_config::RpcSendTransactionConfig;
use serde::{ Serialize, Deserialize };
use std::str::FromStr;

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

    let out_token = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();

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

    let fee_percentage = 1.0;
    let fee_vault: Option<Pubkey> = Some(user);

    // If a fee recipient is specified then setup its token account to receive fee tokens (create if needed).
    // Fee tokens are always paid in the input token.
    let mut fee_vault_token_account = None;
    if let Some(vault_key) = fee_vault {
        let vault_in_token_account = in_token_client.get_associated_token_address(&vault_key);
        dbg!("Vault's input-token ATA={}", vault_in_token_account);
        match in_token_client.get_account_info(&vault_in_token_account).await {
            Ok(_) => {
                dbg!("Vault ATA for input tokens exists. Skipping creation.");
            }
            Err(TokenError::AccountNotFound) | Err(TokenError::AccountInvalidOwner) => {
                dbg!("Vault's input-tokens ATA does not exist. Creating..");
                in_token_client.create_associated_token_account(&vault_key).await?;
            }
            Err(error) => {
                return Err(error.into()); // Return the error to handle it properly
            }
        }
        fee_vault_token_account = Some(vault_in_token_account);
    }

    let mut instructions = vec![];

    {
        instructions.push(
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(25000)
        );
        instructions.push(
            solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(600000)
        );
    }

    let mut swap_amount_in = sell_transaction.amount;
    println!("Swap amount in: {}", swap_amount_in);
    if let Some(vault_token_account) = fee_vault_token_account {
        let percent = fee_percentage;
        if percent >= 100.0 {
            return Err("Fee percentage must be less than 100".into());
        }
        let fee = ((percent / 100.0) * (sell_transaction.amount as f64)).trunc() as u64;
        swap_amount_in -= fee;
        // Append instruction to transfer fees to fee vault.
        let fee_transfer_instruction: Instruction = spl_token::instruction::transfer(
            &spl_token::ID,
            &user_in_token_account,
            &vault_token_account,
            &user,
            &[&user],
            fee
        )?;
        dbg!(
            "Appending fee-transfer instruction. Fee-percentage={}, Fee-amount={}. Fee-vault-owner={}. Fee-vault-ata={}",
            percent,
            fee,
            fee_vault,
            vault_token_account
        );
        instructions.push(fee_transfer_instruction);
    }

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

    let max_attempts = 5;
    let mut attempts = 0;

    loop {
        let recent_blockhash = match client.get_latest_blockhash().await {
            Ok(blockhash) => blockhash,
            Err(err) => {
                dbg!("Error getting latest blockhash: {:?}", err);
                attempts += 1;
                if attempts >= max_attempts {
                    return Err("Error getting latest blockhash".into());
                } else {
                    continue;
                }
            }
        };

        let transaction = Transaction::new_signed_with_payer(
            &instructions,
            Some(&keypair_arc.pubkey()),
            &[&keypair_arc],
            recent_blockhash
        );

        let res = client.send_and_confirm_transaction_with_spinner_and_config(
            &transaction,
            CommitmentConfig::confirmed(),
            RpcSendTransactionConfig {
                ..RpcSendTransactionConfig::default()
            }
        ).await;

        match res {
            Ok(signature) => {
                dbg!("Transaction successful with signature: {}", signature);
                return Ok(signature);
            }
            Err(e) => {
                dbg!("Error sending and confirming transaction: {:?}", e);
                attempts += 1;
                if attempts >= max_attempts {
                    return Err("Error sending and confirming transaction".into());
                }
            }
        }
    }
}
