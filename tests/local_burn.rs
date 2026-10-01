//! Local cross-check: burn notes built with the protocol crate's own constructor must pass, the
//! golden reject vectors and the old layout must fail, and every failure names its reason.

use anyhow::Result;
use miden_protocol::account::{AccountId, AccountIdVersion, AccountType, AssetCallbackFlag};
use miden_protocol::asset::{Asset, AssetAmount, FungibleAsset};
use miden_protocol::crypto::rand::{FeltRng, RandomCoin};
use miden_protocol::note::{
    Note, NoteAssets, NoteAttachment, NoteAttachmentScheme, NoteAttachments, NoteRecipient,
    NoteStorage, NoteTag, NoteType, PartialNoteMetadata,
};
use miden_protocol::{Felt, Word};
use miden_standards::note::{BurnNote, NetworkAccountTarget, NoteExecutionHint};
use miden_usdcx::note::xreserve_burn::{
    XReserveBurnNote, FIXED_XUSDC_BURN_TAG, XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME,
};
use miden_usdcx::vectors::load;
use miden_usdcx::xreserve::encoding::{CircleDomain, ForeignChainAddress, XReserveBurnItems};
use usdcx_burn_validation::{validate_burn_note, BurnValidationError};

const AMOUNT: u64 = 1_000_000;

fn faucet() -> AccountId {
    AccountId::dummy(
        [7; 15],
        AccountIdVersion::Version1,
        AccountType::Public,
        AssetCallbackFlag::Enabled,
    )
}

fn other_faucet() -> AccountId {
    AccountId::dummy(
        [8; 15],
        AccountIdVersion::Version1,
        AccountType::Public,
        AssetCallbackFlag::Enabled,
    )
}

fn burner() -> AccountId {
    AccountId::dummy(
        [1; 15],
        AccountIdVersion::Version1,
        AccountType::Private,
        AssetCallbackFlag::Disabled,
    )
}

fn rng() -> RandomCoin {
    RandomCoin::new(Word::from([1u32, 2, 3, 4]))
}

/// A 20-byte EVM address left-padded into 32 bytes, as a user would encode it.
fn recipient() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[12..].copy_from_slice(&[0xa1; 20]);
    bytes
}

fn items() -> XReserveBurnItems {
    XReserveBurnItems {
        dest_domain: CircleDomain::new(6),
        dest_recipient: ForeignChainAddress::new(recipient()),
    }
}

/// Same note as `XReserveBurnNote::new` builds, except the withdrawal attachment is given as raw
/// words so malformed payloads can be tested.
fn note_with_attachment(scheme: u16, words: Vec<Word>, faucet_id: AccountId) -> Result<Note> {
    let asset = FungibleAsset::new(faucet_id, AMOUNT)?;
    let storage = NoteStorage::new(Asset::from(asset).as_elements().to_vec())?;
    let recipient = NoteRecipient::new(rng().draw_word(), BurnNote::script(), storage);
    let metadata = PartialNoteMetadata::new(burner(), NoteType::Public)
        .with_tag(NoteTag::new(FIXED_XUSDC_BURN_TAG));
    let vault = NoteAssets::new(vec![asset.into()])?;
    let routing = NetworkAccountTarget::new(faucet_id, NoteExecutionHint::Always)?;
    let attachments = NoteAttachments::new(vec![
        NoteAttachment::from(routing),
        NoteAttachment::with_words(NoteAttachmentScheme::new(scheme)?, words)?,
    ])?;
    Ok(Note::with_attachments(
        vault,
        metadata,
        recipient,
        attachments,
    ))
}

#[test]
fn a_note_built_by_the_protocol_crate_is_valid() -> Result<()> {
    let note = XReserveBurnNote::create(
        burner(),
        faucet(),
        AssetAmount::new(AMOUNT)?,
        items(),
        &mut rng(),
    )?;
    let burn = validate_burn_note(&note, faucet())?;
    assert_eq!(burn.note_id, note.id());
    assert_eq!(burn.remote_depositor, burner());
    assert_eq!(burn.amount, AMOUNT);
    assert_eq!(burn.dest_domain, 6);
    assert_eq!(burn.dest_recipient, recipient());
    Ok(())
}

#[test]
fn a_note_routed_to_another_faucet_is_rejected() -> Result<()> {
    let note = XReserveBurnNote::create(
        burner(),
        faucet(),
        AssetAmount::new(AMOUNT)?,
        items(),
        &mut rng(),
    )?;
    assert_eq!(
        validate_burn_note(&note, other_faucet()),
        Err(BurnValidationError::RoutingTarget)
    );
    Ok(())
}

#[test]
fn a_withdrawal_to_miden_itself_is_rejected() -> Result<()> {
    let to_miden = XReserveBurnItems {
        dest_domain: CircleDomain::MIDEN,
        dest_recipient: ForeignChainAddress::new(recipient()),
    };
    let note = XReserveBurnNote::create(
        burner(),
        faucet(),
        AssetAmount::new(AMOUNT)?,
        to_miden,
        &mut rng(),
    )?;
    assert_eq!(
        validate_burn_note(&note, faucet()),
        Err(BurnValidationError::DestinationIsMiden)
    );
    Ok(())
}

