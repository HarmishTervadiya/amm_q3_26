use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};
use constant_product_curve::{ConstantProduct, LiquidityPair};

use crate::{error::AmmError, state::Config};

#[derive(Accounts)]
pub struct Swap<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,

    pub mint_x: Box<InterfaceAccount<'info, Mint>>,
    pub mint_y: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        has_one = mint_x,
        has_one = mint_y,
        seeds = [b"config", config.seed.to_le_bytes().as_ref()],
        bump = config.config_bump
    )]
    pub config: Box<Account<'info, Config>>,

    #[account(
        mut,
        seeds = [b"lp", config.key().as_ref()],
        bump = config.lp_bump,
    )]
    pub mint_lp: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = config,
    )]
    pub vault_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = config,
    )]
    pub vault_y: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_x,
        associated_token::authority = signer,
    )]
    pub user_ata_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_y,
        associated_token::authority = signer,
    )]
    pub user_ata_y: Box<InterfaceAccount<'info, TokenAccount>>,

    /// CHECK: Protocol fee recipient stored on the config. Owns the treasury ATAs below.
    #[account(address = config.treasury @ AmmError::InvalidTreasury)]
    pub treasury: UncheckedAccount<'info>,

    #[account(
        init_if_needed,
        payer = signer,
        associated_token::mint = mint_x,
        associated_token::authority = treasury,
    )]
    pub treasury_x: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init_if_needed,
        payer = signer,
        associated_token::mint = mint_y,
        associated_token::authority = treasury,
    )]
    pub treasury_y: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub associated_token_program: Program<'info, AssociatedToken>,
}

impl<'info> Swap<'info> {
    pub fn swap(&mut self, is_x: bool, amount: u64, min: u64) -> Result<()> {
        require!(!self.config.locked, AmmError::PoolLocked);
        require!(amount > 0, AmmError::InvalidAmount);
        let mut curve = ConstantProduct::init(
            self.vault_x.amount,
            self.vault_y.amount,
            self.mint_lp.supply,
            self.config.fee,
            Some(6),
        )
        .unwrap();

        let p = match is_x {
            true => LiquidityPair::X,
            false => LiquidityPair::Y,
        };

        let swap_result: constant_product_curve::SwapResult = curve
            .swap(p, amount, min)
            .map_err(|_| AmmError::SlippageExceeded)?;

        // Half of the swap fee goes to the protocol treasury, the other
        // half stays in the pool as a reward for LPs.
        let treasury_fee = swap_result.fee / 2;
        let vault_deposit = swap_result
            .deposit
            .checked_sub(treasury_fee)
            .ok_or(AmmError::Underflow)?;

        // User input is split: pool share -> vault, protocol share -> treasury ATA.
        // The fee is denominated in the input mint, so it is routed to the
        // matching treasury ATA (X -> treasury_x, Y -> treasury_y).
        let (user_from, vault_to, treasury_to, mint_info, decimals) = match is_x {
            true => (
                self.user_ata_x.to_account_info(),
                self.vault_x.to_account_info(),
                self.treasury_x.to_account_info(),
                self.mint_x.to_account_info(),
                self.mint_x.decimals,
            ),
            false => (
                self.user_ata_y.to_account_info(),
                self.vault_y.to_account_info(),
                self.treasury_y.to_account_info(),
                self.mint_y.to_account_info(),
                self.mint_y.decimals,
            ),
        };
        self.deposit_to(user_from.clone(), vault_to, mint_info.clone(), vault_deposit, decimals)?;
        if treasury_fee > 0 {
            self.deposit_to(user_from, treasury_to, mint_info, treasury_fee, decimals)?;
        }
        self.withdraw_tokens(is_x, swap_result.withdraw)
    }

    /// Transfer `amount` of the input mint from the user to `to`.
    pub fn deposit_to(
        &mut self,
        from: AccountInfo<'info>,
        to: AccountInfo<'info>,
        mint: AccountInfo<'info>,
        amount: u64,
        decimals: u8,
    ) -> Result<()> {
        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from,
                    to,
                    mint,
                    authority: self.signer.to_account_info(),
                },
            ),
            amount,
            decimals,
        )
    }

    pub fn withdraw_tokens(&mut self, is_x: bool, amount: u64) -> Result<()> {
        let (from, to, mint, decimals) = match is_x {
            true => (
                self.vault_y.to_account_info(),
                self.user_ata_y.to_account_info(),
                self.mint_y.to_account_info(),
                self.mint_y.decimals,
            ),
            false => (
                self.vault_x.to_account_info(),
                self.user_ata_x.to_account_info(),
                self.mint_x.to_account_info(),
                self.mint_x.decimals,
            ),
        };

        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from,
                    to,
                    mint,
                    authority: self.config.to_account_info(),
                },
                &[&[
                    b"config",
                    &self.config.seed.to_le_bytes(),
                    &[self.config.config_bump],
                ]],
            ),
            amount,
            decimals,
        )
    }
}