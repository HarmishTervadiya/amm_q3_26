use {
    anchor_lang::{
        solana_program::instruction::Instruction, system_program::ID as SYSTEM_PROGRAM_ID,
        InstructionData, ToAccountMetas,
    },
    anchor_spl::associated_token::{self, ID as ASSOCIATED_TOKEN_PROGRAM_ID},
    litesvm::LiteSVM,
    litesvm_token::{spl_token::ID as TOKEN_PROGRAM_ID, CreateAssociatedTokenAccount, MintTo},
    solana_keypair::Keypair,
    solana_pubkey::Pubkey,
    solana_signer::Signer,
};

pub fn create_deposit_ix(
    mut svm: &mut LiteSVM,
    payer: &Keypair,
    mint_x: Pubkey,
    mint_y: Pubkey,
    mint_lp: Pubkey,
    config: Pubkey,
    vault_x: Pubkey,
    vault_y: Pubkey,
) -> Instruction {
    
}
