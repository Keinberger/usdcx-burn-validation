//! The withdrawal attachment decoded by hand, without the `miden-usdcx` decoder.
//!
//! This is the reference for a verifier that does not use the Miden Rust SDK (for example one
//! written in another language). It spells out every rule the faucet's burn policy enforces on the
//! attachment in the v17 format. The constants and offsets below are the two things that changed
//! from v16; a verifier that uses the SDK path (`crate::validate_burn_note`) gets them from the
//! crate and does not need this file.

use miden_protocol::note::NoteAttachment;
use miden_protocol::Felt;

use crate::BurnValidationError;

/// The attachment scheme that carries the withdrawal destination. v16 used 6.
pub const WITHDRAWAL_ATTACHMENT_SCHEME: u16 = 5;

/// Circle's domain identifier for Miden. A withdrawal to Miden itself is refused.
pub const MIDEN_DOMAIN: u32 = 10_007;

/// The attachment is exactly three words of four felts each.
pub const ATTACHMENT_WORDS: usize = 3;

/// The destination decoded from the attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Destination {
    pub dest_domain: u32,
    pub dest_recipient: [u8; 32],
}

/// Reads a felt that must hold an unsigned 32-bit value.
fn felt_as_u32(felt: Felt) -> Option<u32> {
    u32::try_from(felt.as_canonical_u64()).ok()
}

/// Decodes the v17 withdrawal attachment:
///
/// - scheme 5;
/// - exactly three words, that is twelve felts;
/// - felt 0: the destination domain, must fit in a u32;
/// - felts 1 to 3: must be zero;
/// - felts 4 to 11: the recipient as eight values that must each fit in a u32; each value becomes
///   four bytes, least significant byte first, and the eight groups are joined in order into the
///   32-byte recipient.
///
/// v16 differed in the scheme (6) and in the positions: the recipient sat at felts 1 to 8 and the
/// padding at felts 9 to 11.
pub fn decode_withdrawal_attachment(
    attachment: &NoteAttachment,
) -> Result<Destination, BurnValidationError> {
    if attachment.attachment_scheme().as_u16() != WITHDRAWAL_ATTACHMENT_SCHEME {
        return Err(BurnValidationError::WithdrawalMissing);
    }
    let words = attachment.content().as_words();
    if words.len() != ATTACHMENT_WORDS {
        return Err(BurnValidationError::WithdrawalMalformed);
    }
    // the three words as twelve felts, in order
    let felts: &[Felt] = attachment.content().as_elements();

    let dest_domain = felt_as_u32(felts[0]).ok_or(BurnValidationError::WithdrawalMalformed)?;
    if felts[1..4].iter().any(|felt| *felt != Felt::ZERO) {
        return Err(BurnValidationError::WithdrawalMalformed);
    }
    let mut dest_recipient = [0u8; 32];
    for (index, felt) in felts[4..12].iter().enumerate() {
        let limb = felt_as_u32(*felt).ok_or(BurnValidationError::WithdrawalMalformed)?;
        dest_recipient[index * 4..index * 4 + 4].copy_from_slice(&limb.to_le_bytes());
    }

    Ok(Destination {
        dest_domain,
        dest_recipient,
    })
}
