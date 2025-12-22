use base64::{Engine, prelude::BASE64_STANDARD};
use borsh::BorshSerialize;
use clap::{Parser, Subcommand};
use near_api::{
    Contract, NearToken, NetworkConfig, RPCEndpoint, Signer,
    types::{AccountId, PublicKey},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::json;
use std::{collections::HashMap, fmt::Display, str::FromStr, sync::Arc};
use tokio::process::Command;

#[derive(Parser)]
#[command(name = "manage")]
#[command(about = "Management CLI for intear-dex", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Otc {
        #[command(subcommand)]
        action: OtcAction,
    },
}

#[derive(Subcommand)]
enum OtcAction {
    Deploy,
    SetAuthorizedKey {
        account_id: AccountId,
        key: PublicKey,
    },
    StorageDeposit {
        account_id: AccountId,
        amount: NearToken,
    },
    DepositAssets {
        account_id: AccountId,
        asset_id: AssetId,
        amount: u128,
    },
}

struct Config {
    deployer_id: AccountId,
    signer: Arc<Signer>,
    dex_contract_id: AccountId,
}

async fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let deployer_id =
        std::env::var("DEPLOYER_ID").map_err(|_| "DEPLOYER_ID environment variable not set")?;
    let deployer_id =
        AccountId::from_str(&deployer_id).map_err(|e| format!("Invalid DEPLOYER_ID: {}", e))?;
    let signer =
        Signer::from_keystore_with_search_for_keys(deployer_id.clone(), &network()).await?;
    let dex_contract_id = std::env::var("DEX_CONTRACT_ID").unwrap_or("dex.intear.near".to_string());
    let dex_contract_id = AccountId::from_str(&dex_contract_id)
        .map_err(|e| format!("Invalid DEX_CONTRACT_ID: {}", e))?;

    Ok(Config {
        deployer_id,
        signer,
        dex_contract_id,
    })
}

fn network() -> NetworkConfig {
    NetworkConfig {
        rpc_endpoints: vec![RPCEndpoint::new("https://rpc.intea.rs".parse().unwrap())],
        ..NetworkConfig::mainnet()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let config = load_config().await?;

    println!("Loaded config: deployer_id = {}", config.deployer_id);

    match cli.command {
        Commands::Otc { action } => match action {
            OtcAction::Deploy => {
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
                println!("Optimizing otc-dex");
                assert!(
                    Command::new("wasm-opt")
                        .args([
                            "-O",
                            "./target/wasm32-unknown-unknown/release/otc_dex.wasm",
                            "-o",
                            "./target/wasm32-unknown-unknown/release/otc_dex.wasm"
                        ])
                        .status()
                        .await
                        .unwrap()
                        .success()
                );
                println!("Deploying otc-dex");
                let wasm =
                    std::fs::read("./target/wasm32-unknown-unknown/release/otc_dex.wasm").unwrap();
                let wasm_base64 = BASE64_STANDARD.encode(&wasm);
                let args = json!({
                    "last_part_of_id": "otc",
                    "code_base64": wasm_base64,
                });
                let result = Contract(config.dex_contract_id.clone())
                    .call_function("deploy_dex_code", args)
                    .transaction()
                    .max_gas()
                    .deposit(NearToken::from_yoctonear(1))
                    .with_signer(config.deployer_id.clone(), Arc::clone(&config.signer))
                    .send_to(&network())
                    .await?;
                println!("Deployed. Result: {:?}", result.outcome());
            }
            OtcAction::SetAuthorizedKey { account_id, key } => {
                let account_signer =
                    Signer::from_keystore_with_search_for_keys(account_id.clone(), &network())
                        .await?;
                let dex_id = format!("{}/{}", config.deployer_id.clone(), "otc");
                #[derive(BorshSerialize)]
                struct OtcSetAuthorizedKeyArgs {
                    key_bytes: Vec<u8>,
                }
                let args = json!({
                    "dex_id": dex_id,
                    "method": "set_authorized_key",
                    "args": BASE64_STANDARD.encode(borsh::to_vec(&OtcSetAuthorizedKeyArgs {
                        key_bytes: match key {
                            PublicKey::ED25519(public_key) => [vec![0], public_key.0.to_vec()].concat(),
                            PublicKey::SECP256K1(public_key) => [vec![1], public_key.0.to_vec()].concat(),
                        },
                    }).unwrap()),
                    "attached_assets": {},
                });
                let result = Contract(config.dex_contract_id.clone())
                    .call_function("dex_call", args)
                    .transaction()
                    .max_gas()
                    .deposit(NearToken::from_yoctonear(1))
                    .with_signer(account_id.clone(), account_signer)
                    .send_to(&network())
                    .await?;
                println!("Set the authorized key. Result: {:?}", result.outcome());
            }
            OtcAction::StorageDeposit { account_id, amount } => {
                let account_signer =
                    Signer::from_keystore_with_search_for_keys(account_id.clone(), &network())
                        .await?;
                let dex_id = format!("{}/{}", config.deployer_id.clone(), "otc");
                #[derive(BorshSerialize)]
                struct OtcStorageDepositArgs;
                let args = json!({
                    "operations": [{
                        "DexCall": {
                            "dex_id": dex_id,
                            "method": "storage_deposit",
                            "args": BASE64_STANDARD.encode(borsh::to_vec(&OtcStorageDepositArgs).unwrap()),
                            "attached_assets": {
                                "near": amount,
                            },
                        }
                    }]
                });
                let result = Contract(config.dex_contract_id.clone())
                    .call_function("deposit_near", args)
                    .transaction()
                    .max_gas()
                    .deposit(amount)
                    .with_signer(account_id.clone(), account_signer)
                    .send_to(&network())
                    .await?;
                println!("Storage deposit completed. Result: {:?}", result.outcome());
            }
            OtcAction::DepositAssets {
                account_id,
                asset_id,
                amount,
            } => {
                let account_signer =
                    Signer::from_keystore_with_search_for_keys(account_id.clone(), &network())
                        .await?;
                let dex_id = format!("{}/{}", config.deployer_id.clone(), "otc");
                #[derive(BorshSerialize)]
                struct OtcDepositAssetsArgs;
                // U128 serializes as a number
                let attached_assets: HashMap<AssetId, String> =
                    HashMap::from_iter([(asset_id, amount.to_string())]);
                let args = json!({
                    "dex_id": dex_id,
                    "method": "deposit_assets",
                    "args": BASE64_STANDARD.encode(borsh::to_vec(&OtcDepositAssetsArgs).unwrap()),
                    "attached_assets": attached_assets,
                });
                let result = Contract(config.dex_contract_id.clone())
                    .call_function("dex_call", args)
                    .transaction()
                    .max_gas()
                    .deposit(NearToken::from_yoctonear(1))
                    .with_signer(account_id.clone(), account_signer)
                    .send_to(&network())
                    .await?;
                println!("Deposit assets completed. Result: {:?}", result.outcome());
            }
        },
    }

    Ok(())
}

#[derive(
    PartialEq, Eq, Hash, Clone, PartialOrd, Ord, Debug, BorshSerialize, borsh::BorshDeserialize,
)]
pub enum AssetId {
    Near,
    Nep141(AccountId),
    Nep245(AccountId, String),
    Nep171(AccountId, String),
}

