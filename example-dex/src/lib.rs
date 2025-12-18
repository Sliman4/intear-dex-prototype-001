#![no_main]

use std::collections::HashMap;

use near_sdk::{AccountId, BorshStorageKey, NearToken, json_types::U128, near, store::LookupMap};
use tear_sdk::{
    AssetId, AssetWithdrawRequest, AssetWithdrawalType, Dex, DexCallResponse, SwapRequest,
    SwapRequestAmount, SwapResponse,
};

type PoolId = String;

#[near(contract_state)]
pub struct SimpleSwap {
    pools: LookupMap<PoolId, SimplePool>,
    pool_counter: u64,
}

#[near(serializers=[borsh])]
#[derive(BorshStorageKey)]
enum StorageKey {
    Pools,
}

uint::construct_uint! {
    pub struct U256(4);
}

#[near(event_json(standard = "simpleswap"))]
pub enum SimpleSwapEvent {
    #[event_version("1.0.0")]
    PoolCreated {
        pool_id: PoolId,
        owner_id: AccountId,
        assets: (AssetId, AssetId),
    },
    #[event_version("1.0.0")]
    LiquidityAdded {
        pool_id: PoolId,
        amounts: (U128, U128),
    },
    #[event_version("1.0.0")]
    LiquidityRemoved {
        pool_id: PoolId,
        amounts: (U128, U128),
    },
}

impl Default for SimpleSwap {
    fn default() -> Self {
        Self {
            pools: LookupMap::new(StorageKey::Pools),
            pool_counter: 0,
        }
    }
}

#[near]
impl Dex for SimpleSwap {
    fn swap(&mut self, request: SwapRequest) -> SwapResponse {
        let Some(pool) = self.pools.get(&request.message) else {
            panic!("Pool not found");
        };
        assert!(
            pool.assets.0.asset_id == request.asset_in
                || pool.assets.1.asset_id == request.asset_in,
            "Invalid asset in"
        );
        assert!(
            pool.assets.0.asset_id == request.asset_out
                || pool.assets.1.asset_id == request.asset_out,
            "Invalid asset out"
        );
        assert!(
            pool.assets.0.balance.0 > 0 && pool.assets.1.balance.0 > 0,
            "Pool is empty"
        );
        let first_in = pool.assets.0.asset_id == request.asset_in;

        match request.amount {
            SwapRequestAmount::ExactIn(amount_in) => {
                assert!(amount_in.0 > 0, "Amount must be greater than 0");
                let in_balance = U256::from(if first_in {
                    pool.assets.0.balance.0
                } else {
                    pool.assets.1.balance.0
                });
                let out_balance = U256::from(if first_in {
                    pool.assets.1.balance.0
                } else {
                    pool.assets.0.balance.0
                });
                let amount_out = (U256::from(amount_in.0) * out_balance
                    / (in_balance + U256::from(amount_in.0)))
                .as_u128();
                SwapResponse {
                    amount_in,
                    amount_out: U128(amount_out),
                }
            }
            SwapRequestAmount::ExactOut(amount_out) => {
                assert!(amount_out.0 > 0, "Amount must be greater than 0");
                let in_balance = U256::from(if first_in {
                    pool.assets.0.balance.0
                } else {
                    pool.assets.1.balance.0
                });
                let out_balance = U256::from(if first_in {
                    pool.assets.1.balance.0
                } else {
                    pool.assets.0.balance.0
                });
                let amount_in = ((in_balance * U256::from(amount_out.0))
                    / (out_balance - U256::from(amount_out.0))
                    + U256::one())
                .as_u128();
                SwapResponse {
                    amount_in: U128(amount_in),
                    amount_out: U128(amount_out.0),
                }
            }
        }
    }
}

