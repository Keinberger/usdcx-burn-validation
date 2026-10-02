# USDCx burn note validation example

Validates one USDCx burn note in the v17 format (0xMiden/protocol PR 3983), with the same checks
the faucet's burn policy enforces before it burns. It covers the structural checks; consumption is
a separate step (`GetNetworkNoteStatus(noteId)` must report `NullifierCommitted`).

The withdrawal attachment, the part of the note that changed between v16 and v17, can be decoded
in two ways, and this crate shows both:

| | With the Miden Rust SDK (`miden-usdcx`) | By hand |
|---|---|---|
| Entry point | `validate_burn_note` | `validate_burn_note_manual` |
| Decoder | `XUsdcBurnAttachment::try_from(&attachment)?.items()` | `src/manual.rs` |
| For | a verifier written in Rust | a verifier in another language, or one that reads felts itself |
| v16 to v17 | bump the crate; the call is unchanged, scheme and layout come with it | change the scheme number (6 to 5) and the felt offsets |

Both paths produce the same result for every note; `tests/manual_vs_sdk.rs` pins that against the
protocol's golden vectors and hand-made edge cases.

## Run

```sh
cargo run --bin validate-burn -- note.hex <faucet-id>            # decodes with the crate
cargo run --bin validate-burn -- note.hex <faucet-id> --manual   # decodes by hand
```

`note.hex` holds the note serialized with `Note::to_bytes()`, hex encoded. `GetNotesById` returns a
protobuf `CommittedNote`; convert it into a `miden_protocol::note::Note` first (`miden-client` does
this as `FetchedNote::Public(note, _)`), then write `note.to_bytes()` as hex. A note whose details
the node did not return (a private note) cannot be validated.

`<faucet-id>` is the USDCx faucet, `0x…` hex or bech32. Exit code 0 means every check passed and
the withdrawal values are printed; exit code 1 prints the first check that failed; exit code 2 means
the input could not be read (usage, hex, note bytes or faucet id) and nothing was validated.

## The checks, in order (`src/lib.rs`)

1. public note
2. script root equals `BurnNote::script_root()` of the deployed `miden-standards` release
3. exactly two attachments
4. scheme-2 `NetworkAccountTarget` names this faucet
5. the scheme-5 withdrawal attachment decodes (see below)
6. domain is not Miden (`10007`)
7. exactly one asset, fungible USDCx from this faucet
8. note storage equals the asset's eight felts; the amount is read from the asset

The values returned: `burnTxId` is the note ID (`0x` + 64 lowercase hex); `remoteDepositor` is
`metadata.sender` embedded as bytes32 the same way as on the deposit side (the Ethereum embedding
of the Miden account ID, `remote_depositor_bytes32`); the amount is in base units (six decimals).

The note tag (`BURN`, `0x4255524E`) is not a criterion: our wallet sets it, but neither the faucet
nor the attester refuses a burn for its tag, so a verifier must not either.

## Path 1: with the Miden Rust SDK

```rust
use miden_usdcx::note::xreserve_burn::{XUsdcBurnAttachment, XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME};

let withdrawal = note
    .attachments()
    .iter()
    .find(|a| a.attachment_scheme().as_u16() == XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_SCHEME)
    .ok_or(BurnValidationError::WithdrawalMissing)?;
let items = XUsdcBurnAttachment::try_from(withdrawal)
    .map_err(|_| BurnValidationError::WithdrawalMalformed)?
    .into_items();
let dest_domain = items.dest_domain.as_u32();
let dest_recipient: [u8; 32] = *items.dest_recipient.as_bytes();
```

This is the decoder the faucet's own Rust code and the Miden withdrawal attester use. It accepts
exactly what the burn policy accepts and rejects exactly what the policy rejects (the crate's
golden vectors pin both). The scheme constant and the layout are inside the crate, so **a verifier
written this way moves from v16 to v17 by bumping the dependency**; the call above is the same in
both. Only the low-level codec changed (`Vec<Felt>` to `[Word; 3]`). The same holds for the script
root: `BurnNote::script_root()` returns the root of the `miden-standards` release you depend on.

