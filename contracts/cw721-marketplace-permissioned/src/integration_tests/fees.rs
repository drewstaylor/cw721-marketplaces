#![cfg(test)]
use cosmwasm_std::{Addr, Coin, Uint128};
use cw_multi_test::Executor;

use cw20::{BalanceResponse, Cw20ExecuteMsg, Cw20QueryMsg, Expiration};
use cw721::OwnerOfResponse;
use cw721_base::{
    msg::ExecuteMsg as Cw721ExecuteMsg, msg::QueryMsg as Cw721QueryMsg, Extension, MintMsg,
};
use cw721_marketplace_utils::prelude::PageResult;
use rstest::rstest;

use crate::integration_tests::util::{
    bank_query, create_cw20, create_cw721, create_swap_with_fees, mint_native, mock_app, query,
};
use crate::msg::{ExecuteMsg, FinishSwapMsg, QueryMsg, SwapMsg, WithdrawMsg};
use crate::state::SwapType;

static DENOM: &str = "aarch";

// Swap buyer pays with ARCH
// ensuring marketplace fees are respected
#[rstest]
#[case(10, false, 20, 2)]
#[case(100, true, 22, 22)]
#[case(1000, true, 15, 150)]
#[case(10000, true, 30, 3000)]
#[case(100000, true, 5, 5000)]
#[case(u128::MAX, false, 10, 34028236692093846346337460743176821145)]
fn test_fees_native(
    #[case] amount: u128,
    #[case] in_arch: bool,
    #[case] fee_split: u64,
    #[case] expected: u128,
) {
    let mut app = mock_app();

    let amount = if in_arch {
        amount * 10u128.pow(18)
    } else {
        amount
    };
    let expected = if in_arch {
        expected * 10u128.pow(18)
    } else {
        expected
    };

    // Swap owner deploys
    let swap_admin = Addr::unchecked("swap_deployer");
    // cw721_owner owns the cw721
    let cw721_owner = Addr::unchecked("original_owner");
    // arch_owner owns ARCH
    let arch_owner = Addr::unchecked("arch_owner");

    // cw721_owner creates the cw721
    let nft = create_cw721(&mut app, &cw721_owner);

    // swap_admin creates the swap contract
    let swap = create_swap_with_fees(&mut app, &swap_admin, nft.clone(), fee_split); // 1% Marketplace fees
    let swap_inst = swap.clone();

    // Mint native to `arch_owner`
    mint_native(
        &mut app,
        arch_owner.to_string(),
        Uint128::from(amount), // 10 ARCH as aarch
    );

    // cw721_owner mints a cw721
    let token_id = "petrify".to_string();
    let token_uri = "https://www.merriam-webster.com/dictionary/petrify".to_string();
    let mint_msg = Cw721ExecuteMsg::Mint(MintMsg::<Extension> {
        token_id: token_id.clone(),
        owner: cw721_owner.to_string(),
        token_uri: Some(token_uri.clone()),
        extension: None,
    });
    let _res = app
        .execute_contract(cw721_owner.clone(), nft.clone(), &mint_msg, &[])
        .unwrap();

    // Create a SwapMsg for creating / finishing a swap
    let creation_msg = SwapMsg {
        id: "firstswap".to_string(),
        cw721: nft.clone(),
        payment_token: None,
        token_id: token_id.clone(),
        expires: Expiration::from(cw20::Expiration::AtHeight(384798573487439743)),
        price: Uint128::from(amount), // 1 ARCH as aarch
        swap_type: SwapType::Sale,
    };
    let finish_msg = FinishSwapMsg {
        id: creation_msg.id.clone(),
    };

    // Seller (cw721_owner) must approve the swap contract to spend their NFT
    let nft_approve_msg = Cw721ExecuteMsg::Approve::<Extension> {
        spender: swap.to_string(),
        token_id: token_id.clone(),
        expires: None,
    };
    let _res = app
        .execute_contract(cw721_owner.clone(), nft.clone(), &nft_approve_msg, &[])
        .unwrap();

    // cw721 seller (cw721_owner) creates a swap
    let _res = app
        .execute_contract(
            cw721_owner.clone(),
            swap_inst.clone(),
            &ExecuteMsg::Create(creation_msg),
            &[],
        )
        .unwrap();

    // Buyer purchases cw721, paying 1 ARCH and consuming the swap
    let _res = app
        .execute_contract(
            arch_owner.clone(),
            swap_inst.clone(),
            &ExecuteMsg::Finish(finish_msg),
            &[Coin {
                denom: String::from(DENOM),
                amount: Uint128::from(amount),
            }],
        )
        .unwrap();

    // arch_owner has received the NFT
    let owner_query: OwnerOfResponse = query(
        &mut app,
        nft.clone(),
        Cw721QueryMsg::OwnerOf {
            token_id: token_id.clone(),
            include_expired: None,
        },
    )
    .unwrap();
    assert_eq!(owner_query.owner, arch_owner);

    // Swap was removed from storage
    let swap_query: PageResult = query(
        &mut app,
        swap_inst.clone(),
        QueryMsg::ListingsOfToken {
            token_id: token_id,
            cw721: nft,
            swap_type: Some(SwapType::Sale),
            page: Some(1_u32),
            limit: None,
        },
    )
    .unwrap();
    assert_eq!(swap_query.total, 0);

    // cw721_owner has received the ARCH amount
    let balance_query: Coin = bank_query(&mut app, &cw721_owner);
    assert_eq!(balance_query.amount, Uint128::from(amount - expected));

    // swap_inst has retained its fee
    let balance_query: Coin = bank_query(&mut app, &swap_inst);
    assert_eq!(balance_query.amount, Uint128::from(expected));

    // swap_admin can withdraw native fees
    let withdraw_msg = WithdrawMsg {
        amount: Uint128::from(expected),
        denom: String::from(DENOM),
        payment_token: None,
    };
    let _res = app
        .execute_contract(
            swap_admin.clone(),
            swap_inst.clone(),
            &ExecuteMsg::Withdraw(withdraw_msg),
            &[],
        )
        .unwrap();

    // swap_admin received its withdrawn fees
    let balance_query: Coin = bank_query(&mut app, &swap_admin);
    assert_eq!(balance_query.amount, Uint128::from(expected));
}

