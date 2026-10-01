//! Differential check for the Circle validation example: for every note variant, the production
//! faucet (MockChain, real burn policy) and `validate_burn_note` must reach the same verdict, and
//! on acceptance the validator must return the destination the attester would submit.
//!
//! Two variants diverge on purpose in this harness only, because the test faucet is configured
//! with domain 7 while the validator fixes Miden's domain to 10007; both are asserted explicitly.

mod support;

use anyhow::Result;
use miden_protocol::account::AccountId;
use miden_protocol::asset::{Asset, FungibleAsset};
use miden_protocol::note::{
    Note, NoteAssets, NoteAttachment, NoteAttachmentScheme, NoteAttachments, NoteRecipient,
    NoteStorage, NoteTag, NoteType, PartialNoteMetadata,
};
use miden_protocol::{Felt, Word};
use miden_standards::note::{NetworkAccountTarget, NoteExecutionHint};
use miden_usdcx::note::xreserve_burn::{
    FIXED_XUSDC_BURN_TAG, XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME, XReserveBurnNote,
    XUsdcBurnAttachment,
};
use miden_usdcx::xreserve::encoding::{CircleDomain, ForeignChainAddress, XReserveBurnItems};
use rstest::rstest;
use support::*;
use usdcx_burn_validation::validate_burn_note;

const TOKEN_SUPPLY: u64 = 100_000;
const VALID_BURN: u64 = 5_000;
const DEST_DOMAIN: u32 = 9;

/// A 20-byte EVM address left-padded into 32 bytes.
fn evm_recipient() -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[12..].copy_from_slice(&[0xa1; 20]);
    bytes
}

fn items(domain: u32, recipient: [u8; 32]) -> XReserveBurnItems {
    XReserveBurnItems {
        dest_domain: CircleDomain::new(domain),
        dest_recipient: ForeignChainAddress::new(recipient),
    }
}

fn raw_burn_note(
    sender: AccountId,
    faucet_id: AccountId,
    tag: u32,
    attachments: Vec<NoteAttachment>,
) -> Note {
    let asset = FungibleAsset::new(faucet_id, VALID_BURN).expect("valid burn asset");
    let storage = NoteStorage::new(Asset::from(asset).as_elements().to_vec())
        .expect("stock burn asset storage");
    Note::with_attachments(
        NoteAssets::new(vec![asset.into()]).expect("one burn asset"),
        PartialNoteMetadata::new(sender, NoteType::Public).with_tag(NoteTag::new(tag)),
        NoteRecipient::new(Word::from([1u32, 2, 3, 4]), XReserveBurnNote::script(), storage),
        NoteAttachments::new(attachments).expect("attachments within protocol limits"),
    )
}

fn routing(target_id: AccountId) -> NoteAttachment {
    NetworkAccountTarget::new(target_id, NoteExecutionHint::Always)
        .expect("public network account")
        .into()
}

fn withdrawal(items: XReserveBurnItems) -> NoteAttachment {
    NoteAttachment::from(&XUsdcBurnAttachment::new(items))
}

fn withdrawal_words(scheme: u16, words: Vec<Word>) -> NoteAttachment {
    NoteAttachment::with_words(NoteAttachmentScheme::new(scheme).expect("scheme"), words)
        .expect("attachment")
}

fn with_element(mut attachment_words: Vec<Word>, index: usize, value: Felt) -> Vec<Word> {
    attachment_words[index / Word::NUM_ELEMENTS][index % Word::NUM_ELEMENTS] = value;
    attachment_words
}

fn new_layout_words(items: XReserveBurnItems) -> Vec<Word> {
    items.encode().to_vec()
}

