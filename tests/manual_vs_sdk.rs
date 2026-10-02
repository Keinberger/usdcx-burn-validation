//! The hand-written decoder and the SDK decoder must agree on every note: same accept, same
//! reject, same values. This is what lets a verifier without the SDK copy `src/manual.rs`.

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
use usdcx_burn_validation::manual::{MIDEN_DOMAIN, WITHDRAWAL_ATTACHMENT_SCHEME};
use usdcx_burn_validation::{validate_burn_note, validate_burn_note_manual};

const AMOUNT: u64 = 1_000_000;

fn faucet() -> AccountId {
    AccountId::dummy(
        [7; 15],
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
    RandomCoin::new(Word::from([9u32, 8, 7, 6]))
}

fn note_with_attachment(scheme: u16, words: Vec<Word>) -> Result<Note> {
    let asset = FungibleAsset::new(faucet(), AMOUNT)?;
    let storage = NoteStorage::new(Asset::from(asset).as_elements().to_vec())?;
    let recipient = NoteRecipient::new(rng().draw_word(), BurnNote::script(), storage);
    let metadata = PartialNoteMetadata::new(burner(), NoteType::Public)
        .with_tag(NoteTag::new(FIXED_XUSDC_BURN_TAG));
    let vault = NoteAssets::new(vec![asset.into()])?;
    let routing = NetworkAccountTarget::new(faucet(), NoteExecutionHint::Always)?;
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

/// The constants the manual path hard-codes are the crate's constants.
#[test]
fn manual_constants_equal_the_sdk_constants() {
    assert_eq!(
        WITHDRAWAL_ATTACHMENT_SCHEME,
        XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME
    );
    assert_eq!(MIDEN_DOMAIN, CircleDomain::MIDEN.as_u32());
}

/// Every golden vector, accept and reject, gives the same result through both decoders.
#[test]
fn both_decoders_agree_on_every_golden_vector() -> Result<()> {
    for vector in &load().families.bn {
        let note = note_with_attachment(WITHDRAWAL_ATTACHMENT_SCHEME, vector.items_words())?;
        let sdk = validate_burn_note(&note, faucet());
        let manual = validate_burn_note_manual(&note, faucet());
        assert_eq!(sdk, manual, "{}", vector.id);
        if vector.kind == "accept" {
            let burn = sdk.expect("accept vector");
            assert_eq!(
                burn.dest_recipient,
                *vector.dest_recipient().as_bytes(),
                "{}",
                vector.id
            );
        }
    }
    Ok(())
}

/// A note built by the crate decodes identically by hand.
#[test]
fn both_decoders_agree_on_a_crate_built_note() -> Result<()> {
    let mut recipient = [0u8; 32];
    recipient[12..].copy_from_slice(&[0x5c; 20]);
    let items = XReserveBurnItems {
        dest_domain: CircleDomain::new(3),
        dest_recipient: ForeignChainAddress::new(recipient),
    };
    let note = XReserveBurnNote::create(
        burner(),
        faucet(),
        AssetAmount::new(AMOUNT)?,
        items,
        &mut rng(),
    )?;
    let sdk = validate_burn_note(&note, faucet())?;
    let manual = validate_burn_note_manual(&note, faucet())?;
    assert_eq!(sdk, manual);
    assert_eq!(manual.dest_domain, 3);
    assert_eq!(manual.dest_recipient, recipient);
    Ok(())
}

/// Hand-made edge cases: a non-u32 limb, non-zero padding, a wrong word count and the old scheme
/// number fail the same way through both decoders.
#[test]
fn both_decoders_agree_on_malformed_attachments() -> Result<()> {
    let good = XReserveBurnItems {
        dest_domain: CircleDomain::new(3),
        dest_recipient: ForeignChainAddress::new([0xc3; 32]),
    }
    .encode()
    .to_vec();
    let non_u32 = Felt::from(u32::MAX) + Felt::ONE;

    let mut bad_limb = good.clone();
    bad_limb[2][3] = non_u32;
    let mut bad_padding = good.clone();
    bad_padding[0][2] = Felt::ONE;
    let two_words = good[..2].to_vec();

    for (name, scheme, words) in [
        (
            "recipient limb not u32",
            WITHDRAWAL_ATTACHMENT_SCHEME,
            bad_limb,
        ),
        (
            "padding not zero",
            WITHDRAWAL_ATTACHMENT_SCHEME,
            bad_padding,
        ),
        ("two words", WITHDRAWAL_ATTACHMENT_SCHEME, two_words),
        ("old scheme number", 6, good.clone()),
    ] {
        let note = note_with_attachment(scheme, words)?;
        let sdk = validate_burn_note(&note, faucet());
        let manual = validate_burn_note_manual(&note, faucet());
        assert!(sdk.is_err(), "{name}: sdk accepted");
        assert_eq!(sdk, manual, "{name}");
    }
    Ok(())
}
