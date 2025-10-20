// Copyright (C) 2024, 2025 P2Poolv2 Developers (see AUTHORS)
//
// This file is part of P2Poolv2
//
// P2Poolv2 is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free
// Software Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// P2Poolv2 is distributed in the hope that it will be useful, but WITHOUT ANY
// WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// P2Poolv2. If not, see <https://www.gnu.org/licenses/>.

mod bitcoin_block_validation;

#[cfg(test)]
#[mockall_double::double]
use crate::shares::chain::chain_store::ChainStore;
#[cfg(not(test))]
use crate::shares::chain::chain_store::ChainStore;
use crate::shares::share_block::ShareBlock;
use crate::utils::time_provider::TimeProvider;
use std::error::Error;
use std::sync::Arc;

pub const MAX_UNCLES: usize = 3;
pub const MAX_TIME_DIFF: u64 = 60;

/// Validate the share block, returning Error in case of failure to validate
/// validate nonce and blockhash meets difficulty
/// validate prev_share_blockhash is in store
/// validate uncles are in store and no more than MAX_UNCLES
/// validate timestamp is within the last 10 minutes
/// validate merkle root
/// validate coinbase transaction
pub async fn validate(
    share: &ShareBlock,
    store: Arc<ChainStore>,
    time_provider: &impl TimeProvider,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if let Err(e) = validate_timestamp(share, time_provider).await {
        return Err(format!("Share timestamp validation failed: {e}").into());
    }
    if let Err(e) = validate_prev_share_blockhash(share, store.clone()).await {
        return Err(format!("Share prev_share_blockhash validation failed: {e}").into());
    }
    if let Err(e) = validate_uncles(share, store.clone()).await {
        return Err(format!("Share uncles validation failed: {e}").into());
    }
    // TODO: Populate bitcoin block from ShortIDs in share and use bitcoin_block_validation to validate difficulty
    // OR - Fetch diffculty from bitcoind rpc and validate share blockhash meets difficulty
    Ok(())
}

/// Validate prev_share_blockhash is in store or block is genesis
pub async fn validate_prev_share_blockhash(
    share: &ShareBlock,
    store: Arc<ChainStore>,
) -> Result<(), Box<dyn Error>> {
    if store
        .get_share(&share.header.prev_share_blockhash)
        .is_none()
    {
        return Err(format!(
            "Prev share blockhash {} not found in store",
            share.header.prev_share_blockhash
        )
        .into());
    }
    Ok(())
}

/// Validate the share uncles are in store and no more than MAX_UNCLES
pub async fn validate_uncles(
    share: &ShareBlock,
    store: Arc<ChainStore>,
) -> Result<(), Box<dyn Error>> {
    if share.header.uncles.len() > MAX_UNCLES {
        return Err("Too many uncles".into());
    }
    for uncle in &share.header.uncles {
        if store.get_share(uncle).is_none() {
            return Err(format!("Uncle {uncle} not found in store").into());
        }
    }
    Ok(())
}

