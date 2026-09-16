use {
    amm_video::Config,
    anchor_lang::{prelude::msg, AccountDeserialize, Key},
    anchor_spl::{associated_token, token::TokenAccount},
    litesvm::LiteSVM,
    litesvm_token::CreateMint,
    solana_keypair::Keypair,
    solana_message::{Instruction, Message, VersionedMessage},
    solana_pubkey::Pubkey,
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

mod ix_handlers;
use ix_handlers::*;

fn send(
    svm: &mut LiteSVM,
    ixs: &[Instruction],
    payer: &Keypair,
    signers: &[&Keypair],
) -> litesvm::types::TransactionResult {
    svm.expire_blockhash();
    let blockhash = svm.latest_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &blockhash);
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    svm.send_transaction(tx)
}

// Setup function to initialize LiteSVM and create a payer keypair
fn setup() -> (
    LiteSVM,
    Keypair,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
    Pubkey,
) {
    let program_id = amm_video::id();
    let payer = Keypair::new();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!("../../../target/deploy/amm_video.so");
    svm.add_program(program_id, bytes).unwrap();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();

    // Create two mints (Mint A and Mint B) with 6 decimal places and the maker as the authority
    // This done using litesvm-token's CreateMint utility which creates the mint in the LiteSVM environment
    let mint_x = CreateMint::new(&mut svm, &payer)
        .decimals(6)
        .authority(&payer.pubkey())
        .send()
        .unwrap();

    let mint_y = CreateMint::new(&mut svm, &payer)
        .decimals(6)
        .authority(&payer.pubkey())
        .send()
        .unwrap();

    let config =
        Pubkey::find_program_address(&[b"config", &123u64.to_le_bytes()], &amm_video::id()).0;
    let mint_lp = Pubkey::find_program_address(&[b"lp", config.as_ref()], &amm_video::id()).0;

    // Derive the PDA for the vault associated token account using the config PDA and Mint A
    let vault_x = associated_token::get_associated_token_address(&config, &mint_x);
    let vault_y = associated_token::get_associated_token_address(&config, &mint_y);

    (
        svm, payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y,
    )
}

#[test]
fn test_initialize() {
    let (mut svm, payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y) = setup();

    let instruction = create_initialise_ix(
        &mut svm, &payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y,
    );
    let res = send(&mut svm, &[instruction], &payer, &[&payer]);
    // msg!("{:?}", res.unwrap());
    assert!(res.is_ok());
    assert!(svm.get_account(&vault_x).is_some());
    assert!(svm.get_account(&vault_y).is_some());
    assert!(svm.get_account(&mint_lp).is_some());
    let mut config_account = svm.get_account(&config);
    assert!(config_account.is_some());
    let mut config_account = config_account.unwrap();

    let config_data: Config = Config::try_deserialize(&mut config_account.data.as_slice()).unwrap();

    let maker = payer.pubkey();
    assert_eq!(123u64, config_data.seed);
    assert_eq!(maker, config_data.authority.unwrap());
}

#[test]
pub fn test_deposit() {
    let (mut svm, payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y) = setup();
    let init_ix = create_initialise_ix(
        &mut svm, &payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y,
    );

    let deposit_ix = create_deposit_ix(
        &mut svm, &payer, mint_x, mint_y, mint_lp, config, vault_x, vault_y,
    );

    let res = send(&mut svm, &[init_ix, deposit_ix], &payer, &[&payer]);
    println!("Deposit transaction result: {:?}", res);
    assert!(res.is_ok());

    let user_ata_lp = associated_token::get_associated_token_address(&payer.pubkey(), &mint_lp);

    let vault_x_account = svm.get_account(&vault_x).unwrap();
    let vault_y_account = svm.get_account(&vault_y).unwrap();
    let user_ata_lp_account = svm.get_account(&user_ata_lp).unwrap();

    let vault_x_data: TokenAccount =
        TokenAccount::try_deserialize(&mut vault_x_account.data.as_slice()).unwrap();
    let vault_y_data: TokenAccount =
        TokenAccount::try_deserialize(&mut vault_y_account.data.as_slice()).unwrap();
    let user_ata_lp_data: TokenAccount =
        TokenAccount::try_deserialize(&mut user_ata_lp_account.data.as_slice()).unwrap();

    assert_eq!(vault_x_data.amount, 200_000_000);
    assert_eq!(vault_y_data.amount, 200_000_000);
    assert_eq!(user_ata_lp_data.amount, 100_000_000);
}

// #[test]
// pub fn test_withdraw() {
//     let (mut svm, payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y) = setup();
//     let init_ix = create_initialise_ix(
//         &mut svm, &payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y,
//     );

//     let deposit_ix = create_deposit_ix(
//         &mut svm, &payer, mint_x, mint_y, mint_lp, config, vault_x, vault_y,
//     );

//     let withdraw_ix = create_withdraw_ix(
//         &mut svm, &payer, mint_x, mint_y, mint_lp, config, vault_x, vault_y,
//     );
//     let res = send(
//         &mut svm,
//         &[init_ix, deposit_ix, withdraw_ix],
//         &payer,
//         &[&payer],
//     );
//     assert!(res.is_ok());
// }


// Swap tests

const FEE_BPS: u16 = 123;
const GENESIS_X: u64 = 200_000_000;
const GENESIS_Y: u64 = 200_000_000;

