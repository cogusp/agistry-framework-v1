use anchor_lang::prelude::*;
use anchor_lang::solana_program::{program::invoke, system_instruction}; // For SOL transfers

declare_id!("DV6y2NyFNh8YCPzgdHHQYPdw33BskmeeoM2xWp39xMYS");

// --- Constants ---
const MAX_METADATA_HASH_LENGTH: usize = 70; // e.g., IPFS CIDv1 base32. Adjust as needed.
const MAX_SCHEMA_VERSION_LENGTH: usize = 20;

// --- Program Module ---
#[program]
pub mod agistry_registry {
    use super::*;

    // Initializes the global registry configuration.
    // Should only be called once by the deployer/admin.
    pub fn initialize_registry(
        ctx: Context<InitializeRegistry>,
        initial_metadata_schema_version: String,
        initial_registration_fee: u64,
        initial_fee_recipient: Pubkey,
    ) -> Result<()> {
        let registry_config = &mut ctx.accounts.registry_config;
        require!(
            initial_metadata_schema_version.len() <= MAX_SCHEMA_VERSION_LENGTH,
            AgistryError::SchemaVersionTooLong
        );

        registry_config.admin = ctx.accounts.admin.key();
        registry_config.adapter_counter = 0; // Will be incremented before first use
        registry_config.metadata_schema_version = initial_metadata_schema_version;
        registry_config.registration_fee = initial_registration_fee;
        registry_config.fee_recipient = initial_fee_recipient;
        registry_config.paused = false;
        registry_config.bump = ctx.bumps.registry_config;

        emit!(RegistryInitialized {
            admin: registry_config.admin,
            metadata_schema_version: registry_config.metadata_schema_version.clone(),
            registration_fee: registry_config.registration_fee,
            fee_recipient: registry_config.fee_recipient,
        });
        Ok(())
    }

    // Registers a new adapter.
    pub fn register_adapter(ctx: Context<RegisterAdapter>, metadata_hash: String) -> Result<()> {
        let registry_config = &mut ctx.accounts.registry_config;
        let adapter_account = &mut ctx.accounts.adapter_account;
        let clock = Clock::get()?;

        require!(!registry_config.paused, AgistryError::RegistryPaused);
        require!(
            metadata_hash.len() <= MAX_METADATA_HASH_LENGTH,
            AgistryError::MetadataHashTooLong
        );
        require!(metadata_hash.len() > 0, AgistryError::MetadataHashEmpty);

        // Handle registration fee
        if registry_config.registration_fee > 0 {
            require!(
                ctx.accounts.fee_payer.lamports() >= registry_config.registration_fee,
                AgistryError::InsufficientFundsForFee
            );
            invoke(
                &system_instruction::transfer(
                    ctx.accounts.fee_payer.key,
                    &registry_config.fee_recipient,
                    registry_config.registration_fee,
                ),
                &[
                    ctx.accounts.fee_payer.to_account_info(),
                    registry_config.to_account_info(), // Program account can also be fee recipient
                    ctx.accounts.system_program.to_account_info(),
                ],
            )?;
        }

        registry_config.adapter_counter = registry_config
            .adapter_counter
            .checked_add(1)
            .ok_or(AgistryError::NumericOverflow)?;
        let new_adapter_id = registry_config.adapter_counter;

        adapter_account.id = new_adapter_id;
        adapter_account.owner = ctx.accounts.owner.key();
        adapter_account.metadata_hash = metadata_hash.clone();
        adapter_account.status = AdapterStatus::Active;
        adapter_account.registration_timestamp = clock.unix_timestamp;
        adapter_account.last_update_timestamp = clock.unix_timestamp;
        adapter_account.bump = ctx.bumps.adapter_account;

        emit!(AdapterRegistered {
            adapter_id: new_adapter_id,
            owner: adapter_account.owner,
            metadata_hash,
            registration_timestamp: adapter_account.registration_timestamp
        });

        Ok(())
    }

