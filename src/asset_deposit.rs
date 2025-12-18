use near_contract_standards::{
    fungible_token::receiver::FungibleTokenReceiver,
    non_fungible_token::{self, core::NonFungibleTokenReceiver},
};
use near_sdk::{AccountId, PromiseOrValue, json_types::U128, near};
use tear_sdk::AssetId;

use crate::{DexEngine, DexEngineExt, IntearDexEvent, internal_asset_operations::AccountOrDexId};

#[near]
impl DexEngine {
    #[payable]
    /// Deposit near to the dex engine contract's inner
    /// balance for the user.
    pub fn near_deposit(&mut self) {
        let storage_usage_before = near_sdk::env::storage_usage();
        self.deposit_assets(
            AccountOrDexId::Account(near_sdk::env::predecessor_account_id()),
            AssetId::Near,
            U128(near_sdk::env::attached_deposit().as_yoctonear()),
        );
        self.user_balances.flush();
        let storage_usage_after = near_sdk::env::storage_usage();
        self.user_storage_balances.charge(
            &near_sdk::env::predecessor_account_id(),
            storage_usage_before,
            storage_usage_after,
        );

        IntearDexEvent::UserDeposit {
            account_id: near_sdk::env::predecessor_account_id(),
            asset_id: AssetId::Near,
            amount: U128(near_sdk::env::attached_deposit().as_yoctonear()),
        }
        .emit();
    }
}

#[near]
impl FungibleTokenReceiver for DexEngine {
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        #[allow(unused_variables)] msg: String,
    ) -> PromiseOrValue<U128> {
        let token_id = near_sdk::env::predecessor_account_id();

        let storage_usage_before = near_sdk::env::storage_usage();
        self.deposit_assets(
            AccountOrDexId::Account(sender_id.clone()),
            AssetId::Nep141(token_id.clone()),
            amount,
        );
        self.user_balances.flush();
        let storage_usage_after = near_sdk::env::storage_usage();
        self.user_storage_balances
            .charge(&sender_id, storage_usage_before, storage_usage_after);

        IntearDexEvent::UserDeposit {
            account_id: sender_id.clone(),
            asset_id: AssetId::Nep141(token_id),
            amount,
        }
        .emit();

        PromiseOrValue::Value(U128(0))
    }
}

#[near]
impl NonFungibleTokenReceiver for DexEngine {
    fn nft_on_transfer(
        &mut self,
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: non_fungible_token::TokenId,
        #[allow(unused_variables)] msg: String,
    ) -> PromiseOrValue<bool> {
        let contract_id = near_sdk::env::predecessor_account_id();

        let storage_usage_before = near_sdk::env::storage_usage();
        self.deposit_assets(
            AccountOrDexId::Account(previous_owner_id),
            AssetId::Nep171(contract_id.clone(), token_id.to_string()),
            U128(1),
        );
        self.user_balances.flush();
        let storage_usage_after = near_sdk::env::storage_usage();
        self.user_storage_balances
            .charge(&sender_id, storage_usage_before, storage_usage_after);

        IntearDexEvent::UserDeposit {
            account_id: sender_id.clone(),
            asset_id: AssetId::Nep171(contract_id, token_id.to_string()),
            amount: U128(1),
        }
        .emit();
        PromiseOrValue::Value(false)
    }
}

// There's no interface in near-contract-standards for nep245
#[near]
impl DexEngine {
    pub fn mt_on_transfer(
        &mut self,
        sender_id: AccountId,
        previous_owner_ids: Vec<AccountId>,
        token_ids: Vec<String>,
        amounts: Vec<U128>,
        #[allow(unused_variables)] msg: String,
    ) -> PromiseOrValue<Vec<U128>> {
        assert!(
            previous_owner_ids.len() == token_ids.len()
                && previous_owner_ids.len() == amounts.len(),
            "Invalid input array lengths"
        );

        let contract_id = near_sdk::env::predecessor_account_id();

        let storage_usage_before = near_sdk::env::storage_usage();
        for ((token_id, previous_owner_id), amount) in token_ids
            .iter()
            .zip(previous_owner_ids.iter())
            .zip(amounts.iter())
        {
            self.deposit_assets(
                AccountOrDexId::Account(previous_owner_id.clone()),
                AssetId::Nep245(contract_id.clone(), token_id.clone()),
                *amount,
            );

            IntearDexEvent::UserDeposit {
                account_id: previous_owner_id.clone(),
                asset_id: AssetId::Nep245(contract_id.clone(), token_id.clone()),
                amount: *amount,
            }
            .emit();
        }
        self.user_balances.flush();
        let storage_usage_after = near_sdk::env::storage_usage();
        self.user_storage_balances
            .charge(&sender_id, storage_usage_before, storage_usage_after);

        PromiseOrValue::Value(vec![])
    }
}