impl Display for AssetId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AssetId::Near => write!(f, "near"),
            AssetId::Nep141(contract_id) => write!(f, "nep141:{contract_id}"),
            AssetId::Nep245(contract_id, token_id) => write!(f, "nep245:{contract_id}:{token_id}"),
            AssetId::Nep171(contract_id, token_id) => write!(f, "nep171:{contract_id}:{token_id}"),
        }
    }
}

impl FromStr for AssetId {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "near" => Ok(AssetId::Near),
            _ => match s.split_once(':') {
                Some(("nep141", contract_id)) => {
                    Ok(AssetId::Nep141(contract_id.parse().map_err(|e| {
                        format!("Invalid account id {contract_id}: {e}")
                    })?))
                }
                Some(("nep245", rest)) => {
                    if let Some((contract_id, token_id)) = rest.split_once(':') {
                        Ok(AssetId::Nep245(
                            contract_id
                                .parse()
                                .map_err(|e| format!("Invalid account id {contract_id}: {e}"))?,
                            token_id.to_string(),
                        ))
                    } else {
                        Err(format!("Invalid asset id: {s}"))
                    }
                }
                Some(("nep171", rest)) => {
                    if let Some((contract_id, token_id)) = rest.split_once(':') {
                        Ok(AssetId::Nep171(
                            contract_id
                                .parse()
                                .map_err(|e| format!("Invalid account id {contract_id}: {e}"))?,
                            token_id.to_string(),
                        ))
                    } else {
                        Err(format!("Invalid asset id: {s}"))
                    }
                }
                _ => Err(format!("Invalid asset id: {s}")),
            },
        }
    }
}

impl Serialize for AssetId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serde::Serialize::serialize(&self.to_string(), serializer)
    }
}

impl<'de> Deserialize<'de> for AssetId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        Ok(AssetId::from_str(&s).map_err(|e| serde::de::Error::custom(e))?)
    }
}
