use borsh::{ BorshDeserialize, BorshSerialize };
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use serde::{ Serialize, Deserialize };

#[derive(Debug, Serialize, Deserialize)]
pub struct LiquidityPoolKeysString {
    id: String,
    base_mint: String,
    quote_mint: String,
    lp_mint: String,
    base_decimals: u8,
    quote_decimals: u8,
    lp_decimals: u8,
    version: u8,
    program_id: String,
    authority: String,
    open_orders: String,
    target_orders: String,
    base_vault: String,
    quote_vault: String,
    withdraw_queue: String,
    lp_vault: String,
    market_version: u8,
    market_program_id: String,
    market_id: String,
    market_authority: String,
    market_base_vault: String,
    market_quote_vault: String,
    market_bids: String,
    market_asks: String,
    market_event_queue: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct LiquidityPoolKeys {
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
    pub withdraw_queue: Pubkey,
    pub lp_vault: Pubkey,
    pub market_version: u8,
    pub market_program_id: Pubkey,
    pub market_id: Pubkey,
    pub market_authority: Pubkey,
    pub market_base_vault: Pubkey,
    pub market_quote_vault: Pubkey,
    pub market_bids: Pubkey,
    pub market_asks: Pubkey,
    pub market_event_queue: Pubkey,
}

impl From<LiquidityPoolKeysString> for LiquidityPoolKeys {
    fn from(pool_keys: LiquidityPoolKeysString) -> Self {
        LiquidityPoolKeys {
            id: Pubkey::from_str(&pool_keys.id).unwrap(),
            base_mint: Pubkey::from_str(&pool_keys.base_mint).unwrap(),
            quote_mint: Pubkey::from_str(&pool_keys.quote_mint).unwrap(),
            lp_mint: Pubkey::from_str(&pool_keys.lp_mint).unwrap(),
            base_decimals: pool_keys.base_decimals,
            quote_decimals: pool_keys.quote_decimals,
            lp_decimals: pool_keys.lp_decimals,
            version: pool_keys.version,
            program_id: Pubkey::from_str(&pool_keys.program_id).unwrap(),
            authority: Pubkey::from_str(&pool_keys.authority).unwrap(),
            open_orders: Pubkey::from_str(&pool_keys.open_orders).unwrap(),
            target_orders: Pubkey::from_str(&pool_keys.target_orders).unwrap(),
            base_vault: Pubkey::from_str(&pool_keys.base_vault).unwrap(),
            quote_vault: Pubkey::from_str(&pool_keys.quote_vault).unwrap(),
            withdraw_queue: Pubkey::from_str(&pool_keys.withdraw_queue).unwrap(),
            lp_vault: Pubkey::from_str(&pool_keys.lp_vault).unwrap(),
            market_version: pool_keys.market_version,
            market_program_id: Pubkey::from_str(&pool_keys.market_program_id).unwrap(),
            market_id: Pubkey::from_str(&pool_keys.market_id).unwrap(),
            market_authority: Pubkey::from_str(&pool_keys.market_authority).unwrap(),
            market_base_vault: Pubkey::from_str(&pool_keys.market_base_vault).unwrap(),
            market_quote_vault: Pubkey::from_str(&pool_keys.market_quote_vault).unwrap(),
            market_bids: Pubkey::from_str(&pool_keys.market_bids).unwrap(),
            market_asks: Pubkey::from_str(&pool_keys.market_asks).unwrap(),
            market_event_queue: Pubkey::from_str(&pool_keys.market_event_queue).unwrap(),
        }
    }
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
struct SwapInstructionData {
    instruction: u8,
    amount_in: u64,
    min_amount_out: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct MarketStateLayoutV3 {
    pub _padding: [u8; 13],

    pub own_address: Pubkey,
    pub vault_signer_nonce: u64,

    pub base_mint: Pubkey,
    pub quote_mint: Pubkey,

    pub base_vault: Pubkey,
    pub base_deposits_total: u64,
    pub base_fees_accrued: u64,

    pub quote_vault: Pubkey,
    pub quote_deposits_total: u64,
    pub quote_fees_accrued: u64,

    pub quote_dust_threshold: u64,

    pub request_queue: Pubkey,
    pub event_queue: Pubkey,

    pub bids: Pubkey,
    pub asks: Pubkey,

    pub base_lot_size: u64,
    pub quote_lot_size: u64,

    pub fee_rate_bps: u64,

    pub referrer_rebates_accrued: u64,

    _padding_end: [u8; 7],
}
