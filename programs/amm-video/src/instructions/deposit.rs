use anchor_lang::{prelude::*, Accounts};
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        mint_to_checked, transfer_checked, Mint, MintToChecked, TokenAccount, TokenInterface,
        TransferChecked,
    },
};

use crate::{AmmError, Config};

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,

    pub mint_x: Box<InterfaceAccount<'info, Mint>>,
    pub mint_y: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = signer
    )]
    pub user_ata_x: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = signer
    )]
    pub user_ata_y: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init_if_needed,
        payer = signer,
        associated_token::mint = lp_token,
        associated_token::authority = signer
    )]
    pub user_ata_lp: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = config
    )]
    pub vault_x: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = config
    )]
    pub vault_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [b"lp", config.key().as_ref()],
        bump = config.lp_bump
    )]
    pub lp_token: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        has_one = mint_x,
        has_one = mint_y,
        seeds = [b"config", config.seed.to_le_bytes().as_ref()],
        bump= config.config_bump,
    )]
    pub config: Box<Account<'info, Config>>,

    pub system_program: Program<'info, System>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> Deposit<'info> {
    pub fn deposit(&mut self, lp_amount: u64, amount_x: u64, amount_y: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);
        require_neq!(0, lp_amount, AmmError::InvalidAmount);

        let (x, y) =
            if self.lp_token.supply == 0 && self.vault_x.amount == 0 && self.vault_y.amount == 0 {
                (amount_x, amount_y)
            } else {
                let x = lp_amount
                    .checked_mul(self.vault_x.amount)
                    .ok_or(AmmError::LiquidityLessThanMinimum)?
                    .checked_div(self.lp_token.supply)
                    .ok_or(AmmError::LiquidityLessThanMinimum)?;

                let y = lp_amount
                    .checked_mul(self.vault_y.amount)
                    .ok_or(AmmError::LiquidityLessThanMinimum)?
                    .checked_div(self.lp_token.supply)
                    .ok_or(AmmError::LiquidityLessThanMinimum)?;

                (x, y)
            };

        require_gte!(amount_x, x, AmmError::CurveError);
        require_gte!(amount_y, y, AmmError::CurveError);

        self.transfer_tokens(true, x)?;
        self.transfer_tokens(false, y)?;

        self.mint_lp_tokens(lp_amount)?;

        Ok(())
    }

    pub fn transfer_tokens(&mut self, is_x: bool, amount: u64) -> Result<()> {
        let from = if is_x {
            self.user_ata_x.to_account_info()
        } else {
            self.user_ata_y.to_account_info()
        };

        let to = if is_x {
            self.vault_x.to_account_info()
        } else {
            self.vault_y.to_account_info()
        };

        let mint = if is_x {
            self.mint_x.to_account_info()
        } else {
            self.mint_y.to_account_info()
        };

        let tx = TransferChecked {
            from,
            to,
            authority: self.signer.to_account_info(),
            mint,
        };

        let cpi_ctx = CpiContext::new(self.token_program.key(), tx);
        let decimals = if is_x {
            self.mint_x.decimals
        } else {
            self.mint_y.decimals
        };

        transfer_checked(cpi_ctx, amount, decimals)
    }

    pub fn mint_lp_tokens(&mut self, mint_amount: u64) -> Result<()> {
        let tx = MintToChecked {
            mint: self.lp_token.to_account_info(),
            to: self.user_ata_lp.to_account_info(),
            authority: self.config.to_account_info(),
        };

        let config_seeds = self.config.seed.to_le_bytes();
        let signer_seeds: &[&[&[u8]]] =
            &[&[b"config", config_seeds.as_ref(), &[self.config.config_bump]]];

        let cpi_ctx = CpiContext::new_with_signer(self.token_program.key(), tx, signer_seeds);

        mint_to_checked(cpi_ctx, mint_amount, self.lp_token.decimals)
    }
}
