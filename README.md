# USDCx burn note validation

A small Rust program that checks a USDCx burn note on Miden and returns the four values a
withdrawal needs: the burn ID, who burned, how much, and where the USDC should go. It runs the
same checks the Miden faucet runs before it burns, in the v17 format (0xMiden/protocol PR 3983,
merged on 2026-10-01).

It does not check that the note was consumed. That stays a separate step:
`GetNetworkNoteStatus(noteId)` must report `NullifierCommitted`.

## Five words

- **Note**: a Miden note. A burn note is created by the user and consumed by the faucet.
- **Felt**: a Miden field element, the basic unit of data. Think of it as a number below 2^64.
- **Word**: four felts.
- **Attachment**: a block of words carried by a note. A burn note has two: one that routes the
  note to the faucet, one that holds the withdrawal destination.
- **Scheme**: the small number that says what kind of attachment it is. Routing is scheme 2, the
  withdrawal destination is scheme 5 (it was 6 in v16).

## What the program returns

| Value | Where it is in the note |
|---|---|
| `burnTxId` | the note's ID, `0x` + 64 lowercase hex |
| `remoteDepositor` | the note's sender (the Miden account that burned), embedded in 32 bytes the same way as on the deposit side |
| amount | the note's single asset, in base units (six decimals) |
| destination domain and recipient | the scheme-5 attachment |

## Run

```sh
cargo run --bin validate-burn -- note.hex <faucet-id>            # reads the attachment with the SDK
cargo run --bin validate-burn -- note.hex <faucet-id> --manual   # reads it by hand
```

`note.hex` holds the note serialized with `Note::to_bytes()`, hex encoded. `GetNotesById` returns
a protobuf `CommittedNote`; turn it into a `miden_protocol::note::Note` first (`miden-client` does
this as `FetchedNote::Public(note, _)`), then write `note.to_bytes()` as hex. A private note has no
details on the node and cannot be checked.

`<faucet-id>` is the USDCx faucet, as `0x…` hex or bech32.

Exit code 0: every check passed, the values are printed. Exit code 1: the first failed check is
printed. Exit code 2: the input could not be read (usage, hex, note bytes or faucet ID), nothing
was checked.

## The checks, in order (`src/lib.rs`)

1. The note is public.
2. Its script is the standard `BurnNote` script (`BurnNote::script_root()` of the deployed
   `miden-standards` release). This is what makes "consumed" mean "burned by the faucet".
3. It has exactly two attachments.
4. The scheme-2 attachment names our faucet.
5. The scheme-5 attachment decodes (rules below).
6. The destination domain is not Miden (10007).
7. It has exactly one asset: fungible USDCx issued by our faucet.
8. The note storage equals the asset's eight felts. The amount is read from the asset.

The note tag (`BURN`, `0x4255524E`) is not a check. Our wallet sets it, but neither the faucet nor
our attester refuses a burn because of its tag, so a verifier must not either.

## Reading the destination: two ways

The scheme-5 attachment is the one part of the note whose format changed between v16 and v17 (the
script root also changed, see below). You can read it with the Miden Rust SDK or by hand. Both
give the same result; `tests/manual_vs_sdk.rs` checks that.

### With the Miden Rust SDK

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

This is the decoder from the protocol's own `miden-usdcx` crate, the one our withdrawal attester
uses. It accepts what the faucet accepts and rejects what the faucet rejects.

**Moving from v16 to v17 on this path**: the code above is the same in both versions. Update
`miden-usdcx`, `miden-protocol` and `miden-standards` together to a version that contains PR 3983
and the new scheme and layout come with it. Three v16 constants no longer exist
(`XRESERVE_BURN_WITHDRAWAL_ATTACHMENT_WORDS`, `BURN_NOTE_ITEMS_FELTS`,
`XReserveBurnNote::NUM_PAYLOAD_ITEMS`; use `XUsdcBurnAttachment::NUM_WORDS`), and
`XReserveBurnItems::encode` and `decode` now work on words instead of felts. If your code uses any
of those, that line changes; the call above does not.

