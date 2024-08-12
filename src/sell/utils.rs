use solana_sdk::pubkey::Pubkey;
use serde::{ Deserialize, Serialize };
use solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta;
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_filter::{ RpcFilterType, Memcmp, MemcmpEncodedBytes },
    rpc_config::{ RpcProgramAccountsConfig, RpcAccountInfoConfig },
};
use solana_sdk::commitment_config::CommitmentConfig;
use solana_account_decoder::UiAccountEncoding;
use std::str::FromStr;
use std::sync::Arc;
use std::error::Error;

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

#[derive(Serialize, Deserialize, Debug)]
pub struct LiquidityPoolKeysV4 {
    pub id: Pubkey,
    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,
    pub lp_mint: Pubkey,
    pub base_decimals: u8,
    pub quote_decimals: u8,
    pub lp_decimals: u8,
    pub version: u8,
    pub program_id: Pubkey,
    pub authority: Pubkey,
    pub open_orders: Pubkey,
    pub target_orders: Pubkey,
    pub base_vault: Pubkey,
    pub quote_vault: Pubkey,
    pub market_version: u8,
    pub market_program_id: Pubkey,
    pub market_id: Pubkey,
    pub market_authority: Pubkey,
    pub market_base_vault: Pubkey,
    pub market_quote_vault: Pubkey,
    pub market_bids: Pubkey,
    pub market_asks: Pubkey,
    pub market_event_queue: Pubkey,
    pub withdraw_queue: Pubkey,
    pub lp_vault: Pubkey,
    pub lookup_table_account: Pubkey,
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

    // Try fetching accounts with " token_sol_filters" filters first
    if let Some(account) = fetch_accounts(client.clone(), token_sol_filters).await? {
        return Ok(Some(account));
    }

    // If no account found, try fetching with "token-sol" filters
    fetch_accounts(client, sol_token_filters).await
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

fn create_pool_keys(
    id: Pubkey,
    pool_state: MyAccountData,
    minimal_market_layout_v3: MinimalMarketLayoutV3
) -> LiquidityPoolKeysV4 {
    LiquidityPoolKeysV4 {
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
        lookup_table_account: Pubkey::default(),
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
) -> Result<Option<LiquidityPoolKeysV4>, Box<dyn Error>> {
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

pub async fn calculate_sol_amount_received(
    tx: &EncodedConfirmedTransactionWithStatusMeta,
    _rpc_client: &Arc<RpcClient>,
    _mint: &Pubkey
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
    let mut sol_amount_received: f64 = 0.0;

    for (pre_balance, post_balance) in pre_balances.iter().zip(post_balances.iter()) {
        if post_balance > pre_balance {
            let difference = post_balance - pre_balance;
            sol_amount_received += difference as f64;
        }
    }

    // Convert lamports to SOL (1 SOL = 1_000_000_000 lamports)
    let sol_amount = sol_amount_received / 1_000_000_000.0;

    println!("Calculated SOL amount: {}", sol_amount);

    Ok(sol_amount)
}
