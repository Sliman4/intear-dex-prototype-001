#![deny(clippy::arithmetic_side_effects)]

use std::collections::HashMap;

use intear_dex_types::{
    AssetId, AssetWithdrawRequest, AssetWithdrawalType, Dex, DexCallResponse, SwapRequest,
    SwapResponse, expect,
};
use near_crypto::{PublicKey, Signature};
use near_sdk::{
    AccountId, BlockHeight, BorshStorageKey, NearToken,
    json_types::{U64, U128},
    near,
    store::{LookupMap, LookupSet, TreeMap},
};

#[global_allocator]
static ALLOCATOR: talc::Talck<talc::locking::AssumeUnlockable, talc::ClaimOnOom> = {
    static mut MEMORY: [u8; 0x8000] = [0; 0x8000]; // 32KB
    let span = talc::Span::from_array(core::ptr::addr_of!(MEMORY).cast_mut());
    talc::Talc::new(unsafe { talc::ClaimOnOom::new(span) }).lock()
};

#[near(serializers=[borsh])]
#[derive(BorshStorageKey)]
enum StorageKey {
    Balances,
    AuthorizedKeys,
    StorageBalances,
    UsedExpirableNonces,
    UsedNonExpirableNonces,
}

#[derive(Default)]
#[near(serializers=[borsh, json])]
pub struct StorageBalance {
    total: NearToken,
    used: NearToken,
}

impl StorageBalance {
    fn validate(&self) {
        expect!(self.total >= self.used, "Not enough storage deposit");
    }
}

#[near(contract_state)]
pub struct OtcDex {
    balances: LookupMap<(AccountId, AssetId), U128>,
    authorized_keys: LookupMap<AccountId, PublicKey>,
    storage_balances: LookupMap<AccountId, StorageBalance>,
    used_expirable_nonces: TreeMap<(AccountId, Nonce), ExpiryCondition>,
    used_non_expirable_nonces: LookupSet<(AccountId, Nonce)>,
}

pub type Nonce = U128;

#[near(serializers=[json, borsh])]
enum AuthorizationMethod {
    Signature(Signature),
    Predecessor,
}

#[near(serializers=[json, borsh])]
pub struct AuthorizedTradeIntent {
    trade_intent: TradeIntent,
    authorization_method: AuthorizationMethod,
}

#[near(serializers=[json, borsh])]
pub struct TradeIntent {
    user_id: AccountId,
    asset_in: AssetId,
    asset_out: AssetId,
    amount_in: U128,
    amount_out: U128,
    #[serde(default, skip_serializing_if = "is_default")]
    validity: Validity,
}

#[derive(Default, PartialEq)]
#[near(serializers=[json, borsh])]
pub struct Validity {
    expiry: Option<ExpiryCondition>,
    nonce: Option<Nonce>,
    only_for_whitelisted_parties: Option<Vec<AccountId>>,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    value == &T::default()
}

#[derive(PartialEq)]
#[near(serializers=[borsh, json])]
pub enum ExpiryCondition {
    BlockHeight(BlockHeight),
    Timestamp { milliseconds: U64 },
}

#[near(event_json(standard = "simpleswap"))]
enum OtcTradeEvent {
    #[event_version("1.0.0")]
    AuthorizedKeyChanged {
        account_id: AccountId,
        key: PublicKey,
    },
    #[event_version("1.0.0")]
    Trade {
        authorized_trade_intents: Vec<AuthorizedTradeIntent>,
    },
}

#[near]
impl Dex for OtcDex {
    #[result_serializer(borsh)]
    fn swap(
        &mut self,
        #[allow(unused_variables)]
        #[serializer(borsh)]
        request: SwapRequest,
    ) -> SwapResponse {
        panic!("Method `swap` cannot be used for OtcDex. Use dex_call with `match` method instead.")
    }
}

impl Default for OtcDex {
    fn default() -> Self {
        Self {
            balances: LookupMap::new(StorageKey::Balances),
            authorized_keys: LookupMap::new(StorageKey::AuthorizedKeys),
            storage_balances: LookupMap::new(StorageKey::StorageBalances),
            used_expirable_nonces: TreeMap::new(StorageKey::UsedExpirableNonces),
            used_non_expirable_nonces: LookupSet::new(StorageKey::UsedNonExpirableNonces),
        }
    }
}

