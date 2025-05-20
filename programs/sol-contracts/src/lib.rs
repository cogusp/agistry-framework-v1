use anchor_lang::prelude::*;

declare_id!("DV6y2NyFNh8YCPzgdHHQYPdw33BskmeeoM2xWp39xMYS");

#[program]
pub mod sol_contracts {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        msg!("Greetings from: {:?}", ctx.program_id);
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize {}
