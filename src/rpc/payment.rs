#[cfg(debug_assertions)]
use crate::fiber::graph::SessionRoute;
use crate::fiber::serde_utils::SliceHex;
use crate::fiber::serde_utils::U32Hex;
use crate::fiber::{
    channel::ChannelActorStateStore,
    graph::PaymentSessionStatus,
    network::{HopHint as NetworkHopHint, SendPaymentCommand},
    serde_utils::{EntityHex, U128Hex, U64Hex},
    types::{Hash256, Pubkey},
    NetworkActorCommand, NetworkActorMessage,
};
use crate::{handle_actor_call, log_and_error};
use ckb_jsonrpc_types::Script;
use ckb_types::packed::OutPoint;
use jsonrpsee::{
    core::async_trait,
    proc_macros::rpc,
    types::{error::CALL_EXECUTION_FAILED_CODE, ErrorObjectOwned},
};
use serde_with::serde_as;
use std::collections::HashMap;

use ractor::{call, ActorRef};
use serde::{Deserialize, Serialize};

#[serde_as]
#[derive(Serialize, Deserialize, Debug)]
pub struct GetPaymentCommandParams {
    /// The payment hash of the payment to retrieve
    pub payment_hash: Hash256,
}

#[serde_as]
#[derive(Serialize, Deserialize, Clone)]
pub struct GetPaymentCommandResult {
    /// The payment hash of the payment
    pub payment_hash: Hash256,
    /// The status of the payment
    pub status: PaymentSessionStatus,
    #[serde_as(as = "U64Hex")]
    /// The time the payment was created at, in milliseconds from UNIX epoch
    created_at: u64,
    #[serde_as(as = "U64Hex")]
    /// The time the payment was last updated at, in milliseconds from UNIX epoch
    pub last_updated_at: u64,
    /// The error message if the payment failed
    pub failed_error: Option<String>,
    /// fee paid for the payment
    #[serde_as(as = "U128Hex")]
    pub fee: u128,

    /// The custom records to be included in the payment.
    pub custom_records: Option<PaymentCustomRecords>,

    #[cfg(debug_assertions)]
    /// The route information for the payment
    router: SessionRoute,
}

/// The custom records to be included in the payment.
/// The key is hex encoded of `u32`, and the value is hex encoded of `Vec<u8>` with `0x` as prefix.
/// For example:
/// ```json
/// "custom_records": {
///    "0x1": "0x01020304",
///    "0x2": "0x05060708",
///    "0x3": "0x090a0b0c",
///    "0x4": "0x0d0e0f10010d090a0b0c"
///  }
/// ```
#[serde_as]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Default)]
pub struct PaymentCustomRecords {
    /// The custom records to be included in the payment.
    #[serde(flatten)]
    #[serde_as(as = "HashMap<U32Hex, SliceHex>")]
    pub data: HashMap<u32, Vec<u8>>,
}

impl From<PaymentCustomRecords> for crate::fiber::PaymentCustomRecords {
    fn from(records: PaymentCustomRecords) -> Self {
        crate::fiber::PaymentCustomRecords { data: records.data }
    }
}

#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SendPaymentCommandParams {
    /// the identifier of the payment target
    pub target_pubkey: Option<Pubkey>,

    /// the amount of the payment
    #[serde_as(as = "Option<U128Hex>")]
    pub amount: Option<u128>,

    /// the hash to use within the payment's HTLC
    pub payment_hash: Option<Hash256>,

    /// the TLC expiry delta should be used to set the timelock for the final hop, in milliseconds
    #[serde_as(as = "Option<U64Hex>")]
    pub final_tlc_expiry_delta: Option<u64>,

    /// the TLC expiry limit for the whole payment, in milliseconds, each hop is with a default tlc delta of 1 day
    /// suppose the payment router is with N hops, the total tlc expiry limit is at least (N-1) days
    /// this is also the default value for the payment if this parameter is not provided
    #[serde_as(as = "Option<U64Hex>")]
    pub tlc_expiry_limit: Option<u64>,

    /// the encoded invoice to send to the recipient
    pub invoice: Option<String>,

    /// the payment timeout in seconds, if the payment is not completed within this time, it will be cancelled
    #[serde_as(as = "Option<U64Hex>")]
    pub timeout: Option<u64>,

    /// the maximum fee amounts in shannons that the sender is willing to pay
    #[serde_as(as = "Option<U128Hex>")]
    pub max_fee_amount: Option<u128>,

    /// max parts for the payment, only used for multi-part payments
    #[serde_as(as = "Option<U64Hex>")]
    pub max_parts: Option<u64>,

    /// keysend payment
    pub keysend: Option<bool>,

    /// udt type script for the payment
    pub udt_type_script: Option<Script>,

    /// allow self payment, default is false
    pub allow_self_payment: Option<bool>,

    /// Some custom records for the payment which contains a map of u32 to Vec<u8>
    /// The key is the record type, and the value is the serialized data
    /// For example:
    /// ```json
    /// "custom_records": {
    ///    "0x1": "0x01020304",
    ///    "0x2": "0x05060708",
    ///    "0x3": "0x090a0b0c",
    ///    "0x4": "0x0d0e0f10010d090a0b0c"
    ///  }
    /// ```
    pub custom_records: Option<PaymentCustomRecords>,

    /// Optional route hints to reach the destination through private channels.
    /// A hop hint is a hint for a node to use a specific channel, for example
    /// (pubkey, funding_txid, inbound) where pubkey is the public key of the node,
    /// funding_txid is the funding transaction hash of the channel outpoint, and
    /// inbound is a boolean indicating whether to use the channel to send or receive.
    /// Note: an improper hint may cause the payment to fail, and hop_hints maybe helpful for self payment scenario
    /// for helping the routing algorithm to find the correct path
    pub hop_hints: Option<Vec<HopHint>>,

    /// dry_run for payment, used for check whether we can build valid router and the fee for this payment,
    /// it's useful for the sender to double check the payment before sending it to the network,
    /// default is false
    pub dry_run: Option<bool>,
}

