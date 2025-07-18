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

pub mod chain;
pub mod ckpool_socket;
pub mod genesis;
pub mod handle_mining_message;
pub mod miner_message;
pub mod receive_mining_message;
pub mod store;
pub mod transactions;
pub mod validation;

use crate::shares::miner_message::MinerShare;
use bitcoin::TxMerkleNode;
use bitcoin::{BlockHash, PublicKey, Transaction};
use serde::{Deserialize, Serialize};
use std::error::Error;

#[derive(Clone, Serialize, Deserialize, Debug)]
/// Header for the ShareBlock
/// Helps validate PoW and transaction merkle root.
pub struct ShareHeader {
    /// miner share with blockhash, nonce, time, diff and workinfoid
    pub miner_share: MinerShare,
    /// The hash of the prev share block, will be None for genesis block
    pub prev_share_blockhash: Option<ShareBlockHash>,
    /// The uncles of the share
    pub uncles: Vec<ShareBlockHash>,
    /// Compressed pubkey identifying the miner
    pub miner_pubkey: PublicKey,
    /// Transaction merkle root
    pub merkle_root: TxMerkleNode,
}

impl PartialEq for ShareHeader {
    fn eq(&self, other: &Self) -> bool {
        self.miner_share.hash == other.miner_share.hash
    }
}

impl Eq for ShareHeader {}

impl ShareHeader {
    pub fn genesis(
        genesis_data: &genesis::GenesisData,
        public_key: PublicKey,
        merkle_root: TxMerkleNode,
    ) -> Self {
        Self {
            prev_share_blockhash: None,
            uncles: vec![],
            miner_pubkey: public_key,
            merkle_root,
            miner_share: MinerShare::genesis(genesis_data),
        }
    }
}

/// ShareBlockBuilder is a builder pattern to build from Header and Transactions
/// We always required a Header, therefore the builder starts with a new, not a default.
pub struct ShareBlockBuilder {
    header: ShareHeader,
    transactions: Vec<Transaction>,
}

impl ShareBlockBuilder {
    /// Initialise the builder with a ShareHeader which is required
    pub fn new(header: ShareHeader) -> Self {
        Self {
            header,
            transactions: Vec::new(),
        }
    }

    pub fn with_transactions(mut self, transactions: Vec<Transaction>) -> Self {
        self.transactions = transactions;
        self
    }

    pub fn build(self) -> ShareBlock {
        let mut block = ShareBlock {
            header: self.header,
            transactions: self.transactions,
            cached_blockhash: None,
        };
        block.compute_blockhash();
        block
    }
}

/// Type alias for bitcoin block hash, so we can depend on type to catch potential errors
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug, Hash, Copy)]
pub struct ShareBlockHash(BlockHash);

impl From<&str> for ShareBlockHash {
    fn from(s: &str) -> ShareBlockHash {
        ShareBlockHash(s.parse().expect("Invalid block hash string"))
    }
}

impl std::fmt::Display for ShareBlockHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<ShareBlockHash> for &str {
    fn eq(&self, other: &ShareBlockHash) -> bool {
        self.parse::<BlockHash>().unwrap() == other.0
    }
}

impl PartialEq<&str> for ShareBlockHash {
    fn eq(&self, other: &&str) -> bool {
        self.0 == other.parse().unwrap()
    }
}

impl PartialEq<ShareBlockHash> for &ShareBlockHash {
    fn eq(&self, other: &ShareBlockHash) -> bool {
        self.0 == other.0
    }
}

impl Eq for ShareBlockHash {}