    // Updates the metadata hash for an existing adapter.
    pub fn update_adapter_metadata(
        ctx: Context<UpdateAdapterMetadata>,
        new_metadata_hash: String,
    ) -> Result<()> {
        let adapter_account = &mut ctx.accounts.adapter_account;
        let clock = Clock::get()?;
        require!(
            !ctx.accounts.registry_config.paused,
            AgistryError::RegistryPaused
        );
        require!(
            new_metadata_hash.len() <= MAX_METADATA_HASH_LENGTH,
            AgistryError::MetadataHashTooLong
        );
        require!(new_metadata_hash.len() > 0, AgistryError::MetadataHashEmpty);
        require!(
            adapter_account.status == AdapterStatus::Active,
            AgistryError::CannotUpdateDeprecatedAdapter
        );

        adapter_account.metadata_hash = new_metadata_hash.clone();
        adapter_account.last_update_timestamp = clock.unix_timestamp;

        emit!(AdapterMetadataUpdated {
            adapter_id: adapter_account.id,
            new_metadata_hash,
            update_timestamp: adapter_account.last_update_timestamp
        });
        Ok(())
    }

    // Deprecates an adapter.
    pub fn deprecate_adapter(ctx: Context<OperateOnAdapter>) -> Result<()> {
        let adapter_account = &mut ctx.accounts.adapter_account;
        let clock = Clock::get()?;
        require!(
            !ctx.accounts.registry_config.paused,
            AgistryError::RegistryPaused
        );
        require!(
            adapter_account.status == AdapterStatus::Active,
            AgistryError::AdapterAlreadyDeprecated
        );

        adapter_account.status = AdapterStatus::Deprecated;
        adapter_account.last_update_timestamp = clock.unix_timestamp;

        emit!(AdapterStatusChanged {
            adapter_id: adapter_account.id,
            new_status: AdapterStatus::Deprecated,
            update_timestamp: adapter_account.last_update_timestamp
        });
        Ok(())
    }

    // Transfers ownership of an adapter registration.
    pub fn transfer_adapter_ownership(
        ctx: Context<OperateOnAdapter>,
        new_owner: Pubkey,
    ) -> Result<()> {
        let adapter_account = &mut ctx.accounts.adapter_account;
        require!(
            !ctx.accounts.registry_config.paused,
            AgistryError::RegistryPaused
        );
        require!(
            new_owner != Pubkey::default(),
            AgistryError::NewOwnerCannotBeDefault
        ); // Check for zero pubkey

        let old_owner = adapter_account.owner;
        adapter_account.owner = new_owner;
        // adapter_account.last_update_timestamp = clock.unix_timestamp; // Optional: decide if this updates timestamp

        emit!(AdapterOwnershipTransferred {
            adapter_id: adapter_account.id,
            previous_owner: old_owner,
            new_owner
        });
        Ok(())
    }

    // --- Admin Functions ---
    pub fn set_pause_status(ctx: Context<AdminAction>, paused: bool) -> Result<()> {
        ctx.accounts.registry_config.paused = paused;
        emit!(RegistryPauseStatusChanged { paused });
        Ok(())
    }

    pub fn set_metadata_schema_version(
        ctx: Context<AdminAction>,
        new_version: String,
    ) -> Result<()> {
        require!(
            new_version.len() <= MAX_SCHEMA_VERSION_LENGTH,
            AgistryError::SchemaVersionTooLong
        );
        ctx.accounts.registry_config.metadata_schema_version = new_version.clone();
        emit!(MetadataSchemaVersionSet {
            version: new_version
        });
        Ok(())
    }

    pub fn set_registration_fee(ctx: Context<AdminAction>, new_fee: u64) -> Result<()> {
        ctx.accounts.registry_config.registration_fee = new_fee;
        emit!(RegistrationFeeSet { new_fee });
        Ok(())
    }

    pub fn set_fee_recipient(ctx: Context<AdminAction>, new_recipient: Pubkey) -> Result<()> {
        require!(
            new_recipient != Pubkey::default(),
            AgistryError::NewOwnerCannotBeDefault
        );
        ctx.accounts.registry_config.fee_recipient = new_recipient;
        emit!(FeeRecipientSet { new_recipient });
        Ok(())
    }