/// A hop hint is a hint for a node to use a specific channel.
#[serde_as]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HopHint {
    /// The public key of the node
    pub pubkey: Pubkey,
    /// The outpoint of the channel
    #[serde_as(as = "EntityHex")]
    pub channel_outpoint: OutPoint,

    /// The fee rate to use this hop to forward the payment.
    pub(crate) fee_rate: u64,
    /// The TLC expiry delta to use this hop to forward the payment.
    pub(crate) tlc_expiry_delta: u64,
}

impl From<HopHint> for NetworkHopHint {
    fn from(hop_hint: HopHint) -> Self {
        NetworkHopHint {
            pubkey: hop_hint.pubkey,
            channel_outpoint: hop_hint.channel_outpoint,
            fee_rate: hop_hint.fee_rate,
            tlc_expiry_delta: hop_hint.tlc_expiry_delta,
        }
    }
}

/// RPC module for channel management.
#[rpc(server)]
trait PaymentRpc {
    /// Sends a payment to a peer.
    #[method(name = "send_payment")]
    async fn send_payment(
        &self,
        params: SendPaymentCommandParams,
    ) -> Result<GetPaymentCommandResult, ErrorObjectOwned>;

    /// Retrieves a payment.
    #[method(name = "get_payment")]
    async fn get_payment(
        &self,
        params: GetPaymentCommandParams,
    ) -> Result<GetPaymentCommandResult, ErrorObjectOwned>;
}

pub(crate) struct PaymentRpcServerImpl<S> {
    actor: ActorRef<NetworkActorMessage>,
    _store: S,
}

impl<S> PaymentRpcServerImpl<S> {
    pub(crate) fn new(actor: ActorRef<NetworkActorMessage>, _store: S) -> Self {
        PaymentRpcServerImpl { actor, _store }
    }
}

#[async_trait]
impl<S> PaymentRpcServer for PaymentRpcServerImpl<S>
where
    S: ChannelActorStateStore + Send + Sync + 'static,
{
    async fn send_payment(
        &self,
        params: SendPaymentCommandParams,
    ) -> Result<GetPaymentCommandResult, ErrorObjectOwned> {
        let message = |rpc_reply| -> NetworkActorMessage {
            NetworkActorMessage::Command(NetworkActorCommand::SendPayment(
                SendPaymentCommand {
                    target_pubkey: params.target_pubkey,
                    amount: params.amount,
                    payment_hash: params.payment_hash,
                    final_tlc_expiry_delta: params.final_tlc_expiry_delta,
                    tlc_expiry_limit: params.tlc_expiry_limit,
                    invoice: params.invoice.clone(),
                    timeout: params.timeout,
                    max_fee_amount: params.max_fee_amount,
                    max_parts: params.max_parts,
                    keysend: params.keysend,
                    udt_type_script: params.udt_type_script.clone().map(|s| s.into()),
                    allow_self_payment: params.allow_self_payment.unwrap_or(false),
                    custom_records: params.custom_records.clone().map(|records| records.into()),
                    hop_hints: params
                        .hop_hints
                        .clone()
                        .map(|hints| hints.into_iter().map(|hint| hint.into()).collect()),
                    dry_run: params.dry_run.unwrap_or(false),
                },
                rpc_reply,
            ))
        };
        handle_actor_call!(self.actor, message, params).map(|response| GetPaymentCommandResult {
            payment_hash: response.payment_hash,
            status: response.status,
            created_at: response.created_at,
            last_updated_at: response.last_updated_at,
            failed_error: response.failed_error,
            fee: response.fee,
            custom_records: response
                .custom_records
                .map(|records| PaymentCustomRecords { data: records.data }),
            #[cfg(debug_assertions)]
            router: response.router,
        })
    }

    async fn get_payment(
        &self,
        params: GetPaymentCommandParams,
    ) -> Result<GetPaymentCommandResult, ErrorObjectOwned> {
        let message = |rpc_reply| -> NetworkActorMessage {
            NetworkActorMessage::Command(NetworkActorCommand::GetPayment(
                params.payment_hash,
                rpc_reply,
            ))
        };
        handle_actor_call!(self.actor, message, params).map(|response| GetPaymentCommandResult {
            payment_hash: response.payment_hash,
            status: response.status,
            last_updated_at: response.last_updated_at,
            created_at: response.created_at,
            failed_error: response.failed_error,
            fee: response.fee,
            custom_records: response
                .custom_records
                .map(|records| PaymentCustomRecords { data: records.data }),
            #[cfg(debug_assertions)]
            router: response.router,
        })
    }
}