### By hand

`src/manual.rs` reads the attachment without the crate, for a verifier in another language. The
rules, which are exactly what the faucet enforces:

- scheme number 5;
- exactly three words, that is twelve felts;
- felt 0: the destination domain, must fit in a u32, must not be 10007;
- felts 1 to 3: must be zero;
- felts 4 to 11: the recipient, eight values that must each fit in a u32; turn each into four
  bytes, least significant byte first, and join the eight groups in order to get the 32-byte
  recipient.

**Moving from v16 to v17 on this path**, three edits: the scheme number (6 to 5); the positions
(in v16 the recipient was at felts 1 to 8 and the padding at felts 9 to 11); and the padding rule
(v16 said to ignore the padding, v17 requires it to be zero). Make all three together. With an EVM
address (twelve leading zero bytes) the old word order under scheme 5 still decodes, to a
different recipient; `tests/local_burn.rs` shows that case.

## The script root

The standard `BurnNote` script changed between the v16 and v17 releases of `miden-standards`, so
its root changed too. On the SDK path `BurnNote::script_root()` returns the root of the release you
depend on. By hand, the root cannot be derived; take it from `BurnNote::script_root()` of the
deployed release (we send it with the deployment notice).

## What the faucet does and does not check

Before it burns, the faucet checks: two attachments; the scheme-2 attachment names the faucet; the
scheme-5 attachment has three words; the domain is a u32 and is not the faucet's own domain; the
three padding felts are zero; the eight recipient values are u32; eight storage felts; one asset;
storage equals the asset; the asset was issued by the faucet. All of these are in this program.
The faucet reads its own domain from its storage; this program uses 10007 for it.

The faucet does not check the note type, the tag, or the script root as such (it only consumes
scripts on its allowlist, which is how the root is enforced). It also checks two things this
program cannot see: the minimum burn amount and the supply bound. A consumed note has passed them.

Not covered here: consumption, and a note created and consumed in the same block, which the node
erases and which therefore cannot be fetched or paid.

## Dependencies

`Cargo.toml` pins `miden-protocol`, `miden-standards` and `miden-usdcx` to the final commit of PR
3983 (`372a8f7`), the commit this program was checked against. **Do not replace the pins with
`miden-usdcx = "0.17.0-rc.9"`**: that crates.io release carries the same version string but the
v16 format (scheme 6, old layout). Once a release contains the PR, the three lines move to it.
Rust 1.98.1 (`rust-toolchain.toml`). The first build fetches and compiles the protocol repository,
which takes several minutes.

## How this was checked

- `cargo test` runs twelve tests: a note built by the protocol crate validates with the expected
  values; a note routed to another faucet, a withdrawal to Miden, the old scheme number and the old
  layout are refused; the protocol's eight reference vectors (three valid, five invalid) give the
  expected result; the hand-written and the SDK decoder agree on every vector, on a built note and
  on malformed attachments; the command-line tool round-trips a note on both paths.
- `differential/differential_burn_validation.rs` is a test we ran inside the protocol tree: for
  sixteen note variants the real faucet tries to consume the note in a test chain, and this
  program's verdict must match the faucet's (two variants differ on purpose, because that test
  faucet is configured with domain 7). `differential/differential-run-2026-10-01.log` is one run,
  recorded at PR commit `d1d3e6a` with the SDK path only; the faucet code at that commit is
  identical to the pinned one. The test does not build standalone, it needs the protocol's test
  support.
- Two independent reviews of the code against the faucet's source.

Treat this program as a helper, not as the specification. The specification is the protocol code
at the pinned commit: `crates/miden-usdcx/asm/xreserve/burn_policy.masm` (what the faucet
enforces), `crates/miden-usdcx/src/xreserve/encoding/burn_note.rs` (the decoder),
`crates/miden-usdcx/src/note/xreserve_burn.rs` (scheme constant and note construction).