#[near]
impl SimpleSwap {
    pub fn create_pool(
        &mut self,
        #[allow(unused_mut)] mut attached_assets: HashMap<AssetId, U128>,
        assets: (AssetId, AssetId),
    ) -> DexCallResponse {
        assert!(assets.0 != assets.1, "Assets must be different");

        let pool_id = self.pool_counter.to_string();
        self.pool_counter += 1;

        let storage_usage_before = near_sdk::env::storage_usage();
        let old_pool_with_same_id = self.pools.insert(
            pool_id.clone(),
            SimplePool {
                assets: (
                    AssetWithBalance {
                        asset_id: assets.0.clone(),
                        balance: U128(0),
                    },
                    AssetWithBalance {
                        asset_id: assets.1.clone(),
                        balance: U128(0),
                    },
                ),
                owner_id: near_sdk::env::predecessor_account_id(),
            },
        );
        assert!(
            old_pool_with_same_id.is_none(),
            "Pool with same id somehow already exists"
        );

        self.pools.flush();
        let storage_usage_after = near_sdk::env::storage_usage();
        let storage_cost = near_sdk::env::storage_byte_cost().saturating_mul(
            (storage_usage_after as u128)
                .checked_sub(storage_usage_before as u128)
                .expect("Can't possibly be lower after inserting"),
        );

        let attached_near = NearToken::from_yoctonear(
            attached_assets
                .remove(&AssetId::Near)
                .expect("Near should be attached for storage")
                .0,
        );
        assert!(
            attached_near >= storage_cost,
            "Not enough near attached for storage. Required: {storage_cost}, attached: {attached_near}"
        );
        assert!(
            attached_assets.is_empty(),
            "No assets other than NEAR should be attached"
        );

        SimpleSwapEvent::PoolCreated {
            pool_id: pool_id.clone(),
            owner_id: near_sdk::env::predecessor_account_id(),
            assets,
        }
        .emit();

        DexCallResponse {
            asset_withdraw_requests: if let Some(leftover) = attached_near.checked_sub(storage_cost)
            {
                vec![AssetWithdrawRequest {
                    asset_id: AssetId::Near,
                    amount: U128(leftover.as_yoctonear()),
                    withdrawal_type: AssetWithdrawalType::ToInternalUserBalance(
                        near_sdk::env::predecessor_account_id(),
                    ),
                }]
            } else {
                vec![]
            },
            add_storage_deposit: storage_cost,
            response: near_sdk::serde_json::json!({
                "new_pool_id": pool_id,
            }),
        }
    }

    pub fn add_liquidity(
        &mut self,
        #[allow(unused_mut)] mut attached_assets: HashMap<AssetId, U128>,
        pool_id: PoolId,
    ) -> DexCallResponse {
        let Some(pool) = self.pools.get_mut(&pool_id) else {
            panic!("Pool not found");
        };
        assert!(
            pool.owner_id == near_sdk::env::predecessor_account_id(),
            "Only pool owner can add liquidity"
        );
        let asset_1_amount = attached_assets
            .remove(&pool.assets.0.asset_id)
            .expect("Asset 1 not found");
        let asset_2_amount = attached_assets
            .remove(&pool.assets.1.asset_id)
            .expect("Asset 2 not found");
        assert!(
            attached_assets.is_empty(),
            "No assets other than the two pool assets should be attached"
        );
        pool.assets.0.balance.0 = pool
            .assets
            .0
            .balance
            .0
            .checked_add(asset_1_amount.0)
            .expect("Overflow");
        pool.assets.1.balance.0 = pool
            .assets
            .1
            .balance
            .0
            .checked_add(asset_2_amount.0)
            .expect("Overflow");

        SimpleSwapEvent::LiquidityAdded {
            pool_id: pool_id.clone(),
            amounts: (asset_1_amount, asset_2_amount),
        }
        .emit();

        DexCallResponse {
            asset_withdraw_requests: vec![],
            add_storage_deposit: NearToken::ZERO,
            response: near_sdk::serde_json::json!({}),
        }
    }

    pub fn remove_liquidity(
        &mut self,
        pool_id: PoolId,
        assets_to_remove: (U128, U128),
        attached_assets: HashMap<AssetId, U128>,
    ) -> DexCallResponse {
        assert!(attached_assets.is_empty(), "No assets should be attached");
        let Some(pool) = self.pools.get_mut(&pool_id) else {
            panic!("Pool not found");
        };
        assert!(
            pool.owner_id == near_sdk::env::predecessor_account_id(),
            "Only pool owner can remove liquidity"
        );
        pool.assets.0.balance.0 = pool
            .assets
            .0
            .balance
            .0
            .checked_sub(assets_to_remove.0.0)
            .expect("Not enough balance for asset 1 withdrawal");
        pool.assets.1.balance.0 = pool
            .assets
            .1
            .balance
            .0
            .checked_sub(assets_to_remove.1.0)
            .expect("Not enough balance for asset 2 withdrawal");

        SimpleSwapEvent::LiquidityRemoved {
            pool_id: pool_id.clone(),
            amounts: assets_to_remove,
        }
        .emit();

        DexCallResponse {
            asset_withdraw_requests: vec![
                AssetWithdrawRequest {
                    asset_id: pool.assets.0.asset_id.clone(),
                    amount: assets_to_remove.0,
                    withdrawal_type: AssetWithdrawalType::ToInternalUserBalance(
                        pool.owner_id.clone(),
                    ),
                },
                AssetWithdrawRequest {
                    asset_id: pool.assets.1.asset_id.clone(),
                    amount: assets_to_remove.1,
                    withdrawal_type: AssetWithdrawalType::ToInternalUserBalance(
                        pool.owner_id.clone(),
                    ),
                },
            ],
            add_storage_deposit: NearToken::ZERO,
            response: near_sdk::serde_json::json!({}),
        }
    }
}

#[near(serializers=[borsh])]
struct SimplePool {
    assets: (AssetWithBalance, AssetWithBalance),
    owner_id: AccountId,
}

#[near(serializers=[borsh])]
struct AssetWithBalance {
    asset_id: AssetId,
    balance: U128,
}