// Receive cw20 tokens and release upon approval
// ensuring marketplace fees are respected
#[rstest]
#[case(10, false, 20, 2)]
#[case(100, true, 22, 22)]
#[case(1000, true, 15, 150)]
#[case(10000, true, 30, 3000)]
#[case(100000, true, 5, 5000)]
#[case(u128::MAX, false, 10, 34028236692093846346337460743176821145)]
fn test_fees_cw20(
    #[case] amount: u128,
    #[case] in_arch: bool,
    #[case] fee_split: u64,
    #[case] expected: u128,
) {
    let mut app = mock_app();

    let amount = if in_arch {
        amount * 10u128.pow(18)
    } else {
        amount
    };
    let expected = if in_arch {
        expected * 10u128.pow(18)
    } else {
        expected
    };

    // Swap owner deploys
    let swap_admin = Addr::unchecked("swap_deployer");
    // cw721_owner owns the cw721
    let cw721_owner = Addr::unchecked("original_owner");
    // cw20_owner owns the cw20
    let cw20_owner = Addr::unchecked("cw20_owner");

    // cw721_owner creates the cw721
    let nft = create_cw721(&mut app, &cw721_owner);

    // swap_admin creates the swap contract
    let swap = create_swap_with_fees(&mut app, &swap_admin, nft.clone(), fee_split); // 1% Marketplace fees
    let swap_inst = swap.clone();

    // cw20_owner creates a cw20 coin
    let cw20 = create_cw20(
        &mut app,
        &cw20_owner,
        "testcw".to_string(),
        "tscw".to_string(),
        Uint128::from(amount),
    );
    let cw20_inst = cw20.clone();

    // cw721_owner mints a cw721
    let token_id = "petrify".to_string();
    let token_uri = "https://www.merriam-webster.com/dictionary/petrify".to_string();
    let mint_msg = Cw721ExecuteMsg::Mint(MintMsg::<Extension> {
        token_id: token_id.clone(),
        owner: cw721_owner.to_string(),
        token_uri: Some(token_uri.clone()),
        extension: None,
    });
    let _res = app
        .execute_contract(cw721_owner.clone(), nft.clone(), &mint_msg, &[])
        .unwrap();

    // Create a SwapMsg for creating / finishing a swap
    let creation_msg = SwapMsg {
        id: "firstswap".to_string(),
        cw721: nft.clone(),
        payment_token: Some(Addr::unchecked(cw20.clone())),
        token_id: token_id.clone(),
        expires: Expiration::from(cw20::Expiration::AtHeight(384798573487439743)),
        price: Uint128::from(amount),
        swap_type: SwapType::Sale,
    };
    let finish_msg = FinishSwapMsg {
        id: creation_msg.id.clone(),
    };

    // Seller (cw721_owner) must approve the swap contract to spend their NFT
    let nft_approve_msg = Cw721ExecuteMsg::Approve::<Extension> {
        spender: swap.to_string(),
        token_id: token_id.clone(),
        expires: None,
    };
    let _res = app
        .execute_contract(cw721_owner.clone(), nft.clone(), &nft_approve_msg, &[])
        .unwrap();

    // cw721 seller (cw721_owner) creates a swap
    let _res = app
        .execute_contract(
            cw721_owner.clone(),
            swap_inst.clone(),
            &ExecuteMsg::Create(creation_msg),
            &[],
        )
        .unwrap();

    // cw721 buyer (cw20_owner) must approve swap contract to spend their cw20
    let cw20_approve_msg = Cw20ExecuteMsg::IncreaseAllowance {
        spender: swap.to_string(),
        amount: Uint128::from(amount),
        expires: None,
    };
    let _res = app
        .execute_contract(cw20_owner.clone(), cw20.clone(), &cw20_approve_msg, &[])
        .unwrap();

    // Buyer purchases cw721, consuming the swap
    let _res = app
        .execute_contract(
            cw20_owner.clone(),
            swap_inst.clone(),
            &ExecuteMsg::Finish(finish_msg),
            &[],
        )
        .unwrap();

    // cw20_owner has received the NFT
    let owner_query: OwnerOfResponse = query(
        &mut app,
        nft.clone(),
        Cw721QueryMsg::OwnerOf {
            token_id: token_id,
            include_expired: None,
        },
    )
    .unwrap();
    assert_eq!(owner_query.owner, cw20_owner);

    // cw721_owner has received the cw20 amount (minus fees)
    let balance_query: BalanceResponse = query(
        &mut app,
        cw20_inst.clone(),
        Cw20QueryMsg::Balance {
            address: cw721_owner.to_string(),
        },
    )
    .unwrap();
    assert_eq!(balance_query.balance, Uint128::from(amount - expected));

    // swap_inst has retained its fee
    let balance_query: BalanceResponse = query(
        &mut app,
        cw20_inst.clone(),
        Cw20QueryMsg::Balance {
            address: swap_inst.clone().to_string(),
        },
    )
    .unwrap();
    assert_eq!(balance_query.balance, Uint128::from(expected));

    // swap_admin can withdraw cw20 fees
    let withdraw_msg = WithdrawMsg {
        amount: Uint128::from(expected),
        denom: String::from(DENOM),
        payment_token: Some(cw20_inst.clone()),
    };
    let _res = app
        .execute_contract(
            swap_admin.clone(),
            swap_inst,
            &ExecuteMsg::Withdraw(withdraw_msg),
            &[],
        )
        .unwrap();

    // swap_admin received its withdrawn cw20 fees
    let balance_query: BalanceResponse = query(
        &mut app,
        cw20_inst,
        Cw20QueryMsg::Balance {
            address: swap_admin.to_string(),
        },
    )
    .unwrap();
    assert_eq!(balance_query.balance, Uint128::from(expected));
}
