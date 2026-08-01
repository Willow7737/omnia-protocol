//! Financial shard operations
//!
//! Defines the three core financial operations (Transfer, Mint, Burn) and
//! a read-only BalanceQuery. All amounts are `u64` for simplicity; future
//! iterations may support arbitrary-precision decimals.

use serde::{Deserialize, Serialize};

/// Account identifier — a 32-byte public key (same size as NodeId).
pub type AccountId = [u8; 32];

/// Operations supported by the Financial shard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FinancialOp {
    /// Transfer `amount` from the event creator to `to`.
    Transfer {
        /// Recipient account.
        to: AccountId,
        /// Amount to transfer.
        amount: u64,
    },
    /// Mint `amount` and credit it to `to`.
    ///
    /// Only authorized minter accounts should be able to invoke this.
    /// Authorization is checked in the validator.
    Mint {
        /// Recipient account.
        to: AccountId,
        /// Amount to mint.
        amount: u64,
    },
    /// Burn `amount` from the `from` account.
    Burn {
        /// Account to burn from.
        from: AccountId,
        /// Amount to burn.
        amount: u64,
    },
    /// Read-only balance query — does not mutate state.
    BalanceQuery {
        /// Account to query.
        account: AccountId,
    },
    /// Transfer `amount` from `from` to `to`, authorized by an Ed25519
    /// signature from `from` rather than by the event creator.
    ///
    /// [`Transfer`](Self::Transfer) takes its sender from
    /// `event.creator_pubkey`, which means only the key that signed the
    /// carrying event can move value. That is correct for a node acting
    /// on its own behalf, but it makes the shard unusable by a wallet
    /// that does not run a node: the node would have to sign the event,
    /// and the node's key would become the sender.
    ///
    /// `SignedTransfer` separates the two roles. The node still creates
    /// and signs the carrying event (it owns the sequence number and the
    /// vector clock), but the *authority* to move value comes from
    /// `signature`, computed by the holder of `from` over
    /// [`signed_transfer_message`]. Every node re-verifies that signature
    /// independently when it applies the event, so relaying nodes are
    /// untrusted — a malicious node cannot forge a transfer, only refuse
    /// to relay one.
    SignedTransfer {
        /// Sending account — also the Ed25519 verifying key for `signature`.
        from: AccountId,
        /// Recipient account.
        to: AccountId,
        /// Amount to transfer.
        amount: u64,
        /// Strictly increasing per-sender nonce; replays are rejected.
        nonce: u64,
        /// 64-byte Ed25519 signature over [`signed_transfer_message`].
        ///
        /// `Vec<u8>` rather than `[u8; 64]` because serde has no impls for
        /// 64-element arrays; length is validated on apply.
        signature: Vec<u8>,
    },
}

/// Domain separation tag for [`FinancialOp::SignedTransfer`] signatures.
///
/// Prevents a signature produced for any other Omnia message (wallet
/// login, UBC spend authorization, …) from being replayed as a financial
/// transfer authorization.
pub const SIGNED_TRANSFER_DOMAIN: &[u8] = b"omnia-financial-transfer:v1";

/// Build the canonical message that a sender signs to authorize a
/// [`FinancialOp::SignedTransfer`].
///
/// The encoding is fixed-width and length-prefixed by construction — the
/// domain tag has a constant length and every other field is a fixed-size
/// array or a little-endian `u64` — so no field boundary can be forged by
/// choosing a crafted account or amount. Every field that determines the
/// effect of the transfer is bound: change any one of them and the
/// signature no longer verifies.
pub fn signed_transfer_message(from: &AccountId, to: &AccountId, amount: u64, nonce: u64) -> Vec<u8> {
    let mut msg = Vec::with_capacity(SIGNED_TRANSFER_DOMAIN.len() + 32 + 32 + 8 + 8);
    msg.extend_from_slice(SIGNED_TRANSFER_DOMAIN);
    msg.extend_from_slice(from);
    msg.extend_from_slice(to);
    msg.extend_from_slice(&amount.to_le_bytes());
    msg.extend_from_slice(&nonce.to_le_bytes());
    msg
}
