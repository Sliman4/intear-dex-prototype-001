use near_sdk::{
    NearToken,
    base64::{Engine, prelude::BASE64_STANDARD},
    json_types::U128,
};
use serde_json::json;
use tear_sdk::{AssetId, DexId, SwapRequestAmount};
use tokio::process::Command;

#[tokio::test]
async fn test_contract_is_operational() -> Result<(), Box<dyn std::error::Error>> {
    let contract_wasm = near_workspaces::compile_project("./").await?;

    assert!(
        Command::new("cargo")
            .args([
                "build",
                "--package=example-dex",
                "--release",
                "--target",
                "wasm32-unknown-unknown"
            ])
            .status()
            .await
            .expect("Failed to run cargo build")
            .success()
    );

    let dex_wasm = std::fs::read("./target/wasm32-unknown-unknown/release/example_dex.wasm")
        .expect("Failed to read wasm file");
    let ft_wasm = include_bytes!("./ft.wasm");

    test_basics_on(&contract_wasm, &dex_wasm, ft_wasm).await?;
    Ok(())
}

async fn test_basics_on(
    contract_wasm: &[u8],
    dex_wasm: &[u8],
    ft_wasm: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = near_workspaces::sandbox().await?;
    let dex_engine_contract = sandbox.dev_deploy(contract_wasm).await?;
    let ft = sandbox.dev_deploy(ft_wasm).await?;
    let dex_deployer_account = sandbox.dev_create_account().await?;

    let result = ft
        .call("new_default_meta")
        .args_json(json!({
            "owner_id": dex_deployer_account.id(),
            "total_supply": U128(NearToken::from_near(1_000_000_000).as_yoctonear()),
        }))
        .transact()
        .await?;
    assert!(result.is_success());

    let dex_id_string = "example".to_string();
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
        .await?;
    assert!(result.is_success());

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "storage_deposit")
        .max_gas()
        .deposit(NearToken::from_near(5))
        .args_json(json!({
            "dex_id": dex_id,
        }))
        .transact()
        .await?;
    assert!(result.is_success());

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "deploy_code")
        .max_gas()
        .deposit(NearToken::from_yoctonear(1))
        .args_json(json!({
            "last_part_of_id": dex_id_string,
            "code_base64": BASE64_STANDARD.encode(dex_wasm),
        }))
        .transact()
        .await?;
    assert!(result.is_success());

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "near_deposit")
        .max_gas()
        .deposit(NearToken::from_near(20))
        .args_json(json!({}))
        .transact()
        .await?;
    assert!(result.is_success());

    let result = dex_deployer_account
        .call(ft.id(), "storage_deposit")
        .max_gas()
        .deposit("0.00125 NEAR".parse().unwrap())
        .args_json(json!({
            "account_id": dex_engine_contract.id(),
        }))
        .transact()
        .await?;
    assert!(result.is_success());

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
        .await?;
    println!("{result:#?}");
    assert!(result.is_success());

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "dex_call")
        .max_gas()
        .deposit(NearToken::from_yoctonear(1))
        .args_json(json!({
            "dex_id": dex_id,
            "method": "create_pool",
            "args": {
                "assets": (AssetId::Near, AssetId::Nep141(ft.id().clone())),
            },
            "attached_assets": {
                "near": NearToken::from_millinear(10),
            },
        }))
        .transact()
        .await?;
    assert!(result.is_success());

    let result = dex_deployer_account
        .call(dex_engine_contract.id(), "dex_call")
        .max_gas()
        .deposit(NearToken::from_yoctonear(1))
        .args_json(json!({
            "dex_id": dex_id,
            "method": "add_liquidity",
            "args": {
                "pool_id": "0",
            },
            "attached_assets": {
                "near": U128(NearToken::from_near(1).as_yoctonear()),
                format!("nep141:{}", ft.id()): U128(1_000_000),
            },
        }))
        .transact()
        .await?;
    println!("{result:#?}");
    assert!(result.is_success());

    let outcome = dex_deployer_account
        .call(dex_engine_contract.id(), "swap_one_dex")
        .max_gas()
        .deposit(NearToken::from_yoctonear(1))
        .args_json(json!({
            "dex_id": dex_id,
            "message": "0",
            "asset_in": AssetId::Near,
            "asset_out": AssetId::Nep141(ft.id().clone()),
            "amount": SwapRequestAmount::ExactIn(U128(NearToken::from_millinear(100).as_yoctonear())),
        }))
        .transact()
        .await?;
    println!("{outcome:#?}");
    println!("{}", outcome.total_gas_burnt);
    assert!(outcome.is_success());

    assert!(false);
    Ok(())
}
