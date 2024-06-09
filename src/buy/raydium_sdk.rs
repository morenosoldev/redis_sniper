use borsh::{ BorshDeserialize, BorshSerialize };
use once_cell::sync::Lazy;
use solana_sdk::{ instruction::{ AccountMeta, Instruction }, pubkey::Pubkey };
use std::str::FromStr;
use serde::{ Serialize, Deserialize };

pub static TOKEN_PROGRAM_ID: Lazy<Pubkey> = Lazy::new(||
    Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap()
);

pub static MODEL_DATA_PUBKEY: Lazy<Pubkey> = Lazy::new(||
    Pubkey::from_str("CDSr3ssLcRB6XYPJwAfFt18MZvEZp4LjHcvzBVZ45duo").unwrap()
);

#[derive(Debug)]
pub struct LiquiditySwapFixedInInstructionParamsV4 {
    pool_keys: LiquidityPoolKeys,
    user_keys: UserKeys,
    amount_in: u64,
    min_amount_out: u64,
}

impl LiquiditySwapFixedInInstructionParamsV4 {
    pub fn new(
        pool_keys: LiquidityPoolKeys,
        user_keys: UserKeys,
        amount_in: u64,
        min_amount_out: u64
    ) -> Self {
        LiquiditySwapFixedInInstructionParamsV4 { pool_keys, user_keys, amount_in, min_amount_out }
    }
}

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

#[derive(Debug)]
pub struct UserKeys {
    token_account_in: Pubkey,
    token_account_out: Pubkey,
    owner: Pubkey,
}

impl UserKeys {
    pub fn new(token_account_in: Pubkey, token_account_out: Pubkey, owner: Pubkey) -> Self {
        UserKeys { token_account_in, token_account_out, owner }
    }
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

pub fn make_swap_fixed_in_instruction(
    params: LiquiditySwapFixedInInstructionParamsV4,
    version: u8
) -> Instruction {
    let data = (SwapInstructionData {
        instruction: 9, // Instruction variant identifier
        amount_in: params.amount_in,
        min_amount_out: params.min_amount_out,
    })
        .try_to_vec()
        .unwrap(); // Serialize using Borsh

    let mut keys = vec![
        account_meta_readonly(*TOKEN_PROGRAM_ID, false),
        account_meta(params.pool_keys.id, false),
        account_meta_readonly(params.pool_keys.authority, false),
        account_meta(params.pool_keys.open_orders, false)
    ];
    if version == 4 {
        keys.push(account_meta(params.pool_keys.target_orders, false));
    }
    keys.push(account_meta(params.pool_keys.base_vault, false));
    keys.push(account_meta(params.pool_keys.quote_vault, false));
    if version == 5 {
        keys.push(account_meta(*MODEL_DATA_PUBKEY, false));
    }

    // Serum-related accounts
    keys.push(account_meta_readonly(params.pool_keys.market_program_id, false));
    keys.push(account_meta(params.pool_keys.market_id, false));
    keys.push(account_meta(params.pool_keys.market_bids, false));
    keys.push(account_meta(params.pool_keys.market_asks, false));
    keys.push(account_meta(params.pool_keys.market_event_queue, false));
    keys.push(account_meta(params.pool_keys.market_base_vault, false));
    keys.push(account_meta(params.pool_keys.market_quote_vault, false));
    keys.push(account_meta_readonly(params.pool_keys.market_authority, false));

    // User-related accounts
    keys.push(account_meta(params.user_keys.token_account_in, false));
    keys.push(account_meta(params.user_keys.token_account_out, false));
    keys.push(account_meta_readonly(params.user_keys.owner, true));

    Instruction {
        program_id: params.pool_keys.program_id,
        accounts: keys,
        data,
    }
}

pub fn account_meta(pubkey: Pubkey, is_signer: bool) -> AccountMeta {
    AccountMeta {
        pubkey,
        is_signer,
        is_writable: true, // Set to true
    }
}

pub fn account_meta_readonly(pubkey: Pubkey, is_signer: bool) -> AccountMeta {
    AccountMeta {
        pubkey,
        is_signer,
        is_writable: false, // Set to false for readonly as in radium js SDK idk lmao
    }
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