/// The layout before protocol PR 3983: domain at felt 0, recipient at felts 1 to 8, zeros after.
fn old_layout_words(items: XReserveBurnItems) -> Vec<Word> {
    let mut felts = vec![Felt::from(items.dest_domain.as_u32())];
    felts.extend_from_slice(&items.dest_recipient.to_packed_felts());
    felts.extend_from_slice(&[Felt::ZERO; 3]);
    felts.as_chunks::<4>().0.iter().map(|chunk| Word::from(*chunk)).collect()
}

fn non_u32() -> Felt {
    Felt::from(u32::MAX) + Felt::ONE
}

type NoteFor = fn(AccountId, AccountId) -> Note;

fn valid(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![routing(faucet), withdrawal(items(DEST_DOMAIN, evm_recipient()))],
    )
}

fn built_by_the_crate(sender: AccountId, faucet: AccountId) -> Note {
    let mut rng = miden_protocol::crypto::rand::RandomCoin::new(Word::from([5u32, 6, 7, 8]));
    XReserveBurnNote::create(
        sender,
        faucet,
        miden_protocol::asset::AssetAmount::new(VALID_BURN).expect("amount"),
        items(DEST_DOMAIN, [0xc3; 32]),
        &mut rng,
    )
    .expect("burn note")
}

fn other_tag(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        0x1234_5678,
        vec![routing(faucet), withdrawal(items(DEST_DOMAIN, evm_recipient()))],
    )
}

fn missing_withdrawal(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(sender, faucet, FIXED_XUSDC_BURN_TAG, vec![routing(faucet)])
}

fn four_words(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![
            routing(faucet),
            withdrawal_words(XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME, vec![Word::empty(); 4]),
        ],
    )
}

fn extra_attachment(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![
            routing(faucet),
            withdrawal(items(DEST_DOMAIN, evm_recipient())),
            withdrawal_words(7, vec![Word::empty()]),
        ],
    )
}

fn missing_routing(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![
            withdrawal(items(DEST_DOMAIN, evm_recipient())),
            withdrawal(items(DEST_DOMAIN, evm_recipient())),
        ],
    )
}

fn routed_elsewhere(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![routing(test_faucet_id(42)), withdrawal(items(DEST_DOMAIN, evm_recipient()))],
    )
}

fn domain_not_u32(sender: AccountId, faucet: AccountId) -> Note {
    let words = with_element(new_layout_words(items(DEST_DOMAIN, evm_recipient())), 0, non_u32());
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![
            routing(faucet),
            withdrawal_words(XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME, words),
        ],
    )
}

fn padding_not_zero(sender: AccountId, faucet: AccountId) -> Note {
    let words = with_element(new_layout_words(items(DEST_DOMAIN, evm_recipient())), 3, Felt::ONE);
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![
            routing(faucet),
            withdrawal_words(XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME, words),
        ],
    )
}

fn recipient_limb_not_u32(sender: AccountId, faucet: AccountId) -> Note {
    let words = with_element(new_layout_words(items(DEST_DOMAIN, evm_recipient())), 9, non_u32());
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![
            routing(faucet),
            withdrawal_words(XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME, words),
        ],
    )
}

fn old_scheme_number(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![
            routing(faucet),
            withdrawal_words(6, new_layout_words(items(DEST_DOMAIN, evm_recipient()))),
        ],
    )
}

fn old_layout_full_recipient(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![
            routing(faucet),
            withdrawal_words(
                XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME,
                old_layout_words(items(DEST_DOMAIN, [0xc3; 32])),
            ),
        ],
    )
}

fn old_layout_evm_recipient(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![
            routing(faucet),
            withdrawal_words(
                XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME,
                old_layout_words(items(DEST_DOMAIN, evm_recipient())),
            ),
        ],
    )
}

fn domain_is_test_faucet_domain(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![routing(faucet), withdrawal(items(TEST_DOMAIN.as_u32(), evm_recipient()))],
    )
}