/// The protocol crate's golden vectors: every accept vector validates and yields the expected
/// destination, every reject vector fails as a malformed withdrawal attachment.
#[test]
fn golden_burn_vectors_agree() -> Result<()> {
    let vectors = &load().families.bn;
    assert!(vectors.iter().any(|v| v.kind == "accept"));
    assert!(vectors.iter().any(|v| v.kind == "reject"));
    for vector in vectors {
        let note = note_with_attachment(
            XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME,
            vector.items_words(),
            faucet(),
        )?;
        let result = validate_burn_note(&note, faucet());
        match vector.kind.as_str() {
            "accept" => {
                let burn = result.unwrap_or_else(|e| panic!("{}: {e}", vector.id));
                let expected_domain = vector.dest_domain.expect("accept vector names a domain");
                assert_eq!(burn.dest_domain, expected_domain.as_u32(), "{}", vector.id);
                assert_eq!(
                    burn.dest_recipient,
                    *vector.dest_recipient().as_bytes(),
                    "{}",
                    vector.id
                );
            }
            "reject" => {
                assert_eq!(
                    result,
                    Err(BurnValidationError::WithdrawalMalformed),
                    "{}",
                    vector.id
                );
            }
            other => panic!("{}: unknown vector kind {other}", vector.id),
        }
    }
    Ok(())
}

#[test]
fn the_old_scheme_number_is_rejected() -> Result<()> {
    let words = items().encode().to_vec();
    let note = note_with_attachment(6, words, faucet())?;
    assert_eq!(
        validate_burn_note(&note, faucet()),
        Err(BurnValidationError::WithdrawalMissing)
    );
    Ok(())
}

/// The layout before protocol PR 3983: domain at felt 0, recipient at felts 1 to 8, zeros at
/// felts 9 to 11, under the new scheme number.
fn old_layout_words(recipient: [u8; 32]) -> Vec<Word> {
    let limbs = ForeignChainAddress::new(recipient).to_packed_felts();
    let mut felts = vec![Felt::from(6u32)];
    felts.extend_from_slice(&limbs);
    felts.extend_from_slice(&[Felt::from(0u32); 3]);
    felts
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| Word::from(*chunk))
        .collect()
}

/// A recipient whose first twelve bytes are not zero (a Solana account, for example) puts
/// non-zero values where the new layout requires zeros, so the old layout is rejected.
#[test]
fn the_old_layout_is_rejected_for_a_full_32_byte_recipient() -> Result<()> {
    let words = old_layout_words([0xc3; 32]);
    let note = note_with_attachment(XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME, words, faucet())?;
    assert_eq!(
        validate_burn_note(&note, faucet()),
        Err(BurnValidationError::WithdrawalMalformed)
    );
    Ok(())
}

/// Hazard, pinned on purpose: a left-padded EVM address has twelve leading zero bytes, so the old
/// layout happens to satisfy the new zero-padding rule and decodes to a DIFFERENT recipient. A
/// builder must change the word order together with the scheme number, never the number alone.
#[test]
fn the_old_layout_with_an_evm_address_decodes_to_the_wrong_recipient() -> Result<()> {
    let words = old_layout_words(recipient());
    let note = note_with_attachment(XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME, words, faucet())?;
    let burn = validate_burn_note(&note, faucet())?;
    // felts 4 to 11 of the old layout hold recipient limbs 3 to 7 and three zeros: the twenty
    // address bytes move to the front and twelve zero bytes follow
    let mut shifted = [0u8; 32];
    shifted[..20].copy_from_slice(&[0xa1; 20]);
    assert_ne!(burn.dest_recipient, recipient());
    assert_eq!(burn.dest_recipient, shifted);
    Ok(())
}

/// The command-line tool accepts a serialized note and prints the withdrawal values.
#[test]
fn the_cli_validates_a_serialized_note() -> Result<()> {
    use miden_protocol::utils::serde::Serializable;

    let note = XReserveBurnNote::create(
        burner(),
        faucet(),
        AssetAmount::new(AMOUNT)?,
        items(),
        &mut rng(),
    )?;
    let dir = std::env::temp_dir().join(format!("usdcx-burn-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("note.hex");
    std::fs::write(&path, hex::encode(note.to_bytes()))?;

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_validate-burn"))
        .arg(&path)
        .arg(faucet().to_hex())
        .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("valid burn note"), "{stdout}");
    assert!(
        stdout.contains(&format!("burnTxId        {}", note.id())),
        "{stdout}"
    );
    let depositor = miden_standards::interop::eth::EthEmbeddedAccountId::from_account_id(burner());
    assert!(
        stdout.contains(&format!(
            "remoteDepositor 0x{}",
            hex::encode(depositor.to_bytes32())
        )),
        "{stdout}"
    );
    assert!(stdout.contains("destDomain      6"), "{stdout}");
    assert!(
        stdout.contains(&format!("destRecipient   0x{}", hex::encode(recipient()))),
        "{stdout}"
    );

    let wrong = std::process::Command::new(env!("CARGO_BIN_EXE_validate-burn"))
        .arg(&path)
        .arg(other_faucet().to_hex())
        .output()?;
    assert!(!wrong.status.success());
    assert!(String::from_utf8(wrong.stdout)?
        .contains("rejected: the routing attachment targets another account"));

    let unreadable = std::process::Command::new(env!("CARGO_BIN_EXE_validate-burn"))
        .arg(dir.join("missing.hex"))
        .arg(faucet().to_hex())
        .output()?;
    assert_eq!(unreadable.status.code(), Some(2));
    std::fs::remove_dir_all(&dir)?;
    Ok(())
}
