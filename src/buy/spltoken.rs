use super::raydium_sdk;
use raydium_sdk::TOKEN_PROGRAM_ID;
use solana_sdk::{ pubkey::Pubkey, signature::Keypair, transaction::Transaction };
use spl_associated_token_account::{
    get_associated_token_address,
    instruction::create_associated_token_account,
};
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_client::rpc_client::RpcClient;

pub fn get_or_create_associated_token_account(
    client: &RpcClient,
    key_payer: &Keypair,
    wallet_address: &Pubkey,
    token_mint_address: &Pubkey
) -> Result<Pubkey, Box<dyn std::error::Error>> {
    const MAX_RETRIES: usize = 5;
    const INITIAL_BACKOFF: u64 = 200; // milliseconds
    let mut backoff = INITIAL_BACKOFF;

    let associated_token_account_address = get_associated_token_address(
        &wallet_address,
        &token_mint_address
    );

    let mut retries = 0;
    loop {
        let account_exists = client.get_account(&associated_token_account_address).is_ok();

        if !account_exists {
            // Create the associated token account
            let create_ata_instruction = create_associated_token_account(
                &wallet_address,
                &wallet_address,
                &token_mint_address,
                &*TOKEN_PROGRAM_ID
            );

            let instructions = vec![
                // Set priority fees
                ComputeBudgetInstruction::set_compute_unit_price(25000),
                ComputeBudgetInstruction::set_compute_unit_limit(600000),
                create_ata_instruction
            ];

            let recent_blockhash = client
                .get_latest_blockhash()
                .expect("Failed to get recent blockhash");

            let transaction = Transaction::new_signed_with_payer(
                &instructions,
                Some(&wallet_address),
                &[&key_payer],
                recent_blockhash
            );

            match client.send_and_confirm_transaction_with_spinner(&transaction) {
                Ok(_signature) => {
                    return Ok(associated_token_account_address);
                }
                Err(_) if retries < MAX_RETRIES => {
                    retries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(backoff));
                    backoff *= 2;
                }
                Err(err) => {
                    return Err(Box::new(err));
                }
            }
        } else {
            return Ok(associated_token_account_address);
        }
    }
}