impl AsRef<[u8]> for ShareBlockHash {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

/// Captures a block on the share chain
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct ShareBlock {
    /// The header of the share block
    pub header: ShareHeader,
    /// Any transactions to be included in the share block
    pub transactions: Vec<Transaction>,
    /// Cached BlockHash, skipped for serialization. Only used for internal state.
    #[serde(skip)]
    pub cached_blockhash: Option<ShareBlockHash>,
}

impl ShareBlock {
    pub fn new(
        miner_share: MinerShare,
        miner_pubkey: PublicKey,
        network: bitcoin::Network,
        include_transactions: &mut Vec<Transaction>,
    ) -> Self {
        let coinbase_tx =
            transactions::coinbase::create_coinbase_transaction(&miner_pubkey, network);
        let mut transactions = vec![coinbase_tx];
        transactions.append(include_transactions);
        // Calculate merkle root from transactions
        let merkle_root: TxMerkleNode = bitcoin::merkle_tree::calculate_root(
            transactions.iter().map(Transaction::compute_txid),
        )
        .unwrap()
        .into();
        let mut block = Self {
            header: ShareHeader {
                prev_share_blockhash: None,
                uncles: vec![],
                miner_pubkey,
                merkle_root,
                miner_share,
            },
            transactions,
            cached_blockhash: None,
        };
        block.compute_blockhash();
        block
    }

    pub fn compute_blockhash(&mut self) {
        let mut serialized = Vec::new();
        ciborium::ser::into_writer(&self, &mut serialized).unwrap();
        self.cached_blockhash = Some(ShareBlockHash(bitcoin::hashes::Hash::hash(&serialized)));
    }

    /// Build a genesis share block for a given network
    /// The bitcoin blockhash is hardcoded, so are the coinbase, nonce2, nonce, ntime, diff, sdiff
    /// The workinfoid and clientid are 0 for genesis block on all networks
    pub fn build_genesis_for_network(network: bitcoin::Network) -> Self {
        assert!(
            network == bitcoin::Network::Signet
                || network == bitcoin::Network::Testnet4
                || network == bitcoin::Network::Bitcoin,
            "Network Testnet and Regtest not yet supported"
        );
        let genesis_data = genesis::genesis_data(network).unwrap();
        ShareBlock::build_genesis(&genesis_data, network)
    }

    fn build_genesis(genesis_data: &genesis::GenesisData, network: bitcoin::Network) -> Self {
        let public_key = genesis_data.public_key.parse::<PublicKey>().unwrap();
        let coinbase_tx = transactions::coinbase::create_coinbase_transaction(&public_key, network);
        let transactions = vec![coinbase_tx];
        let merkle_root: TxMerkleNode = bitcoin::merkle_tree::calculate_root(
            transactions.iter().map(Transaction::compute_txid),
        )
        .unwrap()
        .into();
        let mut genesis = Self {
            header: ShareHeader::genesis(genesis_data, public_key, merkle_root),
            transactions,
            cached_blockhash: None,
        };
        genesis.compute_blockhash();
        genesis
    }
}

/// A variant of ShareBlock used for storage that excludes transactions
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct StorageShareBlock {
    /// The header of the share block
    pub header: ShareHeader,
}

impl From<ShareBlock> for StorageShareBlock {
    fn from(block: ShareBlock) -> Self {
        Self {
            header: block.header,
        }
    }
}

#[allow(dead_code)]
impl StorageShareBlock {
    /// Convert back to ShareBlock with empty transactions
    pub fn into_share_block(self) -> ShareBlock {
        ShareBlockBuilder::new(self.header).build()
    }

    /// Convert back to ShareBlock with provided transactions
    pub fn into_share_block_with_transactions(self, transactions: Vec<Transaction>) -> ShareBlock {
        ShareBlockBuilder::new(self.header)
            .with_transactions(transactions)
            .build()
    }

    /// Serialize the message to CBOR bytes
    pub fn cbor_serialize(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut buf = Vec::new();
        if let Err(e) = ciborium::ser::into_writer(&self, &mut buf) {
            return Err(e.into());
        }
        Ok(buf)
    }

