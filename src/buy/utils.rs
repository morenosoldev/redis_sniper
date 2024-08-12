use solana_transaction_status::{ UiInnerInstructions, UiInstruction, UiParsedInstruction };
use serde_json::Value;
use mpl_token_metadata::accounts::Metadata;
pub use mpl_token_metadata::ID;
use super::mongo;
use super::raydium_sdk;
use mongo::TokenMetadata;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use solana_sdk::{ instruction::Instruction, signer::Signer };
use spl_token_client::{
    client::{ SendTransaction, SimulateTransaction },
    token::{ Token, TokenError },
};
use solana_account_decoder::UiAccountEncoding;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta;

use std::sync::Arc;
use std::error::Error;
use serde::{ Deserialize, Serialize };
use solana_client::{
    rpc_filter::{ RpcFilterType, Memcmp, MemcmpEncodedBytes },
    rpc_config::{ RpcProgramAccountsConfig, RpcAccountInfoConfig },
};
use raydium_sdk::LiquidityPoolKeys;
use crate::buy::buy::SwapError;

#[derive(Debug, Deserialize)]
struct SwapResponse {
    id: String,
    success: bool,
    version: String,
    data: SwapData,
}

#[derive(Debug, Deserialize)]
struct SwapData {
    swapType: String,
    inputMint: String,
    inputAmount: String,
    outputMint: String,
    outputAmount: String,
    otherAmountThreshold: String,
    slippageBps: i32,
    priceImpactPct: f64,
    referrerAmount: String,
    routePlan: Vec<RoutePlan>,
}