#[near]
impl OtcDex {
    #[result_serializer(borsh)]
    pub fn r#match(
        &mut self,
        #[serializer(borsh)]
        #[allow(unused_mut)]
        mut attached_assets: HashMap<AssetId, U128>,
        #[serializer(borsh)] args: Vec<u8>,
    ) -> DexCallResponse {
        #[near(serializers=[borsh])]
        struct MatchArgs {
            authorized_trade_intents: Vec<AuthorizedTradeIntent>,
        }
        let Ok(MatchArgs {
            authorized_trade_intents,
        }) = near_sdk::borsh::from_slice(&args)
        else {
            panic!("Invalid args");
        };
        expect!(attached_assets.is_empty(), "No assets should be attached");

        OtcTradeEvent::Trade {
            authorized_trade_intents,
        }
        .emit();
        todo!("settle")
    }

    #[result_serializer(borsh)]
    pub fn storage_deposit(
        &mut self,
        #[allow(unused_mut)]
        #[serializer(borsh)]
        mut attached_assets: HashMap<AssetId, U128>,
        #[serializer(borsh)] args: Vec<u8>,
    ) -> DexCallResponse {
        #[near(serializers=[borsh])]
        struct StorageDepositArgs;
        let Ok(StorageDepositArgs) = near_sdk::borsh::from_slice(&args) else {
            panic!("Invalid args");
        };
        let predecessor_id = near_sdk::env::predecessor_account_id();
        let Some(attached_near) = attached_assets.remove(&AssetId::Near) else {
            panic!("Near not attached");
        };
        let attached_near = NearToken::from_yoctonear(attached_near.0);
        expect!(
            attached_assets.is_empty(),
            "No assets other than NEAR should be attached"
        );
        self.storage_balances
            .entry(predecessor_id.clone())
            .and_modify(|b| b.total = b.total.saturating_add(attached_near))
            .or_insert(StorageBalance {
                total: attached_near,
                used: NearToken::ZERO,
            });
        let storage_usage_before = near_sdk::env::storage_usage();
        self.storage_balances.flush();
        let storage_usage_after = near_sdk::env::storage_usage();
        self.charge_storage_deposit(storage_usage_before, storage_usage_after);

        DexCallResponse {
            add_storage_deposit: attached_near,
            ..Default::default()
        }
    }

    #[result_serializer(borsh)]
    pub fn set_authorized_key(
        &mut self,
        #[serializer(borsh)] attached_assets: HashMap<AssetId, U128>,
        #[serializer(borsh)] args: Vec<u8>,
    ) -> DexCallResponse {
        #[near(serializers=[borsh])]
        struct SetAuthorizedKeyArgs {
            key: PublicKey,
        }
        let Ok(SetAuthorizedKeyArgs { key }) = near_sdk::borsh::from_slice(&args) else {
            panic!("Invalid args");
        };
        expect!(attached_assets.is_empty(), "No assets should be attached");
        let storage_usage_before = near_sdk::env::storage_usage();
        self.authorized_keys
            .insert(near_sdk::env::predecessor_account_id(), key.clone());
        self.authorized_keys.flush();
        let storage_usage_after = near_sdk::env::storage_usage();
        self.charge_storage_deposit(storage_usage_before, storage_usage_after);
        OtcTradeEvent::AuthorizedKeyChanged {
            account_id: near_sdk::env::predecessor_account_id(),
            key,
        }
        .emit();
        DexCallResponse::default()
    }

    #[result_serializer(borsh)]
    pub fn deposit_assets(
        &mut self,
        #[serializer(borsh)] attached_assets: HashMap<AssetId, U128>,
        #[serializer(borsh)] args: Vec<u8>,
    ) -> DexCallResponse {
        #[near(serializers=[borsh])]
        struct DepositAssetsArgs;
        let Ok(DepositAssetsArgs) = near_sdk::borsh::from_slice(&args) else {
            panic!("Invalid args");
        };
        let storage_usage_before = near_sdk::env::storage_usage();
        for (asset_id, amount) in attached_assets.iter() {
            self.balances
                .entry((near_sdk::env::predecessor_account_id(), asset_id.clone()))
                .and_modify(|b| b.0 = b.0.checked_add(amount.0).expect("Balance overflow"))
                .or_insert(*amount);
        }
        self.balances.flush();
        let storage_usage_after = near_sdk::env::storage_usage();
        self.charge_storage_deposit(storage_usage_before, storage_usage_after);
        DexCallResponse::default()
    }

    #[result_serializer(borsh)]
    pub fn withdraw_assets(
        &mut self,
        #[serializer(borsh)] attached_assets: HashMap<AssetId, U128>,
        #[serializer(borsh)] args: Vec<u8>,
    ) -> DexCallResponse {
        #[near(serializers=[borsh])]
        struct WithdrawAssetsArgs {
            assets: Vec<WithdrawRequest>,
        }
        #[near(serializers=[borsh])]
        struct WithdrawRequest {
            asset_id: AssetId,
            // If None, the entire balance of the asset will be withdrawn.
            amount: Option<U128>,
            // If None, the assets will be withdrawn to the user's own account.
            to: Option<AccountId>,
            to_inner_balance: bool,
        }
        let Ok(WithdrawAssetsArgs { assets }) = near_sdk::borsh::from_slice(&args) else {
            panic!("Invalid args");
        };
        expect!(attached_assets.is_empty(), "No assets should be attached");
        let storage_usage_before = near_sdk::env::storage_usage();
        let mut asset_withdraw_requests = Vec::new();
        for WithdrawRequest {
            asset_id,
            amount,
            to,
            to_inner_balance,
        } in assets.iter()
        {
            let amount = amount.unwrap_or_else(|| {
                self.balances
                    .get(&(near_sdk::env::predecessor_account_id(), asset_id.clone()))
                    .copied()
                    .unwrap_or_default()
            });
            self.balances
                .entry((near_sdk::env::predecessor_account_id(), asset_id.clone()))
                .and_modify(|b| {
                    b.0 = b.0.checked_sub(amount.0).unwrap_or_else(|| {
                        panic!(
                            "Not enough balance for asset {asset_id}: {} < {}",
                            b.0, amount.0,
                        )
                    })
                })
                .or_insert_with(|| {
                    panic!("Not enough balance for asset {asset_id}: 0 < {}", amount.0)
                });
            let to = to
                .clone()
                .unwrap_or_else(near_sdk::env::predecessor_account_id);
            asset_withdraw_requests.push(AssetWithdrawRequest {
                asset_id: asset_id.clone(),
                amount,
                withdrawal_type: if *to_inner_balance {
                    AssetWithdrawalType::ToInternalUserBalance(to)
                } else {
                    AssetWithdrawalType::WithdrawUnderlyingAsset(to)
                },
            })
        }
        self.balances.flush();
        let storage_usage_after = near_sdk::env::storage_usage();
        self.charge_storage_deposit(storage_usage_before, storage_usage_after);
        DexCallResponse {
            asset_withdraw_requests,
            ..Default::default()
        }
    }

    #[result_serializer(borsh)]
    pub fn storage_balance_of(
        &self,
        #[serializer(borsh)] account_id: AccountId,
    ) -> Option<&StorageBalance> {
        self.storage_balances.get(&account_id)
    }

    #[result_serializer(borsh)]
    pub fn get_authorized_key(
        &self,
        #[serializer(borsh)] account_id: AccountId,
    ) -> Option<&PublicKey> {
        self.authorized_keys.get(&account_id)
    }

    #[result_serializer(borsh)]
    pub fn is_nonce_used(
        &self,
        #[serializer(borsh)] nonce: Nonce,
        #[serializer(borsh)] account_id: AccountId,
    ) -> bool {
        self.used_expirable_nonces
            .contains_key(&(account_id.clone(), nonce))
            || self
                .used_non_expirable_nonces
                .contains(&(account_id, nonce))
    }

    #[result_serializer(borsh)]
    pub fn get_balance(
        &self,
        #[serializer(borsh)] account_id: AccountId,
        #[serializer(borsh)] asset_id: AssetId,
    ) -> Option<&U128> {
        self.balances.get(&(account_id.clone(), asset_id.clone()))
    }
}

