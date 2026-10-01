# USDCx burn note validation example

Validates one USDCx burn note with the decoder the faucet's own Rust code uses (`miden-usdcx` from
0xMiden/protocol PR 3983). The check set and order follow the Miden withdrawal attester's structural
filter; one check goes further than the attester and matches the faucet instead: the rejection of
Miden as destination. It covers the structural checks; consumption is a separate step
(`GetNetworkNoteStatus(noteId)` must report `NullifierCommitted`).

## Run

```sh
cargo run --bin validate-burn -- note.hex <faucet-id>
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
2. script root equals `BurnNote::script_root()`
3. exactly two attachments
4. scheme-2 `NetworkAccountTarget` names this faucet
5. scheme-5 withdrawal attachment decodes: three words, domain at felt 0 as u32, felts 1 to 3 zero, recipient at felts 4 to 11 as eight u32 values
6. domain is not Miden (`10007`)
7. exactly one asset, fungible USDCx from this faucet
8. note storage equals the asset's eight felts; the amount is read from the asset

The values returned: `burnTxId` is the note ID (`0x` + 64 lowercase hex); `remoteDepositor` is
`metadata.sender` embedded as bytes32 the same way as on the deposit side (the Ethereum embedding
of the Miden account ID, `remote_depositor_bytes32`); the amount is in base units (six decimals).

The note tag (`BURN`, `0x4255524E`) is not a criterion: our wallet sets it, but neither the faucet
nor the attester refuses a burn for its tag, so a verifier must not either.

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

## Tests (`tests/local_burn.rs`)

- a note built by the protocol crate's own constructor validates and yields the expected values
- a note routed to another faucet is rejected
- a withdrawal to Miden itself is rejected
- the protocol crate's golden vectors agree: all accept vectors validate, all reject vectors fail
- the old scheme number is rejected, and the old payload layout is rejected for a recipient with
  non-zero leading bytes
- pinned hazard: with a left-padded EVM address (twelve leading zero bytes), the old word order
  under the new scheme number still decodes, to a different recipient. A builder must change the
  word order together with the scheme number, never the number alone
- the command-line tool round-trips a serialized note (accepts, then rejects for another faucet)

```sh
cargo test
```

## Dependencies

`Cargo.toml` pins `miden-protocol`, `miden-standards` and `miden-usdcx` as git dependencies on the
commit of 0xMiden/protocol PR 3983 this crate was validated against. `miden-usdcx` is not published
yet; once a release containing the PR exists, the three lines move to that version. Rust 1.98.1
(`rust-toolchain.toml`). The first build fetches the protocol repository and compiles it, which
takes several minutes.

## Differential check against the faucet

`differential/differential_burn_validation.rs` is the test that was run inside the protocol tree
(as `crates/miden-usdcx/tests/`, with this crate as a dev-dependency): for sixteen note variants the
production faucet consumes the note in a MockChain and its verdict must equal this crate's. It is
kept here for reference; it does not build standalone because it uses the protocol's test support.
