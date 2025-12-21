mod common;
use common::*;

use intear_dex::internal_asset_operations::AccountOrDexId;
use intear_dex_types::{AssetId, DexId};
use near_sdk::serde_json::json;
use near_sdk::{
    NearToken,
    base64::{Engine, prelude::BASE64_STANDARD},
    json_types::{Base64VecU8, U128},
    near,
};

#[tokio::test]
async fn test_regular_flow() {
    let wasms = get_compiled_wasms().await;
    let contract_wasm = &wasms.contract_wasm;
    let dex_wasm = &wasms.otc_dex_wasm;
    let ft_wasm = &wasms.ft_wasm;
    let sandbox = near_workspaces::sandbox().await.unwrap();
    let dex_engine_contract = sandbox.dev_deploy(contract_wasm).await.unwrap();
    let ft = sandbox.dev_deploy(ft_wasm).await.unwrap();
    let dex_deployer_account = sandbox.dev_create_account().await.unwrap();

    let result = ft
        .call("new_default_meta")
        .args_json(json!({
            "owner_id": dex_deployer_account.id(),
            "total_supply": U128(NearToken::from_near(100_000_000_000).as_yoctonear()),
        }))
        .transact()
        .await
        .unwrap();
    assert!(result.is_success());

    let dex_id_string = "dex".to_string();
    let dex_id = DexId {
        deployer: dex_deployer_account.id().clone(),
        id: dex_id_string.clone(),
    };

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "dex_storage_deposit")
        .max_gas()
        .deposit(NearToken::from_near(5))
        .args_json(json!({
            "dex_id": dex_id,
        }))
        .transact()
        .await
        .unwrap();
    assert!(result.is_success());

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "storage_deposit")
        .max_gas()
        .deposit(NearToken::from_near(5))
        .args_json(json!({
            "dex_id": dex_id,
        }))
        .transact()
        .await
        .unwrap();
    assert!(result.is_success());

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "deploy_dex_code")
        .max_gas()
        .deposit(NearToken::from_yoctonear(1))
        .args_json(json!({
            "last_part_of_id": dex_id_string,
            "code_base64": BASE64_STANDARD.encode(dex_wasm),
        }))
        .transact()
        .await
        .unwrap();
    assert!(result.is_success());

    let initial_near_balance = dex_deployer_account.view_account().await.unwrap().balance;
    let mut total_near_burnt = NearToken::from_yoctonear(0);
    assert_inner_asset_balance(
        &dex_engine_contract,
        AccountOrDexId::Account(dex_deployer_account.id().clone()),
        AssetId::Near,
        None,
    )
    .await
    .unwrap();

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "register_assets")
        .max_gas()
        .deposit(NearToken::from_yoctonear(1))
        .args_json(json!({
            "asset_ids": [AssetId::Near, AssetId::Nep141(ft.id().clone())],
            "for": AccountOrDexId::Dex(dex_id.clone()),
        }))
        .transact()
        .await
        .unwrap();
    assert!(result.is_success());
    track_tokens_burnt(&result, &mut total_near_burnt);

    assert_inner_asset_balance(
        &dex_engine_contract,
        AccountOrDexId::Account(dex_deployer_account.id().clone()),
        AssetId::Near,
        None,
    )
    .await
    .unwrap();
    assert_inner_asset_balance(
        &dex_engine_contract,
        AccountOrDexId::Account(dex_deployer_account.id().clone()),
        AssetId::Nep141(ft.id().clone()),
        None,
    )
    .await
    .unwrap();

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "register_assets")
        .max_gas()
        .deposit(NearToken::from_yoctonear(1))
        .args_json(json!({
            "asset_ids": [AssetId::Near, AssetId::Nep141(ft.id().clone())],
            "for": AccountOrDexId::Account(dex_deployer_account.id().clone()),
        }))
        .transact()
        .await
        .unwrap();
    assert!(result.is_success());
    track_tokens_burnt(&result, &mut total_near_burnt);

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "deposit_near")
        .max_gas()
        .deposit(NearToken::from_near(20))
        .args_json(json!({}))
        .transact()
        .await
        .unwrap();
    assert!(result.is_success());
    track_tokens_burnt(&result, &mut total_near_burnt);
    assert_near_balance(
        &dex_deployer_account,
        initial_near_balance
            .saturating_sub(NearToken::from_near(20))
            .saturating_sub(total_near_burnt)
            .saturating_sub(NearToken::from_yoctonear(2)),
    )
    .await
    .unwrap();
    assert_inner_asset_balance(
        &dex_engine_contract,
        AccountOrDexId::Account(dex_deployer_account.id().clone()),
        AssetId::Near,
        Some(U128(NearToken::from_near(20).as_yoctonear())),
    )
    .await
    .unwrap();

    let result = dex_deployer_account
        .call(ft.id(), "storage_deposit")
        .max_gas()
        .deposit("0.00125 NEAR".parse().unwrap())
        .args_json(json!({
            "account_id": dex_engine_contract.id(),
        }))
        .transact()
        .await
        .unwrap();
    assert!(result.is_success());
    track_tokens_burnt(&result, &mut total_near_burnt);

    let initial_ft_balance = ft
        .view("ft_balance_of")
        .args_json(json!({
            "account_id": dex_deployer_account.id(),
        }))
        .await
        .unwrap()
        .json::<U128>()
        .unwrap();
    assert_inner_asset_balance(
        &dex_engine_contract,
        AccountOrDexId::Account(dex_deployer_account.id().clone()),
        AssetId::Nep141(ft.id().clone()),
        Some(U128(0)),
    )
    .await
    .unwrap();
    let result = dex_deployer_account
        .call(ft.id(), "ft_transfer_call")
        .max_gas()
        .deposit(NearToken::from_yoctonear(1))
        .args_json(json!({
            "receiver_id": dex_engine_contract.id(),
            "amount": U128(1_000_000_000),
            "msg": "",
        }))
        .transact()
        .await
        .unwrap();
    assert!(result.is_success());
    track_tokens_burnt(&result, &mut total_near_burnt);
    assert_ft_balance(
        &dex_deployer_account,
        ft.clone(),
        U128(initial_ft_balance.0 - 1_000_000_000),
    )
    .await
    .unwrap();
    assert_inner_asset_balance(
        &dex_engine_contract,
        AccountOrDexId::Account(dex_deployer_account.id().clone()),
        AssetId::Nep141(ft.id().clone()),
        Some(U128(1_000_000_000)),
    )
    .await
    .unwrap();

    #[near(serializers=[borsh])]
    struct StorageDepositArgs;
    assert_inner_asset_balance(
        &dex_engine_contract,
        AccountOrDexId::Account(dex_deployer_account.id().clone()),
        AssetId::Near,
        Some(U128(NearToken::from_near(20).as_yoctonear())),
    )
    .await
    .unwrap();

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "dex_call")
        .max_gas()
        .deposit(NearToken::from_yoctonear(1))
        .args_json(json!({
            "dex_id": dex_id.clone(),
            "method": "storage_deposit",
            "args": BASE64_STANDARD.encode(near_sdk::borsh::to_vec(&StorageDepositArgs).unwrap()),
            "attached_assets": {
                "near": U128(NearToken::from_millinear(10).as_yoctonear()),
            },
        }))
        .transact()
        .await
        .unwrap();
    assert!(result.is_success());
    track_tokens_burnt(&result, &mut total_near_burnt);
    assert!(result.json::<Base64VecU8>().unwrap().0.is_empty());

    // TODO: Add more
}
