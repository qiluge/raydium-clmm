pub mod access_control;
pub mod error;
pub mod instructions;
pub mod libraries;
pub mod states;

use crate::access_control::*;
use crate::error::ErrorCode;
use crate::libraries::tick_math;
use anchor_lang::prelude::*;
use instructions::*;
use states::*;

declare_id!("7sSUSz5fEcX6CNrbu3Z3JRdTGqPQdxJYTwKYP8NF95Pp");

#[program]
pub mod amm_core {

    use super::*;

    // ---------------------------------------------------------------------
    // Factory instructions
    // The Factory facilitates creation of pools and control over the protocol fees

    /// Initialize the factory state and set the protocol owner
    ///
    /// # Arguments
    ///
    /// * `ctx`- Initializes the factory state account
    /// * `factory_state_bump` - Bump to validate factory state address
    ///
    pub fn init_factory(ctx: Context<Initialize>) -> Result<()> {
        instructions::init_factory(ctx)
    }

    /// Updates the owner of the factory
    /// Must be called by the current owner
    ///
    /// # Arguments
    ///
    /// * `ctx`- Checks whether protocol owner has signed
    ///
    pub fn set_owner(ctx: Context<SetOwner>) -> Result<()> {
        instructions::set_owner(ctx)
    }

    /// Enables a fee amount with the given tick_spacing
    /// Fee amounts may never be removed once enabled
    ///
    /// # Arguments
    ///
    /// * `ctx`- Checks whether protocol owner has signed and initializes the fee account
    /// * `fee_state_bump` - Bump to validate fee state address
    /// * `fee` - The fee amount to enable, denominated in hundredths of a bip (i.e. 1e-6)
    /// * `tick_spacing` - The spacing between ticks to be enforced for all pools created
    /// with the given fee amount
    ///
    pub fn enable_fee_amount(
        ctx: Context<EnableFeeAmount>,
        fee: u32,
        tick_spacing: u16,
    ) -> Result<()> {
        instructions::enable_fee_amount(ctx, fee, tick_spacing)
    }

    // ---------------------------------------------------------------------
    // Pool instructions

    /// Creates a pool for the given token pair and fee, and sets the initial price
    ///
    /// A single function in place of Uniswap's Factory.createPool(), PoolDeployer.deploy()
    /// Pool.initialize() and pool.Constructor()
    ///
    /// # Arguments
    ///
    /// * `ctx`- Validates token addresses and fee state. Initializes pool, observation and
    /// token accounts
    /// * `pool_state_bump` - Bump to validate Pool State address
    /// * `observation_state_bump` - Bump to validate Observation State address
    /// * `sqrt_price_x32` - the initial sqrt price (amount_token_1 / amount_token_0) of the pool as a Q32.32
    ///
    pub fn create_and_init_pool(
        ctx: Context<CreateAndInitPool>,
        sqrt_price_x32: u64,
    ) -> Result<()> {
        instructions::create_and_init_pool(ctx, sqrt_price_x32)
    }

    // ---------------------------------------------------------------------
    // Oracle