    pub fn withdraw_fees(ctx: Context<WithdrawFees>) -> Result<()> {
        let registry_config = &ctx.accounts.registry_config;
        let fee_recipient_account_info = ctx.accounts.fee_recipient.to_account_info();
        let registry_config_account_info = registry_config.to_account_info();

        let lamports_to_withdraw = registry_config_account_info.lamports();
        require!(lamports_to_withdraw > 0, AgistryError::NoFeesToWithdraw);

        // Check if registry_config PDA is the fee_recipient
        if registry_config.fee_recipient != registry_config_account_info.key() {
            // Transfer from PDA to actual fee_recipient
            **registry_config_account_info.try_borrow_mut_lamports()? -= lamports_to_withdraw;
            **fee_recipient_account_info.try_borrow_mut_lamports()? += lamports_to_withdraw;

            emit!(FeesWithdrawn {
                recipient: registry_config.fee_recipient,
                amount: lamports_to_withdraw,
            });
        } else {
            // This case means the PDA itself is the fee recipient, which is unusual
            // unless intended for direct program control or burning.
            // No actual transfer needed if PDA is the target, but emit event if desired.
            msg!("Fees are already in the designated fee_recipient (which is the registry PDA).");
        }
        Ok(())
    }
}

// --- Account Contexts ---

#[derive(Accounts)]
pub struct InitializeRegistry<'info> {
    #[account(
        init,
        payer = admin,
        space = RegistryConfig::LEN,
        seeds = [b"registry_config"], // Seed for the PDA
        bump
    )]
    pub registry_config: Account<'info, RegistryConfig>,
    #[account(mut)]
    pub admin: Signer<'info>, // The deployer/admin paying for initialization
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(metadata_hash: String)] // Used for space calculation if string length varies
pub struct RegisterAdapter<'info> {
    #[account(
        init,
        payer = owner,
        space = AdapterAccount::LEN_WITH_HASH(metadata_hash.len()),
        seeds = [b"adapter", registry_config.adapter_counter.checked_add(1).unwrap().to_le_bytes().as_ref()], // Seed with next ID
        bump
    )]
    pub adapter_account: Account<'info, AdapterAccount>,
    #[account(
        mut,
        seeds = [b"registry_config"],
        bump = registry_config.bump
    )]
    pub registry_config: Account<'info, RegistryConfig>,
    #[account(mut)]
    pub owner: Signer<'info>, // The one registering and initially owning the adapter
    /// CHECK: This account is used as the payer for the registration fee.
    #[account(mut)]
    pub fee_payer: AccountInfo<'info>, // Can be same as owner or different
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(new_metadata_hash: String)]
pub struct UpdateAdapterMetadata<'info> {
    #[account(
        mut,
        seeds = [b"adapter", adapter_account.id.to_le_bytes().as_ref()],
        bump = adapter_account.bump,
        has_one = owner @ AgistryError::Unauthorized // Constraint: signer must be owner
    )]
    pub adapter_account: Account<'info, AdapterAccount>,
    #[account(seeds = [b"registry_config"], bump = registry_config.bump)]
    // Read-only for pause check
    pub registry_config: Account<'info, RegistryConfig>,
    pub owner: Signer<'info>,
}

// Common context for operations requiring adapter ownership and admin check
#[derive(Accounts)]
pub struct OperateOnAdapter<'info> {
    #[account(
        mut,
        seeds = [b"adapter", adapter_account.id.to_le_bytes().as_ref()],
        bump = adapter_account.bump,
        has_one = owner @ AgistryError::Unauthorized
    )]
    pub adapter_account: Account<'info, AdapterAccount>,
    #[account(seeds = [b"registry_config"], bump = registry_config.bump)]
    pub registry_config: Account<'info, RegistryConfig>, // For pause check
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct AdminAction<'info> {
    #[account(
        mut,
        seeds = [b"registry_config"],
        bump = registry_config.bump,
        has_one = admin @ AgistryError::UnauthorizedAdmin // Constraint: signer must be admin
    )]
    pub registry_config: Account<'info, RegistryConfig>,
    pub admin: Signer<'info>,
}

#[derive(Accounts)]
pub struct WithdrawFees<'info> {
    #[account(
        mut, // PDA needs to be mutable to decrease its lamports
        seeds = [b"registry_config"],
        bump = registry_config.bump,
        constraint = registry_config.admin == admin.key() @ AgistryError::UnauthorizedAdmin
    )]
    pub registry_config: Account<'info, RegistryConfig>,
    /// CHECK: This is the account where fees will be sent. It's validated against registry_config.fee_recipient.
    #[account(mut, address = registry_config.fee_recipient @ AgistryError::IncorrectFeeRecipient)]
    pub fee_recipient: AccountInfo<'info>,
    pub admin: Signer<'info>, // Admin must authorize withdrawal
}

// --- Account State Structs ---