fn domain_is_miden(sender: AccountId, faucet: AccountId) -> Note {
    raw_burn_note(
        sender,
        faucet,
        FIXED_XUSDC_BURN_TAG,
        vec![
            routing(faucet),
            withdrawal(items(CircleDomain::MIDEN.as_u32(), evm_recipient())),
        ],
    )
}

async fn faucet_verdict(pf: &mut ProductionFaucet, note: &Note) -> Result<bool> {
    let result = pf
        .mock_chain
        .build_transaction(pf.faucet_id)
        .authenticated_input_note(note.id())
        .build()?
        .execute()
        .await;
    Ok(result.is_ok())
}

/// `faucet` is what the real burn policy does with the note, `validator` what the example says.
#[rstest]
#[case::valid(valid, true, true)]
#[case::built_by_the_crate(built_by_the_crate, true, true)]
#[case::other_tag(other_tag, true, true)]
#[case::missing_withdrawal(missing_withdrawal, false, false)]
#[case::four_words(four_words, false, false)]
#[case::extra_attachment(extra_attachment, false, false)]
#[case::missing_routing(missing_routing, false, false)]
#[case::routed_elsewhere(routed_elsewhere, false, false)]
#[case::domain_not_u32(domain_not_u32, false, false)]
#[case::padding_not_zero(padding_not_zero, false, false)]
#[case::recipient_limb_not_u32(recipient_limb_not_u32, false, false)]
#[case::old_scheme_number(old_scheme_number, false, false)]
#[case::old_layout_full_recipient(old_layout_full_recipient, false, false)]
#[case::old_layout_evm_recipient(old_layout_evm_recipient, true, true)]
// Harness-only divergences: the test faucet's domain is 7, the validator fixes Miden to 10007.
#[case::domain_is_test_faucet_domain(domain_is_test_faucet_domain, false, true)]
#[case::domain_is_miden(domain_is_miden, true, false)]
#[tokio::test]
async fn faucet_and_validator_agree(
    #[case] note_for: NoteFor,
    #[case] faucet: bool,
    #[case] validator: bool,
) -> Result<()> {
    let mut pf = setup_production_faucet(TOKEN_SUPPLY, |sender, faucet_id| {
        vec![note_for(sender, faucet_id)]
    })?;
    let note = pf.seeded_notes[0].clone();

    let accepted = faucet_verdict(&mut pf, &note).await?;
    assert_eq!(accepted, faucet, "faucet verdict differs from the expectation");

    let verdict = validate_burn_note(&note, pf.faucet_id);
    assert_eq!(verdict.is_ok(), validator, "validator verdict: {verdict:?}");

    if let Ok(burn) = verdict {
        // The destination the validator reports is exactly what the crate decoder yields from the
        // attachment, which is what the attester puts into Circle's prepare request.
        let attachment = note
            .attachments()
            .iter()
            .find(|a| a.attachment_scheme().as_u16() == XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME)
            .expect("withdrawal attachment");
        let decoded = XUsdcBurnAttachment::try_from(attachment)?.into_items();
        assert_eq!(burn.dest_domain, decoded.dest_domain.as_u32());
        assert_eq!(burn.dest_recipient, *decoded.dest_recipient.as_bytes());
        assert_eq!(burn.amount, VALID_BURN);
        assert_eq!(burn.remote_depositor, pf.recipient_id);
        assert_eq!(burn.note_id, note.id());
    }
    Ok(())
}

/// The hazard in numbers: the old word order with an EVM address is consumed by the faucet and
/// decodes to a recipient that is not the one the user wrote.
#[tokio::test]
async fn old_layout_with_evm_address_pays_a_different_recipient() -> Result<()> {
    let pf = setup_production_faucet(TOKEN_SUPPLY, |sender, faucet_id| {
        vec![old_layout_evm_recipient(sender, faucet_id)]
    })?;
    let burn = validate_burn_note(&pf.seeded_notes[0], pf.faucet_id)?;
    assert_ne!(burn.dest_recipient, evm_recipient());
    Ok(())
}