    /// Increase the maximum number of price and liquidity observations that this pool will store
    ///
    /// An `ObservationState` account is created per unit increase in cardinality_next,
    /// and `observation_cardinality_next` is accordingly incremented.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Holds the pool and payer addresses, along with a vector of
    /// observation accounts which will be initialized
    /// * `observation_account_bumps` - Vector of bumps to initialize the observation state PDAs
    ///
    pub fn increase_observation_cardinality_next<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info,IncreaseObservationCardinalityNextCtx<'info>>,
        observation_account_bumps: Vec<u8>,
    ) -> Result<()> {
        instructions::increase_observation_cardinality_next(ctx, observation_account_bumps)
    }


    // ---------------------------------------------------------------------
    // Pool owner instructions

    /// Set the denominator of the protocol's % share of the fees.
    ///
    /// Unlike Uniswap, protocol fee is globally set. It can be updated by factory owner
    /// at any time.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Checks for valid owner by looking at signer and factory owner addresses.
    /// Holds the Factory State account where protocol fee will be saved.
    /// * `fee_protocol` - new protocol fee for all pools
    ///
    pub fn set_fee_protocol(ctx: Context<SetFeeProtocol>, fee_protocol: u8) -> Result<()> {
        assert!(fee_protocol >= 2 && fee_protocol <= 10);
        let mut factory_state = ctx.accounts.factory_state.load_mut()?;
        let fee_protocol_old = factory_state.fee_protocol;
        factory_state.fee_protocol = fee_protocol;

        emit!(SetFeeProtocolEvent {
            fee_protocol_old,
            fee_protocol
        });

        Ok(())
    }

    /// Collect the protocol fee accrued to the pool
    ///
    /// # Arguments
    ///
    /// * `ctx` - Checks for valid owner by looking at signer and factory owner addresses.
    /// Holds the Pool State account where accrued protocol fee is saved, and token accounts to perform
    /// transfer.
    /// * `amount_0_requested` - The maximum amount of token_0 to send, can be 0 to collect fees in only token_1
    /// * `amount_1_requested` - The maximum amount of token_1 to send, can be 0 to collect fees in only token_0
    ///
    pub fn collect_protocol(
        ctx: Context<CollectProtocol>,
        amount_0_requested: u64,
        amount_1_requested: u64,
    ) -> Result<()> {
        instructions::collect_protocol(ctx, amount_0_requested, amount_1_requested)
    }
    /// ---------------------------------------------------------------------
    /// Account init instructions
    ///
    /// Having separate instructions to initialize instructions saves compute units
    /// and reduces code in downstream instructions
    ///

    /// Initializes an empty program account for a price tick
    ///
    /// # Arguments
    ///
    /// * `ctx` - Contains accounts to initialize an empty tick account
    /// * `tick_account_bump` - Bump to validate tick account PDA
    /// * `tick` - The tick for which the account is created
    ///
    pub fn init_tick_account(ctx: Context<InitTickAccount>, tick: i32) -> Result<()> {
        let pool_state = ctx.accounts.pool_state.load()?;
        check_tick(tick, pool_state.tick_spacing)?;
        let mut tick_state = ctx.accounts.tick_state.load_init()?;
        tick_state.bump = *ctx.bumps.get("tick_state").unwrap();
        tick_state.tick = tick;
        Ok(())
    }

    /// Reclaims lamports from a cleared tick account
    ///
    /// # Arguments
    ///
    /// * `ctx` - Holds tick and recipient accounts with validation and closure code
    ///
    pub fn close_tick_account(_ctx: Context<CloseTickAccount>) -> Result<()> {
        Ok(())
    }

    /// Initializes an empty program account for a tick bitmap
    ///
    /// # Arguments
    ///
    /// * `ctx` - Contains accounts to initialize an empty bitmap account
    /// * `bitmap_account_bump` - Bump to validate the bitmap account PDA
    /// * `word_pos` - The bitmap key for which to create account. To find word position from a tick,
    /// divide the tick by tick spacing to get a 24 bit compressed result, then right shift to obtain the
    /// most significant 16 bits.
    ///
    pub fn init_bitmap_account(ctx: Context<InitBitmapAccount>, word_pos: i16) -> Result<()> {
        let pool_state = ctx.accounts.pool_state.load()?;
        let max_word_pos = ((tick_math::MAX_TICK / pool_state.tick_spacing as i32) >> 8) as i16;
        let min_word_pos = ((tick_math::MIN_TICK / pool_state.tick_spacing as i32) >> 8) as i16;
        require!(word_pos >= min_word_pos, ErrorCode::TLM);
        require!(word_pos <= max_word_pos, ErrorCode::TUM);

        let mut bitmap_account = ctx.accounts.bitmap_state.load_init()?;
        bitmap_account.bump = *ctx.bumps.get("bitmap_state").unwrap();
        bitmap_account.word_pos = word_pos;
        Ok(())
    }

    /// Initializes an empty program account for a position
    ///
    /// # Arguments
    ///
    /// * `ctx` - Contains accounts to initialize an empty position account
    /// * `bump` - Bump to validate the position account PDA
    /// * `tick` - The tick for which the bitmap account is created. Program address of
    /// the account is derived using most significant 16 bits of the tick
    ///
    pub fn init_position_account(ctx: Context<InitPositionAccount>) -> Result<()> {
        let mut position_account = ctx.accounts.position_state.load_init()?;
        position_account.bump = *ctx.bumps.get("position_state").unwrap();
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Position instructions

    /// Adds liquidity for the given pool/recipient/tickLower/tickUpper position
    ///
    /// # Arguments
    ///
    /// * `ctx` - Holds the recipient's address and program accounts for
    /// pool, position and ticks.
    /// * `amount` - The amount of liquidity to mint
    ///
    pub fn mint<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, MintContext<'info>>,
        amount: u64,
    ) -> Result<()> {
        instructions::mint(ctx, amount)
    }
    /// Burn liquidity from the sender and account tokens owed for the liquidity to the position.
    /// Can be used to trigger a recalculation of fees owed to a position by calling with an amount of 0 (poke).
    /// Fees must be collected separately via a call to #collect
    ///
    /// # Arguments
    ///
    /// * `ctx` - Holds position and other validated accounts need to burn liquidity
    /// * `amount` - Amount of liquidity to be burned
    ///
    pub fn burn<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, BurnContext<'info>>,
        amount: u64,
    ) -> Result<()> {
        instructions::burn(ctx, amount)
    }
    /// Collects tokens owed to a position.
    ///
    /// Does not recompute fees earned, which must be done either via mint or burn of any amount of liquidity.
    /// Collect must be called by the position owner. To withdraw only token_0 or only token_1, amount_0_requested or
    /// amount_1_requested may be set to zero. To withdraw all tokens owed, caller may pass any value greater than the
    /// actual tokens owed, e.g. u64::MAX. Tokens owed may be from accumulated swap fees or burned liquidity.
    ///
    /// # Arguments
    ///
    /// * `amount_0_requested` - How much token_0 should be withdrawn from the fees owed
    /// * `amount_1_requested` - How much token_1 should be withdrawn from the fees owed
    ///
    pub fn collect(
        ctx: Context<CollectContext>,
        amount_0_requested: u64,
        amount_1_requested: u64,
    ) -> Result<()> {
        instructions::collect(ctx, amount_0_requested, amount_1_requested)
    }
    // ---------------------------------------------------------------------
    // 4. Swap instructions

    /// Swap token_0 for token_1, or token_1 for token_0
    ///
    /// Outstanding tokens must be paid in #swap_callback
    ///
    /// # Arguments
    ///
    /// * `ctx` - Accounts required for the swap. Remaining accounts should contain each bitmap leading to
    /// the end tick, and each tick being flipped
    /// account leading to the destination tick
    /// * `deadline` - The time by which the transaction must be included to effect the change
    /// * `amount_specified` - The amount of the swap, which implicitly configures the swap as exact input (positive),
    /// or exact output (negative)
    /// * `sqrt_price_limit` - The Q32.32 sqrt price √P limit. If zero for one, the price cannot
    /// be less than this value after the swap.  If one for zero, the price cannot be greater than
    /// this value after the swap.
    ///
    pub fn swap(
        ctx: Context<SwapContext>,
        amount_specified: i64,
        sqrt_price_limit_x32: u64,
    ) -> Result<()> {
        instructions::swap(ctx, amount_specified, sqrt_price_limit_x32)
    }
    // /// Component function for flash swaps
    // ///
    // /// Donate given liquidity to in-range positions then make callback
    // /// Only callable by a smart contract which implements uniswapV3FlashCallback(),
    // /// where profitability check can be performed
    // ///
    // /// Flash swaps is an advanced feature for developers, not directly available for UI based traders.
    // /// Periphery does not provide an implementation, but a sample is provided
    // /// Ref- https://github.com/Uniswap/v3-periphery/blob/main/contracts/examples/PairFlash.sol
    // ///
    // ///
    // /// Flow
    // /// 1. FlashDapp.initFlash()
    // /// 2. Core.flash()
    // /// 3. FlashDapp.uniswapV3FlashCallback()
    // ///
    // /// @param amount_0 Amount of token 0 to donate
    // /// @param amount_1 Amount of token 1 to donate
    // pub fn flash(ctx: Context<SetFeeProtocol>, amount_0: u64, amount_1: u64) -> Result<()> {
    //     todo!()
    // }

    // Non fungible position manager

    /// Creates a new position wrapped in a NFT
    ///
    /// # Arguments
    ///
    /// * `ctx` - Holds pool, tick, bitmap, position and token accounts
    /// * `amount_0_desired` - Desired amount of token_0 to be spent
    /// * `amount_1_desired` - Desired amount of token_1 to be spent
    /// * `amount_0_min` - The minimum amount of token_0 to spend, which serves as a slippage check
    /// * `amount_1_min` - The minimum amount of token_1 to spend, which serves as a slippage check
    /// * `deadline` - The time by which the transaction must be included to effect the change
    ///
    #[access_control(check_deadline(deadline))]
    pub fn mint_tokenized_position<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, MintTokenizedPosition<'info>>,
        amount_0_desired: u64,
        amount_1_desired: u64,
        amount_0_min: u64,
        amount_1_min: u64,
        deadline: i64,
    ) -> Result<()> {
        instructions::mint_tokenized_position(
            ctx,
            amount_0_desired,
            amount_1_desired,
            amount_0_min,
            amount_1_min,
            deadline,
        )
    }
    /// Attach metaplex metadata to a tokenized position. Permissionless to call.
    /// Optional and cosmetic in nature.
    ///
    /// # Arguments
    ///
    /// * `ctx` - Holds validated metadata account and tokenized position addresses
    ///
    pub fn add_metaplex_metadata(ctx: Context<AddMetaplexMetadata>) -> Result<()> {
        instructions::add_metaplex_metadata(ctx)
    }

    /// Increases liquidity in a tokenized position, with amount paid by `payer`
    ///
    /// # Arguments
    ///
    /// * `ctx` - Holds the pool, tick, bitmap, position and token accounts
    /// * `amount_0_desired` - Desired amount of token_0 to be spent
    /// * `amount_1_desired` - Desired amount of token_1 to be spent
    /// * `amount_0_min` - The minimum amount of token_0 to spend, which serves as a slippage check
    /// * `amount_1_min` - The minimum amount of token_1 to spend, which serves as a slippage check
    /// * `deadline` - The time by which the transaction must be included to effect the change
    ///
    #[access_control(check_deadline(deadline))]
    pub fn increase_liquidity<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, IncreaseLiquidity<'info>>,
        amount_0_desired: u64,
        amount_1_desired: u64,
        amount_0_min: u64,
        amount_1_min: u64,
        deadline: i64,
    ) -> Result<()> {
        instructions::increase_liquidity(
            ctx,
            amount_0_desired,
            amount_1_desired,
            amount_0_min,
            amount_1_min,
            deadline,
        )
    }
    /// Decreases the amount of liquidity in a position and accounts it to the position
    ///
    /// # Arguments
    ///
    /// * `ctx` - Holds the pool, tick, bitmap, position and token accounts
    /// * `liquidity` - The amount by which liquidity will be decreased
    /// * `amount_0_min` - The minimum amount of token_0 that should be accounted for the burned liquidity
    /// * `amount_1_min` - The minimum amount of token_1 that should be accounted for the burned liquidity
    /// * `deadline` - The time by which the transaction must be included to effect the change
    ///
    #[access_control(check_deadline(deadline))]
    #[access_control(is_authorized_for_token(&ctx.accounts.owner_or_delegate, &ctx.accounts.nft_account))]
    pub fn decrease_liquidity<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, DecreaseLiquidity<'info>>,
        liquidity: u64,
        amount_0_min: u64,
        amount_1_min: u64,
        deadline: i64,
    ) -> Result<()> {
        instructions::decrease_liquidity(ctx, liquidity, amount_0_min, amount_1_min, deadline)
    }

    /// Collects up to a maximum amount of fees owed to a specific tokenized position to the recipient
    ///
    /// # Arguments
    ///
    /// * `ctx` - Validated addresses of the tokenized position and token accounts. Fees can be sent
    /// to third parties
    /// * `amount_0_max` - The maximum amount of token0 to collect
    /// * `amount_1_max` - The maximum amount of token0 to collect
    ///
    #[access_control(is_authorized_for_token(&ctx.accounts.owner_or_delegate, &ctx.accounts.nft_account))]
    pub fn collect_from_tokenized<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, CollectFromTokenized<'info>>,
        amount_0_max: u64,
        amount_1_max: u64,
    ) -> Result<()> {
        instructions::collect_from_tokenized(ctx, amount_0_max, amount_1_max)
    }

    /// Swaps `amount_in` of one token for as much as possible of another token,
    /// across a single pool
    ///
    /// # Arguments
    ///
    /// * `ctx` - Accounts required for the swap
    /// * `deadline` - The time by which the transaction must be included to effect the change
    /// * `amount_in` - Token amount to be swapped in
    /// * `amount_out_minimum` - The minimum amount to swap out, which serves as a slippage check
    /// * `sqrt_price_limit` - The Q32.32 sqrt price √P limit. If zero for one, the price cannot
    /// be less than this value after the swap.  If one for zero, the price cannot be greater than
    /// this value after the swap.
    ///
    #[access_control(check_deadline(deadline))]
    pub fn exact_input_single<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, ExactInputSingle<'info>>,
        deadline: i64,
        amount_in: u64,
        amount_out_minimum: u64,
        sqrt_price_limit_x32: u64,
    ) -> Result<()> {
        instructions::exact_input_single(
            ctx,
            deadline,
            amount_in,
            amount_out_minimum,
            sqrt_price_limit_x32,
        )
    }
    /// Swaps `amount_in` of one token for as much as possible of another token,
    /// across the path provided
    ///
    /// # Arguments
    ///
    /// * `ctx` - Accounts for token transfer and swap route
    /// * `deadline` - Swap should if fail if past deadline
    /// * `amount_in` - Token amount to be swapped in
    /// * `amount_out_minimum` - Panic if output amount is below minimum amount. For slippage.
    /// * `additional_accounts_per_pool` - Additional observation, bitmap and tick accounts per pool
    ///
    #[access_control(check_deadline(deadline))]
    pub fn exact_input<'a, 'b, 'c, 'info>(
        ctx: Context<'a, 'b, 'c, 'info, ExactInput<'info>>,
        deadline: i64,
        amount_in: u64,
        amount_out_minimum: u64,
        additional_accounts_per_pool: Vec<u8>,
    ) -> Result<()> {
        instructions::exact_input(
            ctx,
            deadline,
            amount_in,
            amount_out_minimum,
            additional_accounts_per_pool,
        )
    }
    //  /// Swaps as little as possible of one token for `amount_out` of another token,
    // /// across a single pool
    // ///
    // /// # Arguments
    // ///
    // /// * `ctx` - Token and pool accounts for swap
    // /// * `zero_for_one` - Direction of swap. Swap token_0 for token_1 if true
    // /// * `deadline` - Swap should if fail if past deadline
    // /// * `amount_out` - Token amount to be swapped out
    // /// * `amount_in_maximum` - For slippage. Panic if required input exceeds max limit.
    // /// * `sqrt_price_limit` - Limit price √P for slippage
    // ///
    // pub fn exact_output_single(
    //     ctx: Context<ExactInputSingle>,
    //     zero_for_one: bool,
    //     deadline: u64,
    //     amount_out: u64,
    //     amount_in_maximum: u64,
    //     sqrt_price_limit_x32: u64,
    // ) -> Result<()> {
    //     todo!()
    // }

    // /// Swaps as little as possible of one token for `amount_out` of another
    // /// along the specified path (reversed)
    // ///
    // /// # Arguments
    // ///
    // /// * `ctx` - Accounts for token transfer and swap route
    // /// * `deadline` - Swap should if fail if past deadline
    // /// * `amount_out` - Token amount to be swapped out
    // /// * `amount_in_maximum` - For slippage. Panic if required input exceeds max limit.
    // ///
    // pub fn exact_output(
    //     ctx: Context<ExactInput>,
    //     deadline: u64,
    //     amount_out: u64,
    //     amount_out_maximum: u64,
    // ) -> Result<()> {
    //     todo!()
    // }
}

/// Common checks for a valid tick input.
/// A tick is valid iff it lies within tick boundaries and it is a multiple
/// of tick spacing.
///
/// # Arguments
///
/// * `tick` - The price tick
///
pub fn check_tick(tick: i32, tick_spacing: u16) -> Result<()> {
    require!(tick >= tick_math::MIN_TICK, ErrorCode::TLM);
    require!(tick <= tick_math::MAX_TICK, ErrorCode::TUM);
    require!(tick % tick_spacing as i32 == 0, ErrorCode::TMS);
    Ok(())
}

/// Common checks for valid tick inputs.
///
/// # Arguments
///
/// * `tick_lower` - The lower tick
/// * `tick_upper` - The upper tick
///
pub fn check_ticks(tick_lower: i32, tick_upper: i32) -> Result<()> {
    require!(tick_lower < tick_upper, ErrorCode::TLU);
    Ok(())
}