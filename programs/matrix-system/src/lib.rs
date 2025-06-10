use anchor_lang::prelude::*;
use anchor_lang::solana_program::{self, clock::Clock};
use anchor_lang::AnchorDeserialize;
use anchor_lang::AnchorSerialize;
use anchor_spl::token::{self, Token, TokenAccount};
use anchor_spl::associated_token::AssociatedToken;
use chainlink_solana as chainlink;
use solana_program::program_pack::Pack;
#[cfg(not(feature = "no-entrypoint"))]
use {solana_security_txt::security_txt};

declare_id!("DegWuGAfwzMFK5BWC33q6XW7Ebk7fesZShuaXh2HveHS");

#[cfg(not(feature = "no-entrypoint"))]
security_txt! {
    name: "Donut",
    project_url: "https://mydonut.io",
    contacts: "email:dev@mydonut.io,whatsapp:+1 (530) 501-2621",
    policy: "https://github.com/MyDonutProject/my-donut-contract/blob/main/SECURITY.md",
    preferred_languages: "en",
    source_code: "https://github.com/MyDonutProject/my-donut-contract/blob/main/programs/matrix-system/src/lib.rs",
    source_revision: env!("GITHUB_SHA", "unknown-revision"),
    source_release: env!("PROGRAM_VERSION", "unknown-version"),
    encryption: "",
    auditors: "",
    acknowledgements: "We thank all security researchers who contributed to the security of our protocol."
}

// Minimum deposit amount in USD (10 dollars in base units - 8 decimals)
const MINIMUM_USD_DEPOSIT: u64 = 10_00000000; // 10 USD with 8 decimals (Chainlink format)

// Maximum price feed staleness (24 hours in seconds)
const MAX_PRICE_FEED_AGE: i64 = 86400;

// Default SOL price in case of stale feed ($100 USD per SOL)
const DEFAULT_SOL_PRICE: i128 = 100_00000000; // $100 with 8 decimals

// Maximum number of upline accounts that can be processed in a single transaction
const MAX_UPLINE_DEPTH: usize = 6;

// Number of Vault A accounts in the remaining_accounts (including pool)
const VAULT_A_ACCOUNTS_COUNT: usize = 5; // pool + a_vault + a_vault_lp + a_vault_lp_mint + a_token_vault

// Constants for strict address verification
pub mod verified_addresses {
    use solana_program::pubkey::Pubkey;
 
    // Pool address
    pub static POOL_ADDRESS: Pubkey = solana_program::pubkey!("GH9FRAPij2qKGvTo6jYQ9Mac3UWakP2FJGbmRV8paB9C");
    
    // Vault A addresses (DONUT token vault)
    pub static A_VAULT: Pubkey = solana_program::pubkey!("9NhsA1synYQEyR5nH9yjZfoN5TwhVD6B1pkBts447C9P");
    pub static A_VAULT_LP: Pubkey = solana_program::pubkey!("B7GhHWh3zJ1qjxSZTZiqFDgN1qzyV2MZRHNj1okQSWrU");
    pub static A_VAULT_LP_MINT: Pubkey = solana_program::pubkey!("6QNDN8rDqp4pvzzjFh776vTLXrLVWU8mcSVucZsdqoca");
    pub static A_TOKEN_VAULT: Pubkey = solana_program::pubkey!("BFxsdzhwos7Nbevqz75WRYTQcNXyoYqv2N9pg7yoThLt");
    
    // Meteora pool addresses
    pub static B_VAULT_LP: Pubkey = solana_program::pubkey!("EP1AC21DEtHQQ543cQGmguEfPku7tnU2YwxenzYJ7V6j");
    pub static B_VAULT: Pubkey = solana_program::pubkey!("FERjPVNEa7Udq8CEv68h6tPL46Tq7ieE49HrE2wea3XT");
    pub static B_TOKEN_VAULT: Pubkey = solana_program::pubkey!("HZeLxbZ9uHtSpwZC3LBr4Nubd14iHwz7bRSghRZf5VCG");
    pub static B_VAULT_LP_MINT: Pubkey = solana_program::pubkey!("FZN7QZ8ZUUAxMPfxYEYkH3cXUASzH8EqA6B4tyCL8f1j");
    
    // Token addresses
    pub static TOKEN_MINT: Pubkey = solana_program::pubkey!("DoNUT4ufy3esSrsBo77PoXda7MWrbKSfaZSBUsCfQz6f");
    pub static WSOL_MINT: Pubkey = solana_program::pubkey!("So11111111111111111111111111111111111111112");
    
    // Chainlink addresses
    pub static CHAINLINK_PROGRAM: Pubkey = solana_program::pubkey!("HEvSKofvBgfaexv23kMabbYqxasxU3mQ4ibBMEmJWHny");
    pub static SOL_USD_FEED: Pubkey = solana_program::pubkey!("CH31Xns5z3M1cTAbKW34jcxPPciazARpijcHj9rxtemt");
}

// Admin account addresses
pub mod admin_addresses {
    use solana_program::pubkey::Pubkey;

    pub static MULTISIG_TREASURY: Pubkey = solana_program::pubkey!("G1zNHXtAEysE5DUZ6pqf81qY8WpV15AgkaLGh2SEERkV");
    pub static AUTHORIZED_INITIALIZER: Pubkey = solana_program::pubkey!("24bvHXAxxVT2HxuoBptyG9guhE4vUKUTzFWPVBmHRCzw");
}

// ===== METEORA DYNAMIC VAULT STRUCTURES =====

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug)]
pub struct VaultBumps {
    pub vault_bump: u8,
    pub token_vault_bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct LockedProfitTracker {
    pub last_updated_locked_profit: u64,
    pub last_report: u64,
    pub locked_profit_degradation: u64,
}

#[account]
#[derive(Debug)]
pub struct Vault {
    pub enabled: u8,
    pub bumps: VaultBumps,
    pub total_amount: u64,
    pub token_vault: Pubkey,
    pub fee_vault: Pubkey,
    pub token_mint: Pubkey,
    pub lp_mint: Pubkey,
    pub strategies: [Pubkey; 30], // MAX_STRATEGY = 30
    pub base: Pubkey,
    pub admin: Pubkey,
    pub operator: Pubkey,
    pub locked_profit_tracker: LockedProfitTracker,
}

impl Vault {
    pub fn get_amount_by_share(
        &self,
        current_time: u64,
        share: u64,
        total_supply: u64,
    ) -> Option<u64> {
        if total_supply == 0 {
            return Some(0);
        }
        
        let total_amount = self.get_unlocked_amount(current_time)?;
        
        u64::try_from(
            u128::from(share)
                .checked_mul(u128::from(total_amount))?
                .checked_div(u128::from(total_supply))?,
        )
        .ok()
    }
    
    pub fn get_unlocked_amount(&self, current_time: u64) -> Option<u64> {
        self.total_amount.checked_sub(
            self.locked_profit_tracker
                .calculate_locked_profit(current_time)?,
        )
    }
}

impl LockedProfitTracker {
    pub fn calculate_locked_profit(&self, current_time: u64) -> Option<u64> {
        const LOCKED_PROFIT_DEGRADATION_DENOMINATOR: u128 = 1_000_000_000_000;
        
        let duration = u128::from(current_time.checked_sub(self.last_report)?);
        let locked_profit_degradation = u128::from(self.locked_profit_degradation);
        let locked_fund_ratio = duration.checked_mul(locked_profit_degradation)?;

        if locked_fund_ratio > LOCKED_PROFIT_DEGRADATION_DENOMINATOR {
            return Some(0);
        }
        
        let locked_profit = u128::from(self.last_updated_locked_profit);
        let locked_profit = locked_profit
            .checked_mul(LOCKED_PROFIT_DEGRADATION_DENOMINATOR.checked_sub(locked_fund_ratio)?)?
            .checked_div(LOCKED_PROFIT_DEGRADATION_DENOMINATOR)?;
            
        u64::try_from(locked_profit).ok()
    }
}

// ===== METEORA DYNAMIC AMM STRUCTURES =====

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq)]
pub enum PoolType {
    Permissioned,
    Permissionless,
}

impl Default for PoolType {
    fn default() -> Self {
        PoolType::Permissioned
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug)]
pub struct PoolFees {
    pub trade_fee_numerator: u64,
    pub trade_fee_denominator: u64,
    pub protocol_trade_fee_numerator: u64,
    pub protocol_trade_fee_denominator: u64,
}

#[derive(Copy, Clone, Debug, AnchorSerialize, AnchorDeserialize, Default)]
pub struct PartnerInfo {
    pub fee_numerator: u64,
    pub partner_authority: Pubkey,
    pub pending_fee_a: u64,
    pub pending_fee_b: u64,
}

#[derive(Copy, Clone, Debug, AnchorSerialize, AnchorDeserialize, Default)]
pub struct Bootstrapping {
    pub activation_point: u64,
    pub whitelisted_vault: Pubkey,
    #[deprecated]
    pub pool_creator: Pubkey,
    pub activation_type: u8,
}

