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

use crate::shares::miner_message::{MinerWorkbase, UserWorkbase};
use crate::shares::{ShareBlock, ShareBlockHash, ShareHeader};
use bitcoin::Txid;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt::Display;

/// Message trait for network messages that can be serialized/deserialized
/// The trait provides a default implementation for serialization/deserialization
/// using the ciborium crate.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Message {
    Inventory(InventoryMessage),
    NotFound(()),
    GetShareHeaders(Vec<ShareBlockHash>, ShareBlockHash),
    GetShareBlocks(Vec<ShareBlockHash>, ShareBlockHash),
    ShareHeaders(Vec<ShareHeader>),
    ShareBlock(ShareBlock),
    GetData(GetData),
    Workbase(MinerWorkbase),
    UserWorkbase(UserWorkbase),
    Transaction(bitcoin::Transaction),
    MiningShare(ShareBlock),
}

impl Message {
    /// Serialize the message to CBOR bytes
    pub fn cbor_serialize(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut buf = Vec::new();
        if let Err(e) = ciborium::ser::into_writer(&self, &mut buf) {
            return Err(e.into());
        }
        Ok(buf)
    }

    /// Deserialize a message from CBOR bytes
    pub fn cbor_deserialize(bytes: &[u8]) -> Result<Self, Box<dyn Error + Send + Sync>> {
        match ciborium::de::from_reader(bytes) {
            Ok(msg) => Ok(msg),
            Err(e) => Err(e.into()),
        }
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Message::Inventory(_) => write!(f, "Inventory"),
            Message::NotFound(_) => write!(f, "NotFound"),
            Message::GetShareHeaders(_, _) => write!(f, "GetShareHeaders"),
            Message::GetShareBlocks(_, _) => write!(f, "GetShareBlocks"),
            Message::ShareHeaders(_) => write!(f, "ShareHeaders"),
            Message::ShareBlock(_) => write!(f, "ShareBlock"),
            Message::GetData(_) => write!(f, "GetData"),
            Message::Workbase(_) => write!(f, "Workbase"),
            Message::UserWorkbase(_) => write!(f, "UserWorkbase"),
            Message::Transaction(_) => write!(f, "Transaction"),
            Message::MiningShare(_) => write!(f, "MiningShare"),
        }
    }
}

/// The inventory message used to tell a peer what we have in our inventory.
/// The message can be used to tell the peer about share headers, blocks, or transactions that this peer has.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum InventoryMessage {
    BlockHashes(Vec<ShareBlockHash>),
    TransactionHashes(Vec<Txid>),
}

/// Message for requesting data from peers
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GetData {
    Block(ShareBlockHash),
    Txid(Txid),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_inventory_message_serde() {
        let have_shares = vec![
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5".into(),
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb6".into(),
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb7".into(),
        ];

        let msg = Message::Inventory(InventoryMessage::BlockHashes(have_shares.clone()));

        // Test serialization
        let serialized = msg.cbor_serialize().unwrap();

        // Test deserialization
        let deserialized = Message::cbor_deserialize(&serialized).unwrap();

        let deserialized: Vec<ShareBlockHash> = match deserialized {
            Message::Inventory(InventoryMessage::BlockHashes(have_shares)) => have_shares,
            _ => panic!("Expected Inventory variant"),
        };

        // Verify the deserialized message matches original
        assert_eq!(deserialized.len(), 3);
        assert!(deserialized.contains(&have_shares[0]));
        assert!(deserialized.contains(&have_shares[1]));
        assert!(deserialized.contains(&have_shares[2]));
    }

    #[test]
    fn test_get_data_message_serde() {
        // Test BlockHash variant
        let block_msg = Message::GetData(GetData::Block(
            "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5".into(),
        ));
        let serialized = block_msg.cbor_serialize().unwrap();
        let deserialized = Message::cbor_deserialize(&serialized).unwrap();
        match deserialized {
            Message::GetData(GetData::Block(hash)) => {
                assert_eq!(
                    hash,
                    "0000000086704a35f17580d06f76d4c02d2b1f68774800675fb45f0411205bb5"
                )
            }
            _ => panic!("Expected BlockHash variant"),
        }

        // Test Txid variant
        let tx_msg = Message::GetData(GetData::Txid(
            Txid::from_str("d2528fc2d7a4f95ace97860f157c895b6098667df0e43912b027cfe58edf304e")
                .unwrap(),
        ));
        let serialized = tx_msg.cbor_serialize().unwrap();
        let deserialized = Message::cbor_deserialize(&serialized).unwrap();
        match deserialized {
            Message::GetData(GetData::Txid(hash)) => {
                assert_eq!(
                    hash,
                    Txid::from_str(
                        "d2528fc2d7a4f95ace97860f157c895b6098667df0e43912b027cfe58edf304e"
                    )
                    .unwrap()
                )
            }
            _ => panic!("Expected Txid variant"),
        }
    }
}