/// Validate the share timestamp is within the last 60 seconds
pub async fn validate_timestamp(
    share: &ShareBlock,
    time_provider: &impl TimeProvider,
) -> Result<(), Box<dyn Error>> {
    let current_time = time_provider.seconds_since_epoch();

    let block_timestamp = share.header.bitcoin_header.time as u64;
    let time_diff = current_time.abs_diff(block_timestamp);

    if time_diff > MAX_TIME_DIFF {
        return Err(format!(
            "Share timestamp {block_timestamp} is more than {MAX_TIME_DIFF} seconds from current time {current_time}",
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{TestShareBlockBuilder, genesis_for_tests};
    use crate::utils::time_provider::TestTimeProvider;
    use bitcoin::{BlockHash, hashes::Hash};
    use mockall::predicate::*;
    use std::sync::Arc;
    use std::time::SystemTime;

    #[tokio::test]
    async fn test_validate_timestamp_should_fail_for_old_timestamp() {
        let share = TestShareBlockBuilder::new()
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        let mut time_provider = TestTimeProvider(SystemTime::now());
        let share_timestamp = share.header.bitcoin_header.time as u64 - 120;

        time_provider
            .set_time(bitcoin::absolute::Time::from_consensus(share_timestamp as u32).unwrap());

        let result = validate_timestamp(&share, &time_provider).await;
        assert_eq!(
            result.err().unwrap().to_string(),
            format!(
                "Share timestamp {} is more than {MAX_TIME_DIFF} seconds from current time {}",
                share.header.bitcoin_header.time as u64,
                time_provider.seconds_since_epoch()
            )
        );
    }

    #[tokio::test]
    async fn test_validate_timestamp_should_fail_for_future_timestamp() {
        let share = TestShareBlockBuilder::new()
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();
        let mut time_provider = TestTimeProvider(SystemTime::now());
        let future_time = share.header.bitcoin_header.time as u64 + 120;
        time_provider
            .set_time(bitcoin::absolute::Time::from_consensus(future_time as u32).unwrap());

        let share = TestShareBlockBuilder::new()
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        assert!(validate_timestamp(&share, &time_provider).await.is_err());
    }

    #[tokio::test]
    async fn test_validate_timestamp_should_succeed_for_valid_timestamp() {
        let share = TestShareBlockBuilder::new()
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();
        let mut time_provider = TestTimeProvider(SystemTime::now());
        time_provider.set_time(
            bitcoin::absolute::Time::from_consensus(share.header.bitcoin_header.time).unwrap(),
        );

        let share = TestShareBlockBuilder::new()
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        assert!(validate_timestamp(&share, &time_provider).await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_prev_blockhash_exists() {
        // Create and add initial share to chain
        let initial_share = TestShareBlockBuilder::new()
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        // Create new share pointing to existing share - should validate
        let valid_share = TestShareBlockBuilder::new()
            .prev_share_blockhash(initial_share.block_hash().to_string())
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        let mut store = ChainStore::default();

        store
            .expect_get_share()
            .with(mockall::predicate::eq(initial_share.block_hash()))
            .returning(move |_| Some(initial_share.clone()));

        assert!(
            validate_prev_share_blockhash(&valid_share, Arc::new(store))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_validate_prev_blockhash_non_existing() {
        // Create share pointing to non-existent previous hash - should fail validation
        let non_existent_hash =
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb7".to_string();
        let invalid_share = TestShareBlockBuilder::new()
            .prev_share_blockhash(non_existent_hash.clone())
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        let mut store = ChainStore::default();
        store
            .expect_get_share()
            .with(mockall::predicate::eq(
                non_existent_hash.parse::<BlockHash>().unwrap(),
            ))
            .returning(move |_| None);

        assert!(
            validate_prev_share_blockhash(&invalid_share, Arc::new(store))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_validate_uncles() {
        let mut seq = mockall::Sequence::new();
        let mut store = ChainStore::default();

        // Create initial shares to use as uncles
        let uncle1 = TestShareBlockBuilder::new()
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        let uncle1_clone = uncle1.clone();
        store
            .expect_get_share()
            .times(1)
            .in_sequence(&mut seq)
            .with(mockall::predicate::eq(uncle1.block_hash()))
            .returning(move |_| Some(uncle1_clone.clone()));

        let uncle2 = TestShareBlockBuilder::new()
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        let uncle2_clone = uncle2.clone();

        store
            .expect_get_share()
            .times(1)
            .in_sequence(&mut seq)
            .with(mockall::predicate::eq(uncle2.block_hash()))
            .returning(move |_| Some(uncle2_clone.clone()));

        let uncle3 = TestShareBlockBuilder::new()
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        let uncle3_clone = uncle3.clone();

        store
            .expect_get_share()
            .times(1)
            .in_sequence(&mut seq)
            .with(mockall::predicate::eq(uncle3.block_hash()))
            .returning(move |_| Some(uncle3_clone.clone()));

        let uncle4 = TestShareBlockBuilder::new()
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        let _uncle4_clone = uncle4.clone();

        // Test share with non-existent uncle
        let non_existent_hash = "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb7"
            .parse::<BlockHash>()
            .unwrap();

        let _invalid_share_b = TestShareBlockBuilder::new()
            .uncles(vec![uncle1.block_hash(), non_existent_hash])
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        // Test share with valid number of uncles (MAX_UNCLES = 3)
        let valid_share = TestShareBlockBuilder::new()
            .uncles(vec![
                uncle1.block_hash(),
                uncle2.block_hash(),
                uncle3.block_hash(),
            ])
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        let arc_store = Arc::new(store);
        assert!(
            validate_uncles(&valid_share, arc_store.clone())
                .await
                .is_ok()
        );

        // Test share with too many uncles (> MAX_UNCLES)
        let invalid_share = TestShareBlockBuilder::new()
            .uncles(vec![
                uncle1.block_hash(),
                uncle2.block_hash(),
                uncle3.block_hash(),
                uncle4.block_hash(),
            ])
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .build();

        assert!(
            validate_uncles(&invalid_share, arc_store.clone())
                .await
                .is_err()
        );

        assert!(
            validate_uncles(&invalid_share, arc_store.clone())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_validate_share() {
        let mut store = ChainStore::default();

        let share_block = crate::test_utils::build_block_from_work_components(
            "../tests/test_data/validation/stratum/b/",
        );

        // Set up mock expectations
        store
            .expect_add_share()
            .with(mockall::predicate::eq(share_block.clone()))
            .returning(|_| Ok(()));
        store
            .expect_get_share()
            .with(eq(bitcoin::BlockHash::all_zeros()))
            .returning(|_| Some(genesis_for_tests()));

        store
            .expect_setup_share_for_chain()
            .returning(|share_block| share_block);

        let mut time_provider = TestTimeProvider(SystemTime::now());
        time_provider.set_time(
            bitcoin::absolute::Time::from_consensus(share_block.header.bitcoin_header.time)
                .unwrap(),
        );

        // Test handle_request directly without request_id
        let result = validate(&share_block, Arc::new(store), &time_provider).await;

        assert!(result.is_ok());
    }
}