#[derive(Clone, Copy, Debug, AnchorDeserialize, AnchorSerialize)]
pub enum CurveType {
    ConstantProduct,
    Stable {
        amp: u64,
        token_multiplier: TokenMultiplier,
        depeg: Depeg,
        last_amp_updated_timestamp: u64,
    },
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug, Default, Copy, Eq, PartialEq)]
pub struct TokenMultiplier {
    pub token_a_multiplier: u64,
    pub token_b_multiplier: u64,
    pub precision_factor: u8,
}

#[derive(Clone, Copy, Debug, Default, AnchorSerialize, AnchorDeserialize)]
pub struct Depeg {
    pub base_virtual_price: u64,
    pub base_cache_updated: u64,
    pub depeg_type: DepegType,
}

#[derive(Clone, Copy, Debug, Default, AnchorDeserialize, AnchorSerialize, PartialEq)]
pub enum DepegType {
    #[default]
    None,
    Marinade,
    Lido,
    SplStake,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug)]
pub struct Padding {
    pub padding_0: [u8; 6],
    pub padding_1: [u64; 21],
    pub padding_2: [u64; 21],
}

#[account]
#[derive(Debug)]
pub struct Pool {
    pub lp_mint: Pubkey,
    pub token_a_mint: Pubkey,
    pub token_b_mint: Pubkey,
    pub a_vault: Pubkey,
    pub b_vault: Pubkey,
    pub a_vault_lp: Pubkey,
    pub b_vault_lp: Pubkey,
    pub a_vault_lp_bump: u8,
    pub enabled: bool,
    pub protocol_token_a_fee: Pubkey,
    pub protocol_token_b_fee: Pubkey,
    pub fee_last_updated_at: u64,
    pub _padding0: [u8; 24],
    pub fees: PoolFees,
    pub pool_type: PoolType,
    pub stake: Pubkey,
    pub total_locked_lp: u64,
    pub bootstrapping: Bootstrapping,
    pub partner_info: PartnerInfo,
    pub padding: Padding,
    pub curve_type: CurveType,
}

// ===== PROGRAM STRUCTURES =====

// Program state structure
#[account]
pub struct ProgramState {
    pub owner: Pubkey,
    pub multisig_treasury: Pubkey,
    pub next_upline_id: u32,
    pub next_chain_id: u32,
    pub last_mint_amount: u64,
}

impl ProgramState {
    pub const SIZE: usize = 32 + 32 + 4 + 4 + 8; // owner + multisig_treasury + next_upline_id + next_chain_id + last_mint_amount
}

// Structure to store complete information for each upline
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug)]
pub struct UplineEntry {
    pub pda: Pubkey,       // PDA of the user account
    pub wallet: Pubkey,    // Original user wallet
}

// Referral upline structure
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct ReferralUpline {
    pub id: u32,
    pub depth: u8,
    pub upline: Vec<UplineEntry>, // Stores UplineEntry with all information
}

// Referral matrix structure
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default)]
pub struct ReferralChain {
    pub id: u32,
    pub slots: [Option<Pubkey>; 3],
    pub filled_slots: u8,
}

// User account structure
#[account]
#[derive(Default)]
pub struct UserAccount {
    pub is_registered: bool,
    pub referrer: Option<Pubkey>,
    pub owner_wallet: Pubkey,           // Account owner's wallet
    pub upline: ReferralUpline,
    pub chain: ReferralChain,
    pub reserved_sol: u64,       // SOL reserved from the second slot
    pub reserved_tokens: u64,    // Tokens reserved from the second slot
}

impl UserAccount {
    pub const SIZE: usize = 1 + // is_registered
                           1 + 32 + // Option<Pubkey> (1 for is_some + 32 for Pubkey)
                           32 + // owner_wallet
                           4 + 1 + 4 + (MAX_UPLINE_DEPTH * (32 + 32)) + // ReferralUpline
                           4 + (3 * (1 + 32)) + 1 + // ReferralChain
                           8 + // reserved_sol
                           8;  // reserved_tokens
}

// Error codes
#[error_code]
pub enum ErrorCode {
    #[msg("Invalid vault B address")]
    InvalidVaultBAddress,
    
    #[msg("Invalid vault B token vault address")]
    InvalidVaultBTokenVaultAddress,
    
    #[msg("Invalid vault B LP mint address")]
    InvalidVaultBLpMintAddress,

    #[msg("Invalid vault A LP address")]
    InvalidVaultALpAddress,
    
    #[msg("Invalid vault A LP mint address")]
    InvalidVaultALpMintAddress,
    
    #[msg("Invalid token A vault address")]
    InvalidTokenAVaultAddress,

    #[msg("Referrer account is not registered")]
    ReferrerNotRegistered,
    
    #[msg("Not authorized")]
    NotAuthorized,

    #[msg("Slot account not owned by program")]
    InvalidSlotOwner,

    #[msg("Slot account not registered")]
    SlotNotRegistered,

    #[msg("Insufficient deposit amount")]
    InsufficientDeposit,

    #[msg("Failed to process deposit to pool")]
    DepositToPoolFailed,

    #[msg("Failed to process SOL reserve")]
    SolReserveFailed,

    #[msg("Failed to process referrer payment")]
    ReferrerPaymentFailed,
    
    #[msg("Failed to wrap SOL to WSOL")]
    WrapSolFailed,
    
    #[msg("Failed to unwrap WSOL to SOL")]
    UnwrapSolFailed,
    
    #[msg("Failed to mint tokens")]
    TokenMintFailed,
    
    #[msg("Failed to transfer tokens")]
    TokenTransferFailed,

    #[msg("Invalid pool address")]
    InvalidPoolAddress,
    
    #[msg("Invalid vault address")]
    InvalidVaultAddress,
    
    #[msg("Invalid token mint address")]
    InvalidTokenMintAddress,
    
    #[msg("Invalid token account")]
    InvalidTokenAccount,
    
    #[msg("Invalid wallet for ATA")]
    InvalidWalletForATA,

    #[msg("Missing required account for upline")]
    MissingUplineAccount,
    
    #[msg("Payment wallet is not a system account")]
    PaymentWalletInvalid,
    
    #[msg("Token account is not a valid ATA")]
    TokenAccountInvalid,

    #[msg("Missing vault A accounts")]
    MissingVaultAAccounts,
    
    #[msg("Failed to read price feed")]
    PriceFeedReadFailed,
    
    #[msg("Price feed too old")]
    PriceFeedTooOld,
    
    #[msg("Invalid Chainlink program")]
    InvalidChainlinkProgram,
    
    #[msg("Invalid price feed")]
    InvalidPriceFeed,

    #[msg("Failed to read Meteora pool data")]
    PriceMeteoraReadFailed,
    
    #[msg("Meteora pool calculation overflow")]
    MeteoraCalculationOverflow,
}

// Event structure for slot filling
#[event]
pub struct SlotFilled {
    pub slot_idx: u8,     // Slot index (0, 1, 2)
    pub chain_id: u32,    // Chain ID
    pub user: Pubkey,     // User who filled the slot
    pub owner: Pubkey,    // Owner of the matrix
}

// Decimal handling for price display
#[derive(Default)]
pub struct Decimal {
    pub value: i128,
    pub decimals: u32,
}

impl Decimal {
    pub fn new(value: i128, decimals: u32) -> Self {
        Decimal { value, decimals }
    }
}

impl std::fmt::Display for Decimal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut scaled_val = self.value.to_string();
        if scaled_val.len() <= self.decimals as usize {
            scaled_val.insert_str(
                0,
                &vec!["0"; self.decimals as usize - scaled_val.len()].join(""),
            );
            scaled_val.insert_str(0, "0.");
        } else {
            scaled_val.insert(scaled_val.len() - self.decimals as usize, '.');
        }
        f.write_str(&scaled_val)
    }
}

// Helper function to force memory cleanup
fn force_memory_cleanup() {
    // Just create a vector to force a heap allocation
    let _dummy = Vec::<u8>::new();
    // The vector will be automatically freed when it goes out of scope
}

// Function to get SOL/USD price from Chainlink feed
fn get_sol_usd_price<'info>(
    chainlink_feed: &AccountInfo<'info>,
    chainlink_program: &AccountInfo<'info>,
) -> Result<(i128, u32, i64, i64)> { // Returns also the feed_timestamp
    // Get the latest round data
    let round = chainlink::latest_round_data(
        chainlink_program.clone(),
        chainlink_feed.clone(),
    ).map_err(|_| error!(ErrorCode::PriceFeedReadFailed))?;

    // Get the decimals
    let decimals = chainlink::decimals(
        chainlink_program.clone(),
        chainlink_feed.clone(),
    ).map_err(|_| error!(ErrorCode::PriceFeedReadFailed))?;

    // Get current timestamp
    let clock = Clock::get()?;
    let current_timestamp = clock.unix_timestamp;
    
    // Return price, decimals, current time, and feed timestamp
    Ok((round.answer, decimals.into(), current_timestamp, round.timestamp.into()))
}

