#![allow(unused)]

use intear_dex::internal_asset_operations::AccountOrDexId;
use intear_dex_types::AssetId;
use near_sdk::serde_json::json;
use near_sdk::{NearToken, json_types::U128};
use near_workspaces::{Account, Contract};
use tokio::process::Command;
use tokio::sync::OnceCell;

pub struct CompiledWasms {
    pub contract_wasm: Vec<u8>,
    pub simple_amm_dex_wasm: Vec<u8>,
    pub minimal_dex_wasm: Vec<u8>,
    pub otc_dex_wasm: Vec<u8>,
    pub ft_wasm: Vec<u8>,
}

static COMPILED_WASMS: OnceCell<CompiledWasms> = OnceCell::const_new();

pub async fn get_compiled_wasms() -> &'static CompiledWasms {
    COMPILED_WASMS
        .get_or_init(|| async {
            println!("Compiling intear-dex");
            let contract_wasm = near_workspaces::compile_project("./").await.unwrap();

            println!("Compiling simple-amm-dex");
            assert!(
                Command::new("cargo")
                    .args([
                        "build",
                        "--package=simple-amm-dex",
                        "--release",
                        "--target",
                        "wasm32-unknown-unknown"
                    ])
                    .status()
                    .await
                    .unwrap()
                    .success()
            );

            println!("Compiling minimal-dex");
            assert!(
                Command::new("cargo")
                    .args([
                        "build",
                        "--package=minimal-dex",
                        "--release",
                        "--target",
                        "wasm32-unknown-unknown"
                    ])
                    .status()
                    .await
                    .unwrap()
                    .success()
            );

            println!("Compiling otc-dex");
            assert!(
                Command::new("cargo")
                    .args([
                        "build",
                        "--package=otc-dex",
                        "--release",
                        "--target",
                        "wasm32-unknown-unknown"
                    ])
                    .status()
                    .await
                    .unwrap()
                    .success()
            );

            println!("Compilation complete");

            let simple_amm_dex_wasm =
                std::fs::read("./target/wasm32-unknown-unknown/release/simple_amm_dex.wasm")
                    .unwrap();
            let minimal_dex_wasm =
                std::fs::read("./target/wasm32-unknown-unknown/release/minimal_dex.wasm").unwrap();
            let otc_dex_wasm =
                std::fs::read("./target/wasm32-unknown-unknown/release/otc_dex.wasm").unwrap();
            let ft_wasm = include_bytes!("../assets/ft.wasm").to_vec();

            CompiledWasms {
                contract_wasm,
                simple_amm_dex_wasm,
                minimal_dex_wasm,
                otc_dex_wasm,
                ft_wasm,
            }
        })
        .await
}

/// Track tokens burnt from a transaction result and add to total_near_burnt.
pub fn track_tokens_burnt(
    result: &near_workspaces::result::ExecutionFinalResult,
    total_near_burnt: &mut NearToken,
) {
    let near_burnt = result
        .outcomes()
        .iter()
        .map(|o| o.tokens_burnt)
        .reduce(|a, b| a.saturating_add(b))
        .unwrap();
    *total_near_burnt = total_near_burnt.saturating_add(near_burnt)
}

/// Assert the balance of a NEP-141 token of an account.
pub async fn assert_ft_balance(
    account: &Account,
    token: Contract,
    amount: U128,
) -> Result<(), Box<dyn std::error::Error>> {
    let balance = token
        .view("ft_balance_of")
        .args_json(json!({
            "account_id": account.id(),
        }))
        .await?
        .json::<U128>()?;
    if balance != amount {
        return Err(format!(
            "FT balance mismatch: expected {}, actual {}",
            amount.0, balance.0
        )
        .into());
    }
    Ok(())
}

/// Assert the balance of NEAR of an account.
pub async fn assert_near_balance(
    account: &Account,
    amount: NearToken,
) -> Result<(), Box<dyn std::error::Error>> {
    let account_details = account.view_account().await?;
    if account_details.balance != amount {
        return Err(format!(
            "NEAR balance mismatch: expected {}, actual {}",
            amount.as_yoctonear(),
            account_details.balance.as_yoctonear()
        )
        .into());
    }
    Ok(())
}

/// Assert the balance of an asset that is custodied by the dex
/// engine contract for a user or a dex.
pub async fn assert_inner_asset_balance(
    dex_engine_contract: &Contract,
    of: AccountOrDexId,
    asset: AssetId,
    amount: Option<U128>,
) -> Result<(), Box<dyn std::error::Error>> {
    let balance = dex_engine_contract
        .view("asset_balance_of")
        .args_json(json!({
            "of": of,
            "asset_id": asset,
        }))
        .await?
        .json::<Option<U128>>()?;
    if balance != amount {
        return Err(format!(
            "Inner asset balance mismatch: expected {:?}, actual {:?}",
            amount, balance
        )
        .into());
    }
    Ok(())
}

/// Assert the total amount of an asset tracked in custody.
pub async fn assert_total_in_custody(
    dex_engine_contract: &Contract,
    asset: AssetId,
    amount: Option<U128>,
) -> Result<(), Box<dyn std::error::Error>> {
    let total = dex_engine_contract
        .view("total_in_custody")
        .args_json(json!({
            "asset_id": asset,
        }))
        .await?
        .json::<Option<U128>>()?;
    if total != amount {
        return Err(format!(
            "Total in custody mismatch: expected {:?}, actual {:?}",
            amount, total
        )
        .into());
    }
    Ok(())
}