impl OtcDex {
    fn charge_storage_deposit(&mut self, before: u64, after: u64) {
        let account_id = near_sdk::env::predecessor_account_id();
        match after.cmp(&before) {
            std::cmp::Ordering::Greater => {
                // charge the difference
                let storage_cost = near_sdk::env::storage_byte_cost()
                    .saturating_mul(after.checked_sub(before).expect("Just compared") as u128);
                self.storage_balances
                    .entry(account_id)
                    .and_modify(|b| b.used = b.used.saturating_add(storage_cost))
                    .or_insert_with(|| panic!("Storage not registered"))
                    .validate();
            }
            std::cmp::Ordering::Less => {
                // refund the difference
                let storage_cost = near_sdk::env::storage_byte_cost()
                    .saturating_mul(before.checked_sub(after).expect("Just compared") as u128);
                self.storage_balances
                    .entry(account_id)
                    .and_modify(|b| {
                        b.used = b
                            .used
                            .checked_sub(storage_cost)
                            .expect("Storage used underflow")
                    })
                    .or_insert_with(|| panic!("Storage not registered"))
                    .validate();
            }
            std::cmp::Ordering::Equal => {
                // nothing changed
            }
        }
    }
}

#[near(serializers=[borsh])]
pub struct SimplePool {
    assets: (AssetWithBalance, AssetWithBalance),
    owner_id: AccountId,
}

#[near(serializers=[borsh])]
pub struct AssetWithBalance {
    asset_id: AssetId,
    balance: U128,
}