// Function to calculate minimum SOL deposit based on USD price
fn calculate_minimum_sol_deposit<'info>(
    chainlink_feed: &AccountInfo<'info>, 
    chainlink_program: &AccountInfo<'info>
) -> Result<u64> {
    let (price, decimals, current_timestamp, feed_timestamp) = get_sol_usd_price(chainlink_feed, chainlink_program)?;
    
    // Check if price feed is too old (24 hours)
    let age = current_timestamp - feed_timestamp;
    
    let sol_price_per_unit = if age > MAX_PRICE_FEED_AGE {
        // Use default price of $100 per SOL
        DEFAULT_SOL_PRICE
    } else {
        price
    };
    
    // Convert price to SOL per unit using dynamic decimals
    let price_f64 = sol_price_per_unit as f64 / 10f64.powf(decimals as f64);
    
    // Convert MINIMUM_USD_DEPOSIT from 8 decimals to floating point
    let minimum_usd_f64 = MINIMUM_USD_DEPOSIT as f64 / 1_00000000.0; // Convert from 8 decimals
    
    // Calculate minimum SOL needed
    let minimum_sol_f64 = minimum_usd_f64 / price_f64;
    
    // Convert to lamports (9 decimals for SOL)
    let minimum_lamports = (minimum_sol_f64 * 1_000_000_000.0) as u64;
    
    Ok(minimum_lamports)
}

// Function to check and adjust the mint value based on history
fn check_mint_limit(program_state: &mut ProgramState, proposed_mint_value: u64) -> Result<u64> {
    // If it's the first mint (last_mint_amount = 0), allow any value
    if program_state.last_mint_amount == 0 {
        msg!("First mint: establishing base value for limiter: {}", proposed_mint_value);
        
        // Update the last value and allow the proposed mint
        program_state.last_mint_amount = proposed_mint_value;
        return Ok(proposed_mint_value);
    }
    
    // Calculate the limit (3x the last value)
    // Use of saturating_mul to safely avoid overflow
    let current_limit = program_state.last_mint_amount.saturating_mul(3);
    
    // Check if the proposed value exceeds the limit
    if proposed_mint_value > current_limit {
        msg!(
            "Mint adjustment: {} exceeds limit of {} (3x last mint). Using previous value: {}",
            proposed_mint_value,
            current_limit,
            program_state.last_mint_amount
        );
        
        // If it exceeds the limit, we use the last known value
        // We don't update last_mint_amount, as we're reusing the same value
        return Ok(program_state.last_mint_amount);
    }
    
    // For values within the limit, update the last value for the next check
    program_state.last_mint_amount = proposed_mint_value;
    
    // Return the original proposed value (it's within the limit)
    Ok(proposed_mint_value)
}

// Calculate DONUT tokens equivalent to a SOL amount using Meteora pool data
// Optimized version: Reduces stack usage to avoid stack overflow
fn get_donut_tokens_amount<'info>(
    pool: &AccountInfo<'info>,              // Pool state account
    a_vault: &AccountInfo<'info>,           // Vault A state account
    b_vault: &AccountInfo<'info>,           // Vault B state account
    a_vault_lp: &AccountInfo<'info>,        // LP token account for vault A
    b_vault_lp: &AccountInfo<'info>,        // LP token account for vault B
    a_vault_lp_mint: &AccountInfo<'info>,   // LP mint for vault A
    b_vault_lp_mint: &AccountInfo<'info>,   // LP mint for vault B
    sol_amount: u64,
) -> Result<u64> {
    const PRECISION_FACTOR: i128 = 1_000_000_000;
    
    msg!("get_donut_tokens_amount called with sol_amount: {}", sol_amount);
    
    // 1. Get current timestamp
    let current_time = Clock::get()?.unix_timestamp as u64;
    
    // 2. Check if the pool is enabled (only reads the necessary field)
    {
        let pool_data = pool.try_borrow_data()?;
        // Offset for the 'enabled' field in the Pool structure
        // lp_mint (32) + token_a_mint (32) + token_b_mint (32) + a_vault (32) + 
        // b_vault (32) + a_vault_lp (32) + b_vault_lp (32) + a_vault_lp_bump (1) = 225
        let enabled_offset = 8 + 225; // 8 bytes of discriminator + offset
        
        if pool_data.len() <= enabled_offset {
            return Err(error!(ErrorCode::PriceMeteoraReadFailed));
        }
        
        let pool_enabled = pool_data[enabled_offset] != 0;
        if !pool_enabled {
            msg!("Pool is disabled");
            return Err(error!(ErrorCode::PriceMeteoraReadFailed));
        }
    }
    
    // 3. Get total_amount of the vaults (without deserializing the entire structure)
    let (vault_a_total, vault_a_enabled) = {
        let vault_data = a_vault.try_borrow_data()?;
        // Offset: discriminator (8) + enabled (1) + bumps (2) = 11
        if vault_data.len() < 19 {
            return Err(error!(ErrorCode::PriceMeteoraReadFailed));
        }
        
        let enabled = vault_data[8] != 0;
        let total_amount = u64::from_le_bytes([
            vault_data[11], vault_data[12], vault_data[13], vault_data[14],
            vault_data[15], vault_data[16], vault_data[17], vault_data[18]
        ]);
        
        (total_amount, enabled)
    };
    
    let (vault_b_total, vault_b_enabled) = {
        let vault_data = b_vault.try_borrow_data()?;
        if vault_data.len() < 19 {
            return Err(error!(ErrorCode::PriceMeteoraReadFailed));
        }
        
        let enabled = vault_data[8] != 0;
        let total_amount = u64::from_le_bytes([
            vault_data[11], vault_data[12], vault_data[13], vault_data[14],
            vault_data[15], vault_data[16], vault_data[17], vault_data[18]
        ]);
        
        (total_amount, enabled)
    };
    
    // Check if the vaults are enabled
    if !vault_a_enabled || !vault_b_enabled {
        msg!("One or both vaults are disabled");
        return Err(error!(ErrorCode::PriceMeteoraReadFailed));
    }
    
    // 4. Read the values of the LP tokens
    let (a_vault_lp_amount, b_vault_lp_amount) = {
        let a_data = a_vault_lp.try_borrow_data()?;
        let b_data = b_vault_lp.try_borrow_data()?;
        
        if a_data.len() < 72 || b_data.len() < 72 {
            return Err(error!(ErrorCode::PriceMeteoraReadFailed));
        }
        
        // Offset for amount in TokenAccount: 64 bytes
        let a_amount = u64::from_le_bytes([
            a_data[64], a_data[65], a_data[66], a_data[67],
            a_data[68], a_data[69], a_data[70], a_data[71]
        ]);
        
        let b_amount = u64::from_le_bytes([
            b_data[64], b_data[65], b_data[66], b_data[67],
            b_data[68], b_data[69], b_data[70], b_data[71]
        ]);
        
        msg!("LP amounts - A: {}, B: {}", a_amount, b_amount);
        (a_amount, b_amount)
    };
    
    // 5. Read the supplies of the LP tokens
    let (a_vault_lp_supply, b_vault_lp_supply) = {
        let a_mint_data = a_vault_lp_mint.try_borrow_data()?;
        let b_mint_data = b_vault_lp_mint.try_borrow_data()?;
        
        if a_mint_data.len() < 44 || b_mint_data.len() < 44 {
            return Err(error!(ErrorCode::PriceMeteoraReadFailed));
        }
        
        // Offset for supply in Mint: 36 bytes
        let a_supply = u64::from_le_bytes([
            a_mint_data[36], a_mint_data[37], a_mint_data[38], a_mint_data[39],
            a_mint_data[40], a_mint_data[41], a_mint_data[42], a_mint_data[43]
        ]);
        
        let b_supply = u64::from_le_bytes([
            b_mint_data[36], b_mint_data[37], b_mint_data[38], b_mint_data[39],
            b_mint_data[40], b_mint_data[41], b_mint_data[42], b_mint_data[43]
        ]);
        
        msg!("LP supplies - A: {}, B: {}", a_supply, b_supply);
        (a_supply, b_supply)
    };
    
    // 6. Calculate the values of the tokens (simplified implementation of get_amount_by_share)
    // Assuming there is no significant locked profit to simplify
    let token_a_amount = if a_vault_lp_supply == 0 {
        0
    } else {
        ((a_vault_lp_amount as u128) * (vault_a_total as u128) / (a_vault_lp_supply as u128)) as u64
    };
    
    let token_b_amount = if b_vault_lp_supply == 0 {
        0
    } else {
        ((b_vault_lp_amount as u128) * (vault_b_total as u128) / (b_vault_lp_supply as u128)) as u64
    };
    
    msg!("Token amounts - A: {}, B: {}", token_a_amount, token_b_amount);
    
    // 7. Check for zero values
    if token_a_amount == 0 || token_b_amount == 0 {
        msg!("One or both token amounts is zero");
        return Err(error!(ErrorCode::PriceMeteoraReadFailed));
    }
    
    // 8. Calculate the ratio of the tokens in the pool
    let ratio = (token_a_amount as i128)
        .checked_mul(PRECISION_FACTOR)
        .and_then(|n| n.checked_div(token_b_amount as i128))
        .ok_or_else(|| {
            msg!("Ratio calculation overflow");
            error!(ErrorCode::MeteoraCalculationOverflow)
        })?;
    
    msg!("Token ratio (A/B): {}", ratio);
    
    // 9. Calculate the quantity of DONUT tokens based on the SOL amount
    let donut_tokens = (sol_amount as i128)
        .checked_mul(ratio)
        .and_then(|n| n.checked_div(PRECISION_FACTOR))
        .ok_or_else(|| {
            msg!("Final calculation overflow");
            error!(ErrorCode::MeteoraCalculationOverflow)
        })?;
    
    if donut_tokens > i128::from(u64::MAX) || donut_tokens < 0 {
        msg!("Result out of u64 range: {}", donut_tokens);
        return Err(error!(ErrorCode::MeteoraCalculationOverflow));
    }
    
    let result = donut_tokens as u64;
    msg!("Final donut_tokens: {}", result);
    
    Ok(if result == 0 { 1 } else { result })
}