    /// Deserialize a message from CBOR bytes
    pub fn cbor_deserialize(bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        match ciborium::de::from_reader(bytes) {
            Ok(msg) => Ok(msg),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::messages::Message;
    use crate::test_utils::simple_miner_share;
    use crate::test_utils::TestBlockBuilder;
    use bitcoin::absolute::Time;
    use rust_decimal_macros::dec;
    use std::collections::HashSet;

    #[test]
    fn test_build_genesis_share_header() {
        let bitcoin_blockhash = "000000000822bbfaf34d53fc43d0c1382054d3aafe31893020c315db8b0a19f9"
            .parse()
            .unwrap();
        let share = ShareBlock::build_genesis_for_network(bitcoin::Network::Signet);

        assert_eq!(share.header.miner_share.workinfoid, 0);
        assert_eq!(share.header.miner_share.clientid, 0);
        assert_eq!(share.header.miner_share.enonce1, "fdf8b667");
        assert_eq!(share.header.miner_share.nonce2, "0000000000000000");
        assert_eq!(share.header.miner_share.nonce, "f15f1590");
        assert_eq!(
            share.header.miner_share.ntime,
            Time::from_consensus(1740044600).unwrap()
        );
        assert_eq!(share.header.miner_share.diff, dec!(1.0));
        assert_eq!(share.header.miner_share.sdiff, dec!(31.465847594928551));
        assert_eq!(share.header.miner_share.hash, bitcoin_blockhash);
        assert!(share.header.uncles.is_empty());
        assert_eq!(
            share.header.miner_pubkey.to_string(),
            "02ac493f2130ca56cb5c3a559860cef9a84f90b5a85dfe4ec6e6067eeee17f4d2d"
        );
        assert_eq!(share.transactions.len(), 1);
        assert!(share.transactions[0].is_coinbase());
        assert_eq!(share.transactions[0].output.len(), 1);
        assert_eq!(share.transactions[0].input.len(), 1);

        let output = &share.transactions[0].output[0];
        assert_eq!(output.value.to_sat(), 1);

        let expected_address = bitcoin::Address::p2pkh(
            "02ac493f2130ca56cb5c3a559860cef9a84f90b5a85dfe4ec6e6067eeee17f4d2d"
                .parse::<PublicKey>()
                .unwrap(),
            bitcoin::Network::Signet,
        );
        assert_eq!(output.script_pubkey, expected_address.script_pubkey());
        assert_eq!(
            share.cached_blockhash,
            Some("a93fc3ede1c185da86c399bdf1cbd36739ac956b3c0953acaf5f84f8782fd1d2".into())
        );
    }

    #[test]
    fn test_share_serialization() {
        let share = TestBlockBuilder::new()
            .blockhash("0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5")
            .prev_share_blockhash(
                "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb4".into(),
            )
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .workinfoid(7452731920372203525)
            .clientid(1)
            .diff(dec!(1.0))
            .sdiff(dec!(1.9041854952356509))
            .build();

        let serialized = Message::MiningShare(share.clone())
            .cbor_serialize()
            .unwrap();
        let deserialized = Message::cbor_deserialize(&serialized).unwrap();

        let deserialized = match deserialized {
            Message::MiningShare(share) => share,
            _ => panic!("Expected MiningShare variant"),
        };

        // assert_eq!(
        //     share.header.miner_share.hash,
        //     deserialized.header.miner_share.hash
        // );
        assert_eq!(
            share.header.prev_share_blockhash,
            deserialized.header.prev_share_blockhash
        );
        assert_eq!(share.header.uncles, deserialized.header.uncles);
        assert_eq!(share.header.miner_pubkey, deserialized.header.miner_pubkey);
        assert_eq!(share.transactions, deserialized.transactions);

        // Only compare non-skipped fields from MinerShare
        assert_eq!(
            share.header.miner_share.workinfoid,
            deserialized.header.miner_share.workinfoid
        );
        assert_eq!(
            share.header.miner_share.clientid,
            deserialized.header.miner_share.clientid
        );
        assert_eq!(
            share.header.miner_share.enonce1,
            deserialized.header.miner_share.enonce1
        );
        assert_eq!(
            share.header.miner_share.nonce2,
            deserialized.header.miner_share.nonce2
        );
        assert_eq!(
            share.header.miner_share.nonce,
            deserialized.header.miner_share.nonce
        );
        assert_eq!(
            share.header.miner_share.ntime,
            deserialized.header.miner_share.ntime
        );
        assert_eq!(
            share.header.miner_share.diff,
            deserialized.header.miner_share.diff
        );
        assert_eq!(
            share.header.miner_share.sdiff,
            deserialized.header.miner_share.sdiff
        );
        assert_eq!(
            share.header.miner_share.username,
            deserialized.header.miner_share.username
        );
    }

    #[test]
    fn test_share_block_new_includes_coinbase_transaction() {
        // Create a test public key
        let pubkey = "020202020202020202020202020202020202020202020202020202020202020202"
            .parse()
            .unwrap();

        // Create a miner share with test values
        let miner_share = simple_miner_share(
            Some("0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5"),
            Some(7452731920372203525),
            Some(1),
            Some(dec!(1.0)),
            Some(dec!(1.9041854952356509)),
        );

        // Create a new share block using ShareBlock::new
        let share = ShareBlock::new(miner_share, pubkey, bitcoin::Network::Regtest, &mut vec![]);

        // Verify the coinbase transaction exists and has expected properties
        assert!(share.transactions[0].is_coinbase());
        assert_eq!(share.transactions[0].output.len(), 1);
        assert_eq!(share.transactions[0].input.len(), 1);

        // Verify the output is a P2PKH to the miner's public key
        let output = &share.transactions[0].output[0];
        assert_eq!(output.value.to_sat(), 1);

        // Verify the output script is P2PKH for the miner's pubkey
        let expected_address = bitcoin::Address::p2pkh(pubkey, bitcoin::Network::Regtest);
        assert_eq!(output.script_pubkey, expected_address.script_pubkey());
    }

    #[test]
    fn test_storage_share_block_conversion() {
        let share = TestBlockBuilder::new()
            .blockhash("0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5")
            .prev_share_blockhash(
                "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb4".into(),
            )
            .miner_pubkey("020202020202020202020202020202020202020202020202020202020202020202")
            .workinfoid(7452731920372203525)
            .clientid(1)
            .diff(dec!(1.0))
            .sdiff(dec!(1.9041854952356509))
            .build();

        // Test conversion to StorageShareBlock
        let storage_share: StorageShareBlock = share.clone().into();

        // Verify header and miner_share are preserved
        assert_eq!(storage_share.header, share.header);

        // Test conversion back to ShareBlock with empty transactions
        let recovered_share = storage_share.clone().into_share_block();
        assert_eq!(recovered_share.header, share.header);
        assert!(recovered_share.transactions.is_empty());

        // Test conversion back with original transactions
        let recovered_share =
            storage_share.into_share_block_with_transactions(share.transactions.clone());
        assert_eq!(recovered_share, share);
    }

    #[test]
    fn test_share_block_hash_partial_eq() {
        // Create two identical ShareBlockHash instances
        let hash1: ShareBlockHash =
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5".into();
        let hash2: ShareBlockHash =
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5".into();
        let hash3: ShareBlockHash =
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb4".into();

        // Test ShareBlockHash == ShareBlockHash
        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);

        // Test &ShareBlockHash == ShareBlockHash
        assert_eq!(&hash1, hash2);
        assert_ne!(&hash1, hash3);

        // Test ShareBlockHash == &str
        assert_eq!(
            hash1,
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5"
        );
        assert_ne!(
            hash1,
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb4"
        );

        // Test &str == ShareBlockHash
        assert_eq!(
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5",
            hash1
        );
        assert_ne!(
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb4",
            hash1
        );
    }

    #[test]
    fn test_share_block_hash_in_collections() {
        // Create ShareBlockHash instances
        let hash1: ShareBlockHash =
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5".into();
        let hash2: ShareBlockHash =
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb4".into();
        let hash3: ShareBlockHash =
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5".into();

        // Test using ShareBlockHash in a HashSet
        let mut hash_set = HashSet::new();
        hash_set.insert(hash1);
        hash_set.insert(hash2);

        // Should not increase size since hash1 and hash3 are equal
        hash_set.insert(hash3);

        assert_eq!(hash_set.len(), 2);
        assert!(hash_set.contains(&hash1));
        assert!(hash_set.contains(&hash3)); // hash3 is equal to hash1
        assert!(hash_set.contains(&hash2));
    }

    #[test]
    fn test_share_block_hash_display() {
        let hash: ShareBlockHash =
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5".into();

        // Test Display implementation
        assert_eq!(
            format!("{}", hash),
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5"
        );
    }
}