Until a release of `miden-usdcx` contains PR 3983, pin the git commit as this crate's `Cargo.toml`
does.

## Path 2: by hand

`src/manual.rs` decodes the attachment without the crate. The rules, which are what the faucet's
MASM policy asserts:

- scheme number 5 (v16: 6);
- exactly three words, that is twelve felts;
- felt 0: destination domain, must fit in a u32, must not be 10007;
- felts 1 to 3: must be zero;
- felts 4 to 11: the recipient, eight values that must each fit in a u32; each value becomes four
  bytes, least significant byte first, and the eight groups are joined in order into the 32-byte
  recipient.

v16 had the recipient at felts 1 to 8 and the padding at felts 9 to 11. Change the scheme number
and the offsets together: with a left-padded EVM address (twelve leading zero bytes) the old word
order under scheme 5 still decodes, to a different recipient (`tests/local_burn.rs` pins that
hazard). A hand-written verifier also has to update its `BurnNote` script root for v17; that root
is not derivable by hand, take it from `miden_standards::note::BurnNote::script_root()` of the
deployed release.

## What the faucet does and does not enforce

Before it burns, the faucet's policy enforces: exactly two attachments, the scheme-2 target is this
faucet, the scheme-5 attachment has three words, the domain is a u32 other than the faucet's
configured domain (10007), the three padding felts are zero, the eight recipient values are u32. The
stock burn script enforces: eight storage felts, one asset, storage equals the asset.
`receive_and_burn` enforces that the asset was issued by this faucet. All of these are in this
program.

Not enforced on chain: the note type, the tag and the script root (the script root is enforced
indirectly: the faucet only consumes allowlisted note scripts). Two on-chain checks are account
state this program cannot read: the minimum burn amount and the supply bound. A consumed note has
passed them.

Not covered: consumption, and a note created and consumed in the same block, which the node erases
and which therefore cannot be fetched or paid.

## Tests

- `tests/local_burn.rs`: a note built by the protocol crate's own constructor validates and yields
  the expected values; routed to another faucet is rejected; a withdrawal to Miden itself is
  rejected; the protocol crate's golden vectors agree (all accept vectors validate, all reject
  vectors fail); the old scheme number is rejected; the old layout is rejected for a recipient with
  non-zero leading bytes; the EVM-address hazard is pinned; the command-line tool round-trips a
  serialized note.
- `tests/manual_vs_sdk.rs`: the hand-written decoder and the SDK decoder agree on every golden
  vector, on a crate-built note and on hand-made malformed attachments; the manual constants equal
  the crate's.

```sh
cargo test
```

## Dependencies

`Cargo.toml` pins `miden-protocol`, `miden-standards` and `miden-usdcx` as git dependencies on the
commit of 0xMiden/protocol PR 3983 this crate was validated against. The published `miden-usdcx`
release does not yet contain these burn-format changes; once a release that includes the PR exists,
the three lines move to that version. Rust 1.98.1 (`rust-toolchain.toml`). The first build fetches
the protocol repository and compiles it, which takes several minutes.

## Differential check against the faucet

`differential/differential_burn_validation.rs` is a test written to run inside the protocol tree
(as `crates/miden-usdcx/tests/`, with this crate as a dev-dependency). For each of sixteen note
variants it has the production faucet attempt to consume the note in a MockChain and asserts the
faucet's verdict (accept or reject, as expected for that variant) and this crate's verdict; two
variants differ on purpose because the test faucet is configured with domain 7 while this crate
fixes Miden to 10007. `differential/differential-run-2026-10-01.log` is one recorded run. It is
kept here for reference; it does not build standalone because it uses the protocol's test support,
and anyone relying on it should run it themselves.
