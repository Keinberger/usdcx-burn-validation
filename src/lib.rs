//! Validation of a USDCx burn note, step by step, in the v17 format (0xMiden/protocol PR 3983).
//!
//! Input: a public note fetched by its ID (`GetNotesById`) and the USDCx faucet account ID.
//! Output: the values a withdrawal needs (`remoteDepositor`, amount, destination domain and
//! recipient), or the first check that failed.
//!
//! Two ways to decode the withdrawal attachment are provided, and they must agree:
//!
//! - [`validate_burn_note`] uses the Miden Rust SDK (`miden-usdcx`), the same decoder the faucet's
//!   own Rust code uses. A verifier built this way does not depend on the attachment format: the
//!   high-level API (`XUsdcBurnAttachment::try_from(&attachment)?.items()`) is the same in v16 and
//!   v17, so moving between them is a dependency bump.
//! - [`validate_burn_note_manual`] decodes the attachment by hand ([`manual`]), for a verifier
//!   written without the SDK. That path has to change its scheme number and felt offsets from v16
//!   to v17; the module spells them out.
//!
//! What neither does: prove that the note was consumed. Consumption is a separate step
//! (`GetNetworkNoteStatus(noteId)` must report `NullifierCommitted`); see `main.rs`.

pub mod manual;

use miden_protocol::account::AccountId;
use miden_protocol::note::{Note, NoteAttachment, NoteId, NoteType};
use miden_standards::interop::eth::EthEmbeddedAccountId;
use miden_standards::note::{BurnNote, NetworkAccountTarget};
use miden_usdcx::note::xreserve_burn::{
    XUsdcBurnAttachment, XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME,
};
use miden_usdcx::xreserve::encoding::CircleDomain;

/// Everything a withdrawal request needs from one valid burn note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedBurn {
    /// Circle's `burnTxId` is this value as `0x` + 64 lowercase hex characters.
    pub note_id: NoteId,
    /// The Miden account that created the note.
    pub remote_depositor: AccountId,
    /// Circle's `remoteDepositor`: that account embedded as bytes32 the same way as on the
    /// deposit side (`EthEmbeddedAccountId::to_bytes32`), hex encoded in the withdrawal request.
    pub remote_depositor_bytes32: [u8; 32],
    /// Burned amount in base units (six decimals).
    pub amount: u64,
    /// Circle domain the user wants to be paid on.
    pub dest_domain: u32,
    /// Recipient on that domain, 32 bytes as the user encoded them.
    pub dest_recipient: [u8; 32],
}

/// The first check that failed, in the order the checks run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BurnValidationError {
    #[error("the note is not public")]
    NotPublic,
    #[error("the script root is not the BurnNote script")]
    WrongScript,
    #[error("expected exactly two attachments, found {0}")]
    AttachmentCount(usize),
    #[error("the routing attachment (scheme 2) is missing or malformed")]
    RoutingMissing,
    #[error("the routing attachment targets another account")]
    RoutingTarget,
    #[error("the withdrawal attachment (scheme {XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME}) is missing")]
    WithdrawalMissing,
    #[error(
        "the withdrawal attachment is malformed (not three words, non-zero padding, or a domain or recipient value that is not a u32)"
    )]
    WithdrawalMalformed,
    #[error("the destination domain is Miden itself")]
    DestinationIsMiden,
    #[error("expected exactly one asset, found {0}")]
    AssetCount(usize),
    #[error("the asset is not fungible USDCx issued by this faucet")]
    WrongAsset,
    #[error("the note storage does not equal the asset")]
    StorageMismatch,
}

/// A decoder for the withdrawal attachment: destination domain and 32-byte recipient.
type Decoder = fn(&NoteAttachment) -> Result<(u32, [u8; 32]), BurnValidationError>;

/// Decodes the withdrawal attachment with the Miden Rust SDK: the `miden-usdcx` crate's own
/// decoder, which is what the faucet's Rust code and the withdrawal attester use.
///
/// This call is the same in v16 and v17. The scheme number and the layout live inside the crate,
/// so a verifier written this way moves between the two by bumping the dependency.
fn decode_with_sdk(attachment: &NoteAttachment) -> Result<(u32, [u8; 32]), BurnValidationError> {
    let items = XUsdcBurnAttachment::try_from(attachment)
        .map_err(|_| BurnValidationError::WithdrawalMalformed)?
        .into_items();
    Ok((items.dest_domain.as_u32(), *items.dest_recipient.as_bytes()))
}