// Function to strictly verify an address
fn verify_address_strict(provided: &Pubkey, expected: &Pubkey, error_code: ErrorCode) -> Result<()> {
    if provided != expected {
        return Err(error!(error_code));
    }
    Ok(())
}

// Verify pool and vault A addresses
fn verify_pool_and_vault_a_addresses<'info>(
    pool: &Pubkey,
    a_vault: &Pubkey,
    a_vault_lp: &Pubkey,
    a_vault_lp_mint: &Pubkey,
) -> Result<()> {
    verify_address_strict(pool, &verified_addresses::POOL_ADDRESS, ErrorCode::InvalidPoolAddress)?;
    verify_address_strict(a_vault, &verified_addresses::A_VAULT, ErrorCode::InvalidVaultAddress)?;
    verify_address_strict(a_vault_lp, &verified_addresses::A_VAULT_LP, ErrorCode::InvalidVaultALpAddress)?;
    verify_address_strict(a_vault_lp_mint, &verified_addresses::A_VAULT_LP_MINT, ErrorCode::InvalidVaultALpMintAddress)?;
    
    Ok(())
}

// Function to strictly verify an ATA account
fn verify_ata_strict<'info>(
    token_account: &AccountInfo<'info>,
    owner: &Pubkey,
    expected_mint: &Pubkey
) -> Result<()> {
    if token_account.owner != &spl_token::id() {
        return Err(error!(ErrorCode::InvalidTokenAccount));
    }
    
    match TokenAccount::try_deserialize(&mut &token_account.data.borrow()[..]) {
        Ok(token_data) => {
            if token_data.owner != *owner {
                return Err(error!(ErrorCode::InvalidWalletForATA));
            }
            
            if token_data.mint != *expected_mint {
                return Err(error!(ErrorCode::InvalidTokenMintAddress));
            }
        },
        Err(_) => {
            return Err(error!(ErrorCode::InvalidTokenAccount));
        }
    }
    
    Ok(())
}

// Function to verify all fixed addresses at once
fn verify_all_fixed_addresses<'info>(
    pool: &Pubkey,
    b_vault: &Pubkey,        
    b_token_vault: &Pubkey,  
    b_vault_lp_mint: &Pubkey, 
    b_vault_lp: &Pubkey,
    token_mint: &Pubkey,
    wsol_mint: &Pubkey,
) -> Result<()> {
    // Pool and vaults verifications
    verify_address_strict(pool, &verified_addresses::POOL_ADDRESS, ErrorCode::InvalidPoolAddress)?;
    verify_address_strict(b_vault_lp, &verified_addresses::B_VAULT_LP, ErrorCode::InvalidVaultAddress)?;
    verify_address_strict(b_vault, &verified_addresses::B_VAULT, ErrorCode::InvalidVaultAddress)?;
    verify_address_strict(b_token_vault, &verified_addresses::B_TOKEN_VAULT, ErrorCode::InvalidVaultAddress)?;
    verify_address_strict(b_vault_lp_mint, &verified_addresses::B_VAULT_LP_MINT, ErrorCode::InvalidVaultAddress)?;
    
    // Token verifications
    verify_address_strict(token_mint, &verified_addresses::TOKEN_MINT, ErrorCode::InvalidTokenMintAddress)?;
    verify_address_strict(wsol_mint, &verified_addresses::WSOL_MINT, ErrorCode::InvalidTokenMintAddress)?;
    
    Ok(())
}

// Function to verify Chainlink addresses
fn verify_chainlink_addresses<'info>(
    chainlink_program: &Pubkey,
    chainlink_feed: &Pubkey,
) -> Result<()> {
    verify_address_strict(chainlink_program, &verified_addresses::CHAINLINK_PROGRAM, ErrorCode::InvalidChainlinkProgram)?;
    verify_address_strict(chainlink_feed, &verified_addresses::SOL_USD_FEED, ErrorCode::InvalidPriceFeed)?;
    
    Ok(())
}

// Verify if an account is a valid wallet (system account)
fn verify_wallet_is_system_account<'info>(wallet: &AccountInfo<'info>) -> Result<()> {
    if wallet.owner != &solana_program::system_program::ID {
        return Err(error!(ErrorCode::PaymentWalletInvalid));
    }
    
    Ok(())
}

// Verify if an account is a valid ATA
fn verify_token_account<'info>(
    token_account: &AccountInfo<'info>,
    wallet: &Pubkey,
    token_mint: &Pubkey
) -> Result<()> {
    if token_account.owner != &spl_token::id() {
        return Err(error!(ErrorCode::TokenAccountInvalid));
    }
    
    let token_data = match TokenAccount::try_deserialize(&mut &token_account.data.borrow()[..]) {
        Ok(data) => data,
        Err(_) => {
            return Err(error!(ErrorCode::TokenAccountInvalid));
        }
    };
    
    if token_data.owner != *wallet {
        return Err(error!(ErrorCode::TokenAccountInvalid));
    }
    
    if token_data.mint != *token_mint {
        return Err(error!(ErrorCode::TokenAccountInvalid));
    }
    
    Ok(())
}

// Function to process deposit to the liquidity pool
fn process_deposit_to_pool<'info>(
    user: &AccountInfo<'info>,
    user_source_token: &AccountInfo<'info>,
    b_vault_lp: &AccountInfo<'info>,
    b_vault: &UncheckedAccount<'info>,
    b_token_vault: &AccountInfo<'info>,
    b_vault_lp_mint: &AccountInfo<'info>,
    vault_program: &UncheckedAccount<'info>,
    token_program: &Program<'info, Token>,
    amount: u64,
) -> Result<()> {
    let deposit_accounts = [
        b_vault.to_account_info(),
        b_token_vault.clone(),
        b_vault_lp_mint.clone(),
        user_source_token.clone(),
        b_vault_lp.clone(),
        user.clone(),
        token_program.to_account_info(),
    ];

    let mut deposit_data = Vec::with_capacity(24);
    deposit_data.extend_from_slice(&[242, 35, 198, 137, 82, 225, 242, 182]); // Deposit sighash
    deposit_data.extend_from_slice(&amount.to_le_bytes());
    deposit_data.extend_from_slice(&0u64.to_le_bytes()); // minimum_lp_token_amount = 0

    solana_program::program::invoke(
        &solana_program::instruction::Instruction {
            program_id: vault_program.key(),
            accounts: deposit_accounts.iter().enumerate().map(|(i, a)| {
                if i == 5 {
                    solana_program::instruction::AccountMeta::new_readonly(a.key(), true)
                } else if i < 5 {
                    solana_program::instruction::AccountMeta::new(a.key(), false)
                } else {
                    solana_program::instruction::AccountMeta::new_readonly(a.key(), false)
                }
            }).collect::<Vec<solana_program::instruction::AccountMeta>>(),
            data: deposit_data,
        },
        &deposit_accounts,
    ).map_err(|_| error!(ErrorCode::DepositToPoolFailed))?;
    
    Ok(())
}

// Function to reserve SOL for the referrer
fn process_reserve_sol<'info>(
    from: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    amount: u64,
) -> Result<()> {
    let ix = solana_program::system_instruction::transfer(
        &from.key(),
        &to.key(),
        amount
    );
    
    solana_program::program::invoke(
        &ix,
        &[from.clone(), to.clone()],
    ).map_err(|_| error!(ErrorCode::SolReserveFailed))?;
    
    Ok(())
}

// Function process_pay_referrer with explicit lifetimes
fn process_pay_referrer<'info>(
    from: &AccountInfo<'info>,
    to: &AccountInfo<'info>,
    amount: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    verify_wallet_is_system_account(to)?;
    
    let ix = solana_program::system_instruction::transfer(
        &from.key(),
        &to.key(),
        amount
    );
    
    // Create a vector of AccountInfo to avoid lifetime problems
    let mut accounts = Vec::with_capacity(2);
    accounts.push(from.clone());
    accounts.push(to.clone());
    
    solana_program::program::invoke_signed(
        &ix,
        &accounts,
        signer_seeds,
    ).map_err(|_| error!(ErrorCode::ReferrerPaymentFailed))?;
    
    Ok(())
}