fn setup_pool_with_liquidity() -> (
    LiteSVM,
    Keypair, // payer / maker
    Keypair, // swapper
    Pubkey, // treasury
    Pubkey, // mint_x
    Pubkey, // mint_y
    Pubkey, // config
    Pubkey, // mint_lp
    Pubkey, // vault_x
    Pubkey, // vault_y
    Pubkey, // swapper_x
    Pubkey, // swapper_y
) {
    let (mut svm, payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y) = setup();
    let treasury = payer.pubkey(); // maker is payer

    let init_ix = create_initialise_ix(
        &mut svm, &payer, mint_x, mint_y, config, mint_lp, vault_x, vault_y,
    );
    let deposit_ix = create_deposit_ix(
        &mut svm, &payer, mint_x, mint_y, mint_lp, config, vault_x, vault_y,
    );

    let res = send(&mut svm, &[init_ix, deposit_ix], &payer, &[&payer]);
    assert!(res.is_ok());

    // Create a separate swapper keypair to avoid ConstraintDuplicateMutableAccount
    let swapper = Keypair::new();
    svm.airdrop(&swapper.pubkey(), 1_000_000_000).unwrap();

    use litesvm_token::{CreateAssociatedTokenAccount, MintTo};

    let swapper_x = CreateAssociatedTokenAccount::new(&mut svm, &payer, &mint_x)
        .owner(&swapper.pubkey())
        .send()
        .unwrap();
    MintTo::new(&mut svm, &payer, &mint_x, &swapper_x, 1000000000)
        .send()
        .unwrap();

    let swapper_y = CreateAssociatedTokenAccount::new(&mut svm, &payer, &mint_y)
        .owner(&swapper.pubkey())
        .send()
        .unwrap();
    MintTo::new(&mut svm, &payer, &mint_y, &swapper_y, 1000000000)
        .send()
        .unwrap();

    (svm, payer, swapper, treasury, mint_x, mint_y, config, mint_lp, vault_x, vault_y, swapper_x, swapper_y)
}

fn token_balance(svm: &LiteSVM, account: &Pubkey) -> u64 {
    if let Some(acc) = svm.get_account(account) {
        let data: TokenAccount = TokenAccount::try_deserialize(&mut acc.data.as_slice()).unwrap();
        data.amount
    } else {
        0
    }
}

#[test]
pub fn test_swap_x_for_y() {
    let (mut svm, _payer, swapper, treasury, mint_x, mint_y, config, mint_lp, vault_x, vault_y, user_x, user_y) =
        setup_pool_with_liquidity();

    let treasury_x = associated_token::get_associated_token_address(&treasury, &mint_x);
    let amount_in = 10_000_000u64;

    let swap_ix = create_swap_ix(
        &swapper, mint_x, mint_y, mint_lp, config, vault_x, vault_y, treasury, true, amount_in, 1,
    );
    let res = send(&mut svm, &[swap_ix], &swapper, &[&swapper]);
    assert!(res.is_ok(), "swap X->Y failed: {:?}", res);

    let expected_treasury_fee = (amount_in * FEE_BPS as u64 / 10_000) / 2;
    // The treasury is the payer, who started with 1B and deposited 200M (leaving 800M)
    assert_eq!(token_balance(&svm, &treasury_x), 800_000_000 + expected_treasury_fee);
    assert_eq!(
        token_balance(&svm, &vault_x),
        GENESIS_X + amount_in - expected_treasury_fee
    );
    assert!(token_balance(&svm, &user_y) > 1_000_000_000);
    assert_eq!(
        token_balance(&svm, &user_x),
        1_000_000_000 - amount_in
    );
}

#[test]
pub fn test_swap_y_for_x() {
    let (mut svm, _payer, swapper, treasury, mint_x, mint_y, config, mint_lp, vault_x, vault_y, user_x, user_y) =
        setup_pool_with_liquidity();

    let treasury_y = associated_token::get_associated_token_address(&treasury, &mint_y);
    let amount_in = 10_000_000u64;

    let swap_ix = create_swap_ix(
        &swapper, mint_x, mint_y, mint_lp, config, vault_x, vault_y, treasury, false, amount_in, 1,
    );
    let res = send(&mut svm, &[swap_ix], &swapper, &[&swapper]);
    assert!(res.is_ok(), "swap Y->X failed: {:?}", res);

    let expected_treasury_fee = (amount_in * FEE_BPS as u64 / 10_000) / 2;
    // The treasury is the payer, who started with 1B and deposited 200M (leaving 800M)
    assert_eq!(token_balance(&svm, &treasury_y), 800_000_000 + expected_treasury_fee);
    assert_eq!(
        token_balance(&svm, &vault_y),
        GENESIS_Y + amount_in - expected_treasury_fee
    );
    assert!(token_balance(&svm, &user_x) > 1_000_000_000);
    assert_eq!(
        token_balance(&svm, &user_y),
        1_000_000_000 - amount_in
    );
}

#[test]
fn test_swap_rejects_slippage() {
    let (mut svm, _payer, swapper, treasury, mint_x, mint_y, config, mint_lp, vault_x, vault_y, _user_x, _user_y) =
        setup_pool_with_liquidity();

    let bad_ix = create_swap_ix(
        &swapper,
        mint_x,
        mint_y,
        mint_lp,
        config,
        vault_x,
        vault_y,
        treasury,
        true,
        10_000_000,
        u64::MAX,
    );
    let res = send(&mut svm, &[bad_ix], &swapper, &[&swapper]);
    assert!(res.is_err(), "swap breaching min_amount_out must fail");
}

#[test]
fn test_swap_rejects_zero_amount() {
    let (mut svm, _payer, swapper, treasury, mint_x, mint_y, config, mint_lp, vault_x, vault_y, _user_x, _user_y) =
        setup_pool_with_liquidity();

    let bad_ix = create_swap_ix(
        &swapper, mint_x, mint_y, mint_lp, config, vault_x, vault_y, treasury, true, 0, 0,
    );
    let res = send(&mut svm, &[bad_ix], &swapper, &[&swapper]);
    assert!(res.is_err(), "zero-amount swap must fail");
}