/// Decodes the withdrawal attachment by hand, with the rules written out in [`manual`].
fn decode_by_hand(attachment: &NoteAttachment) -> Result<(u32, [u8; 32]), BurnValidationError> {
    let destination = manual::decode_withdrawal_attachment(attachment)?;
    Ok((destination.dest_domain, destination.dest_recipient))
}

/// Runs every structural check on `note` and returns the withdrawal values, decoding the
/// withdrawal attachment with the Miden Rust SDK (`miden-usdcx`).
pub fn validate_burn_note(
    note: &Note,
    faucet: AccountId,
) -> Result<VerifiedBurn, BurnValidationError> {
    validate_with(note, faucet, decode_with_sdk)
}

/// Runs every structural check on `note` and returns the withdrawal values, decoding the
/// withdrawal attachment by hand ([`manual::decode_withdrawal_attachment`]). Produces the same
/// result as [`validate_burn_note`] for every note; the tests pin that.
pub fn validate_burn_note_manual(
    note: &Note,
    faucet: AccountId,
) -> Result<VerifiedBurn, BurnValidationError> {
    validate_with(note, faucet, decode_by_hand)
}

/// The checks, in order. Only the decoding of the withdrawal attachment differs between the two
/// public entry points.
fn validate_with(
    note: &Note,
    faucet: AccountId,
    decode: Decoder,
) -> Result<VerifiedBurn, BurnValidationError> {
    // 1. Public: a private note cannot be read from the node, so it cannot be verified.
    if note.metadata().note_type() != NoteType::Public {
        return Err(BurnValidationError::NotPublic);
    }

    // 2. The script is the stock miden-standards BurnNote script. This is what makes "consumed"
    //    mean "burned by the faucet": the script only calls the faucet's `receive_and_burn`. The
    //    root changes with the miden-standards release; it is read from the crate here. A verifier
    //    with a hard-coded root must update it for v17.
    if note.script().root() != BurnNote::script_root() {
        return Err(BurnValidationError::WrongScript);
    }

    // The tag is deliberately not a criterion: our wallet sets `FIXED_XUSDC_BURN_TAG`, but neither
    // the faucet nor the attester refuses a burn for its tag, so a verifier must not either.

    // 3. Exactly two attachments: routing and withdrawal payload.
    let attachments = note.attachments();
    let count = usize::from(attachments.num_attachments());
    if count != 2 {
        return Err(BurnValidationError::AttachmentCount(count));
    }

    // 4. The routing attachment (scheme 2) names this faucet.
    let routing = attachments
        .find(NetworkAccountTarget::ATTACHMENT_SCHEME)
        .ok_or(BurnValidationError::RoutingMissing)?;
    let target =
        NetworkAccountTarget::try_from(routing).map_err(|_| BurnValidationError::RoutingMissing)?;
    if target.target_id() != faucet {
        return Err(BurnValidationError::RoutingTarget);
    }

    // 5. The withdrawal attachment decodes: exactly three words, destination domain at felt 0 as
    //    a u32, felts 1 to 3 zero, recipient at felts 4 to 11 as eight u32 values.
    let withdrawal = attachments
        .iter()
        .find(|attachment| {
            attachment.attachment_scheme().as_u16() == XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME
        })
        .ok_or(BurnValidationError::WithdrawalMissing)?;
    let (dest_domain, dest_recipient) = decode(withdrawal)?;

    // 6. Not a withdrawal to Miden itself.
    if dest_domain == CircleDomain::MIDEN.as_u32() {
        return Err(BurnValidationError::DestinationIsMiden);
    }

    // 7. Exactly one asset: fungible USDCx issued by this faucet.
    let assets = note.assets().as_slice();
    let [asset] = assets else {
        return Err(BurnValidationError::AssetCount(assets.len()));
    };
    if !asset.is_fungible() || asset.faucet_id() != faucet {
        return Err(BurnValidationError::WrongAsset);
    }

    // 8. The storage carries exactly that asset (eight felts). The amount is read from the asset.
    if note.storage().items() != asset.as_elements() {
        return Err(BurnValidationError::StorageMismatch);
    }
    let amount = u64::from(asset.unwrap_fungible().amount());

    let sender = note.metadata().sender();
    Ok(VerifiedBurn {
        note_id: note.id(),
        remote_depositor: sender,
        remote_depositor_bytes32: EthEmbeddedAccountId::from_account_id(sender).to_bytes32(),
        amount,
        dest_domain,
        dest_recipient,
    })
}