// Function to mint tokens for the program vault
pub fn process_mint_tokens<'info>(
    token_mint: &AccountInfo<'info>,
    program_token_vault: &AccountInfo<'info>,
    token_mint_authority: &AccountInfo<'info>,
    token_program: &Program<'info, Token>,
    amount: u64,
    mint_authority_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mint_instruction = spl_token::instruction::mint_to(
        &token_program.key(),
        &token_mint.key(),
        &program_token_vault.key(),
        &token_mint_authority.key(),
        &[],
        amount
    ).map_err(|_| error!(ErrorCode::TokenMintFailed))?;
    
    // Use Vec instead of fixed array to avoid lifetime problems
    let mut mint_accounts = Vec::with_capacity(4);
    mint_accounts.push(token_mint.clone());
    mint_accounts.push(program_token_vault.clone());
    mint_accounts.push(token_mint_authority.clone());
    mint_accounts.push(token_program.to_account_info());
    
    solana_program::program::invoke_signed(
        &mint_instruction,
        &mint_accounts,
        mint_authority_seeds,
    ).map_err(|_| error!(ErrorCode::TokenMintFailed))?;
    
    Ok(())
}

// Function to transfer tokens from vault to user
pub fn process_transfer_tokens<'info>(
    program_token_vault: &AccountInfo<'info>,
    user_token_account: &AccountInfo<'info>,
    vault_authority: &AccountInfo<'info>,
    token_program: &Program<'info, Token>,
    amount: u64,
    authority_seeds: &[&[&[u8]]],
) -> Result<()> {
    if user_token_account.owner != &spl_token::id() {
        return Err(error!(ErrorCode::TokenAccountInvalid));
    }
    
    let transfer_instruction = spl_token::instruction::transfer(
        &token_program.key(),
        &program_token_vault.key(),
        &user_token_account.key(),
        &vault_authority.key(),
        &[],
        amount
    ).map_err(|_| error!(ErrorCode::TokenTransferFailed))?;
    
    // Use Vec instead of fixed array to avoid lifetime problems
    let mut transfer_accounts = Vec::with_capacity(4);
    transfer_accounts.push(program_token_vault.clone());
    transfer_accounts.push(user_token_account.clone());
    transfer_accounts.push(vault_authority.clone());
    transfer_accounts.push(token_program.to_account_info());
    
    solana_program::program::invoke_signed(
        &transfer_instruction,
        &transfer_accounts,
        authority_seeds,
    ).map_err(|_| error!(ErrorCode::TokenTransferFailed))?;
    
    Ok(())
}

/// Process the direct referrer's matrix when a new user registers
/// Returns (bool, Pubkey) where:
/// - bool: indicates if the matrix was completed
/// - Pubkey: referrer key for use in recursion
fn process_referrer_chain<'info>(
   user_key: &Pubkey,
   referrer: &mut Account<'_, UserAccount>,
   next_chain_id: u32,
) -> Result<(bool, Pubkey)> {
   let slot_idx = referrer.chain.filled_slots as usize;
   if slot_idx >= 3 {
       return Ok((false, referrer.key())); 
   }

   referrer.chain.slots[slot_idx] = Some(*user_key);

   // Emit slot filled event
   emit!(SlotFilled {
       slot_idx: slot_idx as u8,
       chain_id: referrer.chain.id,
       user: *user_key,
       owner: referrer.key(),
   });

   referrer.chain.filled_slots += 1;

   if referrer.chain.filled_slots == 3 {
       referrer.chain.id = next_chain_id;
       referrer.chain.slots = [None, None, None];
       referrer.chain.filled_slots = 0;

       return Ok((true, referrer.key()));
   }

   Ok((false, referrer.key()))
}

// Accounts for initialize instruction
#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = owner,
        space = 8 + ProgramState::SIZE
    )]
    pub state: Account<'info, ProgramState>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

// Accounts for registration without referrer with deposit
#[derive(Accounts)]
#[instruction(deposit_amount: u64)]
pub struct RegisterWithoutReferrerDeposit<'info> {
    #[account(mut)]
    pub state: Account<'info, ProgramState>,

    #[account(mut)]
    pub owner: Signer<'info>,
    
    #[account(mut)]
    pub user_wallet: Signer<'info>,
    
    #[account(
        init,
        payer = user_wallet,
        space = 8 + UserAccount::SIZE,
        seeds = [b"user_account", user_wallet.key().as_ref()],
        bump
    )]
    pub user: Account<'info, UserAccount>,

    /// CHECK: User token account for Wrapped SOL, verified in the instruction code
    #[account(mut)]
    pub user_source_token: UncheckedAccount<'info>,
    
    // WSOL mint
    /// CHECK: This is the fixed WSOL mint address
    pub wsol_mint: AccountInfo<'info>,

    // Deposit Accounts (same logic as Slot 1)
    /// CHECK: Pool account (PDA)
    #[account(mut)]
    pub pool: UncheckedAccount<'info>,

    // Existing accounts for vault B (SOL)
    /// CHECK: Vault account for token B (SOL)
    #[account(mut)]
    pub b_vault: UncheckedAccount<'info>,

    /// CHECK: Token vault account for token B (SOL)
    #[account(mut)]
    pub b_token_vault: UncheckedAccount<'info>,

    /// CHECK: LP token mint for vault B
    #[account(mut)]
    pub b_vault_lp_mint: UncheckedAccount<'info>,

    /// CHECK: LP token account for vault B
    #[account(mut)]
    pub b_vault_lp: UncheckedAccount<'info>,

    /// CHECK: Vault program
    pub vault_program: UncheckedAccount<'info>,

    // TOKEN MINT - Added for base user
    /// CHECK: Token mint to create the UplineEntry structure
    pub token_mint: UncheckedAccount<'info>,

    // Required programs
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

// Structure for registration with SOL in a single transaction
// Now includes Chainlink accounts and remaining_accounts
#[derive(Accounts)]
#[instruction(deposit_amount: u64)]
pub struct RegisterWithSolDeposit<'info> {
    #[account(mut)]
    pub state: Account<'info, ProgramState>,

    #[account(mut)]
    pub user_wallet: Signer<'info>,

    // Reference accounts
    #[account(mut)]
    pub referrer: Account<'info, UserAccount>,
    
    #[account(mut)]
    pub referrer_wallet: SystemAccount<'info>,

    // User account
    #[account(
        init,
        payer = user_wallet,
        space = 8 + UserAccount::SIZE,
        seeds = [b"user_account", user_wallet.key().as_ref()],
        bump
    )]
    pub user: Account<'info, UserAccount>,

    // New WSOL ATA account
    #[account(
        init,
        payer = user_wallet,
        associated_token::mint = wsol_mint,
        associated_token::authority = user_wallet
    )]
    pub user_wsol_account: Account<'info, TokenAccount>,
    
    // WSOL mint
    /// CHECK: This is the fixed WSOL mint address
    pub wsol_mint: AccountInfo<'info>,

    // Deposit Accounts (Slot 1 and 3)
    /// CHECK: Pool account (PDA)
    #[account(mut)]
    pub pool: UncheckedAccount<'info>,

    // Existing accounts for vault B (SOL)
    /// CHECK: Vault account for token B (SOL)
    #[account(mut)]
    pub b_vault: UncheckedAccount<'info>,

    /// CHECK: Token vault account for token B (SOL)
    #[account(mut)]
    pub b_token_vault: UncheckedAccount<'info>,

    /// CHECK: LP token mint for vault B
    #[account(mut)]
    pub b_vault_lp_mint: UncheckedAccount<'info>,

    /// CHECK: LP token account for vault B
    #[account(mut)]
    pub b_vault_lp: UncheckedAccount<'info>,

    /// CHECK: Vault program
    pub vault_program: UncheckedAccount<'info>,

    // Accounts for SOL reserve (Slot 2)
    #[account(
        mut,
        seeds = [b"program_sol_vault"],
        bump
    )]
    pub program_sol_vault: SystemAccount<'info>,
    
    // ACCOUNTS FOR TOKENS (Slot 2 and 3)
    /// CHECK: Token mint for minting new tokens
    #[account(mut)]
    pub token_mint: UncheckedAccount<'info>,
    
    /// CHECK: Program token vault to store reserved tokens
    #[account(mut)]
    pub program_token_vault: UncheckedAccount<'info>,
    
    /// CHECK: Referrer's ATA to receive tokens
    #[account(mut)]
    pub referrer_token_account: UncheckedAccount<'info>,
    
    // Authority to mint tokens (program PDA)
    /// CHECK: Mint authority PDA
    #[account(
        seeds = [b"token_mint_authority"],
        bump
    )]
    pub token_mint_authority: UncheckedAccount<'info>,
    
    // Vault authority for token transfers
    /// CHECK: Token vault authority
    #[account(
        seeds = [b"token_vault_authority"],
        bump
    )]
    pub vault_authority: UncheckedAccount<'info>,

    // Required programs
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub rent: Sysvar<'info, Rent>,
}

#[program]
pub mod referral_system {
    use super::*;

    // Initialize program state
    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {

        if ctx.accounts.owner.key() != admin_addresses::AUTHORIZED_INITIALIZER {
            return Err(error!(ErrorCode::NotAuthorized));
        }

        let state = &mut ctx.accounts.state;
        state.owner = ctx.accounts.owner.key();
        state.multisig_treasury = admin_addresses::MULTISIG_TREASURY;
        state.next_upline_id = 1;
        state.next_chain_id = 1;
        state.last_mint_amount = 0;
        
        Ok(())
    }
    