#[derive(Debug, Deserialize)]
struct RoutePlan {
    poolId: String,
    inputMint: String,
    outputMint: String,
    feeMint: String,
    feeRate: i32,
    feeAmount: String,
    remainingAccounts: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct MinimalMarketLayoutV3 {
    event_queue: Pubkey,
    bids: Pubkey,
    asks: Pubkey,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LiquidityStateV4 {
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub lp_mint: Pubkey,
    pub base_decimal: u64,
    pub quote_decimal: u64,
    pub open_orders: Pubkey,
    pub target_orders: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub market_program_id: Pubkey,
    pub market_id: Pubkey,
    pub withdraw_queue: Pubkey,
    pub lp_vault: Pubkey,
}

pub async fn get_program_account(
    client: Arc<RpcClient>,
    mint: &Pubkey
) -> Result<Option<(Pubkey, solana_sdk::account::Account)>, Box<dyn Error>> {
    const INPUT_MINT_OFFSET: usize = 400;
    const OUTPUT_MINT_OFFSET: usize = 432;
    const PROGRAM_ID: &str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";

    dbg!(&mint);

    // Define the common filters
    let common_filters = vec![RpcFilterType::DataSize(752)];

    // Define the filters for "sol-token"
    let sol_token_filters = common_filters
        .iter()
        .chain(
            vec![
                RpcFilterType::Memcmp(
                    Memcmp::new(OUTPUT_MINT_OFFSET, MemcmpEncodedBytes::Base58(mint.to_string()))
                ),
                RpcFilterType::Memcmp(
                    Memcmp::new(
                        INPUT_MINT_OFFSET,
                        MemcmpEncodedBytes::Base58(
                            "So11111111111111111111111111111111111111112".to_string()
                        )
                    )
                )
            ].iter()
        )
        .cloned()
        .collect::<Vec<_>>();

    // Define the filters for "token-sol"
    let token_sol_filters = common_filters
        .iter()
        .chain(
            vec![
                RpcFilterType::Memcmp(
                    Memcmp::new(INPUT_MINT_OFFSET, MemcmpEncodedBytes::Base58(mint.to_string()))
                ),
                RpcFilterType::Memcmp(
                    Memcmp::new(
                        OUTPUT_MINT_OFFSET,
                        MemcmpEncodedBytes::Base58(
                            "So11111111111111111111111111111111111111112".to_string()
                        )
                    )
                )
            ].iter()
        )
        .cloned()
        .collect::<Vec<_>>();

    // Function to fetch accounts based on filters
    async fn fetch_accounts(
        client: Arc<RpcClient>,
        filters: Vec<RpcFilterType>
    ) -> Result<Option<(Pubkey, solana_sdk::account::Account)>, Box<dyn Error>> {
        let accounts = client.get_program_accounts_with_config(
            &Pubkey::from_str(PROGRAM_ID)?,
            RpcProgramAccountsConfig {
                filters: Some(filters),
                account_config: RpcAccountInfoConfig {
                    encoding: Some(UiAccountEncoding::Base64),
                    ..Default::default()
                },
                ..Default::default()
            }
        ).await;

        println!("{:?}", accounts);

        match accounts {
            Ok(accounts) => Ok(accounts.into_iter().next()),
            Err(e) => Err(Box::new(e)),
        }
    }

    // Try fetching accounts with "sol-token" filters first
    if let Some(account) = fetch_accounts(client.clone(), sol_token_filters).await? {
        return Ok(Some(account));
    }

    // If no account found, try fetching with "token-sol" filters
    fetch_accounts(client, token_sol_filters).await
}

async fn get_minimal_market_v3(client: &RpcClient, market_id: Pubkey) -> MinimalMarketLayoutV3 {
    let market_info = client
        .get_account_with_commitment(&market_id, CommitmentConfig::finalized()).await
        .unwrap();

    let minimal_market_layout_v3: MinimalMarketLayoutV3 = bincode
        ::deserialize(&market_info.value.unwrap().data)
        .unwrap();
    minimal_market_layout_v3
}

pub async fn get_out_amount(
    output_mint: &str,
    amount: u64,
    slippage_pct: &f64
) -> Result<(u64, u64), SwapError> {
    let slippage_bps = (slippage_pct * 100.0).round() as i32;

    let url = format!(
        "https://transaction-v1.raydium.io/compute/swap-base-in?inputMint=So11111111111111111111111111111111111111112&outputMint={}&amount={}&slippageBps={}&txVersion=V0",
        output_mint,
        amount,
        slippage_bps
    );

    let client = reqwest::Client::new();
    let response = match
        client
            .get(&url)
            .header(
                "User-Agent",
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:128.0) Gecko/20100101 Firefox/128.0"
            )
            .header("Accept", "application/json, text/plain, */*")
            .header("Accept-Language", "en-US,en;q=0.5")
            .header("Accept-Encoding", "gzip, deflate, br, zstd")
            .header("Referer", "https://raydium.io/")
            .header("Origin", "https://raydium.io")
            .header("Connection", "keep-alive")
            .header("Sec-Fetch-Dest", "empty")
            .header("Sec-Fetch-Mode", "cors")
            .header("Sec-Fetch-Site", "same-site")
            .header("Sec-GPC", "1")
            .header("TE", "trailers")
            .header(
                "Cookie",
                "__cf_bm=hVD6YdWXvyXMAGrApQGdgnXTDA8rjORMmO6F6dIa1l0-1722813884-1.0.1.1-.LXXz1cn23gsMerhHo9pmRreQ.Su6Xg7jqTkzWUzWUrPZF_wuQQgIpqCkX0B7KxQ4sskIrXriotSRVrDtSXZBQ; path=/; expires=Sun, 04-Aug-24 23:54:44 GMT; domain=.raydium.io; HttpOnly; Secure; SameSite=None"
            )
            .send().await
    {
        Ok(resp) => { resp }
        Err(err) => {
            return Err(SwapError::TokenError(err.to_string()));
        }
    };

    let swap_response: SwapResponse = match response.json().await {
        Ok(json) => json,
        Err(err) => {
            return Err(SwapError::TokenError(err.to_string()));
        }
    };

    println!("{:?}", swap_response);
    let output_amount = match swap_response.data.outputAmount.parse::<u64>() {
        Ok(amount) => amount,
        Err(err) => {
            return Err(SwapError::TokenError(err.to_string()));
        }
    };

    let min_amount = match swap_response.data.otherAmountThreshold.parse::<u64>() {
        Ok(amount) => amount,
        Err(err) => {
            return Err(SwapError::TokenError(err.to_string()));
        }
    };

    Ok((output_amount, min_amount))
}

fn create_pool_keys(
    id: Pubkey,
    pool_state: MyAccountData,
    minimal_market_layout_v3: MinimalMarketLayoutV3
) -> LiquidityPoolKeys {
    LiquidityPoolKeys {
        id,
        base_mint: pool_state.base_mint,
        quote_mint: pool_state.quote_mint,
        lp_mint: pool_state.lp_mint,
        base_decimals: pool_state.base_decimal as u8,
        quote_decimals: pool_state.quote_decimal as u8,
        lp_decimals: 5,
        version: 4,
        program_id: Pubkey::from_str("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8").unwrap(),
        authority: Pubkey::from_str("5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1").unwrap(),
        open_orders: pool_state.open_orders,
        target_orders: pool_state.target_orders,
        base_vault: pool_state.base_vault,
        quote_vault: pool_state.quote_vault,
        market_version: 3,
        market_program_id: pool_state.market_program_id,
        market_id: pool_state.market_id,
        market_authority: get_associated_authority(
            &pool_state.market_program_id,
            &pool_state.market_id
        ).unwrap(),
        market_base_vault: pool_state.base_vault,
        market_quote_vault: pool_state.quote_vault,
        market_bids: minimal_market_layout_v3.bids,
        market_asks: minimal_market_layout_v3.asks,
        market_event_queue: minimal_market_layout_v3.event_queue,
        withdraw_queue: pool_state.withdraw_queue,
        lp_vault: pool_state.lp_vault,
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct MyAccountData {
    status: u64,
    nonce: u64,
    max_order: u64,
    depth: u64,
    base_decimal: u64,
    quote_decimal: u64,
    state: u64,
    reset_flag: u64,
    min_size: u64,
    vol_max_cut_ratio: u64,
    amount_wave_ratio: u64,
    base_lot_size: u64,
    quote_lot_size: u64,
    min_price_multiplier: u64,
    max_price_multiplier: u64,
    system_decimal_value: u64,
    min_separate_numerator: u64,
    min_separate_denominator: u64,
    trade_fee_numerator: u64,
    trade_fee_denominator: u64,
    pnl_numerator: u64,
    pnl_denominator: u64,
    swap_fee_numerator: u64,
    swap_fee_denominator: u64,
    base_need_take_pnl: u64,
    quote_need_take_pnl: u64,
    quote_total_pnl: u64,
    base_total_pnl: u64,
    pool_open_time: u64,
    punish_pc_amount: u64,
    punish_coin_amount: u64,
    orderbook_to_init_time: u64,
    swap_base_in_amount: u128,
    swap_quote_out_amount: u128,
    swap_base2quote_fee: u64,
    swap_quote_in_amount: u128,
    swap_base_out_amount: u128,
    swap_quote2base_fee: u64,
    base_vault: Pubkey,
    quote_vault: Pubkey,
    base_mint: Pubkey,
    quote_mint: Pubkey,
    lp_mint: Pubkey,
    open_orders: Pubkey,
    market_id: Pubkey,
    market_program_id: Pubkey,
    target_orders: Pubkey,
    withdraw_queue: Pubkey,
    lp_vault: Pubkey,
    owner: Pubkey,
    lp_reserve: u64,
    padding: [u64; 3],
}

pub async fn get_liquidity_pool(
    client: Arc<RpcClient>,
    mint: &Pubkey
) -> Result<Option<LiquidityPoolKeys>, Box<dyn Error>> {
    match get_program_account(client.clone(), mint).await {
        Ok(Some(account)) => {
            let pool_state: MyAccountData = bincode
                ::deserialize(&account.1.data)
                .expect("Failed to deserialize data");

            let minimal_market_layout_v3 = get_minimal_market_v3(
                &client,
                pool_state.market_id
            ).await;
            let pool_keys = create_pool_keys(account.0, pool_state, minimal_market_layout_v3);

            Ok(Some(pool_keys))
        }
        Ok(None) => { Ok(None) }
        Err(e) => { Err(e) }
    }
}

pub fn get_associated_authority(
    program_id: &Pubkey,
    market_id: &Pubkey
) -> std::result::Result<Pubkey, String> {
    let market_id_bytes = market_id.to_bytes();
    let seeds = &[&market_id_bytes[..]];

    for nonce in 0..100u8 {
        let nonce_bytes = [nonce];
        let padding = [0u8; 7];

        let seeds_with_nonce = [
            seeds[0], // Market ID bytes
            &nonce_bytes, // Nonce bytes
            &padding, // Padding bytes
        ];

        match Pubkey::create_program_address(&seeds_with_nonce, program_id) {
            Ok(public_key) => {
                return Ok(public_key);
            }
            Err(_) => {
                continue;
            }
        }
    }

    Err("Unable to find a valid program address".into())
}

pub fn get_second_instruction_amount(
    inner_instructions: &Vec<UiInnerInstructions>,
    is_pump: bool
) -> Option<String> {
    let instruction_index = if is_pump { 0 } else { 1 };

    // Iterate over each UiInnerInstructions
    for inner in inner_instructions {
        dbg!(inner);

        // Check if there are enough instructions
        if inner.instructions.len() > instruction_index {
            // Get the desired instruction
            let instruction = &inner.instructions[instruction_index];

            // Debug print the instruction to understand its structure
            println!("{:?}", instruction);

            // Check if the instruction is parsed
            if let UiInstruction::Parsed(parsed_instruction) = instruction {
                if let UiParsedInstruction::Parsed(instruct) = parsed_instruction {
                    // Extract the amount from the parsed data
                    if let Some(info) = instruct.parsed.get("info") {
                        println!("Info: {:?}", info);
                        if let Some(amount) = info.get("amount") {
                            println!("Amount: {:?}", amount);
                            if let Value::String(amount_str) = amount {
                                return Some(amount_str.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub async fn get_token_metadata(
    mint_address: &str,
    balance: f64,
    client: &RpcClient
) -> Result<TokenMetadata, Box<dyn Error>> {
    let metadata_program_id = &ID; // Replace with your actual metadata program ID
    let token_mint_address = match Pubkey::from_str(mint_address) {
        Ok(pubkey) => pubkey,
        Err(_) => {
            return Ok(default_token_metadata(balance, mint_address));
        }
    };

    // Use match to handle the Result and extract the tuple (address, bump_seed)
    let (metadata_account_address, _) = Pubkey::find_program_address(
        &[b"metadata", metadata_program_id.as_ref(), token_mint_address.as_ref()],
        &metadata_program_id
    );

    let account_data_result = match client.get_account_data(&metadata_account_address).await {
        Ok(data) => data,
        Err(_) => {
            return Ok(default_token_metadata(balance, mint_address));
        }
    };

    let metadata = match Metadata::from_bytes(&account_data_result) {
        Ok(meta) => meta,
        Err(_) => {
            return Ok(default_token_metadata(balance, mint_address));
        }
    };

    let name = metadata.name.trim_matches(char::from(0)).to_string();
    let symbol = metadata.symbol.trim_matches(char::from(0)).to_string();
    let uri = metadata.uri.trim_matches(char::from(0)).to_string();

    let (description, image, twitter, created_on) = match reqwest::get(&uri).await {
        Ok(response) => {
            match response.json::<serde_json::Value>().await {
                Ok(json) =>
                    (
                        json["description"].as_str().unwrap_or_default().to_string(),
                        json["image"].as_str().unwrap_or_default().to_string(),
                        json["twitter"].as_str().unwrap_or_default().to_string(),
                        json["createdOn"].as_str().unwrap_or_default().to_string(),
                    ),
                Err(_) => ("".to_string(), "".to_string(), "".to_string(), "".to_string()),
            }
        }
        Err(_) => ("".to_string(), "".to_string(), "".to_string(), "".to_string()),
    };

    Ok(TokenMetadata {
        name,
        symbol,
        balance,
        mint: mint_address.to_string(),
        description,
        image,
        twitter,
        created_on,
    })
}

fn default_token_metadata(balance: f64, mint_address: &str) -> TokenMetadata {
    TokenMetadata {
        name: "".to_string(),
        symbol: "".to_string(),
        balance,
        mint: mint_address.to_string(),
        description: "".to_string(),
        image: "".to_string(),
        twitter: "".to_string(),
        created_on: "".to_string(),
    }
}
pub struct AtaCreationBundle {
    pub token_in: AtaInfo,
    pub token_out: AtaInfo,
}

pub struct AtaInfo {
    pub instruction: Option<Instruction>,
    pub ata_pubkey: Pubkey,
    pub balance: u64,
}

pub async fn get_or_create_ata_for_token_in_and_out_with_balance<
    S: Signer + 'static,
    PC: SendTransaction + SimulateTransaction + 'static
>(
    token_in: &Token<PC>,
    token_out: &Token<PC>,
    payer: Arc<S>
) -> Result<AtaCreationBundle, Box<dyn std::error::Error>> {
    let (create_token_in_ata_ix, token_in_ata, token_in_balance) = token_ata_creation_instruction(
        token_in,
        &payer
    ).await?;
    let (create_token_out_ata_ix, token_out_ata, token_out_balance) =
        token_ata_creation_instruction(token_out, &payer).await?;

    Ok(AtaCreationBundle {
        token_in: AtaInfo {
            instruction: create_token_in_ata_ix,
            ata_pubkey: token_in_ata,
            balance: token_in_balance,
        },
        token_out: AtaInfo {
            instruction: create_token_out_ata_ix,
            ata_pubkey: token_out_ata,
            balance: token_out_balance,
        },
    })
}

async fn token_ata_creation_instruction<
    S: Signer + 'static,
    PC: SendTransaction + SimulateTransaction + 'static
>(
    token: &Token<PC>,
    payer: &Arc<S>
) -> Result<(Option<Instruction>, Pubkey, u64), Box<dyn std::error::Error>> {
    let payer_token_account = token.get_associated_token_address(&payer.pubkey());
    let (instruction, amount) = match token.get_account_info(&payer_token_account).await {
        Ok(res) => (None, res.base.amount),
        Err(TokenError::AccountNotFound) | Err(TokenError::AccountInvalidOwner) => {
            dbg!("User does not have ATA {payer_token_account} for token. Creating");
            (
                Some(
                    spl_associated_token_account::instruction::create_associated_token_account(
                        &payer.pubkey(),
                        &payer.pubkey(),
                        token.get_address(),
                        &spl_token::ID
                    )
                ),
                0,
            )
        }
        Err(error) => {
            dbg!("Error retrieving user's input-tokens ATA: {}", error);
            return Err("Error retrieving user's input-tokens ATA".into());
        }
    };
    Ok((instruction, payer_token_account, amount))
}

pub async fn calculate_sol_amount_spent(
    tx: &EncodedConfirmedTransactionWithStatusMeta
) -> Result<f64, Box<dyn std::error::Error>> {
    println!("Transaction: {:?}", tx);

    let meta = tx.transaction.meta.as_ref().ok_or("No meta found in the transaction")?;

    // Get the pre and post balances from the meta
    let pre_balances = &meta.pre_balances;
    let post_balances = &meta.post_balances;

    if pre_balances.len() != post_balances.len() {
        return Err("Pre and post balances length mismatch".into());
    }

    // Iterate through balances to find the difference
    let mut sol_amount_spent: f64 = 0.0;

    for (pre_balance, post_balance) in pre_balances.iter().zip(post_balances.iter()) {
        if pre_balance > post_balance {
            let difference = pre_balance - post_balance;
            sol_amount_spent += difference as f64;
        }
    }

    // Convert lamports to SOL (1 SOL = 1_000_000_000 lamports)
    let sol_amount = sol_amount_spent / 1_000_000_000.0;

    println!("Calculated SOL amount spent: {}", sol_amount);

    Ok(sol_amount)
}

/// Helper function for pubkey serialize
pub fn pubkey_to_string<S>(pubkey: &Pubkey, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer
{
    serializer.serialize_str(&pubkey.to_string())
}

/// Helper function for pubkey deserialize
pub fn string_to_pubkey<'de, D>(deserializer: D) -> Result<Pubkey, D::Error>
    where D: serde::Deserializer<'de>
{
    let s = String::deserialize(deserializer)?;
    Pubkey::from_str(&s).map_err(serde::de::Error::custom)
}