#[account]
pub struct RegistryConfig {
    pub admin: Pubkey,
    pub adapter_counter: u64, // Stores the count of registered adapters, also used for next ID
    pub metadata_schema_version: String, // Max length defined by MAX_SCHEMA_VERSION_LENGTH
    pub registration_fee: u64, // In lamports
    pub fee_recipient: Pubkey,
    pub paused: bool,
    pub bump: u8, // Bump seed for the PDA
}

impl RegistryConfig {
    // Calculate space: 32 (admin) + 8 (counter) + (4 + X schema_ver_len) + 8 (fee) + 32 (recipient) + 1 (paused) + 1 (bump) + 8 (discriminator)
    const LEN_BASE: usize = 32 + 8 + 8 + 32 + 1 + 1 + 8;
    pub const LEN: usize = Self::LEN_BASE + (4 + MAX_SCHEMA_VERSION_LENGTH);
}

#[account]
pub struct AdapterAccount {
    pub id: u64,
    pub owner: Pubkey,
    pub metadata_hash: String, // Max length defined by MAX_METADATA_HASH_LENGTH
    pub status: AdapterStatus,
    pub registration_timestamp: i64,
    pub last_update_timestamp: i64,
    pub bump: u8, // Bump seed for this adapter's PDA
}

impl AdapterAccount {
    // Calculate space: 8 (id) + 32 (owner) + (4 + X hash_len) + 1 (status enum) + 8 (reg_ts) + 8 (last_upd_ts) + 1 (bump) + 8 (discriminator)
    const LEN_BASE: usize = 8 + 32 + 1 + 8 + 8 + 1 + 8;
    pub fn LEN_WITH_HASH(hash_len: usize) -> usize {
        Self::LEN_BASE + (4 + hash_len)
    }
}

// --- Enums & Custom Errors ---

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub enum AdapterStatus {
    Active,
    Deprecated,
}

#[error_code]
pub enum AgistryError {
    #[msg("The provided metadata hash is too long.")]
    MetadataHashTooLong,
    #[msg("The metadata hash cannot be empty.")]
    MetadataHashEmpty,
    #[msg("The provided schema version string is too long.")]
    SchemaVersionTooLong,
    #[msg("Unauthorized: Caller is not the owner of this adapter or registry.")]
    Unauthorized,
    #[msg("Unauthorized: Caller is not the admin of the registry.")]
    UnauthorizedAdmin,
    #[msg("Cannot update a deprecated adapter.")]
    CannotUpdateDeprecatedAdapter,
    #[msg("Adapter is already deprecated.")]
    AdapterAlreadyDeprecated,
    #[msg("The new owner pubkey cannot be the default/zero pubkey.")]
    NewOwnerCannotBeDefault,
    #[msg("Numeric overflow occurred.")]
    NumericOverflow,
    #[msg("Registry is currently paused.")]
    RegistryPaused,
    #[msg("Insufficient funds to pay the registration fee.")]
    InsufficientFundsForFee,
    #[msg("The fee recipient account for withdrawal does not match the configured one.")]
    IncorrectFeeRecipient,
    #[msg("No fees available to withdraw.")]
    NoFeesToWithdraw,
}

// --- Events ---
#[event]
pub struct RegistryInitialized {
    pub admin: Pubkey,
    pub metadata_schema_version: String,
    pub registration_fee: u64,
    pub fee_recipient: Pubkey,
}

#[event]
pub struct AdapterRegistered {
    pub adapter_id: u64,
    pub owner: Pubkey,
    pub metadata_hash: String,
    pub registration_timestamp: i64,
}

#[event]
pub struct AdapterMetadataUpdated {
    pub adapter_id: u64,
    pub new_metadata_hash: String,
    pub update_timestamp: i64,
}

#[event]
pub struct AdapterStatusChanged {
    pub adapter_id: u64,
    pub new_status: AdapterStatus,
    pub update_timestamp: i64,
}

#[event]
pub struct AdapterOwnershipTransferred {
    pub adapter_id: u64,
    pub previous_owner: Pubkey,
    pub new_owner: Pubkey,
}

#[event]
pub struct RegistryPauseStatusChanged {
    pub paused: bool,
}
#[event]
pub struct MetadataSchemaVersionSet {
    pub version: String,
}
#[event]
pub struct RegistrationFeeSet {
    pub new_fee: u64,
}
#[event]
pub struct FeeRecipientSet {
    pub new_recipient: Pubkey,
}
#[event]
pub struct FeesWithdrawn {
    pub recipient: Pubkey,
    pub amount: u64,
}