    // Register without a referrer (multisig treasury or owner only)
    pub fn register_without_referrer(ctx: Context<RegisterWithoutReferrerDeposit>, deposit_amount: u64) -> Result<()> {
        // Verify if the caller is the multisig treasury
        if ctx.accounts.owner.key() != ctx.accounts.state.multisig_treasury {
            return Err(error!(ErrorCode::NotAuthorized));
        }
    
        // STRICT VERIFICATION OF ALL ADDRESSES
        verify_all_fixed_addresses(
            &ctx.accounts.pool.key(),
            &ctx.accounts.b_vault.key(),
            &ctx.accounts.b_token_vault.key(),
            &ctx.accounts.b_vault_lp_mint.key(),
            &ctx.accounts.b_vault_lp.key(),
            &ctx.accounts.token_mint.key(),
            &ctx.accounts.wsol_mint.key(),
        )?;

        // Use global upline ID
        let state = &mut ctx.accounts.state;
        let upline_id = state.next_upline_id;
        let chain_id = state.next_chain_id;

        state.next_upline_id += 1;
        state.next_chain_id += 1;

        // Create new user data
        let user = &mut ctx.accounts.user;

        // Initialize user data with an empty upline structure
        user.is_registered = true;
        user.referrer = None;
        user.owner_wallet = ctx.accounts.user_wallet.key();
        user.upline = ReferralUpline {
            id: upline_id,
            depth: 1,
            upline: vec![],
        };
        user.chain = ReferralChain {
            id: chain_id,
            slots: [None, None, None],
            filled_slots: 0,
        };
        
        // Initialize financial data
        user.reserved_sol = 0;
        user.reserved_tokens = 0;

        // Sync the WSOL account 
        let sync_native_ix = spl_token::instruction::sync_native(
            &token::ID,
            &ctx.accounts.user_source_token.key(),
        )?;
        
        let sync_accounts = [ctx.accounts.user_source_token.to_account_info()];
        
        solana_program::program::invoke(
            &sync_native_ix,
            &sync_accounts,
        ).map_err(|_| error!(ErrorCode::WrapSolFailed))?;

        // Deposit to liquidity pool
        process_deposit_to_pool(
            &ctx.accounts.user_wallet.to_account_info(),
            &ctx.accounts.user_source_token.to_account_info(),
            &ctx.accounts.b_vault_lp.to_account_info(),
            &ctx.accounts.b_vault,
            &ctx.accounts.b_token_vault.to_account_info(),
            &ctx.accounts.b_vault_lp_mint.to_account_info(),
            &ctx.accounts.vault_program,
            &ctx.accounts.token_program,
            deposit_amount
        )?;

        Ok(())
    }

    // Register user with SOL in a single transaction - Modified to use remaining_accounts
    pub fn register_with_sol_deposit<'a, 'b, 'c, 'info>(ctx: Context<'a, 'b, 'c, 'info, RegisterWithSolDeposit<'info>>, deposit_amount: u64) -> Result<()> {
        // Check if referrer is registered
        if !ctx.accounts.referrer.is_registered {
            return Err(error!(ErrorCode::ReferrerNotRegistered));
        }

        // Check if we have vault A accounts in remaining_accounts
        if ctx.remaining_accounts.len() < VAULT_A_ACCOUNTS_COUNT + 2 { // +2 for Chainlink accounts
            return Err(error!(ErrorCode::MissingVaultAAccounts));
        }

        // Extract pool and vault A accounts from remaining_accounts
        let pool = &ctx.remaining_accounts[0];         // Pool state
        let a_vault = &ctx.remaining_accounts[1];      // Vault A state
        let a_vault_lp = &ctx.remaining_accounts[2];
        let a_vault_lp_mint = &ctx.remaining_accounts[3];
        let _a_token_vault = &ctx.remaining_accounts[4]; // Still kept for compatibility

        // Verify Pool and Vault A addresses
        verify_pool_and_vault_a_addresses(
            &pool.key(),
            &a_vault.key(),
            &a_vault_lp.key(),
            &a_vault_lp_mint.key()
        )?;

        // Extract Chainlink accounts from remaining_accounts
        let chainlink_feed = &ctx.remaining_accounts[5];
        let chainlink_program = &ctx.remaining_accounts[6];

        // STRICT VERIFICATION OF ALL POOL ADDRESSES
        verify_all_fixed_addresses(
            &ctx.accounts.pool.key(),
            &ctx.accounts.b_vault.key(),
            &ctx.accounts.b_token_vault.key(),
            &ctx.accounts.b_vault_lp_mint.key(),
            &ctx.accounts.b_vault_lp.key(),
            &ctx.accounts.token_mint.key(),
            &ctx.accounts.wsol_mint.key(),
        )?;

        // Verify Chainlink addresses
        verify_chainlink_addresses(
            &chainlink_program.key(),
            &chainlink_feed.key(),
        )?;

        // Get minimum deposit amount from Chainlink feed
        let minimum_deposit = calculate_minimum_sol_deposit(
            chainlink_feed,
            chainlink_program,
        )?;

        // Verify deposit amount meets the minimum requirement
        if deposit_amount < minimum_deposit {
            msg!("Deposit amount: {}, minimum required: {}", deposit_amount, minimum_deposit);
            return Err(error!(ErrorCode::InsufficientDeposit));
        }

        // Verify referrer's ATA account
        verify_ata_strict(
            &ctx.accounts.referrer_token_account.to_account_info(),
            &ctx.accounts.referrer_wallet.key(),
            &ctx.accounts.token_mint.key()
        )?;
        
        // 1. Transfer SOL to WSOL (wrap)
        let transfer_ix = solana_program::system_instruction::transfer(
            &ctx.accounts.user_wallet.key(),
            &ctx.accounts.user_wsol_account.key(),
            deposit_amount
        );
        
        let wrap_accounts = [
            ctx.accounts.user_wallet.to_account_info(),
            ctx.accounts.user_wsol_account.to_account_info(),
        ];
        
        solana_program::program::invoke(
            &transfer_ix,
            &wrap_accounts,
        ).map_err(|_| error!(ErrorCode::WrapSolFailed))?;
        
        // 2. Sync the WSOL account
        let sync_native_ix = spl_token::instruction::sync_native(
            &token::ID,
            &ctx.accounts.user_wsol_account.key(),
        )?;
        
        let sync_accounts = [ctx.accounts.user_wsol_account.to_account_info()];
        
        solana_program::program::invoke(
            &sync_native_ix,
            &sync_accounts,
        ).map_err(|_| error!(ErrorCode::WrapSolFailed))?;
        
        // 3. Create the new UplineEntry structure for the referrer
        let referrer_entry = UplineEntry {
            pda: ctx.accounts.referrer.key(),
            wallet: ctx.accounts.referrer_wallet.key(),
        };
        
        // 4. Create the user's upline by copying the referrer's upline and adding the referrer
        let mut new_upline = Vec::new();
        
        // OPTIMIZATION - Try to reserve exact capacity to avoid reallocations
        if ctx.accounts.referrer.upline.upline.len() >= MAX_UPLINE_DEPTH {
            // If already at depth limit, reserve space for MAX_UPLINE_DEPTH entries only
            new_upline.try_reserve(MAX_UPLINE_DEPTH).ok();
            
            // Copy only the most recent entries
            let start_idx = ctx.accounts.referrer.upline.upline.len() - (MAX_UPLINE_DEPTH - 1);
            new_upline.extend_from_slice(&ctx.accounts.referrer.upline.upline[start_idx..]);
        } else {
            // If space is available, reserve space for all existing entries plus the new one
            new_upline.try_reserve(ctx.accounts.referrer.upline.upline.len() + 1).ok();
            
            // Copy all existing entries
            new_upline.extend_from_slice(&ctx.accounts.referrer.upline.upline);
        }
        
        // Add the current referrer
        new_upline.push(referrer_entry);
        
        // OPTIMIZATION - Reduce capacity to current size
        new_upline.shrink_to_fit();

        // 5. Get upline ID from global counter
        let state = &mut ctx.accounts.state;
        let upline_id = state.next_upline_id;
        let chain_id = state.next_chain_id;

        state.next_chain_id += 1;

        // 6. Create new user data
        let user = &mut ctx.accounts.user;

        user.is_registered = true;
        user.referrer = Some(ctx.accounts.referrer.key());
        user.owner_wallet = ctx.accounts.user_wallet.key();
        user.upline = ReferralUpline {
            id: upline_id,
            depth: ctx.accounts.referrer.upline.depth + 1,
            upline: new_upline,
        };
        user.chain = ReferralChain {
            id: chain_id,
            slots: [None, None, None],
            filled_slots: 0,
        };
        
        // Initialize user financial data
        user.reserved_sol = 0;
        user.reserved_tokens = 0;

        // ===== FINANCIAL LOGIC =====
        // Determine which slot we're filling in the referrer's matrix
        let slot_idx = ctx.accounts.referrer.chain.filled_slots as usize;

        // LOGIC FOR SLOT 1: Deposit to liquidity pool
        if slot_idx == 0 {
            // Transfer SOL to the liquidity pool using the created WSOL account
            process_deposit_to_pool(
                &ctx.accounts.user_wallet.to_account_info(),
                &ctx.accounts.user_wsol_account.to_account_info(),
                &ctx.accounts.b_vault_lp.to_account_info(),
                &ctx.accounts.b_vault,
                &ctx.accounts.b_token_vault.to_account_info(),
                &ctx.accounts.b_vault_lp_mint.to_account_info(),
                &ctx.accounts.vault_program,
                &ctx.accounts.token_program,
                deposit_amount
            )?;
        } 
        // LOGIC FOR SLOT 2: Reserve SOL value and mint tokens
        else if slot_idx == 1 {
            // Closing the WSOL account transfers the lamports back to the owner
            let close_ix = spl_token::instruction::close_account(
                &token::ID,
                &ctx.accounts.user_wsol_account.key(),
                &ctx.accounts.user_wallet.key(),
                &ctx.accounts.user_wallet.key(),
                &[]
            )?;
            
            let close_accounts = [
                ctx.accounts.user_wsol_account.to_account_info(),
                ctx.accounts.user_wallet.to_account_info(),
                ctx.accounts.user_wallet.to_account_info(),
            ];
            
            solana_program::program::invoke(
                &close_ix,
                &close_accounts,
            ).map_err(|_| error!(ErrorCode::UnwrapSolFailed))?;
            
            // Now transfer SOL to reserve
            process_reserve_sol(
                &ctx.accounts.user_wallet.to_account_info(),
                &ctx.accounts.program_sol_vault.to_account_info(),
                deposit_amount
            )?;
            
            // Update reserved value for the referrer
            ctx.accounts.referrer.reserved_sol = deposit_amount;
            
            // Calculate tokens based on pool value USING THE CORRECT METEORA FORM
            let token_amount = get_donut_tokens_amount(
                pool,               // Pool state
                a_vault,            // Vault A state
                &ctx.accounts.b_vault.to_account_info(), // Vault B state
                a_vault_lp,         // LP token account A
                &ctx.accounts.b_vault_lp.to_account_info(), // LP token account B
                a_vault_lp_mint,    // LP mint A
                &ctx.accounts.b_vault_lp_mint.to_account_info(), // LP mint B
                deposit_amount
            )?;
            
            // Check and adjust the value according to the limit
            let adjusted_token_amount = check_mint_limit(state, token_amount)?;

            // Force cleanup after calculation
            force_memory_cleanup();
            
            // Mint tokens for the program vault
            process_mint_tokens(
                &ctx.accounts.token_mint.to_account_info(),
                &ctx.accounts.program_token_vault.to_account_info(),
                &ctx.accounts.token_mint_authority.to_account_info(),
                &ctx.accounts.token_program,
                adjusted_token_amount,
                &[&[
                    b"token_mint_authority".as_ref(),
                    &[ctx.bumps.token_mint_authority]
                ]],
            )?;

            // Add cleanup:
            force_memory_cleanup();
            
            // Update reserved tokens value for the referrer
            ctx.accounts.referrer.reserved_tokens = adjusted_token_amount;
        }
        // LOGIC FOR SLOT 3: Pay referrer (SOL and tokens) and start recursion
        else if slot_idx == 2 {
            // 1. Transfer the reserved SOL value to the referrer
            if ctx.accounts.referrer.reserved_sol > 0 {
                // Verify that referrer_wallet is a system account
                verify_wallet_is_system_account(&ctx.accounts.referrer_wallet.to_account_info())?;
                
                process_pay_referrer(
                    &ctx.accounts.program_sol_vault.to_account_info(),
                    &ctx.accounts.referrer_wallet.to_account_info(),
                    ctx.accounts.referrer.reserved_sol,
                    &[&[
                        b"program_sol_vault".as_ref(),
                        &[ctx.bumps.program_sol_vault]
                    ]],
                )?;
                
                // Zero out the reserved SOL value after payment
                ctx.accounts.referrer.reserved_sol = 0;
            }
            
            // Verify the token account is valid
            verify_token_account(
                &ctx.accounts.referrer_token_account.to_account_info(),
                &ctx.accounts.referrer_wallet.key(),
                &ctx.accounts.token_mint.key()
            )?;
            
            // Transfer the reserved tokens to the referrer using vault_authority
            if ctx.accounts.referrer.reserved_tokens > 0 {
                process_transfer_tokens(
                    &ctx.accounts.program_token_vault.to_account_info(),
                    &ctx.accounts.referrer_token_account.to_account_info(),
                    &ctx.accounts.vault_authority.to_account_info(),
                    &ctx.accounts.token_program,
                    ctx.accounts.referrer.reserved_tokens,
                    &[&[
                        b"token_vault_authority".as_ref(),
                        &[ctx.bumps.vault_authority]
                    ]],
                )?;
                // Add cleanup:
                force_memory_cleanup();
                
                // Zero out the reserved tokens value after payment
                ctx.accounts.referrer.reserved_tokens = 0;
            }
        }
        
        // Process the referrer's matrix
        let (chain_completed, upline_pubkey) = process_referrer_chain(
            &ctx.accounts.user_wallet.key(),
            &mut ctx.accounts.referrer,
            state.next_chain_id,
        )?;

        // Add cleanup:
        force_memory_cleanup();
        
        // If the matrix was completed, increment the global ID for the next one
        if chain_completed {
            state.next_chain_id += 1;
        }

        // If the referrer's matrix was completed, process recursion
        if chain_completed && slot_idx == 2 {
            let mut current_user_pubkey = upline_pubkey;
            let mut current_deposit = deposit_amount;
            let mut wsol_closed = false;

            // Calculate remaining accounts offset - skip the pool, 4 vault A accounts and 2 Chainlink accounts
            let upline_start_idx = VAULT_A_ACCOUNTS_COUNT + 2;

            // Check if we have upline accounts to process (besides the pool, 4 vault A accounts and 2 Chainlink accounts)
            if ctx.remaining_accounts.len() > upline_start_idx && current_deposit > 0 {
                let upline_accounts = &ctx.remaining_accounts[upline_start_idx..];
                
                // OPTIMIZATION - Check if remaining upline accounts are multiples of 3
                if upline_accounts.len() % 3 != 0 {
                    return Err(error!(ErrorCode::MissingUplineAccount));
                }
                
                // Calculate number of trios (PDA, wallet, ATA)
                let trio_count = upline_accounts.len() / 3;
                
                // OPTIMIZATION - Process in smaller batches to save memory
                const BATCH_SIZE: usize = 1; 
                
                // Calculate number of batches (division with rounding up)
                let batch_count = (trio_count + BATCH_SIZE - 1) / BATCH_SIZE;
                
                // Process each batch
                for batch_idx in 0..batch_count {
                    // Calculate batch range
                    let start_trio = batch_idx * BATCH_SIZE;
                    let end_trio = std::cmp::min(start_trio + BATCH_SIZE, trio_count);
                    
                    // Iterate through trios in current batch
                    for trio_index in start_trio..end_trio {
                        // Check maximum depth and if deposit is remaining
                        if trio_index >= MAX_UPLINE_DEPTH || current_deposit == 0 {
                            break;
                        }

                        // Calculate base index for each trio
                        let base_idx = trio_index * 3;
                        
                        // Get current upline information
                        let upline_info = &upline_accounts[base_idx];       // Account PDA
                        let upline_wallet = &upline_accounts[base_idx + 1]; // Wallet 
                        let upline_token = &upline_accounts[base_idx + 2];  // ATA for tokens
                        
                        // OPTIMIZATION - Basic validations before processing the account
                        if upline_wallet.owner != &solana_program::system_program::ID {
                            return Err(error!(ErrorCode::PaymentWalletInvalid));
                        }
                        
                        // Check program ownership first before trying to deserialize
                        if !upline_info.owner.eq(&crate::ID) {
                            return Err(error!(ErrorCode::InvalidSlotOwner));
                        }

                        // STEP 1: Read and process data - Optimized for lower memory usage
                        let mut upline_account_data;
                        {
                            // Limited scope for data borrowing
                            let data = upline_info.try_borrow_data()?;
                            if data.len() <= 8 {
                                return Err(ProgramError::InvalidAccountData.into());
                            }

                            // Deserialize directly without clone
                            let mut account_slice = &data[8..];
                            upline_account_data = UserAccount::deserialize(&mut account_slice)?;

                            // Verify registration immediately
                            if !upline_account_data.is_registered {
                                return Err(error!(ErrorCode::SlotNotRegistered));
                            }
                        }

                        force_memory_cleanup();

                        // Continue processing with deserialized data
                        let upline_slot_idx = upline_account_data.chain.filled_slots as usize;
                        let upline_key = *upline_info.key;
                        
                        // Verify token account only if it's slot 3
                        if upline_slot_idx == 2 {
                            if upline_token.owner != &spl_token::id() {
                                return Err(error!(ErrorCode::TokenAccountInvalid));
                            }
                            
                            verify_token_account(
                                upline_token,
                                &upline_wallet.key(),
                                &ctx.accounts.token_mint.key()
                            )?;
                        }
                        
                        // Add current user to the matrix
                        upline_account_data.chain.slots[upline_slot_idx] = Some(current_user_pubkey);
                        
                        // Emit slot filled event in recursion
                        emit!(SlotFilled {
                            slot_idx: upline_slot_idx as u8,
                            chain_id: upline_account_data.chain.id,
                            user: current_user_pubkey,
                            owner: upline_key,
                        });
                        
                        // Increment filled slots count
                        upline_account_data.chain.filled_slots += 1;
                        
                        // Apply specific financial logic for the deposit
                        if upline_slot_idx == 0 {
                            // SLOT 1: Deposit to pool
                            // Use the WSOL account that was kept open
                            process_deposit_to_pool(
                                &ctx.accounts.user_wallet.to_account_info(),
                                &ctx.accounts.user_wsol_account.to_account_info(),
                                &ctx.accounts.b_vault_lp.to_account_info(),
                                &ctx.accounts.b_vault,
                                &ctx.accounts.b_token_vault.to_account_info(),
                                &ctx.accounts.b_vault_lp_mint.to_account_info(),
                                &ctx.accounts.vault_program,
                                &ctx.accounts.token_program,
                                current_deposit
                            )?;
                            
                            // Deposit was used, doesn't continue in recursion
                            current_deposit = 0;
                        } 
                        else if upline_slot_idx == 1 {
                            // SLOT 2: Reserve for upline (SOL and tokens)
                            // Close WSOL account if still open
                            if !wsol_closed {
                                let close_ix = spl_token::instruction::close_account(
                                    &token::ID,
                                    &ctx.accounts.user_wsol_account.key(),
                                    &ctx.accounts.user_wallet.key(),
                                    &ctx.accounts.user_wallet.key(),
                                    &[]
                                )?;
                                
                                let close_accounts = [
                                    ctx.accounts.user_wsol_account.to_account_info(),
                                    ctx.accounts.user_wallet.to_account_info(),
                                    ctx.accounts.user_wallet.to_account_info(),
                                ];
                                
                                solana_program::program::invoke(
                                    &close_ix,
                                    &close_accounts,
                                ).map_err(|_| error!(ErrorCode::UnwrapSolFailed))?;
                                
                                wsol_closed = true;
                            }
                            
                            // Now reserve the SOL
                            process_reserve_sol(
                                &ctx.accounts.user_wallet.to_account_info(),
                                &ctx.accounts.program_sol_vault.to_account_info(),
                                current_deposit
                            )?;
                            
                            // Update the reserved SOL value for the upline
                            upline_account_data.reserved_sol = current_deposit;
                            
                            // Calculate tokens based on pool value (using vault A accounts) 
                            let token_amount = get_donut_tokens_amount(
                                pool,               // Pool state
                                a_vault,            // Vault A state
                                &ctx.accounts.b_vault.to_account_info(), // Vault B state
                                a_vault_lp,         // LP token account A
                                &ctx.accounts.b_vault_lp.to_account_info(), // LP token account B
                                a_vault_lp_mint,    // LP mint A
                                &ctx.accounts.b_vault_lp_mint.to_account_info(), // LP mint B
                                current_deposit
                            )?;
                            
                            // Check and adjust the value according to the limit
                            let adjusted_token_amount = check_mint_limit(state, token_amount)?;

                            // Force cleanup after calculation
                            force_memory_cleanup();
                            
                            // Mint tokens for the program vault
                            process_mint_tokens(
                                &ctx.accounts.token_mint.to_account_info(),
                                &ctx.accounts.program_token_vault.to_account_info(),
                                &ctx.accounts.token_mint_authority.to_account_info(),
                                &ctx.accounts.token_program,
                                adjusted_token_amount,
                                &[&[
                                    b"token_mint_authority".as_ref(),
                                    &[ctx.bumps.token_mint_authority]
                                ]],
                            )?;

                            force_memory_cleanup();
                            
                            // Update the reserved tokens value for the upline
                            upline_account_data.reserved_tokens = adjusted_token_amount;
                            
                            // Deposit was reserved, doesn't continue in recursion
                            current_deposit = 0;
                        }
                        // SLOT 3: Pay reserved SOL and tokens to upline
                        else if upline_slot_idx == 2 {
                            // Pay reserved SOL
                            if upline_account_data.reserved_sol > 0 {
                                let reserved_sol = upline_account_data.reserved_sol;
                                
                                // Verify that wallet is a system account
                                if upline_wallet.owner != &solana_program::system_program::ID {
                                    return Err(error!(ErrorCode::PaymentWalletInvalid));
                                }
                                
                                // Create the transfer instruction
                                let ix = solana_program::system_instruction::transfer(
                                    &ctx.accounts.program_sol_vault.key(),
                                    &upline_wallet.key(),
                                    reserved_sol
                                );
                                
                                // Use Vec instead of array to avoid lifetime problems
                                let mut accounts = Vec::with_capacity(2);
                                accounts.push(ctx.accounts.program_sol_vault.to_account_info());
                                accounts.push(upline_wallet.clone());
                                
                                // Invoke the instruction with signature
                                solana_program::program::invoke_signed(
                                    &ix,
                                    &accounts,
                                    &[&[
                                        b"program_sol_vault".as_ref(),
                                        &[ctx.bumps.program_sol_vault]
                                    ]],
                                ).map_err(|_| error!(ErrorCode::ReferrerPaymentFailed))?;
                                
                                // Zero out the reserved SOL value
                                upline_account_data.reserved_sol = 0;
                            }
                            
                            // Pay reserved tokens
                            if upline_account_data.reserved_tokens > 0 {
                                let reserved_tokens = upline_account_data.reserved_tokens;
                                
                                // Verify if the token account is valid
                                if upline_token.owner != &spl_token::id() {
                                    return Err(error!(ErrorCode::TokenAccountInvalid));
                                }
                                
                                // Transfer tokens using function with individual parameters
                                process_transfer_tokens(
                                    &ctx.accounts.program_token_vault.to_account_info(),
                                    upline_token,
                                    &ctx.accounts.vault_authority.to_account_info(),
                                    &ctx.accounts.token_program,
                                    reserved_tokens,
                                    &[&[
                                        b"token_vault_authority".as_ref(),
                                        &[ctx.bumps.vault_authority]
                                    ]],
                                )?;
                                
                                // Add cleanup:
                                force_memory_cleanup();
                                
                                // Zero out the reserved tokens value after payment
                                upline_account_data.reserved_tokens = 0;
                            }
                        }
                        
                        // Check if matrix is complete
                        let chain_completed = upline_account_data.chain.filled_slots == 3;
                        
                        // Process matrix completion only if necessary
                        if chain_completed {
                            // Get new ID for the reset matrix
                            let next_chain_id_value = state.next_chain_id;
                            state.next_chain_id += 1;
                            
                            // Reset matrix with new ID
                            upline_account_data.chain.id = next_chain_id_value;
                            upline_account_data.chain.slots = [None, None, None];
                            upline_account_data.chain.filled_slots = 0;
                            
                            // Update current user for recursion
                            current_user_pubkey = upline_key;
                        }
                        
                        // STEP 2: Save changes back to the account
                        {
                            // New scope for mutable borrowing
                            let mut data = upline_info.try_borrow_mut_data()?;
                            let mut write_data = &mut data[8..];
                            upline_account_data.serialize(&mut write_data)?;
                        }

                        // Add cleanup:
                        force_memory_cleanup();
                        
                        // If matrix was not completed, stop processing here
                        if !chain_completed {
                            break;
                        }
                        
                        // Check maximum depth after processing
                        if trio_index >= MAX_UPLINE_DEPTH - 1 {
                            break;
                        }
                    }
                    
                    // Stop batch processing if no more deposits
                    if current_deposit == 0 {
                        break;
                    }
                }

                // Handle any remaining deposit
                if current_deposit > 0 {
                    // Deposit to pool if WSOL is still open
                    if !wsol_closed {
                        process_deposit_to_pool(
                            &ctx.accounts.user_wallet.to_account_info(),
                            &ctx.accounts.user_wsol_account.to_account_info(),
                            &ctx.accounts.b_vault_lp.to_account_info(),
                            &ctx.accounts.b_vault,
                            &ctx.accounts.b_token_vault.to_account_info(),
                            &ctx.accounts.b_vault_lp_mint.to_account_info(),
                            &ctx.accounts.vault_program,
                            &ctx.accounts.token_program,
                            current_deposit
                        )?;
                    }
                }
                
                // Close WSOL account if still open
                if !wsol_closed {
                    let account_info = ctx.accounts.user_wsol_account.to_account_info();
                    if account_info.data_len() > 0 {
                        let close_ix = spl_token::instruction::close_account(
                            &token::ID,
                            &ctx.accounts.user_wsol_account.key(),
                            &ctx.accounts.user_wallet.key(),
                            &ctx.accounts.user_wallet.key(),
                            &[]
                        )?;
                        
                        let close_accounts = [
                            ctx.accounts.user_wsol_account.to_account_info(),
                            ctx.accounts.user_wallet.to_account_info(),
                            ctx.accounts.user_wallet.to_account_info(),
                        ];
                        
                        solana_program::program::invoke(
                            &close_ix,
                            &close_accounts,
                        ).map_err(|_| error!(ErrorCode::UnwrapSolFailed))?;
                    }
                }
            }
        }

        Ok(())
    }
}